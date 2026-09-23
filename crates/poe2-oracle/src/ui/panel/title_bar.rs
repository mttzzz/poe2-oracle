//! The strip above the nameplate: the league select and its menu, the divine rate, the settings
//! gear and the ×. Its empty part drags the panel sideways; a double-click there sends it back to
//! its own place.

use gpui::{
    Context, CursorStyle, IntoElement, MouseButton, MouseDownEvent, SharedString, Window, div,
    prelude::*, rgb,
};

use crate::league_chip;
use crate::price_check::PriceCheckApp;
use crate::ui::hint as hints;
use crate::ui::style::{menu_row, select, title_button, title_gradient};
use crate::ui::theme::{BORDER_GOLD, TEXT_DIM, rems_from_px};

use super::format::{currency_img, format_compact};
use super::menu::render_menu;

const TITLE_HEIGHT: f32 = 32.;
/// Width of the title bar's ⚙ and ×.
const BUTTON_WIDTH: f32 = 34.;
/// The league menu is at least this wide, so «Авто · <league>» fits on one line.
const LEAGUE_MENU_WIDTH: f32 = 250.;
/// The least of the drag area that stays when the panel is narrow.
const DRAG_MIN_WIDTH: f32 = 24.;

/// The league select, the divine rate once the market is loaded (EE2's ⇄ rate in its title bar),
/// the settings gear, and the × that hides the panel (the other way to close it besides Esc).
pub(super) fn render_title_bar(
    state: &PriceCheckApp,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(rems_from_px(TITLE_HEIGHT))
        .pl(rems_from_px(8.))
        .bg(title_gradient())
        .border_b_1()
        .border_color(rgb(BORDER_GOLD))
        .child(render_league_select(state, window, cx))
        .child(render_drag_area(state, cx))
        .child(title_button(
            "settings",
            "⚙",
            BUTTON_WIDTH,
            false,
            cx.listener(|_view, _event: &MouseDownEvent, _window, cx| {
                // Deferred: opening the window updates this very entity, which is mid-update
                // while its own listener runs.
                let app = cx.entity();
                cx.defer(move |cx| crate::app::open_settings(&app, false, cx));
            }),
        ))
        .child(title_button(
            "close",
            "×",
            BUTTON_WIDTH,
            true,
            cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                view.visible = false;
                cx.notify();
            }),
        ))
}

/// The league searches go to, named as the trade site names it in the game client's language --
/// «Авто · Запретные ритуалы ▾» -- which opens the menu of leagues to switch to: the choices the
/// settings window offers, the current one marked. It gives way before the rate and the buttons
/// when the panel is narrow.
fn render_league_select(
    state: &PriceCheckApp,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let label =
        league_chip::chip_label(&state.settings.league, state.league(), state.league_names());
    div()
        .id("league")
        .flex()
        .flex_col()
        .min_w_0()
        .tooltip(hints::hint(
            "Лига, в которой идёт поиск. Нажмите, чтобы сменить: выбор сохранится в настройках, \
             а поиск повторится в новой лиге.",
        ))
        .child(select(
            "select",
            label,
            true,
            cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                view.set_league_menu(true, cx);
            }),
        ))
        .when(state.league_menu, |this| {
            let choices = league_chip::menu(
                &state.settings.league,
                state.leagues(),
                state.league_names(),
            );
            let rows = choices
                .into_iter()
                .enumerate()
                .map(|(index, (choice, label))| {
                    let current = choice == state.settings.league;
                    menu_row(
                        index,
                        current,
                        div().min_w_0().truncate().child(SharedString::from(label)),
                        cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                            view.choose_league(choice.clone(), cx);
                        }),
                    )
                });
            this.child(render_menu(
                "league-menu",
                rows,
                LEAGUE_MENU_WIDTH,
                |view, cx| view.set_league_menu(false, cx),
                window,
                cx,
            ))
        })
}

/// The title bar's empty part and the divine rate in it, once the market is loaded (EE2's ⇄ rate):
/// pressing it and moving drags the panel sideways (`PriceCheckApp::begin_panel_drag`), a
/// double-click puts it back in its own place (`PriceCheckApp::reset_panel_position`). The rate
/// and a little of the empty part stay however narrow the panel.
fn render_drag_area(state: &PriceCheckApp, cx: &Context<PriceCheckApp>) -> impl IntoElement {
    let rate = state.market().map(|market| {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(rems_from_px(3.))
            .pr(rems_from_px(6.))
            .text_size(rems_from_px(12.))
            .text_color(rgb(TEXT_DIM))
            .child("1")
            .children(currency_img(state.currency_icon("divine"), 16.))
            .child(format!("= {}", format_compact(market.exalted_per_divine)))
            .children(currency_img(state.currency_icon("exalted"), 16.))
    });
    div()
        .id("drag")
        .flex()
        .flex_1()
        .h_full()
        .items_center()
        .cursor(CursorStyle::ResizeLeftRight)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, event: &MouseDownEvent, _window, cx| {
                if event.click_count >= 2 {
                    view.reset_panel_position(cx);
                } else {
                    view.begin_panel_drag(cx);
                }
            }),
        )
        // Not over a drag under way, where the pointer stays still over the moving panel.
        .when(!state.dragging_panel(), |this| {
            this.tooltip(hints::hint(
                "Потяните, чтобы сдвинуть панель вбок: следующие проверки с этой стороны откроют \
                 её там же. Двойной щелчок вернёт панель на обычное место.",
            ))
        })
        .child(div().flex_1().min_w(rems_from_px(DRAG_MIN_WIDTH)))
        .children(rate)
}
