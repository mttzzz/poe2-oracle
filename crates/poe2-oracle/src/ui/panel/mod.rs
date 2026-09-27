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
//!
//! The render here lays out the panel's skeleton; what's in it is drawn in parts GPUI keeps from
//! frame to frame (`part`), so a hover or a tooltip redraws the part it's in and no more.

mod filters;
pub(crate) mod format;
mod market;
mod menu;
mod nameplate;
mod part;
mod results;
mod title_bar;
mod waystone;

use gpui::{
    AnyElement, App, Context, IntoElement, MouseDownEvent, Render, Window, div, prelude::*,
    relative, rgb,
};

use poe2_domain::ParsedItem;

use crate::price_check::{BootstrapState, PriceCheckApp, Problem, SearchState};
use crate::tour::{Host, Stop};
use crate::tr;
use crate::ui::fonts;
use crate::ui::hint as hints;
use crate::ui::ornament::FRAME_CLEAR;
use crate::ui::style::{ButtonKind, appear, button, game_frame, ornament_rule};
use crate::ui::theme::{
    BG_PANEL, BORDER_GOLD, CONTENT_PADDING, TEXT, TEXT_DIM, TEXT_WARNING, rems_from_px,
};
use crate::ui::tour;

pub(crate) use part::Parts;

use filters::render_sections;
use part::{Piece, Placer};
use results::render_results;
use waystone::is_waystone;

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
            BootstrapState::Ready => {
                // Taken out while the frame places the parts, which this very state keeps.
                let mut parts = std::mem::take(&mut self.panel_parts);
                let mut placer = Placer::new(&mut parts, cx.entity(), cx);
                let body = render_ready(self, &mut placer, cx);
                placer.finish();
                self.panel_parts = parts;
                body
            }
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

fn render_ready(
    state: &PriceCheckApp,
    placer: &mut Placer,
    cx: &mut Context<PriceCheckApp>,
) -> AnyElement {
    let main = if let Some(problem) = &state.problem {
        render_problem(problem, cx).into_any_element()
    } else if let Some(item) = &state.item {
        render_item(state, item, placer, cx).into_any_element()
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
        .child(placer.place(Piece::TitleBar, cx))
        .child(main)
        .into_any_element()
}

/// Nameplate, info chips, then either the market card (Currency Exchange items) or the profile
/// row, the filter rows and property chips, the search row and the listings -- one scroll area,
/// since a many-modded rare plus a full results page can outgrow even a full-height panel.
fn render_item(
    state: &PriceCheckApp,
    item: &ParsedItem,
    placer: &mut Placer,
    cx: &mut App,
) -> impl IntoElement {
    let searched = !matches!(state.search, SearchState::NotSearched);
    let mut content = div()
        .flex()
        .flex_col()
        .px(rems_from_px(CONTENT_PADDING))
        .pb(rems_from_px(14. - FRAME_CLEAR))
        .child(placer.place(Piece::Chips, cx));
    if state.priced_by_market {
        let results = render_results(state, placer, cx);
        content = content.child(tour::spot(Stop::Listings, results));
    } else {
        // The toolbar's part stays empty, and takes no room, for an item with neither a profile
        // nor tier minimums.
        let toolbar = placer.place(Piece::Toolbar, cx);
        let sections = render_sections(state, placer, cx);
        let waystone_marks = is_waystone(item).then(|| placer.place(Piece::WaystoneMarks, cx));
        let search_row = placer.place(Piece::SearchRow, cx);
        let results = render_results(state, placer, cx);
        content = content
            .child(toolbar)
            .child(tour::spot(Stop::Filters, sections))
            .children(waystone_marks)
            .child(search_row)
            .when(searched, |this| {
                this.child(
                    div()
                        .pt(rems_from_px(10.))
                        .child(ornament_rule(BORDER_GOLD)),
                )
            })
            .child(tour::spot(Stop::Listings, results));
    }
    // Scrolled rows stop at the frame's keep-out, and the last one rests 14 px above the edge.
    div()
        .id("price-check-scroll")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .mb(rems_from_px(FRAME_CLEAR))
        .overflow_y_scroll()
        .child(placer.place(Piece::Nameplate, cx))
        .child(content)
}

/// What went wrong with a check, centred -- and, for an item the parser rejected, a button that
/// opens the report window with its text attached (`PriceCheckApp::report_item`).
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
                        "Tell the developer what's wrong with this item; its text is attached"
                    )))
                    .child(button(
                        "button",
                        tr!("Report a problem"),
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
