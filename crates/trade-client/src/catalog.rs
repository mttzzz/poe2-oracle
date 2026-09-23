//! Stat/item/currency catalog fetches from the live PoE2 trade API's static-data endpoints
//! (`/api/trade2/data/{stats,items,static}`) -- distinct from `leagues`/`search`/`fetch` in
//! `lib.rs`, which operate on trade *listings*, not the catalogs those listings are filtered
//! against. These three endpoints return no rate-limit headers at all (confirmed live this
//! session), so callers don't need to thread a [`crate::rate_limit::RateLimiter`] through them --
//! but their responses still go through `checked_body`, so a refusal reads as a
//! [`crate::TradeApiError`] rather than a JSON parse error.
//!
//! Response shapes verified live via `curl` against the real endpoints, no auth needed:
//! - `/data/stats` -> `{"result":[{"id":"explicit","label":"Explicit","entries":[{"id":"explicit.stat_4080418644","text":"# to Strength","type":"explicit"}, ...]}, ...]}`
//!   (10 groups, 8297 entries total as of this writing).
//! - `/data/static` -> `{result:[{id,label,entries:[{id,text,image}]}]}`: every group -- currency,
//!   but also omens (`Ritual`), runes, essences, fragments... (15 groups, 798 entries on
//!   `ru.pathofexile.com`, verified live 2026-09-22) -- is exchange-tradable, each entry shaped
//!   `{id: "omen-of-abyssal-echoes", text: "Предзнаменование отголосков Бездны", image:
//!   "/gen/image/..."}` with the icon path relative to `web.poecdn.com`. Group labels are
//!   localized and one trailing group has `"label": null`.
//! - `/data/items` -> `{result:[{label:"Weapons",entries:[{type:"Glass Shank"}, ...]}]}` -- a
//!   coarse group label plus a base-type name only, no fine category (see
//!   `item-parser::categories` for the `Item Class:`-driven table this project uses instead).

use std::sync::Arc;

use anyhow::{Context, Result};
use http_client::{AsyncBody, HttpClient};
use poe2_domain::{StatCatalog, TradeStat};
use serde::{Deserialize, Serialize};

use crate::{TradeSite, checked_body};

/// The `{result:[{label,entries:[...]}]}` envelope shared by all three `/data/*` endpoints below
/// -- they differ only in the entry shape.
#[derive(Deserialize)]
struct DataResponse<T> {
    result: Vec<DataGroup<T>>,
}
#[derive(Deserialize)]
struct DataGroup<T> {
    // `Option`, not `String`: confirmed live against `/data/static` (2026-09-22) -- its last
    // group is a real, empty placeholder with `"label": null` rather than an empty string or the
    // group being omitted outright. `/data/stats` and `/data/items` never exhibited this in the
    // same live check, but they share this exact struct, so the type stays defensive for all
    // three rather than only patching the one endpoint observed to trigger it.
    label: Option<String>,
    entries: Vec<T>,
}

#[derive(Deserialize)]
struct StatEntry {
    id: String,
    text: String,
    #[serde(rename = "type")]
    kind: String,
}

fn parse_stat_catalog(body: &str) -> Result<StatCatalog> {
    let parsed: DataResponse<StatEntry> =
        serde_json::from_str(body).context("parsing stats catalog response JSON")?;
    let stats = parsed
        .result
        .into_iter()
        .flat_map(|group| group.entries)
        .map(|entry| TradeStat {
            id: entry.id,
            text: entry.text,
            mod_type: entry.kind,
        })
        .collect();
    Ok(StatCatalog { stats })
}

/// `GET /api/trade2/data/stats` -- every stat the trade API can filter on, flattened across every
/// group. [`TradeStat::mod_type`] is each entry's own `"type"` field, not the outer group's
/// `"id"` -- the two agree in practice, but the per-entry field is the documented source of truth
/// for the search-request id prefix. `site` picks the template language: ids are identical on
/// every site, only `text` is localized.
pub async fn fetch_stat_catalog(
    client: &Arc<dyn HttpClient>,
    site: TradeSite,
) -> Result<StatCatalog> {
    let body = fetch_body(client, site, "stats").await?;
    parse_stat_catalog(&body)
}

/// One exchange-tradable entry from `/data/static` (any group: currency, omens, runes, essences,
/// fragments...): its trade id, its display name in the site's language (e.g. `id: "divine"`,
/// `display_name: "Divine Orb"`), and its icon. `Serialize`/`Deserialize` so the app layer can
/// cache a `Vec<StaticCurrency>` via [`crate::cache::load_or_fetch`], same as
/// [`poe2_domain::StatCatalog`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StaticCurrency {
    pub id: String,
    pub display_name: String,
    /// Absolute icon URL.
    pub icon_url: Option<String>,
}

#[derive(Deserialize)]
struct StaticEntry {
    id: String,
    text: String,
    image: Option<String>,
}

/// Host the API's relative `image` paths are served from.
const ICON_HOST: &str = "https://web.poecdn.com";

fn parse_static_currencies(body: &str) -> Result<Vec<StaticCurrency>> {
    let parsed: DataResponse<StaticEntry> =
        serde_json::from_str(body).context("parsing static catalog response JSON")?;
    Ok(parsed
        .result
        .into_iter()
        .flat_map(|group| group.entries)
        .map(|entry| StaticCurrency {
            id: entry.id,
            display_name: entry.text,
            icon_url: entry.image.map(|image| {
                if image.starts_with("http") {
                    image
                } else {
                    format!("{ICON_HOST}{image}")
                }
            }),
        })
        .collect())
}

/// `GET /api/trade2/data/static` -- every exchange-tradable static item, flattened across groups
/// (an omen or a rune routes to the exchange exactly like a Divine Orb). `display_name` is in
/// `site`'s language, which is what `route_search` compares against the clipboard item's name.
pub async fn fetch_static_currencies(
    client: &Arc<dyn HttpClient>,
    site: TradeSite,
) -> Result<Vec<StaticCurrency>> {
    let body = fetch_body(client, site, "static").await?;
    parse_static_currencies(&body)
}

/// One entry from `/data/items`: a coarse group label plus a base-type name, e.g.
/// `group_label: "Weapons"`, `type_name: "Glass Shank"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemTypeEntry {
    pub group_label: String,
    pub type_name: String,
}

#[derive(Deserialize)]
struct ItemEntry {
    #[serde(rename = "type")]
    type_name: String,
}

fn parse_item_types(body: &str) -> Result<Vec<ItemTypeEntry>> {
    let parsed: DataResponse<ItemEntry> =
        serde_json::from_str(body).context("parsing items catalog response JSON")?;
    Ok(parsed
        .result
        .into_iter()
        .flat_map(|group| {
            let group_label = group.label.unwrap_or_default();
            group.entries.into_iter().map(move |entry| ItemTypeEntry {
                group_label: group_label.clone(),
                type_name: entry.type_name,
            })
        })
        .collect())
}

/// `GET /api/trade2/data/items`.
pub async fn fetch_item_types(
    client: &Arc<dyn HttpClient>,
    site: TradeSite,
) -> Result<Vec<ItemTypeEntry>> {
    let body = fetch_body(client, site, "items").await?;
    parse_item_types(&body)
}

/// Shared `GET /api/trade2/data/{path}` fetch, returning the raw response body -- parsing is
/// split into the separate pure `parse_*` functions above so it's unit-testable without a live or
/// mocked `HttpClient`.
async fn fetch_body(client: &Arc<dyn HttpClient>, site: TradeSite, path: &str) -> Result<String> {
    let url = format!("{}/data/{path}", site.api_base());
    let request = client.get(&url, AsyncBody::default(), true);
    checked_body(request, "GET", &url, None, &format!("{path} catalog")).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_stats_response_shape() {
        let body = r##"{"result":[
            {"id":"explicit","label":"Explicit","entries":[
                {"id":"explicit.stat_4080418644","text":"# to Strength","type":"explicit"}
            ]},
            {"id":"pseudo","label":"Pseudo","entries":[
                {"id":"pseudo.pseudo_total_life","text":"+# total maximum Life","type":"pseudo"}
            ]}
        ]}"##;
        let catalog = parse_stat_catalog(body).expect("should parse");
        assert_eq!(
            catalog.stats,
            vec![
                TradeStat {
                    id: "explicit.stat_4080418644".into(),
                    text: "# to Strength".into(),
                    mod_type: "explicit".into(),
                },
                TradeStat {
                    id: "pseudo.pseudo_total_life".into(),
                    text: "+# total maximum Life".into(),
                    mod_type: "pseudo".into(),
                },
            ]
        );
    }

    #[test]
    fn keeps_every_static_group_with_absolute_icon_urls() {
        // Omens live in the "Ritual" group, not "Currency" (verified live 2026-09-22): keeping
        // only "Currency" left every omen, rune and essence unpriceable.
        let body = r#"{"result":[
            {"id":"Currency","label":"Валюта","entries":[
                {"id":"divine","text":"Божественная сфера","image":"/gen/image/a/divine.png"}
            ]},
            {"id":"Ritual","label":"Ритуал","entries":[
                {"id":"omen-of-abyssal-echoes","text":"Предзнаменование отголосков Бездны"}
            ]},
            {"id":"Misc","label":null,"entries":[]}
        ]}"#;
        let currencies = parse_static_currencies(body).expect("should parse");
        assert_eq!(
            currencies,
            vec![
                StaticCurrency {
                    id: "divine".into(),
                    display_name: "Божественная сфера".into(),
                    icon_url: Some("https://web.poecdn.com/gen/image/a/divine.png".into()),
                },
                StaticCurrency {
                    id: "omen-of-abyssal-echoes".into(),
                    display_name: "Предзнаменование отголосков Бездны".into(),
                    icon_url: None,
                },
            ]
        );
    }

    #[test]
    fn flattens_item_types_with_their_group_label() {
        let body = r#"{"result":[
            {"label":"Weapons","entries":[{"type":"Glass Shank"},{"type":"Rusted Sword"}]},
            {"label":"Flasks","entries":[{"type":"Small Life Flask"}]}
        ]}"#;
        let types = parse_item_types(body).expect("should parse");
        assert_eq!(
            types,
            vec![
                ItemTypeEntry {
                    group_label: "Weapons".into(),
                    type_name: "Glass Shank".into(),
                },
                ItemTypeEntry {
                    group_label: "Weapons".into(),
                    type_name: "Rusted Sword".into(),
                },
                ItemTypeEntry {
                    group_label: "Flasks".into(),
                    type_name: "Small Life Flask".into(),
                },
            ]
        );
    }
}
