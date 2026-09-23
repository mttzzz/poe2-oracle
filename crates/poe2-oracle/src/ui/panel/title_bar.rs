//! The strip above the nameplate: the league chip and its menu, the divine rate, the settings gear
//! and the ×.

use gpui::{
    Context, IntoElement, MouseButton, MouseDownEvent, Window, anchored, deferred, div, point,
    prelude::*, px, rgb,
};

use crate::league_chip;
use crate::price_check::PriceCheckApp;
use crate::ui::hint as hints;
use crate::ui::theme::{
    BG_BUTTON_HOVER, BG_CLOSE_HOVER, BG_CONTROL, BG_PANEL, BG_TITLE, BORDER_GOLD, CONTENT_PADDING,
    GOLD, TEXT, TEXT_DIM, rems_from_px,
};

use super::format::{currency_img, format_compact};

/// The least room the league menu keeps from the panel's edges.
const MENU_MARGIN: f32 = 4.;

/// The app's name, the league chip, the divine rate once the market is loaded (EE2's ⇄ rate in its
/// title bar), the settings gear, and the × that hides the panel (the other way to close it
/// besides Esc).
pub(super) fn render_title_bar(
    state: &PriceCheckApp,
    window: &Window,
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
                .child("PoE2 Oracle"),
        )
        .child(render_league_chip(state, window, cx))
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

/// The league searches go to, named as the trade site names it in the game client's language --
/// «Авто · Запретные ритуалы ▾» -- which opens the menu of leagues to switch to. It gives way
/// before the rate and the buttons when the panel is narrow.
fn render_league_chip(
    state: &PriceCheckApp,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let label =
        league_chip::chip_label(&state.settings.league, state.league(), state.league_names());
    div()
        .id("league-chip")
        .flex()
        .min_w_0()
        .items_center()
        .gap(rems_from_px(3.))
        .mr(rems_from_px(8.))
        .px(rems_from_px(8.))
        .py(rems_from_px(2.))
        .rounded_xs()
        .bg(rgb(if state.league_menu {
            BG_BUTTON_HOVER
        } else {
            BG_CONTROL
        }))
        .text_xs()
        .text_color(rgb(GOLD))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(BG_BUTTON_HOVER)))
        .tooltip(hints::hint(
            "Лига, в которой идёт поиск. Нажмите, чтобы сменить: выбор сохранится в настройках, \
             а поиск повторится в новой лиге.",
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                view.set_league_menu(true, cx)
            }),
        )
        .child(div().min_w_0().truncate().child(label))
        .child(div().flex_none().child("▾"))
        .when(state.league_menu, |this| {
            this.child(render_league_menu(state, window, cx))
        })
}

/// The chip's menu: the choices the settings window's league chips offer, the current one in
/// gold. It hangs from the chip's left edge, kept inside the panel (`anchored`), over a backdrop
/// that takes a click anywhere else to close it -- that click does nothing more. Esc closes it too
/// (`price_check::register_hotkeys`).
fn render_league_menu(
    state: &PriceCheckApp,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let viewport = window.viewport_size();
    // A pixel past the window: `anchored` moves it by whole pixels, which can leave half of one
    // uncovered along the far edges when the chip's bottom falls between pixels.
    let backdrop = div()
        .w(viewport.width + px(1.))
        .h(viewport.height + px(1.))
        .occlude()
        .on_any_mouse_down(cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
            view.set_league_menu(false, cx)
        }));
    let choices = league_chip::menu(
        &state.settings.league,
        state.leagues(),
        state.league_names(),
    );
    let list = div()
        .id("league-menu")
        .flex()
        .flex_col()
        .max_w(viewport.width - px(2. * MENU_MARGIN))
        .max_h(viewport.height - px(2. * MENU_MARGIN))
        .overflow_y_scroll()
        .occlude()
        .py(rems_from_px(4.))
        .rounded_xs()
        .bg(rgb(BG_PANEL))
        .border_1()
        .border_color(rgb(BORDER_GOLD))
        .text_xs()
        .children(choices.into_iter().map(|(choice, label)| {
            let current = choice == state.settings.league;
            div()
                .px(rems_from_px(10.))
                .py(rems_from_px(3.))
                .truncate()
                .text_color(rgb(if current { GOLD } else { TEXT }))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(BG_BUTTON_HOVER)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                        view.choose_league(choice.clone(), cx)
                    }),
                )
                .child(label)
        }));
    // Both float over the whole panel: the backdrop from the window's corner, the list from the
    // chip's bottom-left, where this zero-size holder sits.
    div()
        .absolute()
        .top_full()
        .left_0()
        .child(deferred(
            anchored().position(point(px(0.), px(0.))).child(backdrop),
        ))
        .child(
            deferred(
                anchored()
                    .snap_to_window_with_margin(px(MENU_MARGIN))
                    .child(list),
            )
            .with_priority(1),
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
