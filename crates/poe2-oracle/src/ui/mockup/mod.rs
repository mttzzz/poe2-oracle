//! A dev-only preview of the game-styled look (`ui::style`), for the owner to approve before
//! anything real is restyled (plan-review.md, milestone 10; grill 20 + 25 + 24 + 28, B1-B3): the
//! settings window with its six sidebar sections, and a price panel on a real item -- the
//! `ru_live_krutyaschiy_obodok.txt` ring -- with sample listings. Nothing in it reads or saves
//! settings or searches: its controls answer clicks on their own. `app::run` opens it instead of
//! the app when `POE2_ORACLE_MOCKUP=1` is set -- beside a running copy, with no tray icon,
//! hotkeys or single-copy lock -- and it quits once both its windows are closed.

mod panel;
mod settings;

use gpui::{
    App, AppContext as _, Focusable, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, point, px, size,
};

use crate::platform::win32::Win32Overlay;
use crate::ui::fonts;

use panel::PanelMockup;
use settings::SettingsMockup;

/// The proposed settings window, logical px (at 100 % scale).
const SETTINGS_SIZE: (f32, f32) = (1100., 720.);
/// The panel: 32 rem wide like the real one (`overlay_layout`), at most this tall -- the real
/// one spans the game's height.
const PANEL_WIDTH: f32 = 512.;
const PANEL_MAX_HEIGHT: f32 = 1000.;
/// Between the two windows, and around the panel.
const GAP: f32 = 24.;

/// The leagues the trade site lists, in its Russian names (live 2026-09-23), "Авто" first.
const LEAGUES: [&str; 7] = [
    "Авто · Запретные ритуалы",
    "Запретные ритуалы",
    "HC Forbidden Rites",
    "Руны Альдура",
    "HC Runes of Aldur",
    "Стандарт",
    "Одна жизнь",
];

/// Whether this launch shows the mockup instead of the app.
pub fn requested() -> bool {
    std::env::var_os("POE2_ORACLE_MOCKUP").is_some_and(|value| value == "1")
}

/// Opens both windows side by side on the primary display: the settings on the left, the panel
/// to its right. Quits once both are closed.
pub fn open(cx: &mut App) {
    if let Err(err) = fonts::register(cx) {
        log::warn!("heading fonts unavailable: {err:#}");
    }
    let (left, top, width, height) =
        cx.primary_display()
            .map_or((0., 0., 1920., 1040.), |display| {
                let area = display.visible_bounds();
                (
                    f32::from(area.origin.x),
                    f32::from(area.origin.y),
                    f32::from(area.size.width),
                    f32::from(area.size.height),
                )
            });
    let (settings_width, settings_height) = SETTINGS_SIZE;
    let x = left + ((width - settings_width - GAP - PANEL_WIDTH) / 2.).max(0.);
    let panel_height = (height - 2. * GAP).min(PANEL_MAX_HEIGHT);

    let panel = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(gpui::Bounds::new(
                point(
                    px(x + settings_width + GAP),
                    px(top + ((height - panel_height) / 2.).max(0.)),
                ),
                size(px(PANEL_WIDTH), px(panel_height)),
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("PoE2 Oracle — макет панели".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_background: WindowBackgroundAppearance::Transparent,
            kind: WindowKind::Normal,
            focus: false,
            show: true,
            ..Default::default()
        },
        |_window, cx| cx.new(PanelMockup::new),
    );
    let settings = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(gpui::Bounds::new(
                point(px(x), px(top + ((height - settings_height) / 2.).max(0.))),
                size(px(settings_width), px(settings_height)),
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("PoE2 Oracle — макет настроек".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            kind: WindowKind::Normal,
            window_min_size: Some(size(px(900.), px(600.))),
            focus: true,
            show: true,
            ..Default::default()
        },
        |window, cx| {
            let view = cx.new(SettingsMockup::new);
            window.focus(&view.focus_handle(cx), cx);
            view
        },
    );
    match panel {
        Ok(handle) => square_corners(handle, cx),
        Err(err) => log::warn!("opening the panel mockup failed: {err:#}"),
    }
    match settings {
        Ok(handle) => square_corners(handle, cx),
        Err(err) => log::warn!("opening the settings mockup failed: {err:#}"),
    }
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}

/// Turns Windows 11's rounded corners and outline off, as the real overlay does: they would cut
/// the frame's corner diamonds (`style::game_frame`).
fn square_corners<V: 'static>(handle: WindowHandle<V>, cx: &mut App) {
    let squared = handle.update(cx, |_, window, _| {
        Win32Overlay::from_window(window).and_then(|overlay| overlay.disable_dwm_frame())
    });
    if let Ok(Err(err)) = squared {
        log::warn!("square mockup corners: {err:#}");
    }
}
