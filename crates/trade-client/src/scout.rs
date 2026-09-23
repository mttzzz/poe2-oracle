//! poe2scout's price list: what poe2scout.com prices from the trade site's listings -- uniques
//! above all, which poe.ninja's Currency Exchange market doesn't cover -- for a unique's "worth
//! about" before (or without) its own trade search.
//!
//! Verified live 2026-09-23 on league Forbidden Rites:
//! - `GET https://api.poe2scout.com/poe2/Leagues/{league}/Items` (no auth, `league` with spaces as
//!   `%20`) answers 1288 items: `[{"ItemId":25,"CategoryApiId":"accessory","Text":"Igniferis
//!   Crimson Amulet","Name":"Igniferis","Type":"Crimson Amulet","ApiId":null,"CurrentPrice":1,
//!   "IconUrl":"..."},...]` -- 467 uniques under their English `Name` and base `Type`, no two
//!   sharing a name, and the exchange items under their trade `ApiId`.
//! - `CurrentPrice` is in the league's base currency, Exalted Orbs (`GET /poe2/Leagues` names it,
//!   `"BaseCurrencyApiId":"exalted"`): the Divine Orb's own row, 533.9 that day, is the rate its
//!   prices convert to divines with -- not poe.ninja's (481.6 then), which would mix two sources.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use http_client::{AsyncBody, HttpClient};
use serde::{Deserialize, Serialize};

use crate::cache::{league_file_name, load_or_fetch};
use crate::ninja::DIVINE_UNIT_CUTOVER;
use crate::rates::PriceUnit;
use crate::{checked_body, encode_league};

const API_BASE_URL: &str = "https://api.poe2scout.com/poe2";

/// poe2scout refreshes its prices through the day; half an hour, as for poe.ninja's market.
const PRICES_MAX_AGE: Duration = Duration::from_secs(30 * 60);

/// One league's poe2scout prices. `Serialize`/`Deserialize` for [`load_or_fetch`]'s disk cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoutPrices {
    /// Exalted Orbs one Divine Orb buys by poe2scout's own reckoning.
    pub exalted_per_divine: f64,
    /// Uniques' prices in Exalted Orbs, by English unique name.
    pub uniques: HashMap<String, f64>,
    /// Exchange items' prices in Exalted Orbs, by trade id: the second opinion for an item
    /// poe.ninja's market has no line for in the league (thin leagues like Standard list a third
    /// of the runes). Absent from caches written before it existed.
    #[serde(default)]
    pub exchange: HashMap<String, f64>,
}

impl ScoutPrices {
    /// The unique named `name` (in English)'s price in the unit it reads best in -- divines above
    /// 0.94, exalted below, EE2's cutover as for poe.ninja's prices -- at poe2scout's own rate.
    pub fn unique_price(&self, name: &str) -> Option<(f64, PriceUnit)> {
        self.uniques.get(name).map(|&exalted| self.in_unit(exalted))
    }

    /// The exchange item `trade_id`'s price, read the same way as [`Self::unique_price`].
    pub fn exchange_price(&self, trade_id: &str) -> Option<(f64, PriceUnit)> {
        self.exchange
            .get(trade_id)
            .map(|&exalted| self.in_unit(exalted))
    }

    fn in_unit(&self, exalted: f64) -> (f64, PriceUnit) {
        let divines = exalted / self.exalted_per_divine;
        if divines > DIVINE_UNIT_CUTOVER {
            (divines, PriceUnit::Divine)
        } else {
            (exalted, PriceUnit::Exalted)
        }
    }
}

/// The league's poe2scout prices, from `cache_dir`'s copy while it's under half an hour old
/// (each league gets its own file). A failed download falls back to the last copy however old
/// (see [`load_or_fetch`]).
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
    let usable = |price: Option<f64>| price.filter(|price| price.is_finite() && *price > 0.0);
    let exalted_per_divine = items
        .iter()
        .find(|item| item.api_id.as_deref() == Some("divine"))
        .and_then(|divine| usable(divine.current_price))
        .context("poe2scout has no Divine Orb price for the league")?;
    // Exchange items carry their trade id; uniques have none, and a name.
    let (exchange, uniques): (Vec<Item>, Vec<Item>) =
        items.into_iter().partition(|item| item.api_id.is_some());
    let uniques = uniques
        .into_iter()
        .filter_map(|item| Some((item.name?, usable(item.current_price)?)))
        .collect();
    let exchange = exchange
        .into_iter()
        .filter_map(|item| Some((item.api_id?, usable(item.current_price)?)))
        .collect();
    Ok(ScoutPrices {
        exalted_per_divine,
        uniques,
        exchange,
    })
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
    fn exchange_items_are_priced_by_their_trade_id() {
        let prices = parse_items(ITEMS).expect("parses");
        assert_eq!(
            prices.exchange_price("aug"),
            Some((2.93, PriceUnit::Exalted))
        );
        // Uniques aren't exchange items.
        assert_eq!(prices.exchange_price("Mageblood"), None);
        // A cache written before exchange prices were kept still loads, with none.
        let old = r#"{"exalted_per_divine":500.0,"uniques":{"Igniferis":1.0}}"#;
        let old: ScoutPrices = serde_json::from_str(old).expect("an old cache loads");
        assert_eq!(old.exchange_price("aug"), None);
    }

    #[test]
    fn a_list_without_a_divine_price_is_no_price_list() {
        let body = r#"[{"Name":"Igniferis","ApiId":null,"CurrentPrice":1}]"#;
        assert!(parse_items(body).is_err());
    }
}
