//! Searching and what it finds: the search button, the sellers chip, the search status, the
//! estimate card and the listings table.

use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{
    AnyElement, App, AppContext as _, Context, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, Render, SharedString, Window, div, prelude::*, rgb,
};

use poe2_domain::ParsedItem;
use trade_client::live::MAX_LIVE_SEARCHES;
use trade_client::rates::{Confidence, PriceEstimate, PriceUnit};
use trade_client::{AccountStatus, ListedMod, ListingStatus, ModKind, PriceCurrency};

use crate::listing_match::{self, Asked, WantedStat};
use crate::live_search::LiveSearches;
use crate::price_check::{ListingRow, PriceCheckApp, SearchState};
use crate::relative_time;
use crate::session::SessionStatus;
use crate::ui::theme::{
    BG_BUTTON, BG_BUTTON_HOVER, BG_CONTROL, BG_NAMEPLATE, BG_PANEL, BG_ROW_STRIPE, BORDER,
    BORDER_GOLD, CONTENT_PADDING, GOLD, PRICE_RISE, TEXT, TEXT_DIM, TEXT_MUTED, TEXT_VALUE,
    TEXT_WARNING, TIER_TOP, rems_from_px,
};

use super::format::{amount_in, currency_img, format_ru, format_value};
use super::market::render_market_card;
use crate::ui::hint as hints;

const PRICE_COLUMN: f32 = 176.;
const LEVEL_COLUMN: f32 = 34.;
const LISTED_COLUMN: f32 = 104.;

/// Re-runs the search with the current checkboxes and bounds (Enter in a bound input does too).
pub(super) fn render_search_button(
    state: &PriceCheckApp,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let busy = matches!(
        state.search,
        SearchState::Searching | SearchState::RateLimiting { .. }
    );
    div()
        .mt(rems_from_px(10.))
        .h(rems_from_px(34.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_xs()
        .border_1()
        .border_color(rgb(if busy { BORDER_GOLD } else { GOLD }))
        .bg(rgb(BG_BUTTON))
        .text_base()
        .font_weight(FontWeight::SEMIBOLD)
        .map(|this| {
            if busy {
                this.text_color(rgb(TEXT_DIM)).child("Поиск…")
            } else {
                this.text_color(rgb(GOLD))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(BG_BUTTON_HOVER)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                            view.trigger_search(cx);
                        }),
                    )
                    .child("Поиск")
            }
        })
}

/// Which sellers the search covers -- EE2's Online toggle / PoE Overlay II's "Instant Buyout"
/// dropdown -- and what currency their prices must be in (EE2's price filter), each as one chip
/// that steps through its choices and re-searches.
pub(super) fn render_search_choices(
    state: &PriceCheckApp,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let sellers = match state.listing_status {
        ListingStatus::Available => "выкуп и онлайн",
        ListingStatus::Securable => "только мгновенный выкуп",
        ListingStatus::Online => "только онлайн",
        ListingStatus::OnlineLeague => "онлайн в лиге",
        ListingStatus::Any => "все, включая офлайн",
    };
    // Currencies by their icons, as everywhere in the panel.
    let icon = |id: &str| currency_img(state.currency_icon(id), 14.);
    let currency = div().flex().items_center().gap(rems_from_px(3.));
    let currency = match state.price_currency {
        PriceCurrency::Any => currency.child("любая валюта"),
        PriceCurrency::ExaltedOrDivine => currency
            .children(icon("exalted"))
            .child("или")
            .children(icon("divine")),
        PriceCurrency::Exalted => currency.child("только").children(icon("exalted")),
        PriceCurrency::Divine => currency.child("только").children(icon("divine")),
        PriceCurrency::Chaos => currency.child("только").children(icon("chaos")),
    };
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(rems_from_px(6.))
        .mt(rems_from_px(8.))
        .text_xs()
        .child(div().text_color(rgb(TEXT_DIM)).child("Продавцы:"))
        .child(choice_chip(
            sellers.into_any_element(),
            "Каких продавцов искать: выкуп — купить сразу, онлайн — договориться в игре. \
             Нажмите, чтобы сменить; поиск повторится.",
            PriceCheckApp::cycle_listing_status,
            cx,
        ))
        .child(
            div()
                .ml(rems_from_px(6.))
                .text_color(rgb(TEXT_DIM))
                .child("Цена:"),
        )
        .child(choice_chip(
            currency.into_any_element(),
            "В какой валюте должна быть цена лота. Нажмите, чтобы сменить; поиск повторится.",
            PriceCheckApp::cycle_price_currency,
            cx,
        ))
}

fn choice_chip(
    label: AnyElement,
    hint: &'static str,
    step: fn(&mut PriceCheckApp, &mut Context<PriceCheckApp>),
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    div()
        .id(hint)
        .flex()
        .flex_none()
        .items_center()
        .gap(rems_from_px(3.))
        .px(rems_from_px(8.))
        .py(rems_from_px(2.))
        .rounded_xs()
        .bg(rgb(BG_CONTROL))
        .text_color(rgb(GOLD))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(BG_BUTTON_HOVER)))
        .tooltip(hints::hint(hint))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| step(view, cx)),
        )
        .child(label)
        .child("▾")
}

/// The pricing outcome: the market card for a Currency Exchange item, otherwise the estimate and
/// the listings -- for a unique poe2scout prices, its price there first, whatever the search says.
pub(super) fn render_results(
    state: &PriceCheckApp,
    item: &ParsedItem,
    cx: &Context<PriceCheckApp>,
) -> AnyElement {
    let outcome = render_outcome(state, item, cx);
    match state.scout_unique_price(item) {
        Some(price) if !matches!(state.search, SearchState::NotSearched) => div()
            .flex()
            .flex_col()
            .child(render_scout_line(state, price))
            .child(outcome)
            .into_any_element(),
        _ => outcome,
    }
}

/// poe2scout's price of a unique, averaged over the trade site's listings: a second opinion
/// beside this item's own search, and the only one when the search fails or finds nothing.
fn render_scout_line(state: &PriceCheckApp, (value, unit): (f64, PriceUnit)) -> impl IntoElement {
    div()
        .mt(rems_from_px(10.))
        .flex()
        .items_center()
        .justify_center()
        .gap(rems_from_px(5.))
        .text_sm()
        .text_color(rgb(TEXT_DIM))
        .child("Цена по poe2scout:")
        .child(
            div()
                .text_color(rgb(TEXT))
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("≈ {}", format_ru(value))),
        )
        .children(currency_img(state.currency_icon(unit.trade_id()), 18.))
}

/// Why an exchange item has no market card: poe.ninja has no line for it in the league (a thin
/// league like Standard lists only its busiest items), or poe.ninja itself is out of reach -- said
/// above whatever prices it instead.
fn render_market_gap(state: &PriceCheckApp) -> impl IntoElement {
    let text = match state.market() {
        Some(_) => format!(
            "poe.ninja не отслеживает этот предмет в лиге {}.",
            state.league()
        ),
        None => "poe.ninja сейчас недоступен.".to_owned(),
    };
    div()
        .mt(rems_from_px(10.))
        .text_sm()
        .text_center()
        .text_color(rgb(TEXT_DIM))
        .child(text)
}

/// Asks for the trade listings of an exchange item poe2scout priced
/// (`PriceCheckApp::search_listings`): each search spends the trade site's per-IP budget, so
/// they come only on request.
fn render_listings_button(cx: &Context<PriceCheckApp>) -> impl IntoElement {
    div()
        .id("listings-on-trade-site")
        .mt(rems_from_px(10.))
        .flex_none()
        .px(rems_from_px(10.))
        .py(rems_from_px(3.))
        .rounded_xs()
        .border_1()
        .border_color(rgb(BORDER_GOLD))
        .bg(rgb(BG_CONTROL))
        .text_sm()
        .text_color(rgb(GOLD))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(BG_BUTTON_HOVER)))
        .tooltip(hints::hint(
            "Найти лоты этого предмета на сайте торговли. Каждый поиск расходует лимит \
             запросов площадки.",
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _event: &MouseDownEvent, _window, cx| view.search_listings(cx)),
        )
        .child("Лоты на площадке")
}

fn render_outcome(
    state: &PriceCheckApp,
    item: &ParsedItem,
    cx: &Context<PriceCheckApp>,
) -> AnyElement {
    let status = |text: String, color: u32| {
        div()
            .mt(rems_from_px(10.))
            .text_color(rgb(color))
            .child(text)
            .into_any_element()
    };
    match &state.search {
        SearchState::NotSearched => div().into_any_element(),
        SearchState::Searching if state.priced_by_market => {
            status("Загрузка цен poe.ninja…".to_owned(), TEXT_DIM)
        }
        SearchState::Searching => status("Поиск…".to_owned(), TEXT_DIM),
        SearchState::RateLimiting { wait_secs } => status(
            format!("Лимит запросов trade API — ждём {wait_secs} с…"),
            TEXT_DIM,
        ),
        SearchState::Failed(msg) => status(msg.clone(), TEXT_WARNING),
        SearchState::Empty if state.priced_by_market => div()
            .flex()
            .flex_col()
            .child(render_market_gap(state))
            .child(status("На площадке лотов нет".to_owned(), TEXT_DIM))
            .into_any_element(),
        SearchState::Empty => status("Ничего не найдено".to_owned(), TEXT_DIM),
        SearchState::Market(price) => match state.market() {
            Some(market) => render_market_card(state, item, market, price).into_any_element(),
            None => status("Загрузка цен poe.ninja…".to_owned(), TEXT_DIM),
        },
        SearchState::Scouted { value, unit } => div()
            .flex()
            .flex_col()
            .items_center()
            .child(render_market_gap(state))
            .child(render_scout_line(state, (*value, *unit)))
            .child(render_listings_button(cx))
            .into_any_element(),
        SearchState::Matched {
            total,
            rows,
            trade_url,
            estimate,
            relaxed,
        } => div()
            .flex()
            .flex_col()
            .mt(rems_from_px(10.))
            .children(
                state
                    .priced_by_market
                    .then(|| div().mb(rems_from_px(8.)).child(render_market_gap(state))),
            )
            .children(relaxed.map(|(least, of)| {
                div()
                    .mb(rems_from_px(8.))
                    .text_sm()
                    .text_center()
                    .text_color(rgb(TIER_TOP))
                    .child(format!(
                        "Точных совпадений нет — показаны предметы хотя бы с {least} из {of} \
                         выбранных свойств"
                    ))
            }))
            .children(
                estimate
                    .as_ref()
                    .map(|estimate| render_estimate(state, estimate, cx)),
            )
            .child(render_matched_line(state, *total, trade_url.clone(), cx))
            .child(render_results_table(state, rows, *relaxed, cx))
            .into_any_element(),
    }
}

/// PoE Overlay II's estimated-value card: the value in divine or exalted with its icon, the range
/// it rests on, and how far to trust it -- plus EE2's warning when the listings are mostly in odd
/// currencies (likely price-fixing), in which case only the common-currency ones were counted.
fn render_estimate(
    state: &PriceCheckApp,
    estimate: &PriceEstimate,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let unit = estimate.unit.trade_id();
    let (confidence, confidence_color) = match estimate.confidence {
        Confidence::High => ("высокая", 0x68d391),
        Confidence::Medium => ("средняя", TIER_TOP),
        Confidence::Low => ("низкая", TEXT_WARNING),
    };
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(rems_from_px(2.))
        .mb(rems_from_px(10.))
        .py(rems_from_px(8.))
        .px(rems_from_px(CONTENT_PADDING))
        .rounded_xs()
        .border_1()
        .border_color(rgb(BORDER_GOLD))
        .bg(rgb(BG_NAMEPLATE))
        .child(
            div()
                .text_xs()
                .text_color(rgb(TEXT_DIM))
                .child("Оценочная стоимость"),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(rems_from_px(6.))
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("≈ {}", format_ru(estimate.value)))
                .children(currency_img(state.currency_icon(unit), 24.)),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .justify_center()
                .gap(rems_from_px(10.))
                .text_xs()
                .text_color(rgb(TEXT_DIM))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(rems_from_px(4.))
                        .child("Диапазон:")
                        .child(amount_in(
                            state,
                            format!(
                                "{}–{}",
                                format_ru(estimate.low),
                                format_ru(estimate.high)
                            ),
                            unit,
                            14.,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .gap(rems_from_px(4.))
                        .child("Надёжность:")
                        .child(div().text_color(rgb(confidence_color)).child(confidence)),
                ),
        )
        .when(estimate.likely_price_fixed, |this| {
            this.child(
                div()
                    .text_xs()
                    .text_center()
                    .text_color(rgb(TEXT_WARNING))
                    .child("Цены в основном в редкой валюте: учтены только хаос, возвышения и божественные"),
            )
            // EE2's remedy: the same search among listings priced in exalted or divine orbs.
            .when(state.price_currency == PriceCurrency::Any, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(rems_from_px(4.))
                        .text_xs()
                        .text_color(rgb(GOLD))
                        .cursor_pointer()
                        .hover(|style| style.text_color(rgb(TEXT)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                                view.set_price_currency(PriceCurrency::ExaltedOrDivine, cx);
                            }),
                        )
                        .child("Искать только цены в")
                        .children(currency_img(state.currency_icon("exalted"), 14.))
                        .child("или")
                        .children(currency_img(state.currency_icon("divine"), 14.)),
                )
            })
        })
}

/// The results' header: how many the search found, the watch toggle and the trade site link.
fn render_matched_line(
    state: &PriceCheckApp,
    total: u64,
    trade_url: String,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let host = trade_url
        .split('/')
        .nth(2)
        .unwrap_or("pathofexile.com")
        .to_owned();
    div()
        .flex()
        .flex_col()
        .gap(rems_from_px(4.))
        .pb(rems_from_px(6.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(rems_from_px(8.))
                .child(
                    div()
                        .flex()
                        .gap(rems_from_px(4.))
                        .child(div().text_color(rgb(TEXT_DIM)).child("Найдено:"))
                        .child(total.to_string()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(rems_from_px(8.))
                        .children(render_watch(state, cx))
                        .child(render_link(format!("{host}/trade ↗"), trade_url)),
                ),
        )
        .children(render_watch_refusal(state, cx))
}

/// Under "Ничего не найдено": a search nothing matches yet is just what watching is for.
pub(super) fn render_empty_watch(
    state: &PriceCheckApp,
    cx: &Context<PriceCheckApp>,
) -> Option<impl IntoElement> {
    if !matches!(state.search, SearchState::Empty) || state.priced_by_market {
        return None;
    }
    let toggle = render_watch(state, cx)?;
    Some(
        div()
            .mt(rems_from_px(6.))
            .flex()
            .flex_col()
            .items_center()
            .gap(rems_from_px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(rems_from_px(6.))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(TEXT_DIM))
                            .child("Сообщить, когда появится:"),
                    )
                    .child(toggle),
            )
            .children(render_watch_refusal(state, cx)),
    )
}

/// "Следить": the search's new listings come as cards over the game (`crate::live_search`) --
/// with how many searches are watched. Only for a signed-in player (`crate::session`) and a
/// search the trade site answered.
fn render_watch(state: &PriceCheckApp, cx: &Context<PriceCheckApp>) -> Option<impl IntoElement> {
    let search = state.watchable.clone()?;
    if !cx.try_global::<SessionStatus>()?.signed_in() {
        return None;
    }
    let live = cx.try_global::<LiveSearches>()?;
    let watching = live.is_watched(&search.query_id);
    let count = live.count();
    let (label, hint) = if watching {
        ("◉ Слежу", "Слежение включено — нажмите, чтобы снять")
    } else {
        (
            "Следить",
            "Новые лоты этого поиска будут приходить карточками поверх игры, пока приложение \
             запущено",
        )
    };
    Some(
        div()
            .flex()
            .items_center()
            .gap(rems_from_px(6.))
            .child(
                div()
                    .id("watch")
                    .flex_none()
                    .px(rems_from_px(8.))
                    .py(rems_from_px(2.))
                    .rounded_xs()
                    .border_1()
                    .text_xs()
                    .text_color(rgb(GOLD))
                    .map(|this| {
                        if watching {
                            this.bg(rgb(BG_BUTTON)).border_color(rgb(GOLD))
                        } else {
                            this.bg(rgb(BG_CONTROL)).border_color(rgb(BG_CONTROL))
                        }
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(BG_BUTTON_HOVER)))
                    .tooltip(hints::hint(hint))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_view, _event: &MouseDownEvent, _window, cx| {
                            cx.global_mut::<LiveSearches>().toggle(&search);
                        }),
                    )
                    .child(label),
            )
            .when(count > 0, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(rgb(TEXT_DIM))
                        .child(format!("поисков: {count} из {MAX_LIVE_SEARCHES}")),
                )
            }),
    )
}

/// Why watching the shown search was just refused.
fn render_watch_refusal(
    state: &PriceCheckApp,
    cx: &Context<PriceCheckApp>,
) -> Option<impl IntoElement> {
    let search = state.watchable.as_ref()?;
    let reason = cx.try_global::<LiveSearches>()?.refusal(&search.query_id)?;
    Some(div().text_xs().text_color(rgb(TEXT_WARNING)).child(reason))
}

/// A text link opened in the default browser via GPUI's own `App::open_url`.
pub(super) fn render_link(label: impl Into<String>, url: String) -> impl IntoElement {
    div()
        .flex_none()
        .text_xs()
        .text_color(rgb(TEXT_DIM))
        .cursor_pointer()
        .hover(|style| style.text_color(rgb(GOLD)))
        .on_mouse_down(
            MouseButton::Left,
            move |_event: &MouseDownEvent, _window, cx: &mut App| {
                cx.open_url(&url);
            },
        )
        .child(label.into())
}

/// EE2's striped results table, plus the seller column (the player can turn it off; the space
/// then stays empty so the dates keep their place) and currency icons PoE Overlay II shows. A
/// seller who sells in person gets a ✉: clicking the row copies the whisper for the game's chat.
/// Hovering a row lists the listed item's mods (`ListingTooltip`); after a relaxed search
/// (`relaxed`: at least `.0` of the `.1` stat rows) each row says how many it has.
fn render_results_table(
    state: &PriceCheckApp,
    rows: &[ListingRow],
    relaxed: Option<(u32, u32)>,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let show_seller = state.settings.show_seller_column;
    // The stats the search asks for, for the rows' tooltips to check the listings against.
    let wanted = WantedStat::from_filters(&state.filters);
    // Pseudo totals and free slots count toward a relaxed search too, but no listed mod shows
    // them: the rows' counts only add up to the banner's when every searched row is a mod's.
    let count_matches = relaxed.is_some_and(|(_, of)| of as usize == wanted.len());
    div()
        .flex()
        .flex_col()
        .child(
            table_row()
                .border_b_1()
                .border_color(rgb(BORDER))
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(price_cell().child("Цена"))
                .child(level_cell().child("Ур."))
                .child(seller_cell().when(show_seller, |this| this.child("Продавец")))
                .child(listed_cell().child("Выставлен")),
        )
        .children(rows.iter().enumerate().map(|(index, row)| {
            let listed = relative_time::listed_ago(&row.indexed, now_unix).unwrap_or_default();
            // For a few seconds after a click the seller's cell says the whisper is on the
            // clipboard, over the listed-time column too: the note doesn't fit the seller's alone.
            let copied = state.whisper_copied(index);
            let seller: SharedString = if copied {
                "✓ скопировано — вставьте в чат".into()
            } else {
                match (&row.whisper, show_seller) {
                    (Some(_), true) => format!("✉ {}", row.account_name).into(),
                    (Some(_), false) => "✉".into(),
                    (None, true) => row.account_name.clone().into(),
                    (None, false) => SharedString::default(),
                }
            };
            let tooltip = ListingTooltip::new(&row.mods, &wanted, marker_note(row));
            let matched = count_matches
                .then(|| listing_match::matched_count(&row.mods, &wanted))
                .flatten()
                .filter(|_| !copied);
            table_row()
                .id(("listing", index))
                .when(index % 2 == 0, |this| this.bg(rgb(BG_ROW_STRIPE)))
                .when_some(tooltip, |this, tooltip| {
                    this.tooltip(move |_window, cx| cx.new(|_| tooltip.clone()).into())
                })
                .when_some(row.whisper.clone(), |this, whisper| {
                    this.cursor_pointer()
                        .hover(|style| style.bg(rgb(BG_BUTTON_HOVER)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                                view.copy_whisper(index, whisper.clone(), cx);
                            }),
                        )
                })
                .child(render_price(state, row))
                .child(
                    level_cell().text_color(rgb(TEXT_DIM)).child(
                        // Gems and currency list item level 0: nothing to show.
                        row.item_level
                            .filter(|&level| level > 0)
                            .map(|level| level.to_string())
                            .unwrap_or_default(),
                    ),
                )
                .child(
                    seller_cell()
                        .flex()
                        .gap(rems_from_px(6.))
                        .text_xs()
                        .text_color(rgb(if copied { PRICE_RISE } else { TEXT_DIM }))
                        .child(div().min_w_0().truncate().child(seller))
                        .children(matched.map(|(has, of)| {
                            div()
                                .flex_none()
                                .text_color(rgb(if has == of { PRICE_RISE } else { TEXT_MUTED }))
                                .child(format!("{has}/{of}"))
                        })),
                )
                .when(!copied, |this| {
                    this.child(
                        listed_cell()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(rems_from_px(5.))
                            .text_xs()
                            .text_color(rgb(TEXT_DIM))
                            .child(status_dot(row))
                            .child(listed),
                    )
                })
        }))
}

/// `от 200`, `до 5`, `200–250`: the bounds a roll fell short of.
fn bounds_text(min: Option<f64>, max: Option<f64>) -> String {
    match (min, max) {
        (Some(min), Some(max)) => format!("{}–{}", format_value(min), format_value(max)),
        (Some(min), None) => format!("от {}", format_value(min)),
        (None, Some(max)) => format!("до {}", format_value(max)),
        (None, None) => String::new(),
    }
}

/// A results row's tooltip: the listed item's mods, the ones whose stat the search asks for
/// marked met or short (`listing_match::assess`), the asked-for stats the item lacks -- which a
/// relaxed search lets through -- and what the row's price marker means.
#[derive(Clone)]
struct ListingTooltip {
    mods: Vec<(ListedMod, Asked)>,
    /// The asked-for stats' texts the item has no mod for; empty when the site gave no stat
    /// ids to tell by.
    missing: Vec<String>,
    /// `marker_note`'s explanation of the "× N" or "?" beside the price.
    marker: Option<String>,
}

impl ListingTooltip {
    /// `None` for a listing with neither mods (currency, a gem the site lists bare) nor a price
    /// marker to explain.
    fn new(
        mods: &[ListedMod],
        wanted: &[WantedStat],
        marker: Option<String>,
    ) -> Option<ListingTooltip> {
        if mods.is_empty() && marker.is_none() {
            return None;
        }
        Some(ListingTooltip {
            missing: listing_match::missing(mods, wanted),
            mods: mods
                .iter()
                .map(|listed| (listed.clone(), listing_match::assess(listed, wanted)))
                .collect(),
            marker,
        })
    }
}

/// What the marker `render_price` puts beside a row's price means, if it puts one.
fn marker_note(row: &ListingRow) -> Option<String> {
    if row.listed_times > 2 {
        Some(format!(
            "× {0}: продавец выставил {0} таких по этой цене",
            row.listed_times
        ))
    } else if !row.has_note {
        Some(
            "?: цена взята из названия вкладки тайника, а не из заметки к вещи — возможно, \
             не настоящая"
                .to_owned(),
        )
    } else {
        None
    }
}

impl Render for ListingTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(rems_from_px(400.))
            .flex()
            .flex_col()
            .gap(rems_from_px(2.))
            .p(rems_from_px(8.))
            .rounded_xs()
            .bg(rgb(BG_PANEL))
            .border_1()
            .border_color(rgb(BORDER_GOLD))
            .text_xs()
            .children(self.mods.iter().map(|(listed, asked)| {
                let (color, text) = match asked {
                    Asked::Met => (PRICE_RISE, format!("✓ {}", listed.text)),
                    Asked::Short { min, max } => (
                        TEXT_WARNING,
                        format!("✗ {} (нужно {})", listed.text, bounds_text(*min, *max)),
                    ),
                    Asked::No => {
                        let color = match listed.kind {
                            ModKind::Implicit | ModKind::Enchant | ModKind::Rune => TEXT_DIM,
                            ModKind::Fractured => TIER_TOP,
                            ModKind::Explicit | ModKind::Desecrated => TEXT_VALUE,
                        };
                        (color, listed.text.clone())
                    }
                };
                div()
                    .flex()
                    .gap(rems_from_px(6.))
                    .child(
                        div()
                            .w(rems_from_px(22.))
                            .flex_none()
                            .text_color(rgb(TEXT_MUTED))
                            .child(listed.tier.clone().unwrap_or_default()),
                    )
                    // Width limits on every text line, as the plain hint has: a line that only
                    // wraps inside the column wraps after the tooltip's height is taken and runs
                    // out of the box.
                    .child(
                        div()
                            .flex_1()
                            .max_w(rems_from_px(300.))
                            .text_color(rgb(color))
                            .child(text),
                    )
                    // The item level the mod needs: whether a lower-level base could roll it.
                    .children(listed.level.map(|level| {
                        div()
                            .flex_none()
                            .pl(rems_from_px(8.))
                            .text_color(rgb(TEXT_MUTED))
                            .child(format!("ур. {level}"))
                    }))
            }))
            .when(!self.missing.is_empty(), |this| {
                this.child(
                    div()
                        .mt(rems_from_px(4.))
                        .text_color(rgb(TEXT_WARNING))
                        .child("Нет у этого предмета:"),
                )
                .children(self.missing.iter().map(|text| {
                    div()
                        .max_w(rems_from_px(384.))
                        .text_color(rgb(TEXT_WARNING))
                        .child(format!("✗ {text}"))
                }))
            })
            // See the mods' lines for the width limit.
            .children(self.marker.clone().map(|marker| {
                div()
                    .max_w(rems_from_px(384.))
                    .when(!self.mods.is_empty(), |this| this.mt(rems_from_px(4.)))
                    .text_color(rgb(TEXT_DIM))
                    .child(marker)
            }))
    }
}

/// Amount and currency icon (the currency's name where there's no icon), EE2's markers
/// (`TradeItem.vue`): "× N" when the seller listed it several times at this price, otherwise "?"
/// when the price comes from a stash tab's name rather than the item's own note -- likely not a
/// real price -- and, for anything but divines and exalts, what it's worth in those.
fn render_price(state: &PriceCheckApp, row: &ListingRow) -> impl IntoElement {
    let currency = row.price_currency.as_str();
    let icon = currency_img(state.currency_icon(currency), 18.);
    let name = icon
        .is_none()
        .then(|| state.currency_name(currency).unwrap_or(currency).to_owned());
    let equivalent = (!matches!(currency, "divine" | "exalted"))
        .then(|| state.market())
        .flatten()
        .and_then(|market| {
            let divines = row.price_amount * market.value_in_divines(currency)?;
            let (value, unit) = market.in_display_unit(divines);
            Some((format!("≈ {}", format_ru(value)), unit.trade_id()))
        });
    price_cell()
        .flex()
        .items_center()
        .gap(rems_from_px(4.))
        .child(format_ru(row.price_amount))
        .children(icon)
        .children(name.map(|name| {
            div()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(rgb(TEXT_DIM))
                .child(name)
        }))
        .map(|this| {
            if row.listed_times > 2 {
                this.child(
                    div()
                        .flex_none()
                        .px(rems_from_px(4.))
                        .rounded_xs()
                        .bg(rgb(0xcbd5e0))
                        .text_xs()
                        .text_color(rgb(0x2d3748))
                        .child(format!("× {}", row.listed_times)),
                )
            } else if !row.has_note {
                this.child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT_DIM))
                        .child("?"),
                )
            } else {
                this
            }
        })
        .children(equivalent.map(|(text, unit)| {
            div()
                .min_w_0()
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(amount_in(state, text, unit, 12.))
        }))
}

/// EE2's seller dot (`TradeListing.vue`'s `.account-status`): pink online, orange AFK, red
/// offline. Instant-buyout listings keep the slot empty so the times stay aligned.
fn status_dot(row: &ListingRow) -> impl IntoElement {
    let color = match (row.instant_buyout, row.account_status) {
        (true, _) => None,
        (false, AccountStatus::Online) => Some(0xf687b3),
        (false, AccountStatus::Afk) => Some(0xed8936),
        (false, AccountStatus::Offline) => Some(0xe53e3e),
    };
    div()
        .w(rems_from_px(6.))
        .h(rems_from_px(6.))
        .flex_none()
        .rounded_full()
        .when_some(color, |this, color| this.bg(rgb(color)))
}

fn table_row() -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(rems_from_px(8.))
        .px(rems_from_px(6.))
        .py(rems_from_px(3.))
}

fn price_cell() -> gpui::Div {
    div().w(rems_from_px(PRICE_COLUMN)).flex_none().truncate()
}

fn level_cell() -> gpui::Div {
    div().w(rems_from_px(LEVEL_COLUMN)).flex_none().text_right()
}

fn seller_cell() -> gpui::Div {
    div().flex_1().min_w_0().truncate()
}

fn listed_cell() -> gpui::Div {
    div()
        .w(rems_from_px(LISTED_COLUMN))
        .flex_none()
        .text_right()
        .truncate()
}
