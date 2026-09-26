//! The Currency Exchange market: what everything PoE2's in-game Currency Exchange trades --
//! currency, fragments, omens, runes, essences, catalysts, soul cores... -- is worth in a league,
//! from GGG's own record of the trades the exchange executed, with poe2scout's week of prices and
//! item page beside each item (see `scout`), and poe2scout's price for an item the record doesn't
//! price.
//!
//! GGG's record, verified live 2026-09-23:
//! - `GET https://web.poecdn.com/api/currency-exchange/poe2/{hour}` (no auth; `hour` is the unix
//!   time the hour starts) answers every league's trades in that hour, one row per market:
//!   `{"next_change_id":1790161200,"markets":[{"league":"Forbidden Rites","market_id":"...",
//!   "market_pair":["Metadata/Items/Currency/CurrencyModValues","Metadata/Items/Currency/
//!   CurrencyAddModToRare"],"volume_traded":{"Metadata/Items/Currency/CurrencyModValues":6526,
//!   "Metadata/Items/Currency/CurrencyAddModToRare":3205461},"lowest_stock":{...},
//!   "highest_stock":{...},"lowest_ratio":{...},"highest_ratio":{...}},...]}` -- 3039 markets at
//!   10:00 UTC, 1757 of them Forbidden Rites'. `volume_traded` counts the units of each side that
//!   changed hands, so the ratio of a market's two volumes is the rate its trades executed at.
//! - The hour under way answers 404 (`{"next_change_id":<that hour>,"markets":[]}`). A complete
//!   hour is out minutes after it ends (10:00 was there at 11:04) and never changes (served with
//!   `cache-control: max-age=31535722`), so each is downloaded once and kept -- in memory and as
//!   `cx-hour-{hour}.json` in the cache folder -- while it is in the window.
//! - Items are named by their base item's metadata id. `data/cx-items.tsv` gives each its trade
//!   site id and group: every item on the trade site's exchange list (its generator,
//!   `packaging/data/generate_cx_ids.py`, says how; VibeTools' own `cx-map.json` is GPL-3.0 and has
//!   wrong ids, `zarokh-s-reliquary-key-temporalis` for `temporalis`). Eight items the exchange
//!   trades are not on that list -- Hawk, Panther and Stoat Idols, Raven's Reflection, The
//!   Triskelion Reforged, Shattered Triskelion, Eonyr's Thunder, Helbrym's Hide -- and are left out:
//!   no check routes to the market for them.
//! - The table is built in, and a game data pack (the app's `data_pack`) may bring a newer one
//!   for a run: [`read_exchange_items`] reads a pack's table, refusing a malformed one, and
//!   [`use_exchange_items`] puts it in place before the table is first read.
//!
//! A price -- steps 1 and 2 as VibeTools reads the same record (`cx-feed.js`, POE2 Prices 3.0.7):
//! 1. The window is the newest three complete hours the CDN has.
//! 2. Each pair's volumes are summed over the window's hours, newest first, until the scarcer
//!    side reaches 20 units: a busy pair is priced from the last hour alone, a thin one from up to
//!    three, so no single lopsided fill sets its rate. The ratio of the sums is the pair's
//!    volume-weighted executed rate.
//! 3. The Exalted Orb is valued by its pair with the Divine Orb; the Chaos Orb by its pairs with
//!    those two; every other item by its pairs with all three: the divines its trades there were
//!    worth over the units of it traded. A pair whose hours reach back further than those of the
//!    item's busiest pair (by divines traded) is left out -- an item is valued as freshly as its
//!    main market, and the hours shown are the ones it was valued from.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use futures::future::join;
use http_client::{AsyncBody, HttpClient};
use poe2_domain::pack_table::{PackTable, TableInUse};
use serde::{Deserialize, Serialize};

use crate::cache::{read_stale_cache, write_cache};
use crate::rates::PriceUnit;
use crate::scout::{self, ScoutExchange};
use crate::{TradeApiError, checked_body};

const CX_BASE_URL: &str = "https://web.poecdn.com/api/currency-exchange/poe2";

const HOUR: u64 = 3600;
/// Complete hours a market is made of, at most (VibeTools' `HOURS_WINDOW`).
const WINDOW_HOURS: usize = 3;
/// How many complete hours back the window's hours are looked for: the newest may not be out yet,
/// and a slow CDN may lag more.
const LOOKBACK_HOURS: u64 = 6;
/// Units on a pair's scarcer side that make its hours enough (VibeTools' `LIQUID_MIN`).
const LIQUID_MIN: f64 = 20.0;

/// Trade ids of the core currencies, the ones every item is valued through.
pub(crate) const DIVINE: &str = "divine";
pub(crate) const EXALTED: &str = "exalted";
const CHAOS: &str = "chaos";

/// EE2's `autoCurrency` cutover (`Prices.ts`): a price above 0.94 div reads in divines, anything
/// cheaper in exalted.
pub(crate) const DIVINE_UNIT_CUTOVER: f64 = 0.94;

/// A data pack's table, waiting for the first read of [`EXCHANGE_ITEMS`].
static PACK: PackTable<ExchangeItems<'static>> = PackTable::new();

/// `data/cx-items.tsv`'s rows by metadata id.
static EXCHANGE_ITEMS: LazyLock<HashMap<&'static str, ExchangeItem<'static>>> =
    LazyLock::new(|| {
        PACK.take().map_or_else(
            || {
                read_exchange_items(include_str!("../data/cx-items.tsv"))
                    .expect("data/cx-items.tsv is well-formed")
                    .0
            },
            |pack| pack.0,
        )
    });

/// An item the exchange trades, as the trade site knows it, in a table whose text lives for `'a`.
struct ExchangeItem<'a> {
    trade_id: &'a str,
    /// Its group on the trade site's exchange list: `Currency`, `Ritual`, `Runes`...
    group: &'a str,
}

/// The exchange items table, read ([`read_exchange_items`]) from a text that lives for `'a`.
pub struct ExchangeItems<'a>(HashMap<&'a str, ExchangeItem<'a>>);

/// Reads an exchange items table: one row per item -- its metadata id, trade id and group, none
/// empty, each metadata id once. An error names the first row that isn't one; a table without
/// rows is one too.
pub fn read_exchange_items(table: &str) -> Result<ExchangeItems<'_>, String> {
    let mut items = HashMap::new();
    for (number, row) in table.lines().enumerate() {
        let mut fields = row.split('\t');
        let (Some(metadata_id), Some(trade_id), Some(group), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            let count = row.split('\t').count();
            return Err(format!("line {}: {count} fields", number + 1));
        };
        if [metadata_id, trade_id, group].contains(&"") {
            return Err(format!("line {}: an empty field", number + 1));
        }
        if items
            .insert(metadata_id, ExchangeItem { trade_id, group })
            .is_some()
        {
            return Err(format!("line {}: {metadata_id} listed twice", number + 1));
        }
    }
    if items.is_empty() {
        return Err("no rows".to_owned());
    }
    Ok(ExchangeItems(items))
}

/// Makes `table`, a data pack's ([`read_exchange_items`]), the one exchange items are looked up
/// in for the rest of the run. Refused once the table has been read: the app puts a pack's table
/// in place before anything reads it.
pub fn use_exchange_items(table: ExchangeItems<'static>) -> Result<(), TableInUse> {
    PACK.set(table)
}

/// Hours of GGG's record read so far, by their cache file: two cache folders (tests') never share
/// one.
static READ_HOURS: LazyLock<Mutex<HashMap<PathBuf, Arc<CxHour>>>> = LazyLock::new(Default::default);

/// One hour of GGG's record, as served and as cached: the fields read here only.
#[derive(Serialize, Deserialize)]
struct CxHour {
    markets: Vec<CxMarket>,
}

#[derive(Serialize, Deserialize)]
struct CxMarket {
    league: String,
    market_pair: Vec<String>,
    #[serde(default)]
    volume_traded: HashMap<String, Option<f64>>,
}

/// A run of whole hours of GGG's record, in unix time: `start` of the first, `end` of the last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TradedHours {
    pub start: u64,
    pub end: u64,
}

/// An exchange item's market, priced by GGG's record of the exchange.
#[derive(Debug, Clone, PartialEq)]
pub struct MarketPrice {
    /// The trade site's `/data/static` id for the item.
    pub id: String,
    /// The item's group on the trade site's exchange list: `Currency`, `Ritual`, `Runes`...
    pub category: String,
    /// One unit's worth in Divine Orbs (see the module doc).
    pub divine_value: f64,
    /// The hours `divine_value` comes from.
    pub hours: TradedHours,
    /// Its units traded per hour in the window, in divines.
    pub volume_divine: f64,
    /// The core currency (divine, exalted or chaos) of its busiest pair, by divines traded.
    pub most_traded_with: String,
    /// Units of the item one `most_traded_with` bought there: alch 150 per divine, a mirror
    /// 0.0002 per divine.
    pub most_traded_rate: f64,
    /// The change over poe2scout's week in percent; `None` without two days of prices.
    pub change_7d: Option<f64>,
    /// poe2scout's week day by day, oldest first, as percent changes against its first day with a
    /// price; a `None` day had no price. Empty where `change_7d` is `None`.
    pub sparkline: Vec<Option<f64>>,
    /// The item's poe2scout page, when poe2scout lists it.
    pub details_url: Option<String>,
}

/// One league's exchange market: GGG's prices of what it traded in the window, poe2scout's of
/// the rest, and the core exchange rates.
#[derive(Debug, Clone, PartialEq)]
pub struct Market {
    /// Exalted Orbs one Divine Orb buys (491.2 at 10:00 UTC 2026-09-23 in Forbidden Rites).
    pub exalted_per_divine: f64,
    /// Chaos Orbs one Divine Orb buys (7.76 then).
    pub chaos_per_divine: f64,
    /// The hours the rates come from; `None` when GGG's record was out of reach and poe2scout's
    /// rates stand in.
    pub hours: Option<TradedHours>,
    /// GGG's prices by trade id; every `divine_value` is finite and positive, as are both rates.
    prices: HashMap<String, MarketPrice>,
    scout: Option<ScoutExchange>,
}

impl Market {
    /// GGG's price of the item with trade id `trade_id`; `None` when it didn't trade in the window
    /// against a core currency.
    pub fn price(&self, trade_id: &str) -> Option<&MarketPrice> {
        self.prices.get(trade_id)
    }

    /// poe2scout's price of the item with trade id `trade_id`, at poe2scout's own rate and in the
    /// unit it reads best in: what prices an item GGG's window doesn't.
    pub fn scout_price(&self, trade_id: &str) -> Option<(f64, PriceUnit)> {
        self.scout.as_ref()?.price(trade_id)
    }

    /// One unit of `currency` (a trade id) in divines: the core rates for the core currencies,
    /// else GGG's price, else poe2scout's.
    pub fn value_in_divines(&self, currency: &str) -> Option<f64> {
        match currency {
            DIVINE => Some(1.0),
            EXALTED => Some(1.0 / self.exalted_per_divine),
            CHAOS => Some(1.0 / self.chaos_per_divine),
            _ => self
                .prices
                .get(currency)
                .map(|price| price.divine_value)
                .or_else(|| self.scout.as_ref()?.value_in_divines(currency)),
        }
    }

    /// `divines` in the unit EE2's `autoCurrency` would show it in: divines above 0.94, exalted at
    /// or below -- the rule behind EE2's per-listing normalized prices.
    pub fn in_display_unit(&self, divines: f64) -> (f64, PriceUnit) {
        if divines <= DIVINE_UNIT_CUTOVER {
            (divines * self.exalted_per_divine, PriceUnit::Exalted)
        } else {
            (divines, PriceUnit::Divine)
        }
    }

    /// Adds poe2scout's week and page to every price, and its prices of the rest.
    fn with_scout(mut self, scout: ScoutExchange) -> Market {
        for price in self.prices.values_mut() {
            (price.change_7d, price.sparkline) = week(&scout, &price.id);
            price.details_url = scout
                .items
                .get(&price.id)
                .and_then(|item| item.page.clone());
        }
        self.scout = Some(scout);
        self
    }
}

/// `league`'s market (`league` is the trade league id; `cache_dir` the app's cache root). GGG's
/// hours come from the cache folder when kept there; poe2scout's view of the exchange from its
/// half-hour copy there (see `scout`). Without GGG's record -- the CDN out of reach and no hour
/// kept, or no Divine Orb traded for Exalted Orbs in the league -- poe2scout's prices and rates
/// stand in; the error surfaces only without both.
pub async fn fetch_market(
    client: &Arc<dyn HttpClient>,
    league: &str,
    cache_dir: &Path,
) -> Result<Market> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    fetch_market_at(client, league, cache_dir, now).await
}

async fn fetch_market_at(
    client: &Arc<dyn HttpClient>,
    league: &str,
    cache_dir: &Path,
    now: u64,
) -> Result<Market> {
    let (hours, scout) = join(
        load_hours(client, cache_dir, now),
        scout::fetch_exchange(client, league, cache_dir),
    )
    .await;
    let scout = scout
        .inspect_err(|err| log::warn!("poe2scout's exchange prices are unavailable: {err:#}"))
        .ok();
    match (hours.and_then(|hours| build_market(league, &hours)), scout) {
        (Ok(market), Some(scout)) => Ok(market.with_scout(scout)),
        (Ok(market), None) => Ok(market),
        (Err(err), Some(scout)) => {
            log::warn!("{err:#}; pricing the exchange from poe2scout");
            scout_market(scout)
        }
        (Err(err), None) => Err(err),
    }
}

/// The newest `WINDOW_HOURS` complete hours of GGG's record at `now`, newest first. Hours missing
/// from memory and the cache folder are downloaded until one fails to; without any hour in the
/// last `LOOKBACK_HOURS`, the newest ones kept however old stand in. Hours out of the window are
/// dropped from both.
async fn load_hours(
    client: &Arc<dyn HttpClient>,
    cache_dir: &Path,
    now: u64,
) -> Result<Vec<(u64, Arc<CxHour>)>> {
    let newest = (now / HOUR * HOUR).saturating_sub(HOUR);
    let mut hours = Vec::with_capacity(WINDOW_HOURS);
    let mut failure = None;
    for back in 0..LOOKBACK_HOURS {
        let start = newest.saturating_sub(back * HOUR);
        match load_hour(client, cache_dir, start, failure.is_none()).await {
            Ok(Some(hour)) => {
                hours.push((start, hour));
                if hours.len() == WINDOW_HOURS {
                    break;
                }
            }
            Ok(None) => {}
            Err(err) => {
                log::warn!("GGG's exchange record is out of reach: {err:#}");
                failure = Some(err);
            }
        }
    }
    if hours.is_empty() {
        hours = kept_hours(cache_dir);
    }
    if hours.is_empty() {
        return Err(failure.unwrap_or_else(|| {
            anyhow!("GGG's exchange record has no hour from the last {LOOKBACK_HOURS}")
        }));
    }
    forget_other_hours(cache_dir, &hours);
    Ok(hours)
}

/// Hour `start` of GGG's record: from memory, else the cache folder, else -- `download`
/// permitting -- the CDN. `None` for an hour the CDN doesn't have yet, or, not downloading, one
/// not kept.
async fn load_hour(
    client: &Arc<dyn HttpClient>,
    cache_dir: &Path,
    start: u64,
    download: bool,
) -> Result<Option<Arc<CxHour>>> {
    let path = hour_path(cache_dir, start);
    if let Some(hour) = read_hours().get(&path) {
        return Ok(Some(hour.clone()));
    }
    if let Some(hour) = read_stale_cache(&path) {
        return Ok(Some(remember(path, hour)));
    }
    if !download {
        return Ok(None);
    }
    let url = format!("{CX_BASE_URL}/{start}");
    let request = client.get(&url, AsyncBody::default(), true);
    let body = match checked_body(request, "GET", &url, None, "exchange hour").await {
        Ok(body) => body,
        Err(err)
            if err
                .downcast_ref::<TradeApiError>()
                .is_some_and(|err| err.status == 404) =>
        {
            return Ok(None);
        }
        Err(err) => return Err(err),
    };
    let hour: CxHour = serde_json::from_str(&body).with_context(|| format!("parsing {url}"))?;
    if let Err(err) = write_cache(&path, &hour) {
        log::warn!("keeping {}: {err:#}", path.display());
    }
    Ok(Some(remember(path, hour)))
}

fn hour_path(cache_dir: &Path, start: u64) -> PathBuf {
    cache_dir.join(format!("cx-hour-{start}.json"))
}

/// The hour a cache file name keeps: `cx-hour-{start}.json`.
fn kept_hour_start(file_name: &str) -> Option<u64> {
    file_name
        .strip_prefix("cx-hour-")?
        .strip_suffix(".json")?
        .parse()
        .ok()
}

fn read_hours() -> std::sync::MutexGuard<'static, HashMap<PathBuf, Arc<CxHour>>> {
    READ_HOURS.lock().unwrap_or_else(PoisonError::into_inner)
}

fn remember(path: PathBuf, hour: CxHour) -> Arc<CxHour> {
    let hour = Arc::new(hour);
    read_hours().insert(path, hour.clone());
    hour
}

/// The newest `WINDOW_HOURS` hours in the cache folder, however old, newest first.
fn kept_hours(cache_dir: &Path) -> Vec<(u64, Arc<CxHour>)> {
    let mut starts: Vec<u64> = std::fs::read_dir(cache_dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| kept_hour_start(entry.ok()?.file_name().to_str()?))
        .collect();
    starts.sort_unstable_by(|a, b| b.cmp(a));
    starts
        .into_iter()
        .filter_map(|start| {
            let path = hour_path(cache_dir, start);
            let hour = read_stale_cache(&path)?;
            Some((start, remember(path, hour)))
        })
        .take(WINDOW_HOURS)
        .collect()
}

/// Drops every kept hour of `cache_dir` but `hours`, from memory and from the folder.
fn forget_other_hours(cache_dir: &Path, hours: &[(u64, Arc<CxHour>)]) {
    let kept = |start: u64| hours.iter().any(|&(kept, _)| kept == start);
    read_hours().retain(|path, _| {
        path.parent() != Some(cache_dir)
            || path
                .file_name()
                .and_then(|name| kept_hour_start(name.to_str()?))
                .is_some_and(kept)
    });
    for entry in std::fs::read_dir(cache_dir).into_iter().flatten().flatten() {
        let start = entry.file_name().to_str().and_then(kept_hour_start);
        if start.is_some_and(|start| !kept(start))
            && let Err(err) = std::fs::remove_file(entry.path())
        {
            log::warn!("removing {}: {err}", entry.path().display());
        }
    }
}

/// A pair's trades over its hours (step 2 of the module doc): the units of each side, `a` and
/// `b` in the order of the pair's key.
#[derive(Clone, Copy)]
struct PairTrades {
    a: f64,
    b: f64,
    hours: TradedHours,
}

/// An item's trades with one core currency, from the item's side.
#[derive(Clone, Copy)]
struct CoreTrades {
    currency: &'static str,
    item_units: f64,
    currency_units: f64,
    hours: TradedHours,
}

/// A pair of items by trade id, the lesser first.
type PairKey = (&'static str, &'static str);

/// What one league traded in the window.
struct LeagueTrades {
    pairs: HashMap<PairKey, PairTrades>,
    /// Each item's units traded per hour of the window.
    per_hour: HashMap<&'static str, f64>,
    /// Each item's group.
    groups: HashMap<&'static str, &'static str>,
}

impl LeagueTrades {
    /// `hours` newest first. A market names two items of the trade site's list, or is left out.
    fn read(league: &str, hours: &[(u64, Arc<CxHour>)]) -> LeagueTrades {
        // Each pair's units of each side, hour by hour.
        let mut hourly: HashMap<PairKey, Vec<Option<(f64, f64)>>> = HashMap::new();
        let mut units: HashMap<&'static str, f64> = HashMap::new();
        let mut groups = HashMap::new();
        for (index, (_, hour)) in hours.iter().enumerate() {
            for market in hour.markets.iter().filter(|market| market.league == league) {
                let [metadata_a, metadata_b] = market.market_pair.as_slice() else {
                    continue;
                };
                let (Some(item_a), Some(item_b)) = (
                    EXCHANGE_ITEMS.get(metadata_a.as_str()),
                    EXCHANGE_ITEMS.get(metadata_b.as_str()),
                ) else {
                    continue;
                };
                let traded = |metadata_id: &String| {
                    market
                        .volume_traded
                        .get(metadata_id)
                        .copied()
                        .flatten()
                        .filter(|units| units.is_finite() && *units > 0.0)
                };
                let (Some(units_a), Some(units_b)) = (traded(metadata_a), traded(metadata_b))
                else {
                    continue;
                };
                let (a, b) = (item_a.trade_id, item_b.trade_id);
                if a == b {
                    continue;
                }
                let (key, sides) = if a < b {
                    ((a, b), (units_a, units_b))
                } else {
                    ((b, a), (units_b, units_a))
                };
                let slot = &mut hourly.entry(key).or_insert_with(|| vec![None; hours.len()])[index];
                let sums = slot.get_or_insert((0.0, 0.0));
                sums.0 += sides.0;
                sums.1 += sides.1;
                *units.entry(a).or_default() += units_a;
                *units.entry(b).or_default() += units_b;
                groups.insert(a, item_a.group);
                groups.insert(b, item_b.group);
            }
        }
        let pairs = hourly
            .into_iter()
            .filter_map(|(key, volumes)| Some((key, pair_window(&volumes, hours)?)))
            .collect();
        let loaded = hours.len() as f64;
        let per_hour = units
            .into_iter()
            .map(|(id, units)| (id, units / loaded))
            .collect();
        LeagueTrades {
            pairs,
            per_hour,
            groups,
        }
    }

    /// `item`'s trades with `currency`, if they traded.
    fn with_core(&self, item: &'static str, currency: &'static str) -> Option<CoreTrades> {
        let key = if item < currency {
            (item, currency)
        } else {
            (currency, item)
        };
        let trades = self.pairs.get(&key)?;
        let (item_units, currency_units) = if key.0 == item {
            (trades.a, trades.b)
        } else {
            (trades.b, trades.a)
        };
        Some(CoreTrades {
            currency,
            item_units,
            currency_units,
            hours: trades.hours,
        })
    }

    /// `item`'s trades with each of `currencies` it traded with.
    fn with_cores(&self, item: &'static str, currencies: &[&'static str]) -> Vec<CoreTrades> {
        currencies
            .iter()
            .filter(|&&currency| currency != item)
            .filter_map(|&currency| self.with_core(item, currency))
            .collect()
    }
}

/// Step 2 of the module doc: a pair's volumes in each hour of `hours` (newest first; `None` where
/// it didn't trade) summed newest first until its scarcer side reaches `LIQUID_MIN`.
fn pair_window(volumes: &[Option<(f64, f64)>], hours: &[(u64, Arc<CxHour>)]) -> Option<PairTrades> {
    let (mut a, mut b) = (0.0, 0.0);
    let mut span: Option<(u64, u64)> = None;
    for (volumes, &(start, _)) in volumes.iter().zip(hours) {
        let Some((units_a, units_b)) = volumes else {
            continue;
        };
        a += units_a;
        b += units_b;
        span = Some((start, span.map_or(start, |(_, newest)| newest)));
        if a.min(b) >= LIQUID_MIN {
            break;
        }
    }
    let (oldest, newest) = span?;
    Some(PairTrades {
        a,
        b,
        hours: TradedHours {
            start: oldest,
            end: newest + HOUR,
        },
    })
}

/// An item valued by its trades with the core currencies (step 3 of the module doc).
struct Valuation {
    divine_value: f64,
    hours: TradedHours,
    busiest: CoreTrades,
}

/// `trades` valued with each core currency's `worth` in divines; `None` without trades.
fn value(trades: &[CoreTrades], worth: impl Fn(&str) -> f64) -> Option<Valuation> {
    let divines = |trades: &CoreTrades| trades.currency_units * worth(trades.currency);
    let busiest = *trades
        .iter()
        .max_by(|x, y| divines(x).total_cmp(&divines(y)))?;
    let fresh: Vec<&CoreTrades> = trades
        .iter()
        .filter(|trades| trades.hours.start >= busiest.hours.start)
        .collect();
    let paid: f64 = fresh.iter().map(|trades| divines(trades)).sum();
    let units: f64 = fresh.iter().map(|trades| trades.item_units).sum();
    let end = fresh.iter().map(|trades| trades.hours.end).max()?;
    Some(Valuation {
        divine_value: paid / units,
        hours: TradedHours {
            start: busiest.hours.start,
            end,
        },
        busiest,
    })
}

/// `league`'s market by GGG's `hours` (newest first) alone. An error without a Divine Orb traded
/// for Exalted Orbs or Chaos Orbs traded for either: nothing is valued without the core rates.
fn build_market(league: &str, hours: &[(u64, Arc<CxHour>)]) -> Result<Market> {
    let trades = LeagueTrades::read(league, hours);
    let anchor = trades.with_core(DIVINE, EXALTED).with_context(|| {
        format!(
            "{league} traded no Divine Orb for Exalted Orbs in GGG's last {} hours",
            hours.len()
        )
    })?;
    let exalted_per_divine = anchor.currency_units / anchor.item_units;
    let exalted_worth = 1.0 / exalted_per_divine;
    let chaos = value(&trades.with_cores(CHAOS, &[DIVINE, EXALTED]), |currency| {
        if currency == DIVINE {
            1.0
        } else {
            exalted_worth
        }
    })
    .with_context(|| {
        format!(
            "{league} traded no Chaos Orb in GGG's last {} hours",
            hours.len()
        )
    })?;
    let worth = |currency: &str| match currency {
        DIVINE => 1.0,
        EXALTED => exalted_worth,
        _ => chaos.divine_value,
    };

    let mut prices = HashMap::new();
    for (&id, &group) in &trades.groups {
        let Some(valuation) = value(&trades.with_cores(id, &[DIVINE, EXALTED, CHAOS]), worth)
        else {
            continue;
        };
        // The Divine Orb is worth one by definition and the Exalted Orb its rate; both read in
        // the other, by the pair that sets the rate.
        let (divine_value, hours) = match id {
            DIVINE => (1.0, anchor.hours),
            EXALTED => (exalted_worth, anchor.hours),
            _ => (valuation.divine_value, valuation.hours),
        };
        let busiest = valuation.busiest;
        prices.insert(
            id.to_owned(),
            MarketPrice {
                id: id.to_owned(),
                category: group.to_owned(),
                divine_value,
                hours,
                volume_divine: trades.per_hour.get(id).copied().unwrap_or(0.0) * divine_value,
                most_traded_with: busiest.currency.to_owned(),
                most_traded_rate: busiest.item_units / busiest.currency_units,
                change_7d: None,
                sparkline: Vec::new(),
                details_url: None,
            },
        );
    }
    Ok(Market {
        exalted_per_divine,
        chaos_per_divine: 1.0 / chaos.divine_value,
        hours: Some(anchor.hours),
        prices,
        scout: None,
    })
}

/// A market of poe2scout's prices and rates only, for when GGG's record is out of reach.
fn scout_market(scout: ScoutExchange) -> Result<Market> {
    let chaos = scout
        .value_in_divines(CHAOS)
        .context("poe2scout has no Chaos Orb price for the league")?;
    Ok(Market {
        exalted_per_divine: scout.exalted_per_divine,
        chaos_per_divine: 1.0 / chaos,
        hours: None,
        prices: HashMap::new(),
        scout: Some(scout),
    })
}

/// poe2scout's week of `trade_id` as the market card draws it: each day's change in percent
/// against the first day with a price, and the change over the week. poe2scout prices in Exalted
/// Orbs, so the Exalted Orb's own week -- flat at one -- is the Divine Orb's turned over: its worth
/// in divines.
fn week(scout: &ScoutExchange, trade_id: &str) -> (Option<f64>, Vec<Option<f64>>) {
    let (days_of, turned) = if trade_id == EXALTED {
        (DIVINE, true)
    } else {
        (trade_id, false)
    };
    let Some(item) = scout.items.get(days_of) else {
        return (None, Vec::new());
    };
    let days: Vec<Option<f64>> = item
        .week
        .iter()
        .map(|day| day.map(|price| if turned { 1.0 / price } else { price }))
        .collect();
    let mut priced = days.iter().flatten();
    let (Some(&first), Some(_)) = (priced.next(), priced.next()) else {
        return (None, Vec::new());
    };
    let points: Vec<Option<f64>> = days
        .iter()
        .map(|day| day.map(|price| (price / first - 1.0) * 100.0))
        .collect();
    (points.iter().flatten().last().copied(), points)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use futures::executor::block_on;
    use futures::future::BoxFuture;
    use http_client::http::HeaderValue;
    use http_client::{Request, Response, Url};

    use super::*;

    /// GGG's record of 10:00-11:00 UTC 2026-09-23 cut down to Forbidden Rites' markets among the
    /// Divine, Exalted and Chaos Orbs, the Orb of Annulment and the Omen of Light, plus a Hawk Idol
    /// market (not on the trade site's list), a market that traded nothing and Standard's Divine
    /// Orb market.
    const HOUR_10: &str = include_str!("../tests/fixtures/cx-hour-1790157600.json");
    const AT_10: u64 = 1_790_157_600;
    const AT_09: u64 = AT_10 - HOUR;
    const AT_08: u64 = AT_09 - HOUR;

    /// Forbidden Rites' markets of the same day's two hours before, as served (their other fields
    /// dropped): the Divine Orb's with the Exalted Orb, the Omen of Light's with the Exalted Orb,
    /// and at 09:00 a Mirror of Kalandra's with the Divine Orb.
    const HOUR_09: &str = r#"{"next_change_id":1790157600,"markets":[
        {"league":"Forbidden Rites","market_pair":["Metadata/Items/Currency/CurrencyModValues","Metadata/Items/Currency/CurrencyAddModToRare"],"volume_traded":{"Metadata/Items/Currency/CurrencyModValues":7241,"Metadata/Items/Currency/CurrencyAddModToRare":3581706}},
        {"league":"Forbidden Rites","market_pair":["Metadata/Items/Currency/OmenOnAnnulRemoveAbyssMod","Metadata/Items/Currency/CurrencyAddModToRare"],"volume_traded":{"Metadata/Items/Currency/OmenOnAnnulRemoveAbyssMod":4,"Metadata/Items/Currency/CurrencyAddModToRare":13056}},
        {"league":"Forbidden Rites","market_pair":["Metadata/Items/Currency/CurrencyDuplicate","Metadata/Items/Currency/CurrencyModValues"],"volume_traded":{"Metadata/Items/Currency/CurrencyDuplicate":28,"Metadata/Items/Currency/CurrencyModValues":116811}}]}"#;
    const HOUR_08: &str = r#"{"next_change_id":1790154000,"markets":[
        {"league":"Forbidden Rites","market_pair":["Metadata/Items/Currency/CurrencyModValues","Metadata/Items/Currency/CurrencyAddModToRare"],"volume_traded":{"Metadata/Items/Currency/CurrencyModValues":5958,"Metadata/Items/Currency/CurrencyAddModToRare":2898009}},
        {"league":"Forbidden Rites","market_pair":["Metadata/Items/Currency/OmenOnAnnulRemoveAbyssMod","Metadata/Items/Currency/CurrencyAddModToRare"],"volume_traded":{"Metadata/Items/Currency/OmenOnAnnulRemoveAbyssMod":11,"Metadata/Items/Currency/CurrencyAddModToRare":35652}}]}"#;

    /// poe2scout's Currency category that day cut down to the Divine, Exalted and Chaos Orbs, the
    /// Orb of Annulment and the Regal Shard.
    const SCOUT_CURRENCY: &str = include_str!("../tests/fixtures/scout-currencies-currency.json");
    /// poe2scout's league list and category list that day, cut down.
    const SCOUT_LEAGUES: &str = r#"[{"Value":"Forbidden Rites","ShortName":"forbiddenrites","IsCurrent":true,"DivinePrice":491.84062373629854,"ChaosDivinePrice":7.759840276322102,"BaseCurrencyApiId":"exalted"}]"#;
    const SCOUT_CATEGORIES: &str = r#"{"CurrencyCategories":[{"CurrencyCategoryId":21,"ApiId":"currency","Label":"Currency"},{"CurrencyCategoryId":27,"ApiId":"ritual","Label":"Ritual Omens"}],"UniqueCategories":[]}"#;

    const LEAGUE: &str = "Forbidden Rites";

    fn hour(body: &str) -> Arc<CxHour> {
        Arc::new(serde_json::from_str(body).expect("the hour parses"))
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1e-9 * expected.abs().max(1.0),
            "{actual} != {expected}"
        );
    }

    /// GGG's CDN with the hours in `hours` (any other answers 404, as one not out yet does) and
    /// poe2scout with the day's Currency category (its Ritual category fails); with `down`, every
    /// request of that host fails.
    struct Stub {
        hours: Vec<(u64, &'static str)>,
        cdn_down: bool,
        scout_down: bool,
    }

    impl HttpClient for Stub {
        fn user_agent(&self) -> Option<&HeaderValue> {
            None
        }

        fn proxy(&self) -> Option<&Url> {
            None
        }

        fn send(
            &self,
            request: Request<AsyncBody>,
        ) -> BoxFuture<'static, Result<Response<AsyncBody>>> {
            let uri = request.uri();
            let league = "/poe2/Leagues/Forbidden%20Rites";
            let (status, body) = match (uri.host(), uri.path(), uri.query()) {
                (Some("web.poecdn.com"), _, _) if self.cdn_down => (503, "Service Unavailable"),
                (Some("web.poecdn.com"), path, None) => {
                    let start = path
                        .strip_prefix("/api/currency-exchange/poe2/")
                        .and_then(|start| start.parse::<u64>().ok());
                    match self.hours.iter().find(|&&(at, _)| Some(at) == start) {
                        Some(&(_, body)) => (200, body),
                        None => (404, r#"{"markets":[]}"#),
                    }
                }
                (Some("api.poe2scout.com"), _, _) if self.scout_down => (500, "down"),
                (Some("api.poe2scout.com"), "/poe2/Leagues", None) => (200, SCOUT_LEAGUES),
                (Some("api.poe2scout.com"), path, None)
                    if path == format!("{league}/Items/Categories") =>
                {
                    (200, SCOUT_CATEGORIES)
                }
                (Some("api.poe2scout.com"), path, Some(query))
                    if path == format!("{league}/Currencies/ByCategory")
                        && query.starts_with("Category=currency&Page=1&") =>
                {
                    (200, SCOUT_CURRENCY)
                }
                _ => (500, "Internal Server Error"),
            };
            let response = Response::builder()
                .status(status)
                .body(AsyncBody::from(body))
                .map_err(anyhow::Error::from);
            Box::pin(async move { response })
        }
    }

    fn client(stub: Stub) -> Arc<dyn HttpClient> {
        Arc::new(stub)
    }

    fn day_hours() -> Vec<(u64, &'static str)> {
        vec![(AT_10, HOUR_10), (AT_09, HOUR_09), (AT_08, HOUR_08)]
    }

    /// A fresh directory path under the system temp dir, removed on drop (this crate has no
    /// `tempfile` dependency).
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new() -> ScratchDir {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            ScratchDir(
                std::env::temp_dir()
                    .join(format!("poe2-oracle-cx-test-{}-{n}", std::process::id())),
            )
        }

        /// The hours kept in the folder, oldest first.
        fn kept(&self) -> Vec<u64> {
            let mut kept: Vec<u64> = std::fs::read_dir(&self.0)
                .into_iter()
                .flatten()
                .filter_map(|entry| kept_hour_start(entry.ok()?.file_name().to_str()?))
                .collect();
            kept.sort_unstable();
            kept
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            read_hours().retain(|path, _| path.parent() != Some(self.0.as_path()));
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_saved_hour_values_everything_in_divines_through_the_core_pairs() {
        let market = build_market(LEAGUE, &[(AT_10, hour(HOUR_10))]).expect("the hour prices");
        let ten_to_eleven = TradedHours {
            start: AT_10,
            end: AT_10 + HOUR,
        };
        // 6526 divines went for 3,205,461 exalted -- Standard's 245 for 122,236 aren't mixed in.
        let exalted_per_divine = 3_205_461.0 / 6526.0;
        assert_close(market.exalted_per_divine, exalted_per_divine);
        assert_eq!(market.hours, Some(ten_to_eleven));
        // The Chaos Orb by its two pairs: 195,452 divines and 627,490 exalted for 1,526,644 chaos.
        let chaos = (195_452.0 + 627_490.0 / exalted_per_divine) / (1_516_744.0 + 9900.0);
        assert_close(market.chaos_per_divine, 1.0 / chaos);
        assert_close(market.value_in_divines("chaos").unwrap(), chaos);

        // The Omen of Light by its three: divines, exalted and chaos paid over omens bought.
        let omen = market.price("omen-of-light").expect("the omen traded");
        let paid = 58_649.0 + 20_823.0 / exalted_per_divine + 151_293.0 * chaos;
        assert_close(omen.divine_value, paid / (8483.0 + 7.0 + 2906.0));
        assert_eq!(omen.hours, ten_to_eleven);
        assert_eq!(omen.category, "Ritual");
        // Its busiest pair is the Divine Orb's: 8483 omens for 58,649 divines.
        assert_eq!(omen.most_traded_with, "divine");
        assert_close(omen.most_traded_rate, 8483.0 / 58_649.0);
        assert_close(omen.volume_divine, 11_396.0 * omen.divine_value);

        // The core currencies read their own rates.
        assert_eq!(market.price("divine").unwrap().divine_value, 1.0);
        assert_close(
            market.price("exalted").unwrap().divine_value,
            1.0 / exalted_per_divine,
        );
        assert_close(market.price("annul").unwrap().divine_value, {
            let paid = 19_858.0 + 185_355.0 / exalted_per_divine + 61_431.0 * chaos;
            paid / (26_402.0 + 629.0 + 10_784.0)
        });
        // A market that traded nothing prices nothing (the Hawk Idol's, off the trade site's list,
        // isn't read at all).
        assert_eq!(market.price("quipolatls-thesis"), None);
        assert_eq!(market.value_in_divines("quipolatls-thesis"), None);
    }

    #[test]
    fn busy_pairs_price_off_the_last_hour_and_thin_ones_reach_back() {
        let hours = [
            (AT_10, hour(HOUR_10)),
            (AT_09, hour(HOUR_09)),
            (AT_08, hour(HOUR_08)),
        ];
        let trades = LeagueTrades::read(LEAGUE, &hours);
        // The Divine Orb's thousands of units an hour: 10:00 alone, however different 09:00 was.
        let anchor = trades.with_core(DIVINE, EXALTED).unwrap();
        assert_eq!(
            (anchor.item_units, anchor.currency_units),
            (6526.0, 3_205_461.0)
        );
        assert_eq!(
            anchor.hours,
            TradedHours {
                start: AT_10,
                end: AT_10 + HOUR
            }
        );
        // Omens of Light for exalted: 7, then 4, then 11 -- the third hour brings it to 20.
        let thin = trades.with_core("omen-of-light", EXALTED).unwrap();
        assert_eq!((thin.item_units, thin.currency_units), (22.0, 69_531.0));
        assert_eq!(
            thin.hours,
            TradedHours {
                start: AT_08,
                end: AT_10 + HOUR
            }
        );

        let market = build_market(LEAGUE, &hours).expect("the hours price");
        assert_close(market.exalted_per_divine, 3_205_461.0 / 6526.0);
        // The omen's own market is its Divine Orb pair, one hour deep: the exalted pair's three
        // hours are left out of its value, and the hours shown are the one it was valued from.
        let chaos = market.value_in_divines("chaos").unwrap();
        let omen = market.price("omen-of-light").unwrap();
        assert_close(
            omen.divine_value,
            (58_649.0 + 151_293.0 * chaos) / (8483.0 + 2906.0),
        );
        assert_eq!(
            omen.hours,
            TradedHours {
                start: AT_10,
                end: AT_10 + HOUR
            }
        );
        // A mirror sold only at 09:00 is priced from that hour, and says so.
        let mirror = market.price("mirror").unwrap();
        assert_close(mirror.divine_value, 116_811.0 / 28.0);
        assert_eq!(
            mirror.hours,
            TradedHours {
                start: AT_09,
                end: AT_10
            }
        );
        // Turnover counts every hour of the window.
        assert_close(mirror.volume_divine, 28.0 / 3.0 * mirror.divine_value);
    }

    #[test]
    fn poe2scout_adds_the_week_and_the_page_and_prices_what_the_hours_miss() {
        let cache_dir = ScratchDir::new();
        let client = client(Stub {
            hours: day_hours(),
            cdn_down: false,
            scout_down: false,
        });
        // 11:04 UTC: the 11:00 hour is still under way and never asked for.
        let market = block_on(fetch_market_at(
            &client,
            LEAGUE,
            &cache_dir.0,
            AT_10 + HOUR + 276,
        ))
        .expect("the market loads");

        let annul = market.price("annul").expect("GGG priced it");
        assert_eq!(
            annul.details_url.as_deref(),
            Some(
                "https://poe2scout.com/poe2/forbiddenrites/economy/currencies/currency/292/Orb-of-Annulment"
            )
        );
        // poe2scout's week, oldest day first: 354.6 exalted on 09-17, 368.1 today.
        assert_eq!(annul.sparkline.len(), 7);
        assert_eq!(annul.sparkline[0], Some(0.0));
        assert_close(
            annul.change_7d.unwrap(),
            (368.10458 / 354.59274 - 1.0) * 100.0,
        );
        // The Exalted Orb's week is the Divine Orb's turned over: 485.97, then 503.69 per divine.
        let exalted = market.price("exalted").unwrap();
        assert_close(
            exalted.change_7d.unwrap(),
            (485.97095 / 503.69006 - 1.0) * 100.0,
        );
        // The omen isn't in poe2scout's Currency category (and its Ritual one failed): no week, no
        // page, GGG's price all the same.
        let omen = market.price("omen-of-light").unwrap();
        assert_eq!(
            (
                omen.change_7d,
                omen.sparkline.len(),
                omen.details_url.as_deref()
            ),
            (None, 0, None)
        );

        // No Regal Shard traded in the window: poe2scout's price stands in, at its own rate.
        assert_eq!(market.price("regal-shard"), None);
        assert_eq!(
            market.scout_price("regal-shard"),
            Some((0.16666667, PriceUnit::Exalted))
        );
        assert_close(
            market.value_in_divines("regal-shard").unwrap(),
            0.16666667 / 491.84064,
        );
        // GGG's rates, not poe2scout's.
        assert_close(market.exalted_per_divine, 3_205_461.0 / 6526.0);
    }

    #[test]
    fn without_ggg_s_record_poe2scout_prices_the_exchange_and_without_both_nothing_does() {
        let cache_dir = ScratchDir::new();
        let cdn_down = client(Stub {
            hours: day_hours(),
            cdn_down: true,
            scout_down: false,
        });
        let market = block_on(fetch_market_at(
            &cdn_down,
            LEAGUE,
            &cache_dir.0,
            AT_10 + HOUR,
        ))
        .expect("poe2scout stands in");
        assert_eq!(market.hours, None);
        assert_eq!(market.exalted_per_divine, 491.84064);
        assert_close(market.chaos_per_divine, 491.84064 / 63.382828);
        assert_eq!(market.price("annul"), None);
        assert_eq!(
            market.scout_price("annul"),
            Some((366.1509, PriceUnit::Exalted))
        );

        let empty_dir = ScratchDir::new();
        let all_down = client(Stub {
            hours: day_hours(),
            cdn_down: true,
            scout_down: true,
        });
        assert!(
            block_on(fetch_market_at(
                &all_down,
                LEAGUE,
                &empty_dir.0,
                AT_10 + HOUR
            ))
            .is_err()
        );
    }

    #[test]
    fn kept_hours_price_the_league_offline_and_leave_with_the_window() {
        let cache_dir = ScratchDir::new();
        let online = client(Stub {
            hours: day_hours(),
            cdn_down: false,
            scout_down: true,
        });
        // 12:10 UTC: the 11:00 hour isn't out yet, so the window is the three before it.
        let first = block_on(fetch_market_at(
            &online,
            LEAGUE,
            &cache_dir.0,
            AT_10 + 2 * HOUR + 600,
        ))
        .expect("the market loads");
        assert_eq!(cache_dir.kept(), [AT_08, AT_09, AT_10]);

        // Restarted offline: the kept hours price the league as before.
        read_hours().retain(|path, _| path.parent() != Some(cache_dir.0.as_path()));
        let offline = client(Stub {
            hours: Vec::new(),
            cdn_down: true,
            scout_down: true,
        });
        let again = block_on(fetch_market_at(
            &offline,
            LEAGUE,
            &cache_dir.0,
            AT_10 + 2 * HOUR + 600,
        ))
        .expect("the kept hours price it");
        assert_eq!(again, first);

        // A day later, still offline: the newest kept hours stand in however old.
        let later = block_on(fetch_market_at(
            &offline,
            LEAGUE,
            &cache_dir.0,
            AT_10 + 26 * HOUR,
        ))
        .expect("the kept hours price it");
        assert_eq!(later.hours, first.hours);

        // Back online at 13:05, when 11:00 and 12:00 are out: 08:00 drops out of the window.
        let next = client(Stub {
            hours: vec![
                (AT_10 + 2 * HOUR, HOUR_08),
                (AT_10 + HOUR, HOUR_09),
                (AT_10, HOUR_10),
            ],
            cdn_down: false,
            scout_down: true,
        });
        let moved = block_on(fetch_market_at(
            &next,
            LEAGUE,
            &cache_dir.0,
            AT_10 + 3 * HOUR + 300,
        ))
        .expect("the market loads");
        assert_eq!(cache_dir.kept(), [AT_10, AT_10 + HOUR, AT_10 + 2 * HOUR]);
        assert_eq!(
            moved.hours,
            Some(TradedHours {
                start: AT_10 + 2 * HOUR,
                end: AT_10 + 3 * HOUR
            })
        );
    }

    #[test]
    fn an_exchange_items_table_with_a_malformed_row_is_refused() {
        let good = "Metadata/Items/Currency/CurrencyModValues\tdivine\tCurrency\n\
            Metadata/Items/Currency/CurrencyAddModToRare\texalted\tCurrency\n";
        let items = read_exchange_items(good).expect("a well-formed table");
        let divine = &items.0["Metadata/Items/Currency/CurrencyModValues"];
        assert_eq!((divine.trade_id, divine.group), ("divine", "Currency"));

        for (bad, why) in [
            (
                "Metadata/Items/Currency/CurrencyModValues\tdivine\n",
                "2 fields",
            ),
            (
                "Metadata/Items/Currency/CurrencyModValues\tdivine\tCurrency\tmore\n",
                "4 fields",
            ),
            (
                "Metadata/Items/Currency/CurrencyModValues\t\tCurrency\n",
                "no trade id",
            ),
            ("", "no rows"),
        ] {
            assert!(read_exchange_items(bad).is_err(), "{why}");
        }
        let twice = format!("{good}Metadata/Items/Currency/CurrencyModValues\tchaos\tCurrency\n");
        assert!(read_exchange_items(&twice).is_err(), "an item listed twice");
    }

    #[test]
    fn a_pack_table_comes_too_late_once_the_built_in_one_is_read() {
        // Whatever ran first, the built-in table is in use from here.
        LazyLock::force(&EXCHANGE_ITEMS);
        let pack = read_exchange_items("Metadata/Items/Currency/A\ta\tCurrency\n").unwrap();
        assert_eq!(use_exchange_items(pack), Err(TableInUse));
        assert!(EXCHANGE_ITEMS.len() > 1, "the built-in table stays");
    }
}
