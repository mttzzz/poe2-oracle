//! The strip above the nameplate: the league, the divine rate, the settings gear and the ×.

use gpui::{Context, IntoElement, MouseButton, MouseDownEvent, div, prelude::*, rgb};

use crate::price_check::PriceCheckApp;
use crate::ui::theme::{
    BG_BUTTON_HOVER, BG_CLOSE_HOVER, BG_TITLE, BORDER_GOLD, CONTENT_PADDING, TEXT, TEXT_DIM,
    rems_from_px,
};

use super::format::{currency_img, format_compact};

/// League name, the divine rate once the market is loaded (EE2's ⇄ rate in its title bar), the
/// settings gear, and the × that hides the panel (the other way to close it besides Esc).
pub(super) fn render_title_bar(
    state: &PriceCheckApp,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(rems_from_px(26.))
        .pl(rems_from_px(CONTENT_PADDING))
        .bg(rgb(BG_TITLE))
        .border_b_1()
        .border_color(rgb(BORDER_GOLD))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(rgb(TEXT_DIM))
                .child(format!("PoE2 Oracle · {}", state.league())),
        )
        .children(state.market().map(|market| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(rems_from_px(3.))
                .pr(rems_from_px(8.))
                .text_xs()
                .text_color(rgb(TEXT_DIM))
                .child("1")
                .children(currency_img(state.currency_icon("divine"), 14.))
                .child(format!("= {}", format_compact(market.exalted_per_divine)))
                .children(currency_img(state.currency_icon("exalted"), 14.))
        }))
        .child(title_button("⚙").on_mouse_down(
            MouseButton::Left,
            cx.listener(|_view, _event: &MouseDownEvent, _window, cx| {
                // Deferred: opening the window updates this very entity, which is mid-update
                // while its own listener runs.
                let app = cx.entity();
                cx.defer(move |cx| crate::app::open_settings(&app, false, cx));
            }),
        ))
        .child(
            title_button("×")
                .hover(|style| style.bg(rgb(BG_CLOSE_HOVER)).text_color(rgb(TEXT)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                        view.visible = false;
                        cx.notify();
                    }),
                ),
        )
}

fn title_button(label: &'static str) -> gpui::Div {
    div()
        .w(rems_from_px(34.))
        .h_full()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .text_color(rgb(TEXT_DIM))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(BG_BUTTON_HOVER)).text_color(rgb(TEXT)))
        .child(label)
}
