//! Waystone marks: the marker at the end of each of a waystone's own modifier rows, and the
//! marked modifiers listed under the rows.

use gpui::{Context, FontWeight, IntoElement, MouseButton, MouseDownEvent, div, prelude::*, rgb};

use poe2_domain::ParsedItem;
use stat_filters::{FilterTag, SearchFilter};

use crate::price_check::PriceCheckApp;
use crate::settings::WaystoneMark;
use crate::tr;
use crate::ui::style::{CARD_RADIUS, ease_hover};
use crate::ui::theme::{
    BG_CARD, BORDER_CARD, GOLD_LIGHT, MARK_DANGER, MARK_WANTED, MARK_WARNING, TEXT_MUTED, blend,
    rems_from_px,
};

use super::filters::stat_text;

/// A checked waystone: EE2's map check, where the player marks modifiers (see `WaystoneMark`).
pub(super) fn is_waystone(item: &ParsedItem) -> bool {
    item.category
        .as_ref()
        .is_some_and(|category| category.id == "map.waystone")
}

/// The mark a waystone row can carry -- its key (the trade stat id, the same on every client
/// language) and current mark -- or `None` for rows that aren't the waystone's own modifiers.
pub(super) fn waystone_mark_of(
    state: &PriceCheckApp,
    filter: &SearchFilter,
) -> Option<(String, Option<WaystoneMark>)> {
    if matches!(
        filter.tag,
        FilterTag::Property | FilterTag::Pseudo | FilterTag::EmptyAffix
    ) {
        return None;
    }
    let key = filter.trade_ids.first()?;
    Some((key.clone(), state.settings.waystone_marks.get(key).copied()))
}

pub(super) fn mark_color(mark: WaystoneMark) -> u32 {
    match mark {
        WaystoneMark::Danger => MARK_DANGER,
        WaystoneMark::Warning => MARK_WARNING,
        WaystoneMark::Wanted => MARK_WANTED,
    }
}

fn mark_label(mark: WaystoneMark) -> &'static str {
    match mark {
        WaystoneMark::Danger => tr!("Danger:"),
        WaystoneMark::Warning => tr!("Caution:"),
        WaystoneMark::Wanted => tr!("Wanted:"),
    }
}

/// The marker at a waystone row's end, warming to gold under the pointer: a click steps the mark
/// (none, danger, warning, wanted).
pub(super) fn render_mark_button(
    key: String,
    mark: Option<WaystoneMark>,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let color = mark.map_or(TEXT_MUTED, mark_color);
    let button = div()
        .id("mark")
        .flex_none()
        .px(rems_from_px(2.))
        .text_size(rems_from_px(13.))
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                // Only the mark: the row's own click toggles the search filter.
                cx.stop_propagation();
                view.cycle_waystone_mark(&key, cx);
            }),
        )
        .child(if mark.is_some() { "◆" } else { "◇" });
    ease_hover("mark", button, move |button, hover| {
        button.text_color(rgb(blend(color, GOLD_LIGHT, hover)))
    })
}

/// The marked modifiers a waystone has, danger first -- what the player checks it for -- or, while
/// nothing is marked yet, how to mark: a card under the modifiers, one per line, so marking one
/// never moves the rows being marked.
pub(super) fn render_waystone_marks(
    state: &PriceCheckApp,
    item: &ParsedItem,
) -> Option<impl IntoElement> {
    if !is_waystone(item) {
        return None;
    }
    let site = state.trade_site();
    let mut marked: Vec<(WaystoneMark, String)> = state
        .filters
        .iter()
        .filter_map(|filter| {
            let (_, mark) = waystone_mark_of(state, filter)?;
            Some((mark?, stat_text(filter, site).0))
        })
        .collect();
    // Danger first -- what the player checks a waystone for -- spelled out rather than taken
    // from the enum's declaration order.
    marked.sort_by_key(|(mark, _)| match mark {
        WaystoneMark::Danger => 0,
        WaystoneMark::Warning => 1,
        WaystoneMark::Wanted => 2,
    });
    Some(
        div()
            .flex()
            .flex_col()
            .gap(rems_from_px(4.))
            .mt(rems_from_px(10.))
            .px(rems_from_px(12.))
            .py(rems_from_px(8.))
            .rounded(rems_from_px(CARD_RADIUS))
            .bg(rgb(BG_CARD))
            .border_1()
            .border_color(rgb(BORDER_CARD))
            .text_size(rems_from_px(12.))
            .children(marked.iter().map(|(mark, text)| {
                div()
                    .flex()
                    .gap(rems_from_px(6.))
                    .text_color(rgb(mark_color(*mark)))
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(mark_label(*mark)),
                    )
                    .child(div().min_w_0().child(text.clone()))
            }))
            .when(marked.is_empty(), |this| {
                this.child(div().text_color(rgb(TEXT_MUTED)).child(tr!(
                    "◇ at the end of a modifier marks it: danger, caution or wanted"
                )))
            }),
    )
}
