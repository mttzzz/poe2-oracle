//! `gpui::Render` for `crate::price_check::PriceCheckApp` -- the Price Check overlay's root view.
//! Orchestration lives in `price_check`; this module only reads its public state and calls its
//! public methods.
//!
//! The layout takes the best of the two overlays the player compares it with:
//! - Exiled Exchange 2 (`renderer/src/web/price-check/`): the full-height panel glued to the
//!   inventory, one compact row per stat filter (checkbox, tier and source tag, stat text,
//!   min/max inputs), the info chips, and the price/level/listed results table.
//! - PoE Overlay II: the item name as a header in its rarity colour, rolled values highlighted
//!   inside the stat text, and a prominent "Search" button.
//!
//! Its own words are in the interface language (`crate::i18n`): the Russian follows EE2's own
//! Russian locale (`renderer/public/data/ru/app_i18n.json`), the English the game's and the
//! trade site's English terms. The item's own text -- its name, mods and properties -- stays in
//! the language the game copied it in.
//!
//! It draws in the app's own game-styled look (`ui::style`, approved on the style mockup):
//! near-black surfaces in the game's double gold frame, gold accents and ornaments, the rarity
//! colours, the game's blue for rolled values, and restrained motion -- the panel rises in over
//! a moment for each check, controls ease into their hover. Text wraps instead of truncating --
//! Russian stat lines run long, and a row that doesn't fit grows taller rather than squeezing its
//! neighbours: every fixed-size control is `flex_none`, every text column `min_w_0`. Every length
//! is in rems (`theme::rems_from_px`), so the whole panel -- text, controls, icons and gaps --
//! follows the player's UI scale. The item's name is set in the stand-in for its tooltip face
//! (`ui::fonts`), the panel's own headings in the face of the client in the interface language;
//! everything else in GPUI's default system UI font (Segoe UI on Windows), which covers Cyrillic.

mod filters;
pub(crate) mod format;
mod market;
mod menu;
mod nameplate;
mod results;
mod title_bar;
mod waystone;

use gpui::{
    AnyElement, Context, IntoElement, MouseDownEvent, Render, Window, div, prelude::*, relative,
    rgb,
};

use poe2_domain::ParsedItem;

use crate::price_check::{BootstrapState, PriceCheckApp, Problem, SearchState};
use crate::tour::{Host, Stop};
use crate::tr;
use crate::ui::fonts;
use crate::ui::hint as hints;
use crate::ui::style::{ButtonKind, appear, button, game_frame, ornament_rule};
use crate::ui::theme::{
    BG_PANEL, BORDER_GOLD, CONTENT_PADDING, TEXT, TEXT_DIM, TEXT_WARNING, rems_from_px,
};
use crate::ui::tour;

use filters::render_sections;
use nameplate::{render_chips, render_nameplate};
use results::{render_empty_watch, render_results, render_search_row, render_toolbar};
use title_bar::render_title_bar;
use waystone::render_waystone_marks;

impl Render for PriceCheckApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `bootstrap` before `visible`: the Loading/Failed placeholders are the window's only
        // feedback while the catalog loads or after it failed, and `visible` only turns true
        // after a successfully copied item -- gating on `visible` first made them unreachable.
        let body: AnyElement = match &self.bootstrap {
            BootstrapState::Loading => {
                centered_message(tr!("Loading trade site data…"), TEXT_DIM).into_any_element()
            }
            BootstrapState::Failed(error) => centered_message(
                tr!(
                    "No data from the trade site — no internet connection, or the site is down. \
                     Retrying automatically.\n\n{error}",
                    error = error
                ),
                TEXT_WARNING,
            )
            .into_any_element(),
            BootstrapState::Ready if !self.visible => return div(),
            BootstrapState::Ready => render_ready(self, window, cx).into_any_element(),
        };

        // Each check plays the panel's rise in again (`PriceCheckApp::appearances`). The tour's
        // spotlight lies over it while the tour stands at one of the panel's parts.
        div()
            .relative()
            .size_full()
            .child(appear(
                ("appear", self.appearances),
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .bg(rgb(BG_PANEL))
                    .text_color(rgb(TEXT))
                    .text_size(rems_from_px(14.))
                    .line_height(relative(1.35))
                    .child(body)
                    .child(game_frame()),
            ))
            .children(tour::layer(Host::Panel, window, cx))
    }
}

fn render_ready(state: &PriceCheckApp, window: &Window, cx: &Context<PriceCheckApp>) -> AnyElement {
    let main = if let Some(problem) = &state.problem {
        render_problem(problem, cx).into_any_element()
    } else if let Some(item) = &state.item {
        render_item(state, item, window, cx).into_any_element()
    } else {
        centered_message(
            tr!(
                "Hover over an item and press {hotkey}",
                hotkey = state.settings.hotkey
            ),
            TEXT_DIM,
        )
        .into_any_element()
    };
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(render_title_bar(state, window, cx))
        .child(main)
        .into_any_element()
}

/// Nameplate, info chips, then either the market card (Currency Exchange items) or the profile
/// row, the filter rows and property chips, the search row and the listings -- one scroll area,
/// since a many-modded rare plus a full results page can outgrow even a full-height panel.
fn render_item(
    state: &PriceCheckApp,
    item: &ParsedItem,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let searched = !matches!(state.search, SearchState::NotSearched);
    div()
        .id("price-check-scroll")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(render_nameplate(item, state.trade_site(), cx))
        .child(
            div()
                .flex()
                .flex_col()
                .px(rems_from_px(CONTENT_PADDING))
                .pb(rems_from_px(14.))
                .child(render_chips(state, item, cx))
                .map(|this| {
                    if state.priced_by_market {
                        this.child(tour::spot(Stop::Listings, render_results(state, item, cx)))
                    } else {
                        this.children(render_toolbar(state, window, cx))
                            .child(tour::spot(
                                Stop::Filters,
                                render_sections(state, item, window, cx),
                            ))
                            .children(render_waystone_marks(state, item))
                            .child(render_search_row(state, cx))
                            .when(searched, |this| {
                                this.child(
                                    div()
                                        .pt(rems_from_px(10.))
                                        .child(ornament_rule(BORDER_GOLD)),
                                )
                            })
                            .child(tour::spot(Stop::Listings, render_results(state, item, cx)))
                            .children(render_empty_watch(state, cx))
                    }
                }),
        )
}

/// What went wrong with a check, centred -- and, for an item the parser rejected, a button that
/// opens the item problem form with its text filled in (`PriceCheckApp::report_item`).
fn render_problem(problem: &Problem, cx: &Context<PriceCheckApp>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(rems_from_px(12.))
        .p(rems_from_px(16.))
        .child(
            div()
                .w_full()
                .text_center()
                .text_color(rgb(TEXT_WARNING))
                .child(problem.message()),
        )
        .when(problem.reportable(), |this| {
            this.child(
                div()
                    .id("report-rejected-item")
                    .flex_none()
                    .tooltip(hints::hint(tr!(
                        "Opens a GitHub form with this item's text filled in: all that's left is \
                         to describe what's wrong and send it."
                    )))
                    .child(button(
                        "button",
                        tr!("Report to the developer"),
                        ButtonKind::Secondary,
                        fonts::interface_font(),
                        cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                            view.report_item(cx);
                        }),
                    )),
            )
        })
}

/// A message in the middle of the panel, wrapped to its width: a problem can run long (a saved
/// item text's path, a taken combo's explanation).
fn centered_message(text: impl IntoElement, color: u32) -> impl IntoElement {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .p(rems_from_px(16.))
        .child(
            div()
                .w_full()
                .text_center()
                .text_color(rgb(color))
                .child(text),
        )
}
