//! Where counters live -- the daily statistics, the report rate limits, the digest's lock: in Redis
//! when `REDIS_URL` names one, so that every replica shares them and a restart keeps them, else in
//! this process's memory. Redis failing never fails a request: the call is logged and the request
//! goes on as if the counter weren't there. A lost count or a skipped limit is better than players'
//! reports bouncing off a Redis hiccup.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Duration;

use parking_lot::Mutex;
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use tracing::warn;

/// A Redis call that takes longer counts as failed; a request never waits on Redis for more.
const REDIS_TIMEOUT: Duration = Duration::from_secs(2);

/// [`Store::admit`] in Redis: one script, so that no other request's count can land between the
/// check and the increment. Requests at once share one connection, and with separate calls each
/// would read the counts before any of them counted. KEYS are the counters; ARGV holds each one's
/// max and expiry (Unix seconds) in turn. When no counter is at its max -- the rule [`full`]
/// keeps -- it counts one against every counter. It answers 1 when it counted, else 0, followed
/// by the counts it found.
const ADMIT_SCRIPT: &str = r"
local answer = {0}
local open = true
for i, key in ipairs(KEYS) do
    local count = tonumber(redis.call('GET', key)) or 0
    answer[i + 1] = count
    if count >= tonumber(ARGV[2 * i - 1]) then
        open = false
    end
end
if open then
    for i, key in ipairs(KEYS) do
        redis.call('INCR', key)
        redis.call('EXPIREAT', key, ARGV[2 * i])
    end
    answer[1] = 1
end
return answer
";

pub enum Store {
    /// Boxed: the memory's tables would make every `Store` as large as they are.
    Memory(Box<Mutex<Memory>>),
    Redis(ConnectionManager),
}

/// A request's share of one limit: the counter's key, how many it takes, and when the counter's
/// window ends (Unix seconds), taking the counter with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counter {
    pub key: String,
    pub max: u64,
    pub expires_at: i64,
}

/// One thing to count: what it adds, under which key, and when the key expires (Unix seconds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    pub key: String,
    pub what: What,
    pub expires_at: i64,
    /// For a counter named from what requests bring: where the day's counter names are listed, so
    /// that a readout, which knows only Redis, finds the counter.
    pub named: Option<Named>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
    /// One more: `INCR`.
    Count,
    /// One more visitor, this hash of one, `PFADD`: a distinct count is a HyperLogLog sketch, a
    /// table that holds no element and yields only how many different ones went in.
    Element([u8; 32]),
}

/// A counter's name in the day's list of names ([`Write::named`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub index: String,
    pub name: String,
}

impl Store {
    pub fn memory() -> Store {
        Store::Memory(Box::default())
    }

    /// Redis at `url`, connected on first use and reconnected after a failure.
    pub fn redis(url: &str) -> redis::RedisResult<Store> {
        let client = redis::Client::open(url)?;
        let config = ConnectionManagerConfig::new()
            .set_connection_timeout(Some(REDIS_TIMEOUT))
            .set_response_timeout(Some(REDIS_TIMEOUT))
            .set_number_of_retries(1)
            .set_max_delay(Duration::from_secs(1));
        Ok(Store::Redis(ConnectionManager::new_lazy_with_config(
            client, config,
        )?))
    }

    /// Adds one to `key`, which expires at `expires_at`.
    pub async fn increment(&self, key: &str, expires_at: i64, now: i64) {
        let write = Write {
            key: key.to_owned(),
            what: What::Count,
            expires_at,
            named: None,
        };
        self.record(&[write], now).await;
    }

    /// Makes each of `writes`, for Redis in one step: a call that fails loses all of them, and the
    /// request that counted goes on.
    pub async fn record(&self, writes: &[Write], now: i64) {
        match self {
            Store::Memory(memory) => memory.lock().record(writes, now),
            Store::Redis(redis) => {
                let mut pipe = redis::pipe();
                for write in writes {
                    match &write.what {
                        What::Count => pipe.cmd("INCR").arg(&write.key).ignore(),
                        What::Element(element) => {
                            pipe.cmd("PFADD").arg(&write.key).arg(&element[..]).ignore()
                        }
                    };
                    pipe.cmd("EXPIREAT")
                        .arg(&write.key)
                        .arg(write.expires_at)
                        .ignore();
                    if let Some(named) = &write.named {
                        pipe.cmd("SADD").arg(&named.index).arg(&named.name).ignore();
                        pipe.cmd("EXPIREAT")
                            .arg(&named.index)
                            .arg(write.expires_at)
                            .ignore();
                    }
                }
                if let Err(error) = within(pipe.query_async::<()>(&mut redis.clone())).await {
                    warn!(%error, writes = writes.len(), "Redis: counts are lost");
                }
            }
        }
    }

    /// The counts under `keys`, 0 for a key never counted; `None` when Redis fails.
    pub async fn values(&self, keys: &[String], now: i64) -> Option<Vec<u64>> {
        match self {
            Store::Memory(memory) => {
                let memory = memory.lock();
                Some(keys.iter().map(|key| memory.value(key, now)).collect())
            }
            Store::Redis(redis) => {
                let mut mget = redis::cmd("MGET");
                mget.arg(keys);
                match within(mget.query_async::<Vec<Option<u64>>>(&mut redis.clone())).await {
                    Ok(values) => Some(values.into_iter().map(Option::unwrap_or_default).collect()),
                    Err(error) => {
                        warn!(%error, "Redis: counts unavailable");
                        None
                    }
                }
            }
        }
    }

    /// How many different elements each of `keys` holds, 0 for a key nothing was added to; `None`
    /// when Redis fails.
    pub async fn distinct_values(&self, keys: &[String], now: i64) -> Option<Vec<u64>> {
        match self {
            Store::Memory(memory) => {
                let memory = memory.lock();
                Some(keys.iter().map(|key| memory.distinct(key, now)).collect())
            }
            Store::Redis(_) if keys.is_empty() => Some(Vec::new()),
            Store::Redis(redis) => {
                let mut pipe = redis::pipe();
                for key in keys {
                    pipe.cmd("PFCOUNT").arg(key);
                }
                match within(pipe.query_async::<Vec<u64>>(&mut redis.clone())).await {
                    Ok(values) => Some(values),
                    Err(error) => {
                        warn!(%error, "Redis: distinct counts unavailable");
                        None
                    }
                }
            }
        }
    }

    /// The counter names each of the day lists `indexes` holds ([`Write::named`]), sorted; `None`
    /// when Redis fails.
    pub async fn names(&self, indexes: &[String], now: i64) -> Option<Vec<Vec<String>>> {
        match self {
            Store::Memory(memory) => {
                let memory = memory.lock();
                Some(
                    indexes
                        .iter()
                        .map(|index| memory.names(index, now))
                        .collect(),
                )
            }
            Store::Redis(_) if indexes.is_empty() => Some(Vec::new()),
            Store::Redis(redis) => {
                let mut pipe = redis::pipe();
                for index in indexes {
                    pipe.cmd("SMEMBERS").arg(index);
                }
                match within(pipe.query_async::<Vec<Vec<String>>>(&mut redis.clone())).await {
                    Ok(mut names) => {
                        names.iter_mut().for_each(|names| names.sort_unstable());
                        Some(names)
                    }
                    Err(error) => {
                        warn!(%error, "Redis: counter names unavailable");
                        None
                    }
                }
            }
        }
    }

    /// The salt kept under `key`: the one already there, else `fresh`, which is kept until
    /// `expires_at`, so that every replica takes the same one. `None` when Redis fails.
    pub async fn salt(
        &self,
        key: &str,
        fresh: [u8; 32],
        expires_at: i64,
        now: i64,
    ) -> Option<[u8; 32]> {
        match self {
            Store::Memory(memory) => Some(memory.lock().salt(key, fresh, expires_at, now)),
            Store::Redis(redis) => {
                let mut pipe = redis::pipe();
                pipe.cmd("SET")
                    .arg(key)
                    .arg(&fresh[..])
                    .arg("NX")
                    .arg("EX")
                    .arg((expires_at - now).max(1))
                    .ignore()
                    .cmd("GET")
                    .arg(key);
                match within(pipe.query_async::<Vec<Option<Vec<u8>>>>(&mut redis.clone())).await {
                    Ok(kept) => kept
                        .into_iter()
                        .next()
                        .flatten()
                        .and_then(|kept| <[u8; 32]>::try_from(kept).ok()),
                    Err(error) => {
                        warn!(%error, key, "Redis: no salt for the distinct counts");
                        None
                    }
                }
            }
        }
    }

    /// Counts a request against each of `counters`, unless one of them is full already: then it
    /// counts nothing and answers the seconds until every full one has expired. A request turned
    /// away by one limit thus uses up none of the others -- a flood from one address can't eat
    /// the day's budget shared by everyone. The check and the count are one step, under the lock
    /// or in one Redis script ([`ADMIT_SCRIPT`]), so requests at once can't all pass on counts
    /// none of them has made yet.
    pub async fn admit(&self, counters: &[Counter], now: i64) -> Result<(), i64> {
        match self {
            Store::Memory(memory) => {
                let mut memory = memory.lock();
                let values: Vec<u64> = counters
                    .iter()
                    .map(|counter| memory.value(&counter.key, now))
                    .collect();
                if let Some(wait) = full(counters, &values, now) {
                    return Err(wait);
                }
                for counter in counters {
                    memory.increment(&counter.key, counter.expires_at, now);
                }
                Ok(())
            }
            Store::Redis(redis) => {
                let mut script = redis::cmd("EVAL");
                script.arg(ADMIT_SCRIPT).arg(counters.len());
                for counter in counters {
                    script.arg(&counter.key);
                }
                for counter in counters {
                    script.arg(counter.max).arg(counter.expires_at);
                }
                match within(script.query_async::<Vec<u64>>(&mut redis.clone())).await {
                    Ok(answer) => admitted(counters, &answer, now),
                    Err(error) => {
                        warn!(%error, "Redis: report let through without its rate limits");
                        Ok(())
                    }
                }
            }
        }
    }

    /// Takes `key` until `expires_at` if nobody holds it: whether this caller got it. When Redis
    /// fails the caller gets it, so a Redis outage can't silence what the key guards.
    pub async fn claim(&self, key: &str, expires_at: i64, now: i64) -> bool {
        match self {
            Store::Memory(memory) => {
                let mut memory = memory.lock();
                if memory.value(key, now) > 0 {
                    return false;
                }
                memory.increment(key, expires_at, now);
                true
            }
            Store::Redis(redis) => {
                let mut set = redis::cmd("SET");
                set.arg(key)
                    .arg(1)
                    .arg("NX")
                    .arg("EX")
                    .arg((expires_at - now).max(1));
                match within(set.query_async::<Option<String>>(&mut redis.clone())).await {
                    Ok(taken) => taken.is_some(),
                    Err(error) => {
                        warn!(%error, key, "Redis: taking the key without its lock");
                        true
                    }
                }
            }
        }
    }
}

/// The seconds until every full counter of `counters` expires, if one is full; `values` are
/// their counts. A counter is full at its max.
pub fn full(counters: &[Counter], values: &[u64], now: i64) -> Option<i64> {
    counters
        .iter()
        .zip(values)
        .filter(|(counter, value)| **value >= counter.max)
        .map(|(counter, _)| (counter.expires_at - now).max(1))
        .max()
}

/// What [`ADMIT_SCRIPT`]'s `answer` means for `counters`: taken when it counted, else turned away
/// for as long as [`full`] says of the counts it found.
fn admitted(counters: &[Counter], answer: &[u64], now: i64) -> Result<(), i64> {
    match answer.split_first() {
        Some((0, counts)) => Err(full(counters, counts, now).unwrap_or(1)),
        _ => Ok(()),
    }
}

async fn within<T>(call: impl Future<Output = redis::RedisResult<T>>) -> Result<T, String> {
    match tokio::time::timeout(REDIS_TIMEOUT, call).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err(format!("no answer in {} s", REDIS_TIMEOUT.as_secs())),
    }
}

/// Counters without Redis: this process's alone, gone on restart.
#[derive(Default)]
pub struct Memory {
    /// Each counter's count and when it expires (Unix seconds).
    counters: HashMap<String, (u64, i64)>,
    /// Each distinct counter's elements, exactly: without Redis there is no sketch to keep small.
    sets: HashMap<String, (HashSet<[u8; 32]>, i64)>,
    /// Each day's list of counter names.
    names: HashMap<String, (BTreeSet<String>, i64)>,
    /// The salts of the periods now running.
    salts: HashMap<String, ([u8; 32], i64)>,
    /// When expired counters are next swept out.
    next_sweep: i64,
}

impl Memory {
    fn value(&self, key: &str, now: i64) -> u64 {
        match self.counters.get(key) {
            Some(&(count, expires_at)) if expires_at > now => count,
            _ => 0,
        }
    }

    fn increment(&mut self, key: &str, expires_at: i64, now: i64) {
        self.sweep(now);
        self.bump(key, expires_at, now);
    }

    fn bump(&mut self, key: &str, expires_at: i64, now: i64) {
        let counter = self
            .counters
            .entry(key.to_owned())
            .or_insert((0, expires_at));
        if counter.1 <= now {
            counter.0 = 0;
        }
        *counter = (counter.0 + 1, expires_at);
    }

    /// Drops what has expired, at most once a minute.
    fn sweep(&mut self, now: i64) {
        if now >= self.next_sweep {
            self.counters.retain(|_, (_, expires_at)| *expires_at > now);
            self.sets.retain(|_, (_, expires_at)| *expires_at > now);
            self.names.retain(|_, (_, expires_at)| *expires_at > now);
            self.salts.retain(|_, (_, expires_at)| *expires_at > now);
            self.next_sweep = now + 60;
        }
    }

    fn record(&mut self, writes: &[Write], now: i64) {
        self.sweep(now);
        for write in writes {
            match &write.what {
                What::Count => self.bump(&write.key, write.expires_at, now),
                What::Element(element) => {
                    let set = self
                        .sets
                        .entry(write.key.clone())
                        .or_insert_with(|| (HashSet::new(), write.expires_at));
                    if set.1 <= now {
                        set.0.clear();
                    }
                    set.0.insert(*element);
                    set.1 = write.expires_at;
                }
            }
            if let Some(named) = &write.named {
                let names = self
                    .names
                    .entry(named.index.clone())
                    .or_insert_with(|| (BTreeSet::new(), write.expires_at));
                if names.1 <= now {
                    names.0.clear();
                }
                names.0.insert(named.name.clone());
                names.1 = write.expires_at;
            }
        }
    }

    fn distinct(&self, key: &str, now: i64) -> u64 {
        match self.sets.get(key) {
            Some((set, expires_at)) if *expires_at > now => set.len() as u64,
            _ => 0,
        }
    }

    fn names(&self, index: &str, now: i64) -> Vec<String> {
        match self.names.get(index) {
            Some((names, expires_at)) if *expires_at > now => names.iter().cloned().collect(),
            _ => Vec::new(),
        }
    }

    fn salt(&mut self, key: &str, fresh: [u8; 32], expires_at: i64, now: i64) -> [u8; 32] {
        self.sweep(now);
        match self.salts.get(key) {
            Some((salt, expires_at)) if *expires_at > now => *salt,
            _ => {
                self.salts.insert(key.to_owned(), (fresh, expires_at));
                fresh
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use mlua::{Lua, Value};
    use oracle_protocol::ReportSource;

    use super::*;
    use crate::limits;

    /// Redis as [`ADMIT_SCRIPT`] sees it: Lua 5.1, which Redis embeds, with `redis.call` over
    /// counters that expire on this stand-in's clock.
    struct LuaRedis {
        lua: Lua,
        now: Rc<Cell<i64>>,
    }

    impl LuaRedis {
        fn new() -> LuaRedis {
            let lua = Lua::new();
            let now = Rc::new(Cell::new(0));
            // Each key's count, and its expiry once EXPIREAT has set one.
            let keys = RefCell::new(HashMap::<String, (u64, Option<i64>)>::new());
            let clock = now.clone();
            let call = lua
                .create_function(
                    move |lua, (command, key, time): (String, String, Option<String>)| {
                        let mut keys = keys.borrow_mut();
                        if keys
                            .get(&key)
                            .is_some_and(|(_, expiry)| expiry.is_some_and(|at| at <= clock.get()))
                        {
                            keys.remove(&key);
                        }
                        match command.as_str() {
                            "GET" => match keys.get(&key) {
                                Some((count, _)) => {
                                    Ok(Value::String(lua.create_string(count.to_string())?))
                                }
                                // Redis's nil, as its Lua sees it.
                                None => Ok(Value::Boolean(false)),
                            },
                            "INCR" => {
                                let (count, _) = keys.entry(key).or_insert((0, None));
                                *count += 1;
                                Ok(Value::Integer(*count as i64))
                            }
                            "EXPIREAT" => {
                                let at = time.and_then(|time| time.parse::<i64>().ok());
                                let Some(at) = at else {
                                    return Err(mlua::Error::runtime("EXPIREAT without a time"));
                                };
                                if let Some((_, expiry)) = keys.get_mut(&key) {
                                    *expiry = Some(at);
                                }
                                Ok(Value::Integer(1))
                            }
                            other => Err(mlua::Error::runtime(format!("unexpected {other}"))),
                        }
                    },
                )
                .unwrap();
            let redis = lua.create_table().unwrap();
            redis.set("call", call).unwrap();
            lua.globals().set("redis", redis).unwrap();
            LuaRedis { lua, now }
        }

        /// [`Store::admit`] the way its Redis path runs, the script run here.
        fn admit(&self, counters: &[Counter], now: i64) -> Result<(), i64> {
            self.now.set(now);
            let keys: Vec<&str> = counters
                .iter()
                .map(|counter| counter.key.as_str())
                .collect();
            // Redis hands every argument over as a string.
            let arguments: Vec<String> = counters
                .iter()
                .flat_map(|counter| [counter.max.to_string(), counter.expires_at.to_string()])
                .collect();
            let globals = self.lua.globals();
            globals.set("KEYS", keys).unwrap();
            globals.set("ARGV", arguments).unwrap();
            let answer: Vec<u64> = self.lua.load(ADMIT_SCRIPT).eval().unwrap();
            admitted(counters, &answer, now)
        }
    }

    #[tokio::test]
    async fn the_redis_script_admits_exactly_as_the_memory_store_does() {
        let memory = Store::memory();
        let redis = LuaRedis::new();
        // 2026-09-26 12:00 in Moscow, then a day and a half of reports from 40 clients through
        // both sources: every limit fills, refuses and opens again with its window.
        let mut now = 1_790_413_200;
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let (mut taken, mut refused) = (0, 0);
        for _ in 0..3000 {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let roll = seed >> 33;
            now += (roll % 90) as i64;
            let source = if roll & 1 == 0 {
                ReportSource::App
            } else {
                ReportSource::Site
            };
            let client = format!("client-{}", (roll >> 8) % 40);
            let counters = limits::counters(source, &client, now);
            let expected = memory.admit(&counters, now).await;
            assert_eq!(
                redis.admit(&counters, now),
                expected,
                "{source:?} from {client} at {now}"
            );
            if expected.is_ok() {
                taken += 1;
            } else {
                refused += 1;
            }
        }
        assert!(
            taken > 300 && refused > 300,
            "{taken} taken, {refused} refused"
        );
    }
}
