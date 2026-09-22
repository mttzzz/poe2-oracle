//! Capability 5 POC: a real async call to the live PoE2 trade API (leagues → search → fetch),
//! with a loading state that re-renders into the resolved listing once the futures complete.
//!
//! Endpoints and request/response shapes are taken from the real Exiled Exchange 2 source
//! (`renderer/src/web/price-check/trade/pathofexile-trade.ts`, `.../background/Leagues.ts`) and
//! were independently verified against the live API with `curl` from inside this project's lane
//! before writing this file (see POC_FINDINGS.md for the cross-check).
//!
//! HTTP goes through `http_client`/`reqwest_client` (Zed's own wrapper), not raw `reqwest` --
//! see `POC_FINDINGS.md`'s deviation notes. No `gpui_tokio` glue is needed: `ReqwestClient`
//! lazily spins up its own background Tokio runtime and returns a plain boxed future safe to
//! `.await` from GPUI's own executor.

use futures::AsyncReadExt;
use gpui::{App, Bounds, Context, Render, Window, WindowBounds, div, prelude::*, px, rgb, size};
use gpui_platform::application;
use http_client::{AsyncBody, HttpClient, Json};
use reqwest_client::ReqwestClient;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
/// Waystones/crossbows etc. are equipment categories from `CATEGORY_TO_TRADE_ID` in the real
/// app; crossbows are PoE2-only (no PoE1 equivalent), which makes a returned result a clean
/// proof this actually hit `trade2`, not a copy-pasted PoE1 endpoint.
const SEARCH_CATEGORY: &str = "weapon.crossbow";
const RESULT_LIMIT: usize = 5;

#[derive(Serialize)]
struct SearchRequest {
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
    option: &'static str,
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
struct LeaguesResponse {
    result: Vec<LeagueEntry>,
}
#[derive(Deserialize)]
struct LeagueEntry {
    id: String,
}

#[derive(Deserialize)]
struct SearchResponse {
    id: String,
    result: Vec<String>,
    total: u64,
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

struct ListingRow {
    display_name: String,
    price: String,
    account: String,
}

enum LoadState {
    Loading(&'static str),
    Resolved { league: String, total: u64, rows: Vec<ListingRow> },
    Error(String),
}

struct TradeApi {
    state: LoadState,
}

impl Render for TradeApi {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.state {
            LoadState::Loading(stage) => {
                div().text_color(rgb(0xcccccc)).child(format!("loading: {stage} …"))
            }
            LoadState::Error(message) => {
                div().text_color(rgb(0xe06060)).child(format!("error: {message}"))
            }
            LoadState::Resolved { league, total, rows } => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0x9a9a9a))
                        .child(format!("{league} — {total} crossbows listed, top {}", rows.len())),
                )
                .children(rows.iter().map(|row| {
                    div()
                        .flex()
                        .justify_between()
                        .gap_4()
                        .text_sm()
                        .child(div().text_color(rgb(0xffffff)).child(row.display_name.clone()))
                        .child(div().text_color(rgb(0xd0913b)).child(row.price.clone()))
                        .child(div().text_color(rgb(0x8a8a8a)).child(row.account.clone()))
                })),
        };

        div()
            .font_family("DejaVu Sans")
            .size_full()
            .p_4()
            .bg(rgb(0x1a1a1a))
            .child(body)
    }
}

async fn run_trade_flow(client: Arc<dyn HttpClient>) -> anyhow::Result<(String, u64, Vec<ListingRow>)> {
    let mut response = client
        .get(
            "https://www.pathofexile.com/api/trade2/data/leagues",
            AsyncBody::default(),
            true,
        )
        .await?;
    let mut body = String::new();
    response.body_mut().read_to_string(&mut body).await?;
    let leagues: LeaguesResponse = serde_json::from_str(&body)?;
    let league = leagues
        .result
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("leagues response had no entries"))?
        .id;

    let search_body = SearchRequest {
        query: SearchQuery {
            status: OptionField { option: "online" },
            stats: vec![StatGroup { kind: "and", filters: vec![] }],
            filters: TypeFilters {
                type_filters: TypeFiltersInner {
                    filters: CategoryFilter { category: OptionField { option: SEARCH_CATEGORY } },
                },
            },
        },
        sort: SearchSort { price: "asc" },
    };
    let search_uri = format!(
        "https://www.pathofexile.com/api/trade2/search/{}",
        urlencoding_space(&league)
    );
    let mut response = client
        .post_json(&search_uri, AsyncBody::from(Json(search_body)))
        .await?;
    let mut body = String::new();
    response.body_mut().read_to_string(&mut body).await?;
    let search: SearchResponse = serde_json::from_str(&body)?;

    let take_ids: Vec<&str> = search.result.iter().take(RESULT_LIMIT).map(String::as_str).collect();
    let fetch_uri = format!(
        "https://www.pathofexile.com/api/trade2/fetch/{}?query={}",
        take_ids.join(","),
        search.id
    );
    let mut response = client.get(&fetch_uri, AsyncBody::default(), true).await?;
    let mut body = String::new();
    response.body_mut().read_to_string(&mut body).await?;
    let fetch: FetchResponse = serde_json::from_str(&body)?;

    let rows = fetch
        .result
        .into_iter()
        .flatten()
        .map(|entry| {
            let price = match entry.listing.price {
                Some(p) => format!("{} {}", p.amount, p.currency),
                None => "no price".to_string(),
            };
            ListingRow {
                display_name: format!("{} ({})", entry.item.name, entry.item.type_line),
                price,
                account: entry.listing.account.name,
            }
        })
        .collect();

    Ok((league, search.total, rows))
}

/// PoE league names can contain spaces (e.g. "Forbidden Rites"); this endpoint is queried with
/// the raw league id, not the URL-encoded form the real app uses in browser-facing trade links.
fn urlencoding_space(s: &str) -> String {
    s.replace(' ', "%20")
}

fn main() {
    application()
        .with_http_client(Arc::new(
            ReqwestClient::user_agent(USER_AGENT).expect("failed to build HTTP client"),
        ))
        .run(|cx: &mut App| {
            let bounds = Bounds::centered(None, size(px(480.), px(300.)), cx);
            cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("Oracle POC — trade_api");
                    let view = cx.new(|_cx| TradeApi { state: LoadState::Loading("leagues") });

                    let client = cx.http_client();
                    let weak = view.downgrade();
                    cx.spawn(async move |cx| {
                        let result = run_trade_flow(client).await;
                        // Observable, harness-greppable proof independent of screenshot capture.
                        match &result {
                            Ok((league, total, rows)) => println!(
                                "TRADE_RESOLVED league={league:?} total={total} rows={} first={:?}",
                                rows.len(),
                                rows.first().map(|r| (&r.display_name, &r.price, &r.account))
                            ),
                            Err(err) => println!("TRADE_ERROR {err}"),
                        }
                        let Some(view) = weak.upgrade() else { return };
                        view.update(cx, |state, cx| {
                            state.state = match result {
                                Ok((league, total, rows)) => LoadState::Resolved { league, total, rows },
                                Err(err) => LoadState::Error(err.to_string()),
                            };
                            cx.notify();
                        });
                    })
                    .detach();

                    view
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
