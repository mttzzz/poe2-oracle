//! Distinct counts: how many different installs and visitors a day or a week saw, without keeping
//! who they were.
//!
//! A count of this kind is a Redis HyperLogLog sketch: a table of small numbers of a fixed size, of
//! which only the approximate number of different elements can be read. An element is the SHA-256
//! of a salt, the client's address ([`crate::limits::client_key`]) and a User-Agent -- for the app
//! its product token without the version, so that an update doesn't make one install two -- which
//! the sketch takes in and forgets. The salt is 32 random bytes made anew for each Moscow day, and
//! for each ISO week for the weekly count; it lives in Redis until its period is over, plus
//! [`SALT_MARGIN`], and is never logged. No hash is kept anywhere: the sketch takes each one in and
//! forgets it, and once the salt is gone nothing can make a hash of an address again. The hashes of
//! two periods share nothing for one client, so a client can't be followed from one day to the next.

use std::collections::HashMap;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request};
use parking_lot::Mutex;
use sha2::{Digest as _, Sha256};
use tracing::warn;

use crate::agent::{APP_PRODUCT, Version, user_agent};
use crate::limits;
use crate::moscow::{Day, Week};
use crate::stats::{self, KEEP_DAYS, Source};
use crate::store::{Store, What, Write};

/// How long a salt outlives its period: a request that read the clock just before the turn, or
/// a replica whose clock is a little behind, still finds it.
const SALT_MARGIN: i64 = 2 * 3600;

/// Who sent a request, as far as the distinct counts go: the client's address and User-Agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Who {
    client: String,
    agent: String,
}

impl Who {
    /// The sender of `request`.
    pub fn of(request: &Request) -> Who {
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip());
        Who {
            client: limits::client_key(request.headers(), peer),
            agent: user_agent(request.headers()).to_owned(),
        }
    }

    /// A sender that is no request, for tests: `client` as [`limits::client_key`] gives it.
    #[cfg(test)]
    pub fn test(client: &str, agent: &str) -> Who {
        Who {
            client: client.to_owned(),
            agent: agent.to_owned(),
        }
    }

    /// The client's address as the limits count it.
    pub fn client(&self) -> &str {
        &self.client
    }

    /// This visitor under `salt`, by its whole User-Agent: a site visitor.
    pub fn visitor(&self, salt: &[u8; 32]) -> [u8; 32] {
        element(salt, &self.client, &self.agent)
    }

    /// This install under `salt`, by the app's product token alone: the same whatever version
    /// it runs.
    pub fn install(&self, salt: &[u8; 32]) -> [u8; 32] {
        element(salt, &self.client, APP_PRODUCT)
    }
}

/// The hash of `salt`, `client` and `agent`, each length-prefixed so that no two triples run
/// together into the same input.
fn element(salt: &[u8; 32], client: &str, agent: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(salt);
    for part in [client, agent] {
        hash.update((part.len() as u32).to_be_bytes());
        hash.update(part.as_bytes());
    }
    hash.finalize().into()
}

/// The span of time a salt, and a distinct count, belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Period {
    Day(Day),
    Week(Week),
}

impl Period {
    /// Where the period's salt is kept in the store.
    fn salt_key(self) -> String {
        match self {
            Period::Day(day) => format!("oracle:salt:{day}"),
            Period::Week(week) => format!("oracle:salt:{week}"),
        }
    }

    fn end(self) -> i64 {
        match self {
            Period::Day(day) => day.end(),
            Period::Week(week) => week.end(),
        }
    }
}

/// The salts of the periods running, as this process has read them from the store.
#[derive(Default)]
pub struct Salts {
    known: Mutex<HashMap<Period, ([u8; 32], i64)>>,
}

impl Salts {
    /// The salt of `period` at `now`: the one the store keeps, or, if nobody has made it yet, a
    /// random one this call makes, and the store keeps for every replica. `None` when the store
    /// or the system's random source fails: nothing is counted then.
    pub async fn of(&self, store: &Store, period: Period, now: i64) -> Option<[u8; 32]> {
        if let Some(&(salt, expires_at)) = self.known.lock().get(&period)
            && expires_at > now
        {
            return Some(salt);
        }
        let mut fresh = [0; 32];
        if let Err(error) = getrandom::fill(&mut fresh) {
            warn!(%error, "no random salt for the distinct counts");
            return None;
        }
        let expires_at = period.end() + SALT_MARGIN;
        let salt = store
            .salt(&period.salt_key(), fresh, expires_at, now)
            .await?;
        let mut known = self.known.lock();
        known.retain(|_, (_, expires_at)| *expires_at > now);
        known.insert(period, (salt, expires_at));
        Some(salt)
    }
}

/// A count of different visitors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Uniq {
    /// Installs active in a day: they opened a stream, checked for updates, or held one open as
    /// the day began.
    AppDay,
    /// The same over the ISO week: its own sketch under the week's own salt, not a sum of days.
    AppWeek,
    /// [`Uniq::AppDay`] by version; an install that updates that day is in two.
    AppDayVersion(Version),
    /// Visitors who loaded a page of the site in a day.
    SiteDay,
    /// Those whose page came from a link tagged with where it was published.
    SiteDayFrom(Source),
}

impl Uniq {
    pub fn name(&self) -> String {
        match self {
            Uniq::AppDay => "uniq_app_day".to_owned(),
            Uniq::AppWeek => "uniq_app_week".to_owned(),
            Uniq::AppDayVersion(version) => format!("uniq_app_day_v_{version}"),
            Uniq::SiteDay => "uniq_site_day".to_owned(),
            Uniq::SiteDayFrom(source) => format!("uniq_site_day_from_{}", source.tag()),
        }
    }

    /// The period whose salt the elements are hashed with, at `now`.
    pub fn period(&self, now: i64) -> Period {
        match self {
            Uniq::AppWeek => Period::Week(Day::of(now).week()),
            _ => Period::Day(Day::of(now)),
        }
    }

    /// Adds `element` to the count of the period `now` is in.
    pub fn write(&self, element: [u8; 32], now: i64) -> Write {
        match self.period(now) {
            Period::Day(_) => stats::write(
                &self.name(),
                What::Element(element),
                matches!(self, Uniq::AppDayVersion(_)),
                now,
            ),
            Period::Week(week) => Write {
                key: week_key(week),
                what: What::Element(element),
                expires_at: week.end() + KEEP_DAYS * crate::moscow::DAY_SECS,
                named: None,
            },
        }
    }
}

/// Where the weekly count of installs is kept.
pub fn week_key(week: Week) -> String {
    format!("oracle:stat:{}:{week}", Uniq::AppWeek.name())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-09-26 12:00 in Moscow, a Saturday.
    const NOON: i64 = 1_790_413_200;
    const DAY: i64 = crate::moscow::DAY_SECS;

    fn who(client: &str, agent: &str) -> Who {
        Who {
            client: client.to_owned(),
            agent: agent.to_owned(),
        }
    }

    /// Counts `who` as a site visitor at `now`, as a page load does.
    async fn seen(store: &Store, salts: &Salts, who: &Who, now: i64) {
        let salt = salts
            .of(store, Uniq::SiteDay.period(now), now)
            .await
            .unwrap();
        let write = Uniq::SiteDay.write(who.visitor(&salt), now);
        store.record(&[write], now).await;
    }

    async fn visitors(store: &Store, now: i64) -> u64 {
        let day = Day::of(now);
        let key = format!("oracle:stat:uniq_site_day:{day}");
        store.distinct_values(&[key], now).await.unwrap()[0]
    }

    #[tokio::test]
    async fn one_client_is_counted_once_a_day_and_two_clients_twice() {
        let (store, salts) = (Store::memory(), Salts::default());
        let (one, other) = (who("203.0.113.1", "Firefox"), who("203.0.113.2", "Firefox"));
        for _ in 0..5 {
            seen(&store, &salts, &one, NOON).await;
        }
        assert_eq!(visitors(&store, NOON).await, 1);
        seen(&store, &salts, &other, NOON + 60).await;
        seen(&store, &salts, &one, NOON + 120).await;
        assert_eq!(visitors(&store, NOON).await, 2);
        // The same address with another browser is another visitor.
        seen(&store, &salts, &who("203.0.113.1", "Chrome"), NOON).await;
        assert_eq!(visitors(&store, NOON).await, 3);
    }

    #[tokio::test]
    async fn the_next_day_takes_a_new_salt_and_counts_the_same_client_afresh() {
        let (store, salts) = (Store::memory(), Salts::default());
        let client = who("203.0.113.1", "Firefox");
        let (today, tomorrow) = (NOON, NOON + DAY);
        seen(&store, &salts, &client, today).await;
        seen(&store, &salts, &client, tomorrow).await;
        assert_eq!(visitors(&store, today).await, 1);
        assert_eq!(visitors(&store, tomorrow).await, 1);

        let day_salt = |now| salts.of(&store, Period::Day(Day::of(now)), now);
        let (first, second) = (
            day_salt(today).await.unwrap(),
            day_salt(tomorrow).await.unwrap(),
        );
        assert_ne!(first, second, "a salt for each day");
        assert_eq!(
            day_salt(today + 60).await,
            Some(first),
            "kept through the day"
        );
        // So the two days' hashes of one client share nothing to follow it by.
        assert_ne!(client.visitor(&first), client.visitor(&second));
        // Nor does the week's salt, which is another still, repeat a day's.
        let week = salts
            .of(&store, Period::Week(Day::of(today).week()), today)
            .await
            .unwrap();
        assert!(week != first && week != second);
    }

    #[tokio::test]
    async fn a_salt_is_the_same_for_every_replica_and_lives_until_its_period_is_over() {
        let store = Store::memory();
        let day = Period::Day(Day::of(NOON));
        // Two replicas of the service, each with its own memory and its own random pick.
        let (first, second) = (Salts::default(), Salts::default());
        let kept = first.of(&store, day, NOON).await.unwrap();
        assert_eq!(second.of(&store, day, NOON + 5).await, Some(kept));
        // Past the margin after midnight the store lets it go, and the day's hashes with it.
        let end = Day::of(NOON).end();
        let gone = first
            .of(&store, day, end + SALT_MARGIN)
            .await
            .expect("a fresh salt");
        assert_ne!(gone, kept);
    }

    #[test]
    fn an_install_is_told_by_its_address_alone_whatever_version_it_runs() {
        let salt = [7; 32];
        let old = who("203.0.113.1", "PoE2-Oracle/0.1.2");
        let new = who(
            "203.0.113.1",
            "PoE2-Oracle/0.1.3 (+https://oracle.pushka.biz)",
        );
        assert_eq!(old.install(&salt), new.install(&salt));
        assert_ne!(
            old.install(&salt),
            who("203.0.113.9", "PoE2-Oracle/0.1.2").install(&salt)
        );
        // A site visitor is told by the whole User-Agent.
        assert_ne!(old.visitor(&salt), new.visitor(&salt));
    }

    #[test]
    fn a_hash_takes_each_part_whole() {
        let salt = [1; 32];
        // The address and the agent can't trade characters.
        assert_ne!(element(&salt, "ab", "c"), element(&salt, "a", "bc"));
        assert_ne!(element(&salt, "", "abc"), element(&salt, "abc", ""));
        assert_ne!(element(&salt, "a", "b"), element(&[2; 32], "a", "b"));
    }

    #[test]
    fn the_weekly_count_keeps_by_the_week_and_the_rest_by_the_day() {
        let element = [9; 32];
        let day = Uniq::AppDay.write(element, NOON);
        assert_eq!(day.key, "oracle:stat:uniq_app_day:2026-09-26");
        assert_eq!(day.expires_at, Day::of(NOON).end() + 120 * DAY);
        let week = Uniq::AppWeek.write(element, NOON);
        assert_eq!(week.key, "oracle:stat:uniq_app_week:2026-W39");
        assert_eq!(week.key, week_key(Day::of(NOON).week()));
        // The Sunday after midnight ends the week's keys 120 days later.
        assert_eq!(week.expires_at, Day::of(NOON).week().end() + 120 * DAY);
        // Every day of a week is in its sketch; the next Monday is in the next.
        let monday = Day::of(NOON).week().end();
        assert_eq!(Uniq::AppWeek.write(element, monday - 1).key, week.key);
        assert_eq!(
            Uniq::AppWeek.write(element, monday).key,
            "oracle:stat:uniq_app_week:2026-W40"
        );
        // Only a name made of a version is listed for the readout.
        let version = Version::bound(Some("0.1.3"), &["0.1.3".to_owned()]);
        let by_version = Uniq::AppDayVersion(version).write(element, NOON);
        assert_eq!(
            by_version.key,
            "oracle:stat:uniq_app_day_v_0.1.3:2026-09-26"
        );
        assert_eq!(
            by_version.named.map(|named| (named.index, named.name)),
            Some((
                "oracle:names:2026-09-26".to_owned(),
                "uniq_app_day_v_0.1.3".to_owned()
            ))
        );
        assert_eq!(day.named, None);
        assert_eq!(
            Uniq::SiteDayFrom(Source::Reddit).write(element, NOON).key,
            "oracle:stat:uniq_site_day_from_reddit:2026-09-26"
        );
    }
}
