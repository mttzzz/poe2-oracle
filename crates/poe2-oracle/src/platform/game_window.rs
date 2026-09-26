//! The running PoE2 client's window geometry, the cursor, and focus hand-back -- everything the
//! price-check panel's EE2-style placement (`crate::overlay_layout`), its dragging and its focus
//! handling need from outside this process.
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
    ClientToScreen, GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint, MonitorFromWindow,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, FindWindowW, GA_ROOT, GetAncestor, GetClientRect, GetCursorPos,
    GetForegroundWindow, GetSystemMetrics, GetWindowThreadProcessId, SM_SWAPBUTTON,
    SetForegroundWindow, WINEVENT_OUTOFCONTEXT, WindowFromPoint,
};
use windows::core::{PCWSTR, w};

use crate::overlay_layout::{self, PanelPositions, PanelSide, PhysicalRect};

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

/// The cursor's x on the screen, physical pixels.
pub fn cursor_x() -> Option<i32> {
    cursor_pos().map(|(x, _)| x)
}

/// Whether the player is at the game `game`: it's in front, or the cursor is over it -- the game
/// shows its tooltips under the cursor even while another window has the keyboard (seen live
/// 2026-09-24). Click-through windows over it, the XP overlay's plates, are passed over, as by the
/// mouse.
pub fn attended(game: HWND) -> bool {
    if unsafe { GetForegroundWindow() } == game {
        return true;
    }
    cursor_pos().is_some_and(|(x, y)| {
        let under = unsafe { WindowFromPoint(POINT { x, y }) };
        !under.is_invalid() && unsafe { GetAncestor(under, GA_ROOT) } == game
    })
}

/// Whether the primary mouse button is held: the left one, or the right one for a player who
/// swapped them in Windows' settings -- `GetAsyncKeyState` reads the physical buttons.
pub fn primary_button_down() -> bool {
    let button = if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 {
        VK_RBUTTON
    } else {
        VK_LBUTTON
    };
    // The high bit is the button's state now: a negative `SHORT`.
    (unsafe { GetAsyncKeyState(button.0.into()) } as i16) < 0
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

/// What the price panel is placed on: the game's client area, the monitor it's on, and the
/// game's DPI scale -- or, without a game window, the monitor under a point standing in for the
/// game, at that monitor's scale.
#[derive(Debug, Clone, Copy)]
pub struct GameScreen {
    pub game: PhysicalRect,
    pub monitor: PhysicalRect,
    pub dpi_scale: f64,
}

impl GameScreen {
    /// The screen around `point`.
    fn at(point: (i32, i32)) -> Option<GameScreen> {
        let game = game_window().and_then(|hwnd| {
            let rect =
                client_rect_on_screen(hwnd).filter(|rect| rect.width > 0 && rect.height > 0)?;
            Some((hwnd, rect))
        });
        let monitor = monitor_of(game.map(|(hwnd, _)| hwnd), point);
        if let Some((hwnd, rect)) = game {
            return Some(GameScreen {
                game: rect,
                monitor: monitor_rect(monitor).unwrap_or(rect),
                dpi_scale: dpi_to_scale(unsafe { GetDpiForWindow(hwnd) }),
            });
        }
        let rect = monitor_rect(monitor)?;
        let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
        let dpi_scale =
            match unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) } {
                Ok(()) => dpi_to_scale(dpi_x),
                Err(_) => 1.0,
            };
        Some(GameScreen {
            game: rect,
            monitor: rect,
            dpi_scale,
        })
    }

    /// The screen around the cursor.
    pub fn at_cursor() -> Option<GameScreen> {
        GameScreen::at(cursor_pos().unwrap_or((0, 0)))
    }
}

/// The monitor the game window `game` is on -- the one it overlaps most -- or, without one, the
/// monitor under `point`.
fn monitor_of(game: Option<HWND>, point: (i32, i32)) -> HMONITOR {
    match game {
        Some(hwnd) => unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) },
        None => unsafe {
            MonitorFromPoint(
                POINT {
                    x: point.0,
                    y: point.1,
                },
                MONITOR_DEFAULTTONEAREST,
            )
        },
    }
}

/// The monitor the game is on -- or, without a game window, the one under the cursor -- as its
/// `HMONITOR` value, which is what `gpui_windows` names a display by (`DisplayId`): the settings
/// window opens on it.
pub fn game_monitor() -> Option<u64> {
    let monitor = monitor_of(game_window(), cursor_pos().unwrap_or((0, 0)));
    (!monitor.is_invalid()).then_some(monitor.0 as u64)
}

fn monitor_rect(monitor: HMONITOR) -> Option<PhysicalRect> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    let r = info.rcMonitor;
    Some(PhysicalRect {
        x: r.left,
        y: r.top,
        width: r.right - r.left,
        height: r.bottom - r.top,
    })
}

pub(crate) fn dpi_to_scale(dpi: u32) -> f64 {
    if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 }
}

/// The monitor the game is on -- or, without a game window, the one under the cursor -- as a
/// notice in its corner needs it (`ui::toast`).
#[derive(Debug, Clone, Copy)]
pub struct WorkArea {
    /// Its `HMONITOR` value: what `gpui_windows` names a display by (`DisplayId`).
    pub monitor: u64,
    /// The monitor less the taskbar.
    pub rect: PhysicalRect,
    pub dpi_scale: f64,
}

/// The game's monitor's [`WorkArea`]; `None` if Windows can't say.
pub fn game_work_area() -> Option<WorkArea> {
    let monitor = monitor_of(game_window(), cursor_pos().unwrap_or((0, 0)));
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if monitor.is_invalid() || !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    let work = info.rcWork;
    let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
    let dpi_scale =
        match unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) } {
            Ok(()) => dpi_to_scale(dpi_x),
            Err(_) => 1.0,
        };
    Some(WorkArea {
        monitor: monitor.0 as u64,
        rect: PhysicalRect {
            x: work.left,
            y: work.top,
            width: work.right - work.left,
            height: work.bottom - work.top,
        },
        dpi_scale,
    })
}

/// The panel's rect and side for a check triggered at the current cursor position: beside the
/// inventory when the cursor is over the right half of the game, beside the stash otherwise --
/// where the player left it on that side (`positions`), else EE2's placement -- as wide as the
/// player's `ui_scale` makes the panel. `None` only if even the cursor/monitor lookup fails.
pub fn panel_at_cursor(
    ui_scale: f32,
    positions: &PanelPositions,
) -> Option<(PhysicalRect, PanelSide)> {
    let (x, y) = cursor_pos()?;
    let screen = GameScreen::at((x, y))?;
    let side = PanelSide::at(screen.game, x);
    let scale = screen.dpi_scale * f64::from(ui_scale);
    Some((
        positions.rect(side, screen.game, screen.monitor, scale),
        side,
    ))
}

/// EE2's placement on `side`: where a double-click on the panel's title bar sends it back.
pub fn automatic_panel_rect(side: PanelSide, ui_scale: f32) -> Option<PhysicalRect> {
    let screen = GameScreen::at_cursor()?;
    Some(overlay_layout::panel_rect(
        screen.game,
        side,
        screen.dpi_scale * f64::from(ui_scale),
    ))
}

/// Where the panel waits before the first check: EE2's placement on the inventory side, since
/// that's where most checks happen and where the Loading/Failed placeholders should show.
pub fn default_panel_rect(ui_scale: f32) -> Option<PhysicalRect> {
    automatic_panel_rect(PanelSide::Inventory, ui_scale)
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
