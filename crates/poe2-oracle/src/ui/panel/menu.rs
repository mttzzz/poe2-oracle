//! The panel's menus -- the league's in the title bar, the profile's above the stats: a
//! [`style::menu_list`] hanging from its select, kept inside the panel, over a backdrop that takes
//! a press anywhere else to close it -- that press does nothing more. Esc closes them too
//! (`price_check::register_hotkeys`).

use gpui::{
    Context, IntoElement, MouseDownEvent, Window, anchored, deferred, div, point, prelude::*, px,
};

use crate::price_check::PriceCheckApp;
use crate::ui::style::{appear, menu_list};
use crate::ui::theme::rems_from_px;

/// The least room a menu keeps from the panel's edges.
const MENU_MARGIN: f32 = 4.;

/// The menu `key` of `rows` ([`style::menu_row`]), at least `min_width` wide, hanging just below
/// the select it follows: laid right after the select in a column, this zero-height slot sits at
/// the select's bottom-left. `close` shuts it.
pub(super) fn render_menu(
    key: &'static str,
    rows: impl IntoIterator<Item = impl IntoElement>,
    min_width: f32,
    close: fn(&mut PriceCheckApp, &mut Context<PriceCheckApp>),
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let viewport = window.viewport_size();
    // A pixel past the window: `anchored` moves it by whole pixels, which can leave half of one
    // uncovered along the far edges when the select's bottom falls between pixels.
    let backdrop = div()
        .w(viewport.width + px(1.))
        .h(viewport.height + px(1.))
        .occlude()
        .on_any_mouse_down(
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                close(view, cx);
            }),
        );
    let list = menu_list(key, rows)
        .min_w(rems_from_px(min_width))
        .max_w(viewport.width - px(2. * MENU_MARGIN));
    // Both float over the whole panel: the backdrop from the window's corner, the list from the
    // slot.
    div()
        .h_0()
        .child(deferred(
            anchored().position(point(px(0.), px(0.))).child(backdrop),
        ))
        .child(
            deferred(
                anchored()
                    .offset(point(px(0.), px(4.)))
                    .snap_to_window_with_margin(px(MENU_MARGIN))
                    .child(appear("menu", list)),
            )
            .with_priority(1),
        )
}
