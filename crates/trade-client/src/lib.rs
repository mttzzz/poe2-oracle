//! PoE2 trade API client (leagues -> search -> fetch). Endpoints and request/response shapes are
//! migrated from the real, independently-verified POC in
//! `crates/poe2-oracle/examples/trade_api.rs` (see `POC_FINDINGS.md`'s "Capability 5" section for
//! the original cross-check against a separate Python `urllib` call) into a real library API
//! here -- a move-and-refactor of already-proven logic, not a rewrite. The exact endpoint URLs,
//! request bodies, and rate-limit-relevant `GET`/`POST` shapes are unchanged from the POC.
//!
//! Deliberately UI/runtime-agnostic: takes an already-constructed `Arc<dyn HttpClient>` rather
//! than building one itself (`reqwest_client::ReqwestClient` construction, including the
//! `USER_AGENT` choice, stays in `crates/poe2-oracle` -- the one place that actually knows what
//! kind of app is making the request). Depends only on `poe2-domain` (for the eventual shared
//! item/currency shapes -- still an empty skeleton as of this migration, see this crate's
//! `Cargo.toml`) and `http_client` (the trait, not a concrete client).

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use futures::AsyncReadExt;
use http_client::{AsyncBody, HttpClient, Json};
use serde::{Deserialize, Serialize};

const TRADE_API_BASE: &str = "https://www.pathofexile.com/api/trade2";

/// One league as returned by `GET /api/trade2/data/leagues`.
#[derive(Debug, Deserialize)]
pub struct League {
    pub id: String,
}

#[derive(Deserialize)]
struct LeaguesResponse {
    result: Vec<League>,
}

/// `GET /api/trade2/data/leagues` -- every currently-active league, most-current first (the POC's
/// proven assumption: taking `leagues[0]` gives the current top league, e.g. "Forbidden Rites").
pub async fn leagues(client: &Arc<dyn HttpClient>) -> Result<Vec<League>> {
    let url = format!("{TRADE_API_BASE}/data/leagues");
    let mut response = client
        .get(&url, AsyncBody::default(), true)
        .await
        .with_context(|| format!("GET {url}"))?;
    let mut body = String::new();
    response
        .body_mut()
        .read_to_string(&mut body)
        .await
        .context("reading leagues response body")?;
    let parsed: LeaguesResponse =
        serde_json::from_str(&body).context("parsing leagues response JSON")?;
    Ok(parsed.result)
}

#[derive(Serialize)]
struct SearchRequestBody {
    query: SearchQuery,
    sort: SearchSort,
}
#[derive(Serialize)]
struct SearchQuery {
    status: OptionField,
    stats: Vec<StatGroup>,
    filters: TypeFilters,
}
#[derive(Serialize)]
struct OptionField {
    // Owned, not `&'static str`: `category` is now a caller-supplied `&str` parameter (the POC
    // only ever passed the `SEARCH_CATEGORY` constant), so this field can't borrow 'static.
    option: String,
}
#[derive(Serialize)]
struct StatGroup {
    #[serde(rename = "type")]
    kind: &'static str,
    filters: Vec<serde_json::Value>,
}
#[derive(Serialize)]
struct TypeFilters {
    type_filters: TypeFiltersInner,
}
#[derive(Serialize)]
struct TypeFiltersInner {
    filters: CategoryFilter,
}
#[derive(Serialize)]
struct CategoryFilter {
    category: OptionField,
}
#[derive(Serialize)]
struct SearchSort {
    price: &'static str,
}

#[derive(Deserialize)]
struct SearchResponse {
    id: String,
    result: Vec<String>,
    total: u64,
}

/// The listing-id page and query id `fetch` needs, from a category search sorted by price
/// ascending with no stat filters -- the exact shape the POC proved against `trade2` (crossbows,
/// a PoE2-only equipment category, as a clean proof this hits `trade2` and not a copy-pasted
/// PoE1 endpoint).
pub struct SearchOutcome {
    pub query_id: String,
    pub total: u64,
    pub listing_ids: Vec<String>,
}

/// `POST /api/trade2/search/{league}` for `category` (a `CATEGORY_TO_TRADE_ID`-style trade
/// category string, e.g. `"weapon.crossbow"`), sorted by price ascending, with no stat filters.
/// `league` is the raw league id (may contain spaces, e.g. "Forbidden Rites") -- this function
/// URL-encodes it itself.
pub async fn search(
    client: &Arc<dyn HttpClient>,
    league: &str,
    category: &str,
) -> Result<SearchOutcome> {
    let body = SearchRequestBody {
        query: SearchQuery {
            status: OptionField {
                option: "online".to_owned(),
            },
            stats: vec![StatGroup {
                kind: "and",
                filters: vec![],
            }],
            filters: TypeFilters {
                type_filters: TypeFiltersInner {
                    filters: CategoryFilter {
                        category: OptionField {
                            option: category.to_owned(),
                        },
                    },
                },
            },
        },
        sort: SearchSort { price: "asc" },
    };
    let url = format!("{TRADE_API_BASE}/search/{}", urlencoding_space(league));
    let mut response = client
        .post_json(&url, AsyncBody::from(Json(body)))
        .await
        .with_context(|| format!("POST {url}"))?;
    let mut response_body = String::new();
    response
        .body_mut()
        .read_to_string(&mut response_body)
        .await
        .context("reading search response body")?;
    let parsed: SearchResponse =
        serde_json::from_str(&response_body).context("parsing search response JSON")?;
    Ok(SearchOutcome {
        query_id: parsed.id,
        total: parsed.total,
        listing_ids: parsed.result,
    })
}

#[derive(Deserialize)]
struct FetchResponse {
    result: Vec<Option<FetchResultItem>>,
}
#[derive(Deserialize)]
struct FetchResultItem {
    item: FetchItem,
    listing: FetchListing,
}
#[derive(Deserialize)]
struct FetchItem {
    name: String,
    #[serde(rename = "typeLine")]
    type_line: String,
}
#[derive(Deserialize)]
struct FetchListing {
    price: Option<FetchPrice>,
    account: FetchAccount,
}
#[derive(Deserialize)]
struct FetchPrice {
    amount: f64,
    currency: String,
}
#[derive(Deserialize)]
struct FetchAccount {
    name: String,
}

/// One resolved listing from `fetch`. Presentation formatting (how to display the price, etc.)
/// is left to the caller -- this is raw parsed API data, not a UI-ready row.
#[derive(Debug)]
pub struct FetchedItem {
    pub name: String,
    pub type_line: String,
    /// `(amount, currency)`, e.g. `(1.0, "transmute")`; `None` if the listing has no set price.
    pub price: Option<(f64, String)>,
    pub account_name: String,
}

/// `GET /api/trade2/fetch/{ids}?query={query_id}` for up to 10 listing ids at a time (the trade
/// API's own per-request limit; unenforced here since the POC never needed more than 5 -- pass a
/// pre-sliced `listing_ids` page). Entries the API returns as `null` (delisted between search and
/// fetch) are silently dropped, matching the POC's `.flatten()`.
pub async fn fetch(
    client: &Arc<dyn HttpClient>,
    listing_ids: &[String],
    query_id: &str,
) -> Result<Vec<FetchedItem>> {
    if listing_ids.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!(
        "{TRADE_API_BASE}/fetch/{}?query={query_id}",
        listing_ids.join(",")
    );
    let mut response = client
        .get(&url, AsyncBody::default(), true)
        .await
        .with_context(|| format!("GET {url}"))?;
    let mut body = String::new();
    response
        .body_mut()
        .read_to_string(&mut body)
        .await
        .context("reading fetch response body")?;
    let parsed: FetchResponse =
        serde_json::from_str(&body).context("parsing fetch response JSON")?;

    Ok(parsed
        .result
        .into_iter()
        .flatten()
        .map(|entry| FetchedItem {
            name: entry.item.name,
            type_line: entry.item.type_line,
            price: entry.listing.price.map(|p| (p.amount, p.currency)),
            account_name: entry.listing.account.name,
        })
        .collect())
}

/// Convenience wrapper: current league (first from `leagues`) -> `search` -> `fetch`, taking at
/// most `limit` listings. Mirrors the POC's `run_trade_flow` end to end.
pub async fn search_current_league(
    client: &Arc<dyn HttpClient>,
    category: &str,
    limit: usize,
) -> Result<(String, u64, Vec<FetchedItem>)> {
    let league = leagues(client)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("leagues response had no entries"))?
        .id;
    let outcome = search(client, &league, category).await?;
    let take_ids: Vec<String> = outcome.listing_ids.into_iter().take(limit).collect();
    let items = fetch(client, &take_ids, &outcome.query_id).await?;
    Ok((league, outcome.total, items))
}

/// PoE league names can contain spaces (e.g. "Forbidden Rites"); the search endpoint is queried
/// with the raw league id in this encoded form, not the URL-encoded form the real app uses in
/// browser-facing trade links.
fn urlencoding_space(s: &str) -> String {
    s.replace(' ', "%20")
}
