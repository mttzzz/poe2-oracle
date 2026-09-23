//! `gpui::Render` for `crate::price_check::PriceCheckApp` -- the Price Check overlay's root view.
//! Orchestration lives in `price_check`; this module only reads its public state and calls its
//! public methods.
//!
//! The layout takes the best of the two overlays the player compares it with:
//! - Exiled Exchange 2 (`renderer/src/web/price-check/`): the full-height panel glued to the
//!   inventory, one compact row per stat filter (checkbox, stat text, min/max inputs, source tag
//!   and tier underneath), the info chips, and the striped price/level/listed results table.
//! - PoE Overlay II: a fully Russian UI (wording follows EE2's own Russian locale,
//!   `renderer/public/data/ru/app_i18n.json`), the item name as a header in its rarity colour,
//!   rolled values highlighted inside the stat text, and a prominent "Поиск" button.
//!
//! Colours are the game's own: near-black panels, gold accents, the rarity colours, and its blue
//! for rolled values. Text wraps instead of truncating -- Russian stat lines run long, and a row
//! that doesn't fit grows taller rather than squeezing its neighbours: every fixed-size control is
//! `flex_none`, every text column `min_w_0`. Every length is in rems (`theme::rems_from_px`), so
//! the whole panel -- text, controls, icons and gaps -- follows the player's UI scale. The item's
//! name is set in the stand-in for its tooltip face (`ui::fonts`); everything else in GPUI's
//! default system UI font (Segoe UI on Windows), which covers Cyrillic.

mod filters;
pub(crate) mod format;
mod market;
mod nameplate;
mod results;
mod title_bar;
mod waystone;

use gpui::{AnyElement, Context, IntoElement, Render, Window, div, prelude::*, relative, rgb};

use poe2_domain::ParsedItem;

use crate::price_check::{BootstrapState, PriceCheckApp};
use crate::ui::theme::{BG_PANEL, CONTENT_PADDING, TEXT, TEXT_DIM, TEXT_WARNING, rems_from_px};

use filters::render_sections;
use nameplate::{render_chips, render_nameplate};
use results::{render_results, render_search_button, render_search_choices};
use title_bar::render_title_bar;
use waystone::render_waystone_marks;

impl Render for PriceCheckApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `bootstrap` before `visible`: the Loading/Failed placeholders are the window's only
        // feedback while the catalog loads or after it failed, and `visible` only turns true
        // after a successfully copied item -- gating on `visible` first made them unreachable.
        let body: AnyElement = match &self.bootstrap {
            BootstrapState::Loading => {
                centered_message("Загрузка каталога…", TEXT_DIM).into_any_element()
            }
            BootstrapState::Failed(msg) => centered_message(
                format!(
                    "Нет данных сайта торговли — нет интернета или сайт недоступен. \
                     Повторяю попытку сам.\n\n{msg}"
                ),
                TEXT_WARNING,
            )
            .into_any_element(),
            BootstrapState::Ready if !self.visible => return div(),
            BootstrapState::Ready => render_ready(self, window, cx).into_any_element(),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG_PANEL))
            .text_color(rgb(TEXT))
            .text_sm()
            .line_height(relative(1.35))
            .child(body)
    }
}

fn render_ready(state: &PriceCheckApp, window: &Window, cx: &Context<PriceCheckApp>) -> AnyElement {
    let main = if let Some(err) = &state.problem {
        centered_message(err.clone(), TEXT_WARNING).into_any_element()
    } else if let Some(item) = &state.item {
        render_item(state, item, window, cx).into_any_element()
    } else {
        centered_message(
            format!(
                "Наведите курсор на предмет и нажмите {}",
                state.settings.hotkey
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
        .child(render_title_bar(state, cx))
        .child(main)
        .into_any_element()
}

/// Nameplate, info chips, then either the market card (Currency Exchange items) or the filter
/// rows, the search button and the listings -- one scroll area, since a many-modded rare plus a
/// full results page can outgrow even a full-height panel.
fn render_item(
    state: &PriceCheckApp,
    item: &ParsedItem,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    div()
        .id("price-check-scroll")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(render_nameplate(item, state.trade_site()))
        .child(
            div()
                .flex()
                .flex_col()
                .px(rems_from_px(CONTENT_PADDING))
                .pb(rems_from_px(CONTENT_PADDING))
                .child(render_chips(state, item, cx))
                .map(|this| {
                    if state.priced_by_market {
                        this.child(render_results(state, item, cx))
                    } else {
                        this.child(render_sections(state, item, window, cx))
                            .children(render_waystone_marks(state, item))
                            .child(render_search_button(state, cx))
                            .child(render_search_choices(state, cx))
                            .child(render_results(state, item, cx))
                    }
                }),
        )
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
