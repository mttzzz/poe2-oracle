//! The plate that says an update went in -- «PoE2 Oracle обновлён до 0.1.1», «Данные игры
//! обновлены» -- for a few seconds after the restart the update made (`crate::updates`). It sits
//! at the bottom right of the work area of the monitor the game is on, over the game, and never
//! takes the keyboard: a click on it leaves the keyboard with the game, as a click on the XP
//! overlay's gear does. × closes it; a click anywhere else on it opens the settings, whose
//! «Обновления» say what runs now. Then its window is gone, and costs nothing more.

use std::time::Duration;

use gpui::{
    App, AsyncApp, Bounds, Context, DisplayId, MouseButton, MouseDownEvent, Render, SharedString,
    WeakEntity, Window, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, point,
    prelude::*, px, rgb, size,
};

use crate::overlay_layout::PhysicalRect;
use crate::platform::game_window;
use crate::platform::win32::Win32Overlay;
use crate::price_check::PriceCheckApp;
use crate::ui::fonts;
use crate::ui::style::{appear, diamond, game_frame, heading, title_button, title_gradient};
use crate::ui::theme::{BASE_REM_SIZE, GOLD, GOLD_LIGHT, rems_from_px};

/// How long the plate stays.
const SHOWN_FOR: Duration = Duration::from_secs(8);
/// The plate's size, and its gap from the corner of the work area, px at 100 % UI scale.
const WIDTH: f32 = 380.;
const HEIGHT: f32 = 52.;
const MARGIN: f32 = 16.;
/// The width of its ×.
const CLOSE_WIDTH: f32 = 40.;

struct Toast {
    text: SharedString,
    /// Whose settings a click opens.
    app: WeakEntity<PriceCheckApp>,
    ui_scale: f32,
}

/// Shows `text` on the plate for [`SHOWN_FOR`], sized for the player's `ui_scale`.
pub fn show(text: String, app: WeakEntity<PriceCheckApp>, ui_scale: f32, cx: &mut App) {
    let Some(area) = game_window::game_work_area() else {
        log::warn!("no monitor to show «{text}» on");
        return;
    };
    let scale = area.dpi_scale * f64::from(ui_scale);
    let physical = |length: f32| (f64::from(length) * scale).round() as i32;
    let (width, height, margin) = (physical(WIDTH), physical(HEIGHT), physical(MARGIN));
    let rect = PhysicalRect {
        x: area.rect.x + area.rect.width - width - margin,
        y: area.rect.y + area.rect.height - height - margin,
        width,
        height,
    };
    // A new window is placed in its display's own logical pixels -- the physical ones at that
    // display's scale -- so it starts out on the game's monitor at that monitor's DPI.
    let logical = |length: i32| px((f64::from(length) / area.dpi_scale) as f32);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(logical(rect.x), logical(rect.y)),
            size(logical(rect.width), logical(rect.height)),
        ))),
        titlebar: None,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        focus: false,
        show: false,
        display_id: Some(DisplayId::new(area.monitor)),
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        window.set_window_title("PoE2 Oracle");
        cx.new(|_| Toast {
            text: text.into(),
            app,
            ui_scale,
        })
    });
    let handle = match opened {
        Ok(handle) => handle,
        Err(err) => {
            log::warn!("the update's plate didn't open: {err:#}");
            return;
        }
    };
    match handle.update(cx, |_, window, _| Win32Overlay::from_window(window)) {
        Ok(Ok(overlay)) => {
            if let Err(err) = overlay.disable_dwm_frame() {
                log::warn!("{err:#}");
            }
            // Frameless, never activated, painting only when it changes, in place -- and only
            // then shown. Not from this update: each call sends GPUI's window messages at once.
            cx.spawn(async move |_| {
                let styled = overlay
                    .remove_frame()
                    .and_then(|()| overlay.set_no_activate())
                    .and_then(|()| overlay.gate_paints())
                    .and_then(|()| overlay.set_bounds(rect));
                if let Err(err) = styled {
                    log::warn!("{err:#}");
                }
                overlay.set_shown(true);
            })
            .detach();
        }
        Ok(Err(err)) => log::warn!("the update's plate has no window handle: {err:#}"),
        Err(err) => log::warn!("the update's plate is gone: {err:#}"),
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SHOWN_FOR).await;
        close(handle, cx);
    })
    .detach();
}

/// Closes the plate, unless a click closed it already.
fn close(handle: WindowHandle<Toast>, cx: &mut AsyncApp) {
    handle
        .update(cx, |_, window, cx| crate::app::close_window(window, cx))
        .ok();
}

impl Render for Toast {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(px(BASE_REM_SIZE * self.ui_scale));
        let face = fonts::interface_font();
        let app = self.app.clone();
        let body = div()
            .id("open-settings")
            .flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .items_center()
            .gap(rems_from_px(10.))
            .pl(rems_from_px(16.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _: &MouseDownEvent, window, cx| {
                    if let Some(app) = app.upgrade() {
                        crate::app::open_settings(&app, cx);
                    }
                    crate::app::close_window(window, cx);
                }),
            )
            .child(diamond(8., GOLD))
            .child(
                heading(face)
                    .min_w_0()
                    .truncate()
                    .text_size(rems_from_px(15.))
                    .text_color(rgb(GOLD_LIGHT))
                    .child(self.text.clone()),
            );
        div()
            .relative()
            .size_full()
            .flex()
            .items_center()
            .bg(title_gradient())
            .child(appear("toast", body))
            .child(title_button(
                "close",
                "×",
                CLOSE_WIDTH,
                |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                    crate::app::close_window(window, cx);
                },
            ))
            .child(game_frame())
    }
}
