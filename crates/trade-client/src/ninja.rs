//! poe.ninja's PoE2 Currency Exchange market: what poe.ninja's economy pages list for everything
//! PoE2 trades on its in-game Currency Exchange -- currency, fragments, omens, runes, essences,
//! catalysts, soul cores... -- priced in divines, with each row's "Last 7 days" chart,
//! "Volume / Hour" and "Most Popular" pair.
//!
//! Verified live 2026-09-22 on league Forbidden Rites (653 lines across the 14 categories):
//! - `GET https://poe.ninja/poe2/api/economy/exchange/current/overview?league={league}&type={type}`
//!   (no auth, `cache-control: public, max-age=1800`) answers `{"core":{"items":[...],"rates":
//!   {"exalted":493.9,"chaos":8.16},"primary":"divine","secondary":"chaos"},"lines":[{"id":"alch",
//!   "primaryValue":0.006083,"volumePrimaryValue":291.7,"maxVolumeCurrency":"divine",
//!   "maxVolumeRate":164.4,"sparkline":{"totalChange":30.08,"data":[-3.17,18.35,39.41,43.72,82.64,
//!   51.48,30.08]}},...],"items":[{"id":"alch","name":"Orb of Alchemy","image":"/gen/image/...",
//!   "category":"Currency","detailsId":"orb-of-alchemy"},...]}`; `lines` and `items` pair up by
//!   `id`, and every category's `core` is the same.
//! - The categories are poe.ninja's own exchange pages (`CATEGORIES`), whose API type, URL and
//!   title differ: omens are type `Ritual`, catalysts `Breach`, liquid emotions `Delirium`,
//!   abyssal bones `Abyss`.
//! - A line's `id` is the trade site's `/api/trade2/data/static` id: 645 of the 653 are, each
//!   under the same English name as the trade `text`. The other eight (Raven's Reflection, The
//!   Triskelion Reforged, Shattered Triskelion, Eonyr's Thunder, Helbrym's Hide, Stoat/Hawk/Panther
//!   Idol) are not on the trade site's list at all.
//! - An item's page is `https://poe.ninja/poe2/economy/{league slug}/{page}/{detailsId}` (the
//!   site's `overviewDetails` route): `.../forbiddenrites/omens/omen-of-abyssal-echoes` answers 200
//!   titled "Omen of Abyssal Echoes - Forbidden Rites - ...". `detailsId` differs from `id` on 70
//!   lines (`alch` is `orb-of-alchemy`).
//! - `league` is the trade league id with spaces as `%20`: `Standard` and `HC Forbidden Rites`
//!   answer like `Forbidden Rites`, while `Hardcore` and an unknown league answer 200 with no lines
//!   and empty `core.rates` -- which [`fetch_market`] reports as an error.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures::future::{join, join_all};
use http_client::{AsyncBody, HttpClient};
use serde::{Deserialize, Serialize};

use crate::cache::{league_file_name, load_or_fetch};
use crate::rates::PriceUnit;
use crate::{checked_body, urlencoding_space};

const API_BASE_URL: &str = "https://poe.ninja/poe2/api";
const PAGE_BASE_URL: &str = "https://poe.ninja/poe2/economy";

/// poe.ninja serves every overview with `max-age=1800` (2026-09-22): a fresher copy of ours would
/// only download the same snapshot again.
const MARKET_MAX_AGE: Duration = Duration::from_secs(30 * 60);

/// Trade ids of poe.ninja's core currencies (`core.items`), the ones `core.rates` quotes.
pub(crate) const DIVINE: &str = "divine";
pub(crate) const EXALTED: &str = "exalted";
const CHAOS: &str = "chaos";

/// EE2 adopts poe.ninja's exchange rates only once some core rate reaches 10 (`Prices.ts`
/// `load()`): a fresh league's snapshot with next to no volume isn't a price list yet.
const MIN_PLAUSIBLE_CORE_RATE: f64 = 10.0;

/// EE2's `autoCurrency` cutover (`Prices.ts`): a price above 0.94 div reads in divines, anything
/// cheaper in exalted.
pub(crate) const DIVINE_UNIT_CUTOVER: f64 = 0.94;

/// One of poe.ninja's exchange pages.
struct Category {
    /// The overview API's `type`.
    api_type: &'static str,
    /// The page's URL segment.
    page: &'static str,
    /// The page's sidebar title.
    title: &'static str,
}

/// poe.ninja's PoE2 exchange pages in its sidebar's order: the "General" group of the page table
/// in the site's own JS bundle (read 2026-09-22), its only pages with the exchange view. Every
/// one answered lines for Forbidden Rites that day.
const CATEGORIES: [Category; 14] = [
    Category {
        api_type: "Currency",
        page: "currency",
        title: "Currency",
    },
    Category {
        api_type: "Fragments",
        page: "fragments",
        title: "Fragments",
    },
    Category {
        api_type: "Abyss",
        page: "abyssal-bones",
        title: "Abyssal Bones",
    },
    Category {
        api_type: "UncutGems",
        page: "uncut-gems",
        title: "Uncut Gems",
    },
    Category {
        api_type: "LineageSupportGems",
        page: "lineage-support-gems",
        title: "Lineage Gems",
    },
    Category {
        api_type: "Essences",
        page: "essences",
        title: "Essences",
    },
    Category {
        api_type: "SoulCores",
        page: "soul-cores",
        title: "Soul Cores",
    },
    Category {
        api_type: "Idols",
        page: "idols",
        title: "Idols",
    },
    Category {
        api_type: "Runes",
        page: "runes",
        title: "Runes",
    },
    Category {
        api_type: "Ritual",
        page: "omens",
        title: "Omens",
    },
    Category {
        api_type: "Expedition",
        page: "expedition",
        title: "Expedition",
    },
    Category {
        api_type: "Delirium",
        page: "liquid-emotions",
        title: "Liquid Emotions",
    },
    Category {
        api_type: "Breach",
        page: "breach-catalyst",
        title: "Catalysts",
    },
    Category {
        api_type: "Verisium",
        page: "verisium",
        title: "Verisium",
    },
];

/// One item's row on its poe.ninja exchange page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketPrice {
    /// The trade site's `/data/static` id for the item (see the module doc).
    pub id: String,
    /// poe.ninja's English name, the trade site's English `text` for the item.
    pub name: String,
    /// The sidebar title of the page listing the item: "Currency", "Omens", "Catalysts", ...
    pub category: String,
    /// One unit's worth in Divine Orbs (`primaryValue`).
    pub divine_value: f64,
    /// The page's "Volume / Hour": "volume traded per hour displayed as normalized value", in
    /// divines (`volumePrimaryValue`). 0 when poe.ninja reports none (its page prints "-"; not
    /// seen live).
    pub volume_divine: f64,
    /// The change over the last 7 days in percent (`sparkline.totalChange`); `None` where the
    /// page reads "Not enough data".
    pub change_7d: Option<f64>,
    /// The "Last 7 days" chart (`sparkline.data`), oldest first: percent changes, seven points on
    /// every line seen live, the last one always equal to `change_7d`. A `None` point is a gap --
    /// poe.ninja breaks its line there and spaces the points by index, its y axis spanning at
    /// least -5..+5 %. 8 of the 653 lines had gaps. Empty where the page reads "Not enough data".
    pub sparkline: Vec<Option<f64>>,
    /// The "Most Popular" pair's other side (`maxVolumeCurrency`): the trade id of the core
    /// currency (divine, exalted or chaos) the item trades against most.
    pub most_traded_with: Option<String>,
    /// That pair's rate (`maxVolumeRate`): units of this item one `most_traded_with` buys --
    /// alch 164.4 per divine, a Warding Rune of Desperation 2 per chaos; on all 653 lines it
    /// agrees with `divine_value` within half a percent. The page flips it where that reads
    /// better: a mirror's 0.0002414 per divine is its "4.1k div ⇆ 1.0 mirror".
    pub most_traded_rate: Option<f64>,
    /// The item's poe.ninja page.
    pub details_url: String,
}

/// One league's poe.ninja market: every exchange page's prices by trade id, plus the core exchange
/// rates. `Serialize`/`Deserialize` for [`load_or_fetch`]'s disk cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Market {
    /// Exalted Orbs one Divine Orb buys (`core.rates.exalted`, 493.9 on 2026-09-22).
    pub exalted_per_divine: f64,
    /// Chaos Orbs one Divine Orb buys (`core.rates.chaos`, 8.16 on 2026-09-22).
    pub chaos_per_divine: f64,
    /// Keyed by trade id; every `divine_value` is finite and positive, as are both rates above.
    prices: HashMap<String, MarketPrice>,
}

impl Market {
    /// poe.ninja's row for the item with trade id `trade_id`; `None` when no page lists it.
    pub fn price(&self, trade_id: &str) -> Option<&MarketPrice> {
        self.prices.get(trade_id)
    }

    /// One unit of `currency` (a trade id) in divines; `None` when poe.ninja doesn't price it.
    /// The core currencies always have a value, whichever pages failed to load: a Divine Orb is
    /// exactly 1, and Exalted and Chaos Orbs without a line of their own go by the core rates.
    pub fn value_in_divines(&self, currency: &str) -> Option<f64> {
        if currency == DIVINE {
            return Some(1.0);
        }
        self.prices
            .get(currency)
            .map(|price| price.divine_value)
            .or_else(|| match currency {
                EXALTED => Some(1.0 / self.exalted_per_divine),
                CHAOS => Some(1.0 / self.chaos_per_divine),
                _ => None,
            })
    }

    /// `divines` in the unit EE2's `autoCurrency` would show it in: divines above 0.94, exalted at
    /// or below -- the rule behind EE2's per-listing normalized prices, so an estimate and a
    /// normalized listing read alike.
    pub fn in_display_unit(&self, divines: f64) -> (f64, PriceUnit) {
        match self.value_in_divines(EXALTED) {
            Some(exalted) if divines <= DIVINE_UNIT_CUTOVER => {
                (divines / exalted, PriceUnit::Exalted)
            }
            _ => (divines, PriceUnit::Divine),
        }
    }
}

/// `league`'s market, from `cache_dir` while under 30 minutes old, else downloaded -- every
/// exchange page at once -- and cached. `league` is the trade league id; `cache_dir` is the app's
/// cache root, where each league gets its own file.
///
/// A page that fails to load, or whose snapshot has no plausible exchange rate yet, is logged and
/// left out; only when every page is does the download fail. A failed download falls back to the
/// league's last cached market however old (see [`load_or_fetch`]), so the error surfaces only
/// with no cache at all -- e.g. for `Hardcore`, which poe.ninja has no prices for.
pub async fn fetch_market(
    client: &Arc<dyn HttpClient>,
    league: &str,
    cache_dir: &Path,
) -> Result<Market> {
    let cache_path = cache_dir.join(league_file_name("ninja-market", league));
    // `download_market` sends nothing until awaited, i.e. only on a cache miss.
    load_or_fetch(&cache_path, MARKET_MAX_AGE, download_market(client, league)).await
}

async fn download_market(client: &Arc<dyn HttpClient>, league: &str) -> Result<Market> {
    let overviews = join_all(
        CATEGORIES
            .iter()
            .map(|category| download_overview(client, league, category)),
    );
    let (overviews, index_state) = join(overviews, download_index_state(client)).await;
    let index_state = index_state
        .inspect_err(|err| {
            eprintln!(
                "poe2-oracle: poe.ninja's league list is unavailable, guessing {league}'s page \
                 slug: {err:#}"
            )
        })
        .ok();
    let league_slug = league_slug(league, index_state.as_ref());
    build_market(&league_slug, CATEGORIES.iter().zip(overviews))
}

async fn download_overview(
    client: &Arc<dyn HttpClient>,
    league: &str,
    category: &Category,
) -> Result<String> {
    let url = format!(
        "{API_BASE_URL}/economy/exchange/current/overview?league={}&type={}",
        urlencoding_space(league),
        category.api_type
    );
    let request = client.get(&url, AsyncBody::default(), true);
    checked_body(request, "GET", &url, None, "poe.ninja overview").await
}

async fn download_index_state(client: &Arc<dyn HttpClient>) -> Result<IndexState> {
    let url = format!("{API_BASE_URL}/data/index-state");
    let request = client.get(&url, AsyncBody::default(), true);
    let body = checked_body(request, "GET", &url, None, "poe.ninja index state").await?;
    serde_json::from_str(&body).with_context(|| format!("parsing {url}"))
}

/// Merges each category's overview body -- or the error loading it -- into one market (see
/// [`fetch_market`] for what is left out). The core rates come from the first category that
/// parsed.
fn build_market<'a>(
    league_slug: &str,
    overviews: impl IntoIterator<Item = (&'a Category, Result<String>)>,
) -> Result<Market> {
    let mut market: Option<Market> = None;
    let mut first_error = None;
    for (category, body) in overviews {
        let parsed = body.and_then(|body| {
            parse_overview(&body, category, league_slug)
                .with_context(|| format!("parsing poe.ninja's {} overview", category.api_type))
        });
        match parsed {
            Ok(parsed) => {
                let market = market.get_or_insert_with(|| Market {
                    exalted_per_divine: parsed.exalted_per_divine,
                    chaos_per_divine: parsed.chaos_per_divine,
                    prices: HashMap::new(),
                });
                for price in parsed.prices {
                    // No id was on two pages on 2026-09-22; should one be, the first page keeps it.
                    market.prices.entry(price.id.clone()).or_insert(price);
                }
            }
            Err(err) => {
                eprintln!(
                    "poe2-oracle: leaving poe.ninja's {} out of the market: {err:#}",
                    category.title
                );
                first_error.get_or_insert(err);
            }
        }
    }
    match (market, first_error) {
        (Some(market), _) => Ok(market),
        (None, Some(err)) => Err(err.context("poe.ninja priced no exchange page")),
        (None, None) => bail!("no poe.ninja exchange page to price"),
    }
}

/// One category's overview, rebased on the Divine Orb.
struct ParsedOverview {
    exalted_per_divine: f64,
    chaos_per_divine: f64,
    prices: Vec<MarketPrice>,
}

fn parse_overview(body: &str, category: &Category, league_slug: &str) -> Result<ParsedOverview> {
    let overview: Overview = serde_json::from_str(body).context("parsing overview JSON")?;
    let core = &overview.core;
    let usable = |rate: f64| rate.is_finite() && rate > 0.0;
    // `core.primary` is the unit of every value -- the divine in every league with prices on
    // 2026-09-22, though the empty `Hardcore` snapshot names the exalted -- so rebase on the
    // divine whatever it is.
    let rates = core.per_primary(DIVINE).and_then(|divines_per_primary| {
        let exalted_per_divine = core.per_primary(EXALTED)? / divines_per_primary;
        let chaos_per_divine = core.per_primary(CHAOS)? / divines_per_primary;
        Some((divines_per_primary, exalted_per_divine, chaos_per_divine))
    });
    let Some((divines_per_primary, exalted_per_divine, chaos_per_divine)) =
        rates.filter(|&(divines, exalted, chaos)| {
            usable(divines)
                && usable(exalted)
                && usable(chaos)
                && exalted.max(chaos) >= MIN_PLAUSIBLE_CORE_RATE
        })
    else {
        // An error, not an empty market: `load_or_fetch` then keeps the league's last good one.
        bail!(
            "poe.ninja has no plausible exchange rate yet (core rates {:?} per {})",
            core.rates,
            core.primary
        );
    };

    let mut items: HashMap<String, Item> = overview
        .items
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect();
    let prices = overview
        .lines
        .into_iter()
        .filter_map(|line| {
            let item = items.remove(&line.id)?;
            let divine_value = line.primary_value? * divines_per_primary;
            if !usable(divine_value) {
                return None;
            }
            let (change_7d, sparkline) = match line.sparkline {
                // poe.ninja's own cell reads "Not enough data" for these (its sparkline
                // component, 2026-09-22).
                Some(sparkline)
                    if !(sparkline.data.first() == Some(&None)
                        && sparkline.total_change == Some(0.0)) =>
                {
                    (sparkline.total_change, sparkline.data)
                }
                _ => (None, Vec::new()),
            };
            let (most_traded_with, most_traded_rate) =
                match (line.max_volume_currency, line.max_volume_rate) {
                    (Some(currency), Some(rate)) if usable(rate) => (Some(currency), Some(rate)),
                    _ => (None, None),
                };
            Some(MarketPrice {
                details_url: format!(
                    "{PAGE_BASE_URL}/{league_slug}/{}/{}",
                    category.page, item.details_id
                ),
                id: line.id,
                name: item.name,
                category: category.title.to_owned(),
                divine_value,
                volume_divine: line.volume_primary_value.unwrap_or(0.0) * divines_per_primary,
                change_7d,
                sparkline,
                most_traded_with,
                most_traded_rate,
            })
        })
        .collect();
    Ok(ParsedOverview {
        exalted_per_divine,
        chaos_per_divine,
        prices,
    })
}

/// poe.ninja's URL segment for `league`: its own `url` for it in `GET /poe2/api/data/index-state`
/// (both league lists, as its pages search them), else its naming rule for every current league
/// on 2026-09-22 -- letters and digits in lowercase, an `HC ` prefix moved to an `hc` suffix
/// (`HC Forbidden Rites` is `forbiddenriteshc`). The list comes first because old leagues break
/// the rule (`Fate of the Vaal` is `vaal`).
fn league_slug(league: &str, index_state: Option<&IndexState>) -> String {
    let listed = index_state.and_then(|index_state| {
        index_state
            .economy_leagues
            .iter()
            .chain(&index_state.old_economy_leagues)
            .find(|listed| listed.name == league)
    });
    if let Some(listed) = listed {
        return listed.url.clone();
    }
    let (name, suffix) = match league.strip_prefix("HC ") {
        Some(name) => (name, "hc"),
        None => (league, ""),
    };
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .chain(suffix.chars())
        .collect()
}

#[derive(Deserialize)]
struct Overview {
    core: Core,
    lines: Vec<Line>,
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Core {
    /// Units of each other core currency one `primary` buys, e.g. `{"exalted":493.9,"chaos":8.16}`.
    rates: HashMap<String, f64>,
    primary: String,
}

impl Core {
    /// Units of the core currency `currency` one `primary` buys.
    fn per_primary(&self, currency: &str) -> Option<f64> {
        if currency == self.primary {
            Some(1.0)
        } else {
            self.rates.get(currency).copied()
        }
    }
}

/// Every field but `id` is optional: poe.ninja's own table copes without each of them.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Line {
    id: String,
    primary_value: Option<f64>,
    volume_primary_value: Option<f64>,
    max_volume_currency: Option<String>,
    max_volume_rate: Option<f64>,
    sparkline: Option<Sparkline>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sparkline {
    total_change: Option<f64>,
    #[serde(default)]
    data: Vec<Option<f64>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    id: String,
    name: String,
    details_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexState {
    economy_leagues: Vec<ListedLeague>,
    #[serde(default)]
    old_economy_leagues: Vec<ListedLeague>,
}

#[derive(Deserialize)]
struct ListedLeague {
    name: String,
    url: String,
}

#[cfg(test)]
impl Market {
    /// A market of bare prices: `values` pairs trade ids with their worth in divines.
    pub(crate) fn from_values(
        exalted_per_divine: f64,
        chaos_per_divine: f64,
        values: &[(&str, f64)],
    ) -> Market {
        let prices = values
            .iter()
            .map(|&(id, divine_value)| {
                let price = MarketPrice {
                    id: id.to_owned(),
                    name: id.to_owned(),
                    category: String::new(),
                    divine_value,
                    volume_divine: 0.0,
                    change_7d: None,
                    sparkline: Vec::new(),
                    most_traded_with: None,
                    most_traded_rate: None,
                    details_url: String::new(),
                };
                (id.to_owned(), price)
            })
            .collect();
        Market {
            exalted_per_divine,
            chaos_per_divine,
            prices,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use futures::executor::block_on;
    use futures::future::BoxFuture;
    use http_client::http::HeaderValue;
    use http_client::{Request, Response, Url};

    use super::*;

    /// The live Forbidden Rites Currency overview of 2026-09-22 cut down to two lines, `core.items`
    /// and image paths dropped.
    const CURRENCY: &str = r#"{"core":{"rates":{"exalted":493.9,"chaos":8.16},"primary":"divine","secondary":"chaos"},
        "lines":[{"id":"alch","primaryValue":0.006083,"volumePrimaryValue":291.7,"maxVolumeCurrency":"divine","maxVolumeRate":164.4,"sparkline":{"totalChange":30.08,"data":[-3.17,18.35,39.41,43.72,82.64,51.48,30.08]}},
                 {"id":"divine","primaryValue":1,"volumePrimaryValue":227637,"maxVolumeCurrency":"chaos","maxVolumeRate":0.1225,"sparkline":{"totalChange":-12.36,"data":[-0.83,-1.64,-8.56,-9.04,-10.02,-11.16,-12.36]}}],
        "items":[{"id":"alch","name":"Orb of Alchemy","category":"Currency","detailsId":"orb-of-alchemy"},
                 {"id":"divine","name":"Divine Orb","category":"Currency","detailsId":"divine-orb"}]}"#;

    /// The same day's Runes overview cut the same way, down to a rune with gaps in its chart.
    const RUNES: &str = r#"{"core":{"rates":{"exalted":493.9,"chaos":8.16},"primary":"divine","secondary":"chaos"},
        "lines":[{"id":"adept-rune","primaryValue":0.01312,"volumePrimaryValue":0.05028,"maxVolumeCurrency":"exalted","maxVolumeRate":0.1544,"sparkline":{"totalChange":-61.58,"data":[-14.94,-18.48,22.94,16.1,-12.87,-39.27,-61.58]}},
                 {"id":"warding-rune-of-desperation","primaryValue":0.06127,"volumePrimaryValue":0.02042,"maxVolumeCurrency":"chaos","maxVolumeRate":2,"sparkline":{"totalChange":-50.0,"data":[null,null,-12.5,null,null,-66.67,-50.0]}}],
        "items":[{"id":"adept-rune","name":"Adept Rune","category":"Runes","detailsId":"adept-rune"},
                 {"id":"warding-rune-of-desperation","name":"Warding Rune of Desperation","category":"Runes","detailsId":"warding-rune-of-desperation"}]}"#;

    /// The same day's `index-state` league lists, cut down.
    const INDEX_STATE: &str = r#"{"economyLeagues":[{"name":"Forbidden Rites","url":"forbiddenrites","displayName":"Forbidden Rites","hardcore":false,"indexed":false}],
        "oldEconomyLeagues":[{"name":"Fate of the Vaal","url":"vaal","displayName":"Fate of the Vaal","hardcore":false,"indexed":false}]}"#;

    fn category(api_type: &str) -> &'static Category {
        CATEGORIES
            .iter()
            .find(|category| category.api_type == api_type)
            .expect("a known category")
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1e-9 * expected.abs().max(1.0),
            "{actual} != {expected}"
        );
    }

    /// poe.ninja with the league list and, for league `Fate of the Vaal` only, the overviews of
    /// the categories in `serves`; every other request is a server error.
    struct NinjaStub {
        serves: &'static [(&'static str, &'static str)],
    }

    impl HttpClient for NinjaStub {
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
            let body = match (uri.host(), uri.path(), uri.query()) {
                (Some("poe.ninja"), "/poe2/api/data/index-state", None) => Some(INDEX_STATE),
                (Some("poe.ninja"), "/poe2/api/economy/exchange/current/overview", Some(query)) => {
                    let api_type = query.strip_prefix("league=Fate%20of%20the%20Vaal&type=");
                    self.serves
                        .iter()
                        .find(|&&(served, _)| api_type == Some(served))
                        .map(|&(_, body)| body)
                }
                _ => None,
            };
            let response = match body {
                Some(body) => Response::builder().status(200).body(AsyncBody::from(body)),
                None => Response::builder()
                    .status(500)
                    .body(AsyncBody::from("Internal Server Error")),
            }
            .map_err(anyhow::Error::from);
            Box::pin(async move { response })
        }
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
                    .join(format!("poe2-oracle-ninja-test-{}-{n}", std::process::id())),
            )
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_real_currency_overview_reads_as_its_page_does() {
        let market = build_market(
            "forbiddenrites",
            [(category("Currency"), Ok(CURRENCY.to_owned()))],
        )
        .expect("the overview parses");
        assert_eq!(market.exalted_per_divine, 493.9);
        assert_eq!(market.chaos_per_divine, 8.16);
        assert_eq!(
            market.price("alch"),
            Some(&MarketPrice {
                id: "alch".to_owned(),
                name: "Orb of Alchemy".to_owned(),
                category: "Currency".to_owned(),
                divine_value: 0.006083,
                volume_divine: 291.7,
                change_7d: Some(30.08),
                sparkline: vec![
                    Some(-3.17),
                    Some(18.35),
                    Some(39.41),
                    Some(43.72),
                    Some(82.64),
                    Some(51.48),
                    Some(30.08)
                ],
                most_traded_with: Some("divine".to_owned()),
                most_traded_rate: Some(164.4),
                // The page goes by `detailsId`, not by the trade id.
                details_url:
                    "https://poe.ninja/poe2/economy/forbiddenrites/currency/orb-of-alchemy"
                        .to_owned(),
            })
        );
    }

    #[test]
    fn two_categories_merge_and_a_failed_one_is_left_out() {
        let cache_dir = ScratchDir::new();
        let client: Arc<dyn HttpClient> = Arc::new(NinjaStub {
            serves: &[("Currency", CURRENCY), ("Runes", RUNES)],
        });

        let market = block_on(fetch_market(&client, "Fate of the Vaal", &cache_dir.0))
            .expect("two categories answered");

        // Each on its own page, under the slug poe.ninja lists for the league -- no rule gives
        // `vaal`.
        let alch = market.price("alch").expect("Currency is in");
        assert_eq!(
            alch.details_url,
            "https://poe.ninja/poe2/economy/vaal/currency/orb-of-alchemy"
        );
        let rune = market
            .price("warding-rune-of-desperation")
            .expect("Runes is in");
        assert_eq!(rune.category, "Runes");
        assert_eq!(
            rune.details_url,
            "https://poe.ninja/poe2/economy/vaal/runes/warding-rune-of-desperation"
        );
        // Days without trades stay gaps where they were.
        assert_eq!(
            rune.sparkline,
            [
                None,
                None,
                Some(-12.5),
                None,
                None,
                Some(-66.67),
                Some(-50.0)
            ]
        );
        assert_eq!(rune.change_7d, Some(-50.0));
    }

    #[test]
    fn no_category_at_all_is_an_error() {
        // An empty market would be cached for half an hour over the league's last good one.
        let cache_dir = ScratchDir::new();
        let client: Arc<dyn HttpClient> = Arc::new(NinjaStub { serves: &[] });
        assert!(block_on(fetch_market(&client, "Fate of the Vaal", &cache_dir.0)).is_err());
    }

    #[test]
    fn a_line_without_a_chart_still_has_its_price() {
        let body = r#"{"core":{"rates":{"exalted":493.9,"chaos":8.16},"primary":"divine"},
            "lines":[{"id":"no-key","primaryValue":0.5,"volumePrimaryValue":3},
                     {"id":"null","primaryValue":0.5,"volumePrimaryValue":3,"sparkline":null},
                     {"id":"thin","primaryValue":0.5,"volumePrimaryValue":3,"sparkline":{"totalChange":0,"data":[null,null,null,null,null,null,0]}}],
            "items":[{"id":"no-key","name":"A","detailsId":"a"},{"id":"null","name":"B","detailsId":"b"},{"id":"thin","name":"C","detailsId":"c"}]}"#;
        let market = build_market(
            "forbiddenrites",
            [(category("Currency"), Ok(body.to_owned()))],
        )
        .expect("the overview parses");
        for id in ["no-key", "null", "thin"] {
            let price = market.price(id).expect("priced");
            assert_eq!(price.divine_value, 0.5, "{id}");
            assert_eq!((price.change_7d, price.sparkline.len()), (None, 0), "{id}");
        }
    }

    #[test]
    fn a_snapshot_quoted_in_exalted_is_rebased_on_the_divine() {
        let body = r#"{"core":{"rates":{"divine":0.002,"chaos":0.0165},"primary":"exalted"},
            "lines":[{"id":"x","primaryValue":250,"volumePrimaryValue":1000}],
            "items":[{"id":"x","name":"X","detailsId":"x"}]}"#;
        let market = build_market(
            "forbiddenrites",
            [(category("Currency"), Ok(body.to_owned()))],
        )
        .expect("the overview parses");
        assert_close(market.exalted_per_divine, 500.0);
        assert_close(market.chaos_per_divine, 8.25);
        let x = market.price("x").expect("priced");
        assert_close(x.divine_value, 0.5);
        assert_close(x.volume_divine, 2.0);
    }

    #[test]
    fn rejects_a_snapshot_without_a_plausible_exchange_rate() {
        // A brand-new league's first snapshot: accepting it would cache nonsense for half an hour.
        let body = CURRENCY.replace(
            r#""rates":{"exalted":493.9,"chaos":8.16}"#,
            r#""rates":{"exalted":1.2,"chaos":0.4}"#,
        );
        assert!(build_market("forbiddenrites", [(category("Currency"), Ok(body))]).is_err());
    }

    #[test]
    fn core_currencies_are_valued_without_their_own_lines() {
        // Currency failed to load: listings priced in divines, exalted or chaos still convert.
        let market = build_market(
            "forbiddenrites",
            [(category("Runes"), Ok(RUNES.to_owned()))],
        )
        .expect("the overview parses");
        assert_eq!(market.value_in_divines("divine"), Some(1.0));
        assert_close(market.value_in_divines("exalted").unwrap(), 1.0 / 493.9);
        assert_close(market.value_in_divines("chaos").unwrap(), 1.0 / 8.16);
        assert_eq!(market.value_in_divines("adept-rune"), Some(0.01312));
        assert_eq!(market.value_in_divines("alch"), None);
    }

    #[test]
    fn without_the_league_list_the_slug_follows_poe_ninjas_naming() {
        assert_eq!(league_slug("HC Forbidden Rites", None), "forbiddenriteshc");
        assert_eq!(league_slug("Standard", None), "standard");
    }
}
