//! The strip above the nameplate: the league select and its menu, the divine rate, the settings
//! gear and the ×. Its empty part drags the panel sideways; a double-click there sends it back to
//! its own place.

use gpui::{
    Context, CursorStyle, IntoElement, MouseButton, MouseDownEvent, SharedString, Window, div,
    prelude::*, rgb,
};

use crate::i18n;
use crate::league_chip;
use crate::price_check::PriceCheckApp;
use crate::tour::Stop;
use crate::tr;
use crate::ui::hint as hints;
use crate::ui::style::{menu_row, select, title_button, title_gradient};
use crate::ui::theme::{BORDER_GOLD, TEXT_DIM, rems_from_px};
use crate::ui::tour;

use super::format::currency_img;
use super::menu::render_menu;

const TITLE_HEIGHT: f32 = 32.;
/// Width of the title bar's ⚙ and ×.
const BUTTON_WIDTH: f32 = 34.;
/// The league menu is at least this wide, so «Авто · <league>» (`Auto · <league>`) fits on one
/// line.
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
        .child(tour::spot(
            Stop::PanelLeague,
            render_league_select(state, window, cx),
        ))
        .child(render_drag_area(state, cx))
        .child(title_button(
            "settings",
            "⚙",
            BUTTON_WIDTH,
            cx.listener(|_view, _event: &MouseDownEvent, _window, cx| {
                // Deferred: opening the window updates this very entity, which is mid-update
                // while its own listener runs.
                let app = cx.entity();
                cx.defer(move |cx| crate::app::open_settings(&app, cx));
            }),
        ))
        .child(title_button(
            "close",
            "×",
            BUTTON_WIDTH,
            cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                view.visible = false;
                cx.notify();
            }),
        ))
}

/// The league searches go to, named as the trade site in the interface language names it --
/// «Авто · Запретные ритуалы ▾», `Auto · Forbidden Rites ▾` -- which opens the menu of leagues to
/// switch to: the choices the settings window offers, the current one marked. It gives way before
/// the rate and the buttons when the panel is narrow.
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
        .tooltip(hints::hint(tr!(
            "The league the search runs in. Click to change it: the choice is saved to the \
             settings, and the search runs again in the new league."
        )))
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
                &state.settings.private_league,
                state.private_leagues(),
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
/// and a little of the empty part stay however narrow the panel. A private league's rate is its
/// public league's, which the rate's own tooltip names.
fn render_drag_area(state: &PriceCheckApp, cx: &Context<PriceCheckApp>) -> impl IntoElement {
    let rate = state.market().map(|market| {
        div()
            .id("rate")
            .flex()
            .flex_none()
            .items_center()
            .gap(rems_from_px(3.))
            .pr(rems_from_px(6.))
            .text_size(rems_from_px(12.))
            .text_color(rgb(TEXT_DIM))
            .child("1")
            .children(currency_img(state.currency_icon("divine"), 16.))
            .child(format!("= {}", i18n::compact(market.exalted_per_divine)))
            .children(currency_img(state.currency_icon("exalted"), 16.))
            .when_some(state.reference_league_name(), |this, league| {
                this.tooltip(hints::hint(tr!(
                    "The rate in {league}: a private league trades too little on the exchange.",
                    league = league
                )))
            })
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
            this.tooltip(hints::hint(tr!(
                "Drag to move the panel sideways: the next checks on this side open it there too. \
                 Double-click to put it back in its usual place."
            )))
        })
        .child(div().flex_1().min_w(rems_from_px(DRAG_MIN_WIDTH)))
        .children(rate)
}
