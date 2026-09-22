//! Capability 5 POC: a real async call to the live PoE2 trade API (leagues → search → fetch),
//! with a loading state that re-renders into the resolved listing once the futures complete.
//!
//! Endpoints and request/response shapes are taken from the real Exiled Exchange 2 source
//! (`renderer/src/web/price-check/trade/pathofexile-trade.ts`, `.../background/Leagues.ts`) and
//! were independently verified against the live API with `curl` from inside this project's lane
//! before writing this file (see POC_FINDINGS.md for the cross-check). The actual endpoint/parse
//! logic proven here now lives in `trade-client` (see the architecture plan's step 8); this
//! example is just the GPUI presentation shell around it, kept as a smoke test for the platform
//! layer -- `trade_client::search_current_league` is the real library API.
//!
//! HTTP goes through `http_client`/`reqwest_client` (Zed's own wrapper), not raw `reqwest` --
//! see `POC_FINDINGS.md`'s deviation notes. No `gpui_tokio` glue is needed: `ReqwestClient`
//! lazily spins up its own background Tokio runtime and returns a plain boxed future safe to
//! `.await` from GPUI's own executor.

use gpui::{App, Bounds, Context, Render, Window, WindowBounds, div, prelude::*, px, rgb, size};
use gpui_platform::application;
use http_client::HttpClient;
use reqwest_client::ReqwestClient;
use std::sync::Arc;
use trade_client::FetchedItem;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
/// Waystones/crossbows etc. are equipment categories from `CATEGORY_TO_TRADE_ID` in the real
/// app; crossbows are PoE2-only (no PoE1 equivalent), which makes a returned result a clean
/// proof this actually hit `trade2`, not a copy-pasted PoE1 endpoint.
const SEARCH_CATEGORY: &str = "weapon.crossbow";
const RESULT_LIMIT: usize = 5;

/// A `FetchedItem` formatted for display -- presentation concerns (how to render "no price",
/// combining name+typeLine) stay in this app-layer example, not in the library.
struct ListingRow {
    display_name: String,
    price: String,
    account: String,
}

impl From<FetchedItem> for ListingRow {
    fn from(item: FetchedItem) -> Self {
        let price = match item.price {
            Some((amount, currency)) => format!("{amount} {currency}"),
            None => "no price".to_string(),
        };
        ListingRow {
            display_name: format!("{} ({})", item.name, item.type_line),
            price,
            account: item.account_name,
        }
    }
}

enum LoadState {
    Loading(&'static str),
    Resolved {
        league: String,
        total: u64,
        rows: Vec<ListingRow>,
    },
    Error(String),
}

struct TradeApi {
    state: LoadState,
}

impl Render for TradeApi {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.state {
            LoadState::Loading(stage) => div()
                .text_color(rgb(0xcccccc))
                .child(format!("loading: {stage} …")),
            LoadState::Error(message) => div()
                .text_color(rgb(0xe06060))
                .child(format!("error: {message}")),
            LoadState::Resolved {
                league,
                total,
                rows,
            } => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_sm().text_color(rgb(0x9a9a9a)).child(format!(
                    "{league} — {total} crossbows listed, top {}",
                    rows.len()
                )))
                .children(rows.iter().map(|row| {
                    div()
                        .flex()
                        .justify_between()
                        .gap_4()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(0xffffff))
                                .child(row.display_name.clone()),
                        )
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
                    let view = cx.new(|_cx| TradeApi {
                        state: LoadState::Loading("leagues"),
                    });

                    let client: Arc<dyn HttpClient> = cx.http_client();
                    let weak = view.downgrade();
                    cx.spawn(async move |cx| {
                        let result = trade_client::search_current_league(
                            &client,
                            SEARCH_CATEGORY,
                            RESULT_LIMIT,
                        )
                        .await
                        .map(|(league, total, items)| {
                            (
                                league,
                                total,
                                items.into_iter().map(ListingRow::from).collect::<Vec<_>>(),
                            )
                        });
                        // Observable, harness-greppable proof independent of screenshot capture.
                        match &result {
                            Ok((league, total, rows)) => println!(
                                "TRADE_RESOLVED league={league:?} total={total} rows={} first={:?}",
                                rows.len(),
                                rows.first()
                                    .map(|r| (&r.display_name, &r.price, &r.account))
                            ),
                            Err(err) => println!("TRADE_ERROR {err}"),
                        }
                        let Some(view) = weak.upgrade() else { return };
                        view.update(cx, |state, cx| {
                            state.state = match result {
                                Ok((league, total, rows)) => LoadState::Resolved {
                                    league,
                                    total,
                                    rows,
                                },
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
