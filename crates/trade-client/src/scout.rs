//! poe2scout's prices (https://poe2scout.com): what it prices from the trade site's listings --
//! uniques above all, which the Currency Exchange doesn't trade -- for a unique's "worth about"
//! before (or without) its own trade search; and its view of the exchange: each item's price, its
//! last week day by day, and its page. The exchange market (`cx`) takes the week and the page for
//! its card, and poe2scout's price for an item GGG's record of the exchange doesn't price.
//!
//! Verified live 2026-09-23 on league Forbidden Rites (no auth; `league` with spaces as `%20`):
//! - `GET https://api.poe2scout.com/poe2/Leagues/{league}/Items` answers 1288 items:
//!   `[{"ItemId":25,"CategoryApiId":"accessory","Text":"Igniferis Crimson Amulet","Name":
//!   "Igniferis","Type":"Crimson Amulet","ApiId":null,"CurrentPrice":1,"IconUrl":"..."},...]` --
//!   467 uniques under their English `Name` and base `Type`, no two sharing a name, and 821
//!   exchange items under their trade `ApiId`.
//! - `GET .../Leagues/{league}/Items/Categories` lists the exchange's 17 categories
//!   (`{"CurrencyCategories":[{"ApiId":"currency","Label":"Currency",...},...],...}`), and
//!   `GET .../Leagues/{league}/Currencies/ByCategory?Category={id}&Page=1&PerPage=250&DataPoints=7
//!   &FrequencyHours=24` answers a page of one (250 items at most; more is refused):
//!   `{"CurrentPage":1,"Pages":1,"Total":38,"Items":[{"ItemId":292,"ApiId":"annul","Text":"Orb of
//!   Annulment","CategoryApiId":"currency","CurrentPrice":371.58,"PriceLogs":[{"Price":368.3,
//!   "Time":"2026-09-23T00:00:00.0000000Z","Quantity":271769},...],...},...]}` -- seven daily
//!   prices, today's first, a day without one `null`. The 17 held the same 635 priced exchange
//!   items as `Items`.
//! - An exchange item's page is `https://poe2scout.com/poe2/{ShortName}/economy/currencies/
//!   {category}/{ItemId}/{Text}` with the name's spaces as `-` -- the link the site's own tables
//!   build (its route table and `utils` module) -- where `ShortName` is the league's in
//!   `GET /poe2/Leagues` (`"Value":"Forbidden Rites","ShortName":"forbiddenrites"`; not a rule:
//!   `Dawn of the Hunt` is `hunt`).
//! - Every price is in the league's base currency, Exalted Orbs (`/Leagues` names it,
//!   `"BaseCurrencyApiId":"exalted"`): the Divine Orb's own price, 533.9 that day, is the rate they
//!   convert to divines with -- not GGG's, which would mix two sources.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use futures::future::{join, join_all};
use http_client::{AsyncBody, HttpClient};
use serde::{Deserialize, Serialize};

use crate::cache::{league_file_name, load_or_fetch};
use crate::cx::{DIVINE, DIVINE_UNIT_CUTOVER};
use crate::rates::PriceUnit;
use crate::{checked_body, encode_league};

const API_BASE_URL: &str = "https://api.poe2scout.com/poe2";
const PAGE_BASE_URL: &str = "https://poe2scout.com/poe2";

/// poe2scout refreshes its prices through the day; a copy is kept for half an hour.
const PRICES_MAX_AGE: Duration = Duration::from_secs(30 * 60);

/// The most items a `ByCategory` page holds: a larger `PerPage` is refused with 400.
const CATEGORY_PAGE_SIZE: u32 = 250;

/// One league's poe2scout prices of uniques. `Serialize`/`Deserialize` for [`load_or_fetch`]'s
/// disk cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoutPrices {
    /// Exalted Orbs one Divine Orb buys by poe2scout's own reckoning.
    pub exalted_per_divine: f64,
    /// Uniques' prices in Exalted Orbs, by English unique name.
    pub uniques: HashMap<String, f64>,
}

impl ScoutPrices {
    /// The unique named `name` (in English)'s price in the unit it reads best in, at poe2scout's
    /// own rate (see [`in_unit`]).
    pub fn unique_price(&self, name: &str) -> Option<(f64, PriceUnit)> {
        self.uniques
            .get(name)
            .map(|&exalted| in_unit(exalted, self.exalted_per_divine))
    }
}

/// `exalted` in the unit it reads best in -- divines above 0.94, exalted below, EE2's cutover as
/// for the exchange market's prices -- at `exalted_per_divine`.
fn in_unit(exalted: f64, exalted_per_divine: f64) -> (f64, PriceUnit) {
    let divines = exalted / exalted_per_divine;
    if divines > DIVINE_UNIT_CUTOVER {
        (divines, PriceUnit::Divine)
    } else {
        (exalted, PriceUnit::Exalted)
    }
}

/// A price poe2scout gives: finite and positive (it lists unpriced items at 0 or `null`).
fn usable(price: Option<f64>) -> Option<f64> {
    price.filter(|price| price.is_finite() && *price > 0.0)
}

/// The league's poe2scout prices of uniques, from `cache_dir`'s copy while it's under half an
/// hour old (each league gets its own file). A failed download falls back to the last copy
/// however old (see [`load_or_fetch`]).
pub async fn fetch_prices(
    client: &Arc<dyn HttpClient>,
    league: &str,
    cache_dir: &Path,
) -> Result<ScoutPrices> {
    let cache_path = cache_dir.join(league_file_name("scout-prices", league));
    // `download_prices` sends nothing until awaited, i.e. only on a cache miss.
    load_or_fetch(&cache_path, PRICES_MAX_AGE, download_prices(client, league)).await
}

async fn download_prices(client: &Arc<dyn HttpClient>, league: &str) -> Result<ScoutPrices> {
    let url = format!("{API_BASE_URL}/Leagues/{}/Items", encode_league(league));
    let request = client.get(&url, AsyncBody::default(), true);
    let body = checked_body(request, "GET", &url, None, "poe2scout items").await?;
    parse_items(&body)
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Item {
    name: Option<String>,
    api_id: Option<String>,
    current_price: Option<f64>,
}

fn parse_items(body: &str) -> Result<ScoutPrices> {
    let items: Vec<Item> = serde_json::from_str(body).context("parsing poe2scout items")?;
    let exalted_per_divine = items
        .iter()
        .find(|item| item.api_id.as_deref() == Some(DIVINE))
        .and_then(|divine| usable(divine.current_price))
        .context("poe2scout has no Divine Orb price for the league")?;
    // Exchange items carry their trade id; uniques have none, and a name.
    let uniques = items
        .into_iter()
        .filter(|item| item.api_id.is_none())
        .filter_map(|item| Some((item.name?, usable(item.current_price)?)))
        .collect();
    Ok(ScoutPrices {
        exalted_per_divine,
        uniques,
    })
}

/// poe2scout's view of one league's Currency Exchange. `Serialize`/`Deserialize` for
/// [`load_or_fetch`]'s disk cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ScoutExchange {
    /// Exalted Orbs one Divine Orb buys by poe2scout's own reckoning.
    pub(crate) exalted_per_divine: f64,
    /// By trade id.
    pub(crate) items: HashMap<String, ScoutExchangeItem>,
}

/// One exchange item as poe2scout has it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ScoutExchangeItem {
    /// Its price in Exalted Orbs; `None` where poe2scout has none.
    pub(crate) price: Option<f64>,
    /// Its daily prices in Exalted Orbs, oldest first, the last one today's so far; `None` for a
    /// day without one.
    pub(crate) week: Vec<Option<f64>>,
    /// Its page; `None` when poe2scout's list of leagues didn't load or lacks the league.
    pub(crate) page: Option<String>,
}

impl ScoutExchange {
    /// `trade_id`'s price in the unit it reads best in, at poe2scout's own rate.
    pub(crate) fn price(&self, trade_id: &str) -> Option<(f64, PriceUnit)> {
        let exalted = self.items.get(trade_id)?.price?;
        Some(in_unit(exalted, self.exalted_per_divine))
    }

    /// `trade_id`'s price in divines, at poe2scout's own rate.
    pub(crate) fn value_in_divines(&self, trade_id: &str) -> Option<f64> {
        Some(self.items.get(trade_id)?.price? / self.exalted_per_divine)
    }
}

/// The league's exchange items as poe2scout has them, kept like [`fetch_prices`]' list.
pub(crate) async fn fetch_exchange(
    client: &Arc<dyn HttpClient>,
    league: &str,
    cache_dir: &Path,
) -> Result<ScoutExchange> {
    let cache_path = cache_dir.join(league_file_name("scout-exchange", league));
    load_or_fetch(
        &cache_path,
        PRICES_MAX_AGE,
        download_exchange(client, league),
    )
    .await
}

/// Every category's items at once. A category that fails to load is logged and left out; only
/// when every one does, or the Divine Orb has no price, is the download an error -- `load_or_fetch`
/// then keeps the league's last good copy.
async fn download_exchange(client: &Arc<dyn HttpClient>, league: &str) -> Result<ScoutExchange> {
    let league_url = format!("{API_BASE_URL}/Leagues/{}", encode_league(league));
    let (categories, short_name) = join(
        download_categories(client, &league_url),
        download_short_name(client, league),
    )
    .await;
    let short_name = short_name
        .inspect_err(|err| log::warn!("poe2scout's pages for {league} are unknown: {err:#}"))
        .ok();
    let categories = categories?;
    let downloads = join_all(
        categories
            .iter()
            .map(|category| download_category(client, &league_url, category)),
    )
    .await;

    let mut items = HashMap::new();
    let mut failure = None;
    for (category, download) in categories.iter().zip(downloads) {
        match download {
            Ok(category_items) => {
                for item in category_items {
                    let Some(trade_id) = item.api_id else {
                        continue;
                    };
                    let page = short_name.as_deref().map(|short_name| {
                        item_page(short_name, category, item.item_id, &item.text)
                    });
                    // Today's first as served.
                    let week = item
                        .price_logs
                        .into_iter()
                        .rev()
                        .map(|log| usable(log?.price))
                        .collect();
                    let price = usable(item.current_price);
                    items.insert(trade_id, ScoutExchangeItem { price, week, page });
                }
            }
            Err(err) => {
                log::warn!("leaving poe2scout's {category} out: {err:#}");
                failure.get_or_insert(err);
            }
        }
    }
    if items.is_empty() {
        return Err(match failure {
            Some(err) => err.context("poe2scout priced no exchange category"),
            None => anyhow!("poe2scout lists no exchange item for {league}"),
        });
    }
    let exalted_per_divine = items
        .get(DIVINE)
        .and_then(|divine| divine.price)
        .context("poe2scout has no Divine Orb price for the league")?;
    Ok(ScoutExchange {
        exalted_per_divine,
        items,
    })
}

async fn download_categories(
    client: &Arc<dyn HttpClient>,
    league_url: &str,
) -> Result<Vec<String>> {
    let url = format!("{league_url}/Items/Categories");
    let request = client.get(&url, AsyncBody::default(), true);
    let body = checked_body(request, "GET", &url, None, "poe2scout categories").await?;
    let categories: Categories =
        serde_json::from_str(&body).with_context(|| format!("parsing {url}"))?;
    Ok(categories
        .currency_categories
        .into_iter()
        .map(|category| category.api_id)
        .collect())
}

/// The league's segment in poe2scout's page links.
async fn download_short_name(client: &Arc<dyn HttpClient>, league: &str) -> Result<String> {
    let url = format!("{API_BASE_URL}/Leagues");
    let request = client.get(&url, AsyncBody::default(), true);
    let body = checked_body(request, "GET", &url, None, "poe2scout leagues").await?;
    let leagues: Vec<ListedLeague> =
        serde_json::from_str(&body).with_context(|| format!("parsing {url}"))?;
    leagues
        .into_iter()
        .find(|listed| listed.value == league)
        .map(|listed| listed.short_name)
        .with_context(|| format!("poe2scout doesn't list {league}"))
}

/// Every page of one category's items.
async fn download_category(
    client: &Arc<dyn HttpClient>,
    league_url: &str,
    category: &str,
) -> Result<Vec<CategoryItem>> {
    let mut items = Vec::new();
    for page in 1.. {
        let url = format!(
            "{league_url}/Currencies/ByCategory?Category={}&Page={page}&PerPage={CATEGORY_PAGE_SIZE}\
             &DataPoints=7&FrequencyHours=24",
            encode_league(category)
        );
        let request = client.get(&url, AsyncBody::default(), true);
        let body = checked_body(request, "GET", &url, None, "poe2scout category").await?;
        let served: CategoryPage =
            serde_json::from_str(&body).with_context(|| format!("parsing {url}"))?;
        let last = served.items.is_empty() || page >= served.pages;
        items.extend(served.items);
        if last {
            break;
        }
    }
    Ok(items)
}

/// An exchange item's page, linked as the site's own tables link it: the name's words joined
/// with `-`, and every segment URL-encoded as JavaScript's `encodeURIComponent` does.
fn item_page(short_name: &str, category: &str, item_id: u64, name: &str) -> String {
    let slug = name.split_whitespace().collect::<Vec<_>>().join("-");
    format!(
        "{PAGE_BASE_URL}/{}/economy/currencies/{}/{item_id}/{}",
        encode_league(short_name),
        encode_league(category),
        encode_league(&slug)
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Categories {
    currency_categories: Vec<Category>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Category {
    api_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListedLeague {
    value: String,
    short_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CategoryPage {
    pages: u32,
    items: Vec<CategoryItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CategoryItem {
    item_id: u64,
    api_id: Option<String>,
    text: String,
    current_price: Option<f64>,
    #[serde(default)]
    price_logs: Vec<Option<PriceLog>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PriceLog {
    price: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows as the live list has them (2026-09-23), trimmed to what is read.
    const ITEMS: &str = r#"[
        {"ItemId":25,"CategoryApiId":"accessory","Text":"Igniferis Crimson Amulet","Name":"Igniferis","Type":"Crimson Amulet","ApiId":null,"CurrentPrice":1},
        {"ItemId":3,"CategoryApiId":"accessory","Text":"Mageblood Utility Belt","Name":"Mageblood","Type":"Utility Belt","ApiId":null,"CurrentPrice":257031.74369351758},
        {"ItemId":7,"CategoryApiId":"currency","Text":"Divine Orb","Name":null,"Type":"Divine Orb","ApiId":"divine","CurrentPrice":533.8741496867618},
        {"ItemId":9,"CategoryApiId":"currency","Text":"Orb of Augmentation","Name":null,"Type":"Orb of Augmentation","ApiId":"aug","CurrentPrice":2.93},
        {"ItemId":40,"CategoryApiId":"armour","Text":"Unpriced Robe","Name":"Unpriced","Type":"Robe","ApiId":null,"CurrentPrice":null}
    ]"#;

    #[test]
    fn uniques_are_priced_at_poe2scouts_own_rate_in_the_unit_they_read_in() {
        let prices = parse_items(ITEMS).expect("parses");
        assert_eq!(prices.exalted_per_divine, 533.8741496867618);
        let (mageblood, unit) = prices.unique_price("Mageblood").expect("priced");
        assert_eq!(unit, PriceUnit::Divine);
        assert!((mageblood - 481.45).abs() < 0.01, "{mageblood}");
        // A cheap unique reads in exalted, as poe2scout lists it.
        assert_eq!(
            prices.unique_price("Igniferis"),
            Some((1.0, PriceUnit::Exalted))
        );
        // An exchange item isn't a unique, and neither is a name without a price.
        assert_eq!(prices.unique_price("Divine Orb"), None);
        assert_eq!(prices.unique_price("Unpriced"), None);
    }

    #[test]
    fn a_list_without_a_divine_price_is_no_price_list() {
        let body = r#"[{"Name":"Igniferis","ApiId":null,"CurrentPrice":1}]"#;
        assert!(parse_items(body).is_err());
    }
}
