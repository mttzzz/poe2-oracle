//! Searching and what it finds: the profile select and «Минимум тира» above the stats, the
//! «Поиск» plate with the sellers and price selects beside it, the search status -- with the
//! broader searches offered when nothing matched -- and the listings table.

use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, Render, SharedString, Window, div, prelude::*, rgb,
};

use poe2_domain::ParsedItem;
use stat_filters::SearchProfile;
use trade_client::live::MAX_LIVE_SEARCHES;
use trade_client::rates::PriceUnit;
use trade_client::{AccountStatus, ListedItem, ListedMod, ListingStatus, PriceCurrency};

use crate::i18n;
use crate::listing_match::{self, Asked, WantedStat};
use crate::live_search::LiveSearches;
use crate::price_check::{ListingRow, PriceCheckApp, SearchFailure, SearchState};
use crate::relative_time;
use crate::session::SessionStatus;
use crate::tour::Stop;
use crate::tr;
use crate::ui::fonts;
use crate::ui::hint as hints;
use crate::ui::item_card::{CardPrice, ItemCard, ModMark, render_item_card};
use crate::ui::style::{
    ButtonKind, CONTROL_RADIUS, alpha, button, ease_hover, glow, inner_glow, link, menu_row, plate,
    select, switch,
};
use crate::ui::theme::{
    BORDER_GOLD, BORDER_ROW, GOLD, GOLD_LIGHT, PRICE_RISE, STATUS_AFK, STATUS_OFFLINE,
    STATUS_ONLINE, TEXT, TEXT_DIM, TEXT_MUTED, TEXT_WARNING, TIER_TOP, blend, rems_from_px,
};
use crate::ui::tour;

use super::format::{amount_in, currency_img, format_value};
use super::market::render_market_card;
use super::menu::render_menu;

const PRICE_COLUMN: f32 = 150.;
const LEVEL_COLUMN: f32 = 30.;
const LISTED_COLUMN: f32 = 100.;
/// The profile menu is at least this wide, so each profile's note fits on one line.
const PROFILE_MENU_WIDTH: f32 = 200.;
/// The «Поиск» plate is at least this wide beside the selects; a narrower row puts the selects
/// under it.
const SEARCH_MIN_WIDTH: f32 = 140.;

/// The profiles the select offers, in PoE Overlay II's order.
const PROFILES: [SearchProfile; 4] = [
    SearchProfile::QuickPrice,
    SearchProfile::ExactMatch,
    SearchProfile::Broad,
    SearchProfile::CraftingBase,
];

/// The row above the stats: how the search reads the item -- its profile, PoE Overlay II's
/// evaluate profiles, for an item searched by its rows -- and on the right «Минимум тира», while a
/// checked row has a tier to take its minimum from. `None` when there is neither.
pub(super) fn render_toolbar(
    state: &PriceCheckApp,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> Option<impl IntoElement> {
    let tier_minimums = state.has_tier_minimums();
    if state.profile.is_none() && !tier_minimums {
        return None;
    }
    let profile = state.profile.map(|profile| {
        div()
            .flex()
            .items_center()
            .gap(rems_from_px(6.))
            .min_w_0()
            .child(
                div()
                    .flex_none()
                    .text_size(rems_from_px(12.))
                    .text_color(rgb(TEXT_DIM))
                    .child(tr!("Profile:")),
            )
            .child(
                div()
                    .id("profile")
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .tooltip(hints::hint(tr!(
                        "How to search for this item: which stats are checked and their \
                         minimums. Picking a profile searches again at once."
                    )))
                    .child(select(
                        "select",
                        profile_label(profile),
                        true,
                        cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                            view.set_profile_menu(true, cx);
                        }),
                    ))
                    .when(state.profile_menu, |this| {
                        this.child(render_profile_menu(state, window, cx))
                    }),
            )
    });
    Some(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(rems_from_px(8.))
            .pt(rems_from_px(6.))
            .children(profile)
            .when(tier_minimums, |this| {
                this.child(
                    action_button(
                        "tier-minimums",
                        tr!("Tier minimum"),
                        tr!(
                            "Sets the minimum of every checked stat with a known tier to the \
                             bottom of that tier: the search finds items with the same tier or \
                             better. Search with the “Search” button."
                        ),
                        PriceCheckApp::set_tier_minimums,
                        cx,
                    )
                    .ml_auto(),
                )
            }),
    )
}

/// A profile's name, as the select and its menu say it.
fn profile_label(profile: SearchProfile) -> &'static str {
    match profile {
        SearchProfile::QuickPrice => tr!("Quick price"),
        SearchProfile::ExactMatch => tr!("Exact match"),
        SearchProfile::Broad => tr!("Broad −10%"),
        SearchProfile::CraftingBase => tr!("Crafting base"),
    }
}

/// What a profile searches, under its name in the menu.
fn profile_note(profile: SearchProfile) -> &'static str {
    match profile {
        SearchProfile::QuickPrice => tr!("up to 4 most valuable stats, your rolls as minimums"),
        SearchProfile::ExactMatch => tr!("all stats, your rolls as minimums"),
        SearchProfile::Broad => tr!("checked stats, minimums 10% lower"),
        SearchProfile::CraftingBase => tr!("implicit and fractured stats on the same base"),
    }
}

/// The profile select's menu: every profile with what it searches under its name, the current
/// one marked; a pick sets the rows up and searches (`PriceCheckApp::choose_profile`).
fn render_profile_menu(
    state: &PriceCheckApp,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let rows = PROFILES.into_iter().enumerate().map(|(index, profile)| {
        menu_row(
            index,
            state.profile == Some(profile),
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(profile_label(profile))
                .child(
                    div()
                        .text_size(rems_from_px(12.))
                        .text_color(rgb(TEXT_DIM))
                        .child(profile_note(profile)),
                ),
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                view.choose_profile(profile, cx);
            }),
        )
    });
    render_menu(
        "profile-menu",
        rows,
        PROFILE_MENU_WIDTH,
        |view, cx| view.set_profile_menu(false, cx),
        window,
        cx,
    )
}

/// The bronze «Поиск» plate: re-runs the search with the current checkboxes and bounds (Enter in
/// a bound input does too). Dimmed and deaf while a search runs.
fn render_search_button(state: &PriceCheckApp, cx: &Context<PriceCheckApp>) -> impl IntoElement {
    let busy = matches!(
        state.search,
        SearchState::Searching | SearchState::RateLimiting { .. }
    );
    let face = fonts::interface_font();
    let search = div()
        .id("search")
        .flex()
        .items_center()
        .justify_center()
        .h(rems_from_px(36.))
        .rounded(rems_from_px(CONTROL_RADIUS))
        .border_1()
        .font_family(face.family)
        .font_weight(face.weight)
        .text_size(rems_from_px(16.))
        .map(|this| {
            if busy {
                this.child(tr!("Searching…"))
            } else {
                this.cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                            view.trigger_search(cx);
                        }),
                    )
                    .child(tr!("Search"))
            }
        });
    ease_hover("search", search, move |search, hover| {
        if busy {
            search
                .bg(plate(0.))
                .border_color(rgb(BORDER_GOLD))
                .text_color(rgb(TEXT_DIM))
        } else {
            search
                .bg(plate(hover))
                .border_color(rgb(blend(GOLD, GOLD_LIGHT, hover)))
                .text_color(rgb(GOLD_LIGHT))
                .shadow(glow(GOLD, hover))
        }
    })
}

/// The search row: the «Поиск» plate -- the tour's Search stop -- taking the width the selects
/// leave, and on its right which sellers the search covers (EE2's Online toggle / PoE Overlay
/// II's "Instant Buyout" dropdown) and what currency their prices must be in (EE2's price
/// filter). The choices name themselves, the tooltips name the selects; a press steps a select to
/// its next choice and searches again. On a narrow panel the selects wrap under the plate.
pub(super) fn render_search_row(
    state: &PriceCheckApp,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let sellers = match state.listing_status {
        ListingStatus::Securable => tr!("Instant Buyout"),
        ListingStatus::Available => tr!("Buyout or In Person"),
        ListingStatus::Online => tr!("In Person"),
        ListingStatus::OnlineLeague => tr!("In Person (Online in League)"),
        ListingStatus::Any => tr!("Any"),
    };
    // Currencies by their icons, as everywhere in the panel.
    let icon = |id: &str| currency_img(state.currency_icon(id), 14.);
    let currency = div().flex().items_center().gap(rems_from_px(3.));
    let currency = match state.price_currency {
        PriceCurrency::Any => currency.child(tr!("Any currency")),
        PriceCurrency::ExaltedOrDivine => currency
            .children(icon("exalted"))
            .child(tr!("or"))
            .children(icon("divine")),
        PriceCurrency::Exalted => currency.child(tr!("Only")).children(icon("exalted")),
        PriceCurrency::Divine => currency.child(tr!("Only")).children(icon("divine")),
        PriceCurrency::Chaos => currency.child(tr!("Only")).children(icon("chaos")),
    };
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(rems_from_px(8.))
        .mt(rems_from_px(10.))
        .child(
            div()
                .flex_1()
                .min_w(rems_from_px(SEARCH_MIN_WIDTH))
                .child(tour::spot(Stop::Search, render_search_button(state, cx))),
        )
        // The two selects keep together when the row wraps.
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(rems_from_px(8.))
                .min_w_0()
                .child(stepping_select(
                    "sellers",
                    sellers.into_any_element(),
                    tr!(
                        "Sellers: Instant Buyout — buy at once, In Person — agree on the trade in \
                         the game, Any — offline sellers too. Click to change; the search runs \
                         again."
                    ),
                    PriceCheckApp::cycle_listing_status,
                    cx,
                ))
                .child(stepping_select(
                    "currency",
                    currency.into_any_element(),
                    tr!(
                        "Price: the currency a listing's price must be in. Click to change; the \
                         search runs again."
                    ),
                    PriceCheckApp::cycle_price_currency,
                    cx,
                )),
        )
}

/// A select showing `value` that a press steps to its next choice (`step`), saying on hover what
/// it is and does.
fn stepping_select(
    key: &'static str,
    value: AnyElement,
    hint: &'static str,
    step: fn(&mut PriceCheckApp, &mut Context<PriceCheckApp>),
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    div()
        .id(key)
        .flex()
        .min_w_0()
        .tooltip(hints::hint(hint))
        .child(select(
            "select",
            value,
            true,
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| step(view, cx)),
        ))
}

/// A secondary button: a search, or a change to the rows, the player asks for -- saying on hover
/// what it does (`hint`).
fn action_button(
    key: &'static str,
    label: impl Into<SharedString>,
    hint: impl Into<SharedString>,
    press: fn(&mut PriceCheckApp, &mut Context<PriceCheckApp>),
    cx: &Context<PriceCheckApp>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(key)
        .flex_none()
        .tooltip(hints::hint(hint))
        .child(button(
            "button",
            label,
            ButtonKind::Secondary,
            fonts::interface_font(),
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| press(view, cx)),
        ))
}

/// The pricing outcome: the market card for a Currency Exchange item, otherwise the listings --
/// for a unique poe2scout prices, its price there first, whatever the search says.
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
        .mt(rems_from_px(12.))
        .flex()
        .items_center()
        .justify_center()
        .gap(rems_from_px(5.))
        .text_color(rgb(TEXT_DIM))
        .child(tr!("poe2scout price:"))
        .child(
            div()
                .text_color(rgb(TEXT))
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("≈ {}", i18n::number(value))),
        )
        .children(currency_img(state.currency_icon(unit.trade_id()), 18.))
}

/// Why an exchange item has no market card: it didn't trade on the exchange in the hours GGG's
/// record covers (a thin league like Standard trades only its busiest items every hour), or the
/// record is out of reach -- said above whatever prices it instead.
fn render_market_gap(state: &PriceCheckApp) -> impl IntoElement {
    let text = match state.market().and_then(|market| market.hours) {
        Some(_) => tr!(
            "Nobody traded this item on the Currency Exchange in {league} in the last hours.",
            league = state.league()
        ),
        None => tr!("GGG's Currency Exchange data is unavailable right now.").to_owned(),
    };
    div()
        .mt(rems_from_px(12.))
        .text_center()
        .text_color(rgb(TEXT_DIM))
        .child(text)
}

/// Asks for the trade listings of an exchange item poe2scout priced
/// (`PriceCheckApp::search_listings`): each search spends the trade site's per-IP budget, so
/// they come only on request.
fn render_listings_button(cx: &Context<PriceCheckApp>) -> impl IntoElement {
    action_button(
        "listings-on-trade-site",
        tr!("Trade site listings"),
        tr!(
            "Find this item's listings on the trade site. Each search spends part of the site's \
             request limit."
        ),
        PriceCheckApp::search_listings,
        cx,
    )
    .mt(rems_from_px(10.))
}

fn render_outcome(
    state: &PriceCheckApp,
    item: &ParsedItem,
    cx: &Context<PriceCheckApp>,
) -> AnyElement {
    let status = |text: SharedString, color: u32| {
        div()
            .mt(rems_from_px(12.))
            .text_color(rgb(color))
            .child(text)
            .into_any_element()
    };
    match &state.search {
        SearchState::NotSearched => div().into_any_element(),
        SearchState::Searching if state.priced_by_market => {
            status(tr!("Loading exchange prices…").into(), TEXT_DIM)
        }
        SearchState::Searching => status(tr!("Searching…").into(), TEXT_DIM),
        SearchState::RateLimiting { wait_secs } => status(
            tr!(
                "Trade API request limit — waiting {secs}s…",
                secs = wait_secs
            )
            .into(),
            TEXT_DIM,
        ),
        SearchState::Failed(
            failure @ SearchFailure::TooComplex {
                weighted_sums: true,
            },
        ) => render_sums_refused(failure, cx).into_any_element(),
        SearchState::Failed(failure) => status(failure.message().into(), TEXT_WARNING),
        SearchState::Empty { .. } if state.priced_by_market => div()
            .flex()
            .flex_col()
            .child(render_market_gap(state))
            .child(status(
                tr!("No listings on the trade site").into(),
                TEXT_DIM,
            ))
            .into_any_element(),
        SearchState::Empty { relaxed } => {
            render_nothing_found(state, *relaxed, cx).into_any_element()
        }
        SearchState::Market(price) => match state.market() {
            Some(market) => render_market_card(state, item, market, price).into_any_element(),
            None => status(tr!("Loading exchange prices…").into(), TEXT_DIM),
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
            relaxed,
        } => div()
            .flex()
            .flex_col()
            .pt(rems_from_px(12.))
            .children(
                state
                    .priced_by_market
                    .then(|| div().mb(rems_from_px(8.)).child(render_market_gap(state))),
            )
            .children(relaxed.map(|(least, of)| {
                div()
                    .mb(rems_from_px(8.))
                    .text_center()
                    .text_color(rgb(TIER_TOP))
                    .child(tr!(
                        "No exact matches — showing items with at least {least} of {of} selected \
                         stats",
                        least = least,
                        of = of
                    ))
            }))
            .child(render_matched_line(state, *total, trade_url.clone(), cx))
            .child(render_results_table(state, rows, *relaxed, cx))
            .into_any_element(),
    }
}

/// «Ничего не найдено» (`Nothing found`), and the broader searches the player can ask for from
/// there, each one search spent only on the press (PoE Overlay II has no automatic one either):
/// the same rows at minimums 10 % below the rolls (`PriceCheckApp::choose_profile`), unless that's
/// the profile already, and listings with all the checked stat rows but one
/// (`PriceCheckApp::search_one_fewer`), unless this was that search (`relaxed`: it asked for `.0`
/// of `.1`).
fn render_nothing_found(
    state: &PriceCheckApp,
    relaxed: Option<(u32, u32)>,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let text = match relaxed {
        Some((least, of)) => tr!(
            "Nothing found — not even with {least} of {of} selected stats",
            least = least,
            of = of
        ),
        None => tr!("Nothing found").to_owned(),
    };
    let broad = state
        .profile
        .is_some_and(|profile| profile != SearchProfile::Broad);
    let one_fewer = state
        .profile
        .filter(|_| relaxed.is_none())
        .and_then(|_| trade_client::one_fewer_match(&state.filters));
    div()
        .mt(rems_from_px(12.))
        .flex()
        .flex_col()
        .items_center()
        .gap(rems_from_px(8.))
        .child(div().text_center().text_color(rgb(TEXT_DIM)).child(text))
        .when(broad || one_fewer.is_some(), |this| {
            this.child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap(rems_from_px(8.))
                    .when(broad, |this| {
                        this.child(action_button(
                            "nothing-found-broad",
                            profile_label(SearchProfile::Broad),
                            tr!(
                                "Search the same checked stats with minimums 10% below your \
                                 rolls. One search on the trade site."
                            ),
                            |view, cx| view.choose_profile(SearchProfile::Broad, cx),
                            cx,
                        ))
                    })
                    .children(one_fewer.map(|(least, of)| {
                        action_button(
                            "nothing-found-one-fewer",
                            tr!("Match {least} of {of}", least = least, of = of),
                            tr!(
                                "Search for listings with at least {least} of {of} checked \
                                 modifiers; checked item properties such as defences and the \
                                 “sum” rows stay required. One search on the trade site.",
                                least = least,
                                of = of
                            ),
                            PriceCheckApp::search_one_fewer,
                            cx,
                        )
                    })),
            )
        })
}

/// The site refused a weighted sum, which it takes only from a signed-in account: why, and the two
/// ways on -- the same item searched without the sums (one search), or signing in again.
fn render_sums_refused(failure: &SearchFailure, cx: &Context<PriceCheckApp>) -> impl IntoElement {
    div()
        .mt(rems_from_px(12.))
        .flex()
        .flex_col()
        .items_center()
        .gap(rems_from_px(8.))
        .child(
            div()
                .text_center()
                .text_color(rgb(TEXT_WARNING))
                .child(failure.message()),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .justify_center()
                .gap(rems_from_px(8.))
                .child(action_button(
                    "sums-refused-without",
                    tr!("Search without sums"),
                    tr!(
                        "Check the item again without the “sum” rows: their mods are searched one \
                         by one. One search on the trade site."
                    ),
                    PriceCheckApp::search_without_sums,
                    cx,
                ))
                .child(action_button(
                    "sums-refused-sign-in",
                    tr!("Sign in"),
                    tr!("Opens the pathofexile.com sign-in page. The sums work once you're in."),
                    |_, cx| crate::login::open(cx),
                    cx,
                )),
        )
}

/// The results' header: how many the search found, the watch switch and the trade site link.
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
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(rems_from_px(8.))
                .child(
                    div()
                        .flex()
                        .gap(rems_from_px(4.))
                        .child(div().text_color(rgb(TEXT_DIM)).child(tr!("Found:")))
                        .child(i18n::integer(total)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(rems_from_px(14.))
                        .children(render_watch(state, cx))
                        .child(render_link(format!("{host}/trade ↗"), trade_url)),
                ),
        )
        .children(render_watch_refusal(state, cx))
}

/// Under «Ничего не найдено» (`Nothing found`): a search nothing matches yet is just what watching
/// is for.
pub(super) fn render_empty_watch(
    state: &PriceCheckApp,
    cx: &Context<PriceCheckApp>,
) -> Option<impl IntoElement> {
    if !matches!(state.search, SearchState::Empty { .. }) || state.priced_by_market {
        return None;
    }
    let toggle = render_watch(state, cx)?;
    Some(
        div()
            .mt(rems_from_px(10.))
            .flex()
            .flex_col()
            .items_center()
            .gap(rems_from_px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(rems_from_px(10.))
                    .child(
                        div()
                            .text_size(rems_from_px(12.))
                            .text_color(rgb(TEXT_DIM))
                            .child(tr!("Notify me when one is listed:")),
                    )
                    .child(toggle),
            )
            .children(render_watch_refusal(state, cx)),
    )
}

/// «Следить» (`Live search`, the trade site's name for it): the search's new listings come as
/// cards over the game (`crate::live_search`) -- with how many searches are watched. Only for a
/// signed-in player (`crate::session`) and a search the trade site answered.
fn render_watch(state: &PriceCheckApp, cx: &Context<PriceCheckApp>) -> Option<impl IntoElement> {
    let search = state.watchable.clone()?;
    if !cx.try_global::<SessionStatus>()?.signed_in() {
        return None;
    }
    let live = cx.try_global::<LiveSearches>()?;
    let watching = live.is_watched(&search.query_id);
    let count = live.count();
    let hint = if watching {
        tr!("Live search is on — click to turn it off")
    } else {
        tr!("New listings for this search arrive as cards over the game while the app runs")
    };
    let toggle = div()
        .id("watch")
        .flex()
        .flex_none()
        .items_center()
        .gap(rems_from_px(6.))
        .cursor_pointer()
        .text_size(rems_from_px(12.))
        .tooltip(hints::hint(hint))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_view, _event: &MouseDownEvent, _window, cx| {
                cx.global_mut::<LiveSearches>().toggle(&search);
            }),
        )
        .child(switch("switch", watching))
        .child(tr!("Live search"));
    // The count keeps its room while nothing is watched, only unseen: the switch then stays under
    // the pointer when a click turns watching on and the count appears -- a second click turns it
    // off again instead of landing on the count.
    Some(
        div()
            .flex()
            .items_center()
            .gap(rems_from_px(8.))
            .child(ease_hover("watch", toggle, |toggle, hover| {
                toggle.text_color(rgb(blend(TEXT, GOLD_LIGHT, hover)))
            }))
            .child(
                div()
                    .text_size(rems_from_px(12.))
                    .text_color(rgb(TEXT_DIM))
                    .when(count == 0, |this| this.opacity(0.))
                    .child(tr!(
                        "{count} of {max} in use",
                        count = count,
                        max = MAX_LIVE_SEARCHES
                    )),
            ),
    )
}

/// Why watching the shown search was just refused, in the language the panel is drawn in.
fn render_watch_refusal(
    state: &PriceCheckApp,
    cx: &Context<PriceCheckApp>,
) -> Option<impl IntoElement> {
    let search = state.watchable.as_ref()?;
    let refusal = cx.try_global::<LiveSearches>()?.refusal(&search.query_id)?;
    Some(
        div()
            .text_size(rems_from_px(12.))
            .text_color(rgb(TEXT_WARNING))
            .child(refusal.message()),
    )
}

/// A text link opened in the default browser via GPUI's own `App::open_url`.
pub(super) fn render_link(label: impl Into<SharedString>, url: String) -> impl IntoElement {
    let label = label.into();
    link(
        label.clone(),
        label,
        move |_event: &MouseDownEvent, _window, cx: &mut App| {
            cx.open_url(&url);
        },
    )
}

/// EE2's results table, plus the seller column (the player can turn it off; the space then stays
/// empty so the dates keep their place) and currency icons PoE Overlay II shows; a row lights up
/// under the pointer. A seller who sells in person gets a ✉: clicking the row copies the whisper
/// for the game's chat. Hovering a row shows the listed item as the game's tooltip draws it
/// (`ListingTooltip`); after a relaxed search (`relaxed`: at least `.0` of the `.1` stat rows)
/// each row says how many it has.
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
    let wanted: Rc<[WantedStat]> = WantedStat::from_filters(&state.filters).into();
    // Pseudo totals and free slots count toward a relaxed search too, but no listed mod shows
    // them: the rows' counts only add up to the banner's when every searched row is a mod's.
    let count_matches = relaxed.is_some_and(|(_, of)| of as usize == wanted.len());
    let last = rows.len().saturating_sub(1);
    div()
        .flex()
        .flex_col()
        .child(
            table_row()
                .h(rems_from_px(22.))
                .border_b_1()
                .border_color(rgb(BORDER_GOLD))
                .text_size(rems_from_px(11.))
                .text_color(rgb(TEXT_MUTED))
                .child(price_cell().child(tr!("Price")))
                .child(level_cell().child(tr!("iLvl")))
                .child(seller_cell().when(show_seller, |this| this.child(tr!("Seller"))))
                .child(listed_cell().child(tr!("Listed"))),
        )
        .children(rows.iter().enumerate().map(|(index, row)| {
            let listed = relative_time::listed_ago(&row.indexed, now_unix).unwrap_or_default();
            // For a few seconds after a click the seller's cell says the whisper is on the
            // clipboard, over the listed-time column too: the note doesn't fit the seller's alone.
            let copied = state.whisper_copied(index);
            let seller: SharedString = if copied {
                tr!("✓ copied — paste it into the chat").into()
            } else {
                match (&row.whisper, show_seller) {
                    (Some(_), true) => format!("✉ {}", row.account_name).into(),
                    (Some(_), false) => "✉".into(),
                    (None, true) => row.account_name.clone().into(),
                    (None, false) => SharedString::default(),
                }
            };
            let tooltip = listing_tooltip(state, row, wanted.clone());
            let matched = count_matches
                .then(|| listing_match::matched_count(&row.item.mods, &wanted))
                .flatten()
                .filter(|_| !copied);
            let element = table_row()
                .id(("listing", index))
                .h(rems_from_px(30.))
                .when(index < last, |this| {
                    this.border_b_1().border_color(rgb(BORDER_ROW))
                })
                .tooltip(tooltip)
                .when_some(row.whisper.clone(), |this, whisper| {
                    this.cursor_pointer().on_mouse_down(
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
                        row.item
                            .item_level
                            .filter(|&level| level > 0)
                            .map(|level| level.to_string())
                            .unwrap_or_default(),
                    ),
                )
                .child(
                    seller_cell()
                        .flex()
                        .gap(rems_from_px(6.))
                        .text_size(rems_from_px(12.))
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
                            .text_size(rems_from_px(12.))
                            .text_color(rgb(TEXT_DIM))
                            .child(status_dot(row))
                            .child(listed),
                    )
                });
            ease_hover(("listing", index), element, |row, hover| {
                row.bg(alpha(GOLD, 0.07 * hover)).shadow(inner_glow(hover))
            })
        }))
}

/// «от 200» (`at least 200`), «до 5», `200–250`: the bounds a roll fell short of.
fn bounds_text(min: Option<f64>, max: Option<f64>) -> String {
    match (min, max) {
        (Some(min), Some(max)) => format!("{}–{}", format_value(min), format_value(max)),
        (Some(min), None) => tr!("at least {value}", value = format_value(min)),
        (None, Some(max)) => tr!("at most {value}", value = format_value(max)),
        (None, None) => String::new(),
    }
}

/// A results row's tooltip: the listed item's card (`ui::item_card`), its mods whose stat the
/// search asks for marked met or short (`listing_match::assess`), and at the card's foot the
/// asked-for stats the item lacks -- which a relaxed search lets through -- and what the row's
/// price marker means.
struct ListingTooltip(ItemCard);

/// The builder `tooltip` takes for `row`. The card is made when the tooltip opens, not for every
/// row each frame: a row keeps only its item's handle and the search's stats.
fn listing_tooltip(
    state: &PriceCheckApp,
    row: &ListingRow,
    wanted: Rc<[WantedStat]>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let item = row.item.clone();
    let site = state.trade_site();
    let price = card_price(state, row);
    let marker = marker_note(row);
    move |_window, cx| {
        let card = ItemCard::new(&item, site, Some(price.clone()), |listed| {
            mod_mark(listed, &wanted)
        })
        .with_notes(listing_notes(&item, &wanted, marker.clone()));
        cx.new(|_| ListingTooltip(card)).into()
    }
}

/// How `listed` stands against the stats `wanted`, as the card marks it.
fn mod_mark(listed: &ListedMod, wanted: &[WantedStat]) -> ModMark {
    match listing_match::assess(listed, wanted) {
        Asked::No => ModMark::None,
        Asked::Met => ModMark::Met,
        Asked::Short { min, max } => {
            ModMark::Short(tr!("needs {bounds}", bounds = bounds_text(min, max)))
        }
    }
}

/// The card's notes on the listing: the stats `wanted` the item has no mod for -- none when the
/// site gave no stat ids to tell by -- and `marker`, what the mark beside its price means.
fn listing_notes(
    item: &ListedItem,
    wanted: &[WantedStat],
    marker: Option<String>,
) -> Vec<(String, u32)> {
    let missing = listing_match::missing(&item.mods, wanted);
    let mut notes = Vec::with_capacity(missing.len() + 2);
    if !missing.is_empty() {
        notes.push((tr!("Missing from this item:").to_owned(), TEXT_WARNING));
        notes.extend(
            missing
                .into_iter()
                .map(|text| (format!("✗ {text}"), TEXT_WARNING)),
        );
    }
    notes.extend(marker.map(|marker| (marker, TEXT_DIM)));
    notes
}

/// The row's price, for the card's price note: amount and currency icon, as `render_price` shows
/// it.
fn card_price(state: &PriceCheckApp, row: &ListingRow) -> CardPrice {
    let currency = row.price_currency.as_str();
    CardPrice {
        amount: i18n::number(row.price_amount).into(),
        icon: state
            .currency_icon(currency)
            .map(|url| url.to_owned().into()),
        currency_name: state
            .currency_name(currency)
            .unwrap_or(currency)
            .to_owned()
            .into(),
    }
}

/// What the marker `render_price` puts beside a row's price means, if it puts one.
fn marker_note(row: &ListingRow) -> Option<String> {
    if row.listed_times > 2 {
        Some(tr!(
            "× {count}: the seller listed {count} of these at this price",
            count = row.listed_times
        ))
    } else if row.item.note.is_none() {
        Some(
            tr!(
                "?: the price comes from the stash tab's name, not the item's own note — it may \
                 not be real"
            )
            .to_owned(),
        )
    } else {
        None
    }
}

impl Render for ListingTooltip {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        render_item_card(&self.0, window)
    }
}

/// Amount and currency icon (the currency's name where there's no icon), EE2's markers
/// (`TradeItem.vue`): "× N" when the seller listed it several times at this price, otherwise "?"
/// when the price comes from a stash tab's name rather than the item's own note -- likely not a
/// real price -- and, for anything but divines and exalts, what it's worth in those.
fn render_price(state: &PriceCheckApp, row: &ListingRow) -> impl IntoElement {
    let currency = row.price_currency.as_str();
    let icon = currency_img(state.currency_icon(currency), 20.);
    let name = icon
        .is_none()
        .then(|| state.currency_name(currency).unwrap_or(currency).to_owned());
    let equivalent = (!matches!(currency, "divine" | "exalted"))
        .then(|| state.market())
        .flatten()
        .and_then(|market| {
            let divines = row.price_amount * market.value_in_divines(currency)?;
            let (value, unit) = market.in_display_unit(divines);
            Some((format!("≈ {}", i18n::number(value)), unit.trade_id()))
        });
    price_cell()
        .flex()
        .items_center()
        .gap(rems_from_px(4.))
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::SEMIBOLD)
                .child(i18n::number(row.price_amount)),
        )
        .children(icon)
        .children(name.map(|name| {
            div()
                .min_w_0()
                .truncate()
                .text_size(rems_from_px(12.))
                .text_color(rgb(TEXT_DIM))
                .child(name)
        }))
        .map(|this| {
            if row.listed_times > 2 {
                this.child(
                    div()
                        .flex_none()
                        .px(rems_from_px(4.))
                        .rounded(rems_from_px(3.))
                        .bg(alpha(GOLD, 0.18))
                        .text_size(rems_from_px(11.))
                        .text_color(rgb(GOLD_LIGHT))
                        .child(format!("× {}", row.listed_times)),
                )
            } else if row.item.note.is_none() {
                this.child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT_MUTED))
                        .child("?"),
                )
            } else {
                this
            }
        })
        .children(equivalent.map(|(text, unit)| {
            div()
                .min_w_0()
                .text_size(rems_from_px(12.))
                .text_color(rgb(TEXT_MUTED))
                .child(amount_in(state, text, unit, 14.))
        }))
}

/// EE2's seller dot (`TradeListing.vue`'s `.account-status`): pink online, orange AFK, red
/// offline. Instant-buyout listings keep the slot empty so the times stay aligned.
fn status_dot(row: &ListingRow) -> impl IntoElement {
    let color = match (row.instant_buyout, row.account_status) {
        (true, _) => None,
        (false, AccountStatus::Online) => Some(STATUS_ONLINE),
        (false, AccountStatus::Afk) => Some(STATUS_AFK),
        (false, AccountStatus::Offline) => Some(STATUS_OFFLINE),
    };
    div()
        .size(rems_from_px(6.))
        .flex_none()
        .rounded_full()
        .when_some(color, |this, color| this.bg(rgb(color)))
}

fn table_row() -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(rems_from_px(8.))
        .px(rems_from_px(8.))
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
