//! The running PoE2 client's window geometry, the cursor, and focus hand-back -- everything the
//! price-check panel's EE2-style placement (`crate::overlay_layout`) and focus handling need from
//! outside this process.
//!
//! The game window is found by its title, `"Path of Exile 2"`, the same way EE2 attaches its
//! overlay (`OverlayController.attachByTitle`, `main/src/windowing/GameWindow.ts`); the title is
//! not localized (verified live 2026-09-22 against a Russian client). If it can't be found, the
//! monitor under the cursor stands in for it -- PoE2 normally runs fullscreen/borderless, where
//! the two rects coincide.
//!
//! All coordinates are physical pixels: `app::run` opts the process into per-monitor-v2 DPI
//! awareness before any window exists.

use std::sync::LazyLock;

use anyhow::{Result, bail};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, FindWindowW, GetClientRect, GetCursorPos, GetForegroundWindow,
    GetWindowThreadProcessId, SetForegroundWindow, WINEVENT_OUTOFCONTEXT,
};
use windows::core::{PCWSTR, w};

use crate::overlay_layout::{self, PhysicalRect};

/// The kind of each new foreground window; the receiving end is [`watch_foreground`]'s.
static FOREGROUND_CHANGES: LazyLock<(
    async_channel::Sender<Foreground>,
    async_channel::Receiver<Foreground>,
)> = LazyLock::new(async_channel::unbounded);

/// Physical cursor position, or `None` if the call fails (secure desktop, e.g. UAC prompt).
fn cursor_pos() -> Option<(i32, i32)> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.ok()?;
    Some((point.x, point.y))
}

pub(crate) fn game_window() -> Option<HWND> {
    unsafe { FindWindowW(PCWSTR::null(), w!("Path of Exile 2")) }
        .ok()
        .filter(|hwnd| !hwnd.is_invalid())
}

pub(crate) fn client_rect_on_screen(hwnd: HWND) -> Option<PhysicalRect> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) }.ok()?;
    let mut origin = POINT::default();
    if !unsafe { ClientToScreen(hwnd, &mut origin) }.as_bool() {
        return None;
    }
    Some(PhysicalRect {
        x: origin.x,
        y: origin.y,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    })
}

/// The game's client area and its DPI scale, `None` without a game window.
pub fn game_client() -> Option<(PhysicalRect, f64)> {
    let hwnd = game_window()?;
    let rect = client_rect_on_screen(hwnd).filter(|rect| rect.width > 0 && rect.height > 0)?;
    Some((rect, dpi_to_scale(unsafe { GetDpiForWindow(hwnd) })))
}

/// The game's client area and its DPI scale -- or, without a game window, the monitor under
/// `point` and that monitor's scale.
fn game_area(point: (i32, i32)) -> Option<(PhysicalRect, f64)> {
    if let Some(game) = game_client() {
        return Some(game);
    }
    let monitor = unsafe {
        MonitorFromPoint(
            POINT {
                x: point.0,
                y: point.1,
            },
            MONITOR_DEFAULTTONEAREST,
        )
    };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
    let scale =
        match unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) } {
            Ok(()) => dpi_to_scale(dpi_x),
            Err(_) => 1.0,
        };
    let r = info.rcMonitor;
    Some((
        PhysicalRect {
            x: r.left,
            y: r.top,
            width: r.right - r.left,
            height: r.bottom - r.top,
        },
        scale,
    ))
}

pub(crate) fn dpi_to_scale(dpi: u32) -> f64 {
    if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 }
}

/// EE2's panel rect for a check triggered at the current cursor position (inventory side when
/// the cursor is over the right half of the game, stash side otherwise), as wide as the player's
/// `ui_scale` makes the panel. `None` only if even the cursor/monitor lookup fails.
pub fn panel_rect_at_cursor(ui_scale: f32) -> Option<PhysicalRect> {
    let (x, y) = cursor_pos()?;
    let (game, dpi_scale) = game_area((x, y))?;
    Some(overlay_layout::panel_rect(
        game,
        x,
        dpi_scale * f64::from(ui_scale),
    ))
}

/// Where the panel waits before the first check: the inventory side, since that's where most
/// checks happen and where the Loading/Failed placeholders should show.
pub fn default_panel_rect(ui_scale: f32) -> Option<PhysicalRect> {
    let probe = cursor_pos().unwrap_or((0, 0));
    let (game, dpi_scale) = game_area(probe)?;
    Some(overlay_layout::panel_rect(
        game,
        game.x + game.width,
        dpi_scale * f64::from(ui_scale),
    ))
}

/// Pause after handing keyboard focus back to the game, before synthesizing keys into it.
pub const FOCUS_SWITCH_DELAY: std::time::Duration = std::time::Duration::from_millis(50);

/// Gives keyboard focus back to the game (EE2's `assertGameActive`) after the player has clicked
/// into the panel and the panel closes. A no-op if the game window isn't found.
pub fn focus_game() {
    if let Some(hwnd) = game_window() {
        let _ = unsafe { SetForegroundWindow(hwnd) };
    }
}

/// If this process owns the foreground window -- the player clicked into the panel -- gives focus
/// back to the game and returns `true`. Called before synthesizing the copy combo: with the panel
/// focused, the combo would land in the panel instead of the game and the check would time out.
pub fn reclaim_game_focus() -> bool {
    let mut owner = 0u32;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut owner)) };
    if owner != std::process::id() {
        return false;
    }
    game_window().is_some_and(|hwnd| unsafe { SetForegroundWindow(hwnd) }.as_bool())
}

/// Who has the keyboard, as far as the price-check hotkey is concerned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Foreground {
    /// The game.
    Game,
    /// One of this app's own windows: the panel the player clicked into, or the settings.
    ThisApp,
    /// Anything else -- a browser, a chat. Its keys are its own.
    Other,
}

/// Which of [`Foreground`]'s kinds the foreground window is.
pub fn foreground() -> Foreground {
    classify(unsafe { GetForegroundWindow() })
}

fn classify(window: HWND) -> Foreground {
    if game_window().is_some_and(|game| game == window) {
        return Foreground::Game;
    }
    let mut owner = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut owner)) };
    if owner == std::process::id() {
        Foreground::ThisApp
    } else {
        Foreground::Other
    }
}

/// Reports every change of the foreground window, in any process, on the returned channel, so
/// the hotkey follows the game without polling. Call once, from a thread that pumps messages
/// (GPUI's main thread): an out-of-context WinEvent hook is called from that thread's message
/// loop. The hook lasts as long as the process.
pub fn watch_foreground() -> Result<async_channel::Receiver<Foreground>> {
    let changes = FOREGROUND_CHANGES.1.clone();
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground_change),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if hook.is_invalid() {
        bail!("SetWinEventHook(EVENT_SYSTEM_FOREGROUND) failed");
    }
    Ok(changes)
}

/// The event names the window that became the foreground; `GetForegroundWindow` can still
/// return the previous one while the event is being delivered (seen live: a probe window's event
/// arrived with no delay, yet `GetForegroundWindow` still named the game).
unsafe extern "system" fn on_foreground_change(
    _hook: HWINEVENTHOOK,
    _event: u32,
    window: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let _ = FOREGROUND_CHANGES.0.try_send(classify(window));
}
