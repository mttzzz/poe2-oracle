//! Waystone marks: the marker at the end of each of a waystone's own modifier rows, and the
//! marked modifiers listed under the rows.

use gpui::{Context, FontWeight, IntoElement, MouseButton, MouseDownEvent, div, prelude::*, rgb};

use poe2_domain::ParsedItem;
use stat_filters::{FilterTag, SearchFilter};

use crate::price_check::PriceCheckApp;
use crate::settings::WaystoneMark;
use crate::ui::theme::{GOLD, TEXT_MUTED, rems_from_px};

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
        WaystoneMark::Danger => 0xe53e3e,
        WaystoneMark::Warning => 0xed8936,
        WaystoneMark::Wanted => 0x48bb78,
    }
}

fn mark_label(mark: WaystoneMark) -> &'static str {
    match mark {
        WaystoneMark::Danger => "Опасно:",
        WaystoneMark::Warning => "Осторожно:",
        WaystoneMark::Wanted => "Желанно:",
    }
}

/// The marker at a waystone row's end: a click steps the mark (none, danger, warning, wanted).
pub(super) fn render_mark_button(
    key: String,
    mark: Option<WaystoneMark>,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    div()
        .flex_none()
        .px(rems_from_px(2.))
        .text_size(rems_from_px(13.))
        .text_color(rgb(mark.map_or(TEXT_MUTED, mark_color)))
        .cursor_pointer()
        .hover(|style| style.text_color(rgb(GOLD)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                // Only the mark: the row's own click toggles the search filter.
                cx.stop_propagation();
                view.cycle_waystone_mark(&key, cx);
            }),
        )
        .child(if mark.is_some() { "◆" } else { "◇" })
}

/// The marked modifiers a waystone has, danger first -- what the player checks it for -- or, while
/// nothing is marked yet, how to mark. Listed under the modifiers, one per line: marking one never
/// moves the rows being marked.
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
            .gap(rems_from_px(3.))
            .mt(rems_from_px(8.))
            .text_xs()
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
                this.child(
                    div()
                        .text_color(rgb(TEXT_MUTED))
                        .child("◇ в конце свойства — пометить его опасным, спорным или желанным"),
                )
            }),
    )
}
