//! Win32 platform helper: the raw Win32 calls behind the app's overlay windows.
//!
//! This module only covers what GPUI's own Windows backend does not already expose
//! declaratively:
//!
//! - Click-through: unlike Wayland, Windows has no GPUI-level API for this at all. Reading
//!   `gpui_windows`'s `CreateWindowExW` call (`crates/gpui_windows/src/window.rs`) confirms it
//!   never sets `WS_EX_LAYERED`/`WS_EX_TRANSPARENT` for any `WindowKind`, so this toggles them
//!   directly via `SetWindowLongPtrW(GWL_EXSTYLE, ...)` -- the standard Win32 click-through
//!   recipe (`WS_EX_LAYERED` for a compositor-blended window, `WS_EX_TRANSPARENT` to remove the
//!   window from hit-testing so clicks fall through to whatever is behind it).
//! - Always-on-top: `WindowKind::PopUp` already sets `WS_EX_TOOLWINDOW | WS_EX_TOPMOST` at
//!   creation time (`gpui_windows/src/window.rs:490`) -- genuinely native, real always-on-top;
//!   [`Win32Overlay::set_bounds`] keeps it topmost when it moves the window.
//!
//! - Sizing the settings window with its content, which the UI scale grows and shrinks:
//!   `gpui_windows`' own `resize` keeps the top left corner in place, and its least size is set
//!   for good when the window is created, so [`Win32Overlay::zoom`] scales the window about the
//!   pointer and [`Win32Overlay::set_min_size`] answers `WM_GETMINMAXINFO` in its place.
//!
//! A transparent background and the popup window kind need no raw code:
//! `WindowOptions { kind: WindowKind::PopUp, titlebar: None,
//! window_background: WindowBackgroundAppearance::Transparent, .. }`. The frame Windows still
//! draws around such a window does (see [`Win32Overlay::disable_dwm_frame`] and
//! [`Win32Overlay::remove_frame`]).
//!
//! Verified live on the test machine (Windows 11, 200 % DPI) since 2026-09-22: click-through,
//! frame removal, placement and focus all behave as described here.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use anyhow::{Context as _, Result, bail};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{
    GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, SetLastError, WIN32_ERROR, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, GetMonitorInfoW, InvalidateRect,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, RGN_OR, SetWindowRgn, ValidateRect,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, GWL_EXSTYLE, GWL_STYLE, GWLP_WNDPROC, GetCursorPos,
    GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic,
    IsZoomed, MA_NOACTIVATE, MINMAXINFO, SW_HIDE, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, WM_DPICHANGED, WM_GETMINMAXINFO, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_SHOWWINDOW, WM_SIZE,
    WM_WINDOWPOSCHANGED, WNDPROC, WS_CAPTION, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_STATICEDGE, WS_EX_TRANSPARENT, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};

use super::game_window::{client_rect_on_screen, dpi_to_scale};
use crate::overlay_layout::PhysicalRect;

/// The windows whose procedure [`overlay_proc`] wraps, by the window's handle value: GPUI's own
/// procedure, and what the wrapper does before it. An entry goes with its window
/// (`WM_NCDESTROY`), since Windows hands its handle value to later windows.
static WRAPPED: LazyLock<Mutex<HashMap<isize, Wrapped>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct Wrapped {
    gpui_proc: isize,
    /// Answers `WM_MOUSEACTIVATE` with `MA_NOACTIVATE` ([`Win32Overlay::set_no_activate`]).
    no_activate: bool,
    /// Paints only in bursts ([`Win32Overlay::gate_paints`]).
    gate: Option<PaintGate>,
    /// The least client area the player can size the window to, in pixels at 96 DPI
    /// ([`Win32Overlay::set_min_size`]).
    min_size: Option<(f32, f32)>,
}

/// When a gated window's paints last went through to GPUI, and until when they all do; ticks of
/// `GetTickCount64`, milliseconds.
struct PaintGate {
    open_until: u64,
    last_passed: u64,
}

impl PaintGate {
    /// Whether a paint at `now` goes through: within a burst, or a trickle's worth after the last.
    fn due(&self, now: u64) -> bool {
        now < self.open_until || now.saturating_sub(self.last_passed) >= PAINT_TRICKLE_MS
    }
}

/// Whether `hwnd`'s next paint would go through to GPUI: any window but a
/// [`Win32Overlay::gate_paints`] one outside its bursts and trickle. For
/// `redraw_filter::filtered_redraw_window`, on `gpui_windows`' vsync thread.
pub(super) fn paint_due(hwnd: HWND) -> bool {
    let now = unsafe { GetTickCount64() };
    WRAPPED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&(hwnd.0 as isize))
        .and_then(|wrapped| wrapped.gate.as_ref())
        .is_none_or(|gate| gate.due(now))
}

/// `WM_MOUSELEAVE`, which the `windows` crate files under `Win32_UI_Controls`.
const WM_MOUSELEAVE: u32 = 0x02A3;

/// How long a gated window's paints go through once a burst opens: well past its transitions,
/// 120 ms (`ui::style::TRANSITION`).
const PAINT_BURST_MS: u64 = 400;
/// How often a gated window's paint goes through outside a burst: whatever made its view dirty
/// unforeseen still shows within this.
const PAINT_TRICKLE_MS: u64 = 500;

/// A raw Win32 handle to a single GPUI window. `Copy`: it's a plain handle, and deferred window
/// operations (see [`Win32Overlay::set_bounds`]) need to carry it into a spawned task.
#[derive(Debug, Clone, Copy)]
pub struct Win32Overlay {
    hwnd: HWND,
}

impl Win32Overlay {
    /// Resolves `handle`'s Win32 window handle. Call this only after the window has actually
    /// been created (e.g. from inside the `cx.open_window` callback) -- a handle requested any
    /// earlier has no platform window yet.
    pub fn from_window(handle: &impl HasWindowHandle) -> Result<Self> {
        let raw = handle
            .window_handle()
            .context("window has no platform handle yet -- call from_window after open_window")?
            .as_raw();
        let hwnd = match raw {
            RawWindowHandle::Win32(h) => HWND(h.hwnd.get() as *mut core::ffi::c_void),
            other => bail!("Win32Overlay requires a Win32 window handle, got {other:?}"),
        };
        Ok(Self { hwnd })
    }

    /// Toggles mouse-transparency via `WS_EX_LAYERED | WS_EX_TRANSPARENT` on `GWL_EXSTYLE`.
    ///
    /// `enabled = true` adds both bits: `WS_EX_TRANSPARENT` removes the window from hit-testing
    /// (pointer input falls through to whatever is behind it), and it only does so for a layered
    /// window. `enabled = false` clears both. `WS_EX_LAYERED` must not stay on an interactive
    /// window: a layered DirectComposition window (`gpui_windows` sets
    /// `WS_EX_NOREDIRECTIONBITMAP`) is hit-tested against a surface that doesn't follow what it
    /// draws -- live, with it left on, clicks and the wheel below y ~1520 of the 2160-pixel price
    /// panel went through to the game while the panel was plainly drawn there.
    pub fn set_click_through(&self, enabled: bool) -> Result<()> {
        // SetWindowLongPtrW's return is the *previous* value; 0 is ambiguous between "no error"
        // (the previous style really was 0) and failure. Clear the last-error code first and
        // only treat a 0 return as a real error if GetLastError also reports one afterward --
        // the standard pattern Microsoft's own docs recommend for this API.
        unsafe { SetLastError(WIN32_ERROR(0)) };
        let current = unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) } as u32;
        let bits = WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0;
        let updated = if enabled {
            current | bits
        } else {
            current & !bits
        };
        let result = unsafe { SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, updated as isize) };
        if result == 0 {
            let err = unsafe { GetLastError() };
            if err.0 != 0 {
                bail!("SetWindowLongPtrW(GWL_EXSTYLE) failed: {err:?}");
            }
        }
        Ok(())
    }

    /// Removes the 1px border and rounded corners Windows 11's DWM draws around every top-level
    /// window. `gpui_windows` creates `WindowKind::PopUp` with `WINDOW_STYLE(0)` (an overlapped
    /// window, `crates/gpui_windows/src/window.rs`), so DWM frames it even with a transparent,
    /// title-less client area -- on a click-through overlay that frame is the only thing left
    /// visible, a permanent outline over the game. Both attributes are Windows 11 (22000+) only;
    /// earlier builds reject them and draw no such frame in the first place.
    pub fn disable_dwm_frame(&self) -> Result<()> {
        let border = DWMWA_COLOR_NONE;
        unsafe {
            DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_BORDER_COLOR,
                (&raw const border).cast(),
                size_of_val(&border) as u32,
            )
        }
        .context("DwmSetWindowAttribute(DWMWA_BORDER_COLOR) failed")?;
        let corners: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_DONOTROUND;
        unsafe {
            DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const corners).cast(),
                size_of_val(&corners) as u32,
            )
        }
        .context("DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE) failed")
    }

    /// Makes the whole window client area. `gpui_windows` creates `WindowKind::PopUp` with
    /// `WINDOW_STYLE(0)`, which Windows turns into a captioned, bordered overlapped window; GPUI
    /// hides the title bar by keeping only the top of the default non-client area
    /// (`events.rs`'s `handle_calc_client_size`), so the border's left, right and bottom edges stay
    /// non-client -- strips where the transparent window background shows the game through, the
    /// gap between the panel and the inventory (14 physical px at 200%, measured live
    /// 2026-09-22). A borderless `WS_POPUP` has no non-client area at all. Deferred like
    /// [`Self::set_bounds`]: `SWP_FRAMECHANGED` sends `WM_NCCALCSIZE`/`WM_SIZE` synchronously.
    pub fn remove_frame(&self) -> Result<()> {
        let frame = (WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX).0;
        let edges =
            (WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE | WS_EX_DLGMODALFRAME | WS_EX_STATICEDGE).0;
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            let ex_style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
            // See `set_click_through` for why the last error is cleared first.
            SetLastError(WIN32_ERROR(0));
            if SetWindowLongPtrW(
                self.hwnd,
                GWL_STYLE,
                ((style & !frame) | WS_POPUP.0) as isize,
            ) == 0
                && GetLastError().0 != 0
            {
                bail!("SetWindowLongPtrW(GWL_STYLE) failed: {:?}", GetLastError());
            }
            SetLastError(WIN32_ERROR(0));
            if SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, (ex_style & !edges) as isize) == 0
                && GetLastError().0 != 0
            {
                bail!(
                    "SetWindowLongPtrW(GWL_EXSTYLE) failed: {:?}",
                    GetLastError()
                );
            }
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        .context("SetWindowPos(SWP_FRAMECHANGED) failed")
    }

    /// Moves/resizes the window to `rect` (physical pixels), keeping it topmost and never
    /// activating it. Call it from a spawned foreground task, never from inside `render`: the
    /// synchronous `WM_SIZE` this triggers re-enters GPUI's own window state, which is why
    /// `gpui_windows` defers its own `SetWindowPos` calls the same way (`window.rs`'s `resize`).
    pub fn set_bounds(&self, rect: PhysicalRect) -> Result<()> {
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                SWP_NOACTIVATE,
            )
        }
        .context("SetWindowPos(bounds) failed")
    }

    /// Keeps the window above every window that isn't topmost -- the game's included -- or, with
    /// `topmost` false, only on top of those: a `WindowKind::Normal` window (the settings window)
    /// is created without `WS_EX_TOPMOST`, and steps down while a window it opened that isn't
    /// topmost (the sign-in window) must show in front of it. Deferred like [`Self::set_bounds`]:
    /// `SetWindowPos` sends `WM_WINDOWPOSCHANGED` synchronously.
    pub fn set_topmost(&self, topmost: bool) -> Result<()> {
        let band = if topmost {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(band),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
        .with_context(|| format!("SetWindowPos(topmost: {topmost}) failed"))
    }

    /// Whether this window currently has keyboard focus (the player clicked into the panel).
    pub fn is_foreground(&self) -> bool {
        let foreground = unsafe { GetForegroundWindow() };
        foreground == self.hwnd
    }

    /// Shows (never activating it -- the game keeps keyboard focus) or hides the OS window.
    /// Hiding is how an idle overlay disappears: `gpui_windows` implements
    /// `WindowBackgroundAppearance::Transparent` with the undocumented
    /// `SetWindowCompositionAttribute(ACCENT_ENABLE_TRANSPARENTGRADIENT)` (`window.rs`'s
    /// `set_background_appearance`), which on the Windows 11 test machine still lays a faint light
    /// tint over everything behind the window -- an empty-but-shown overlay reads as a permanent
    /// pale rectangle over the game (verified live 2026-09-22 on a 4x crop). Deferred like
    /// [`Self::set_bounds`]: `ShowWindow` sends `WM_SHOWWINDOW`/`WM_SIZE` synchronously.
    pub fn set_shown(&self, shown: bool) {
        let command = if shown { SW_SHOWNOACTIVATE } else { SW_HIDE };
        // Returns the *previous* visibility, not an error.
        let _ = unsafe { ShowWindow(self.hwnd, command) };
    }

    /// Shows only `shown` of the window -- rects relative to its top left corner -- and lets the
    /// game show everywhere else: a window region, which DWM clips the window's composition to. A
    /// transparent GPUI background won't do, it tints what's behind it ([`Self::set_shown`]).
    /// Deferred like [`Self::set_bounds`]: `SetWindowRgn` sends `WM_WINDOWPOSCHANGED`
    /// synchronously.
    pub fn set_region(&self, shown: &[PhysicalRect]) -> Result<()> {
        unsafe {
            let region = CreateRectRgn(0, 0, 0, 0);
            for rect in shown {
                let part = CreateRectRgn(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
                CombineRgn(Some(region), Some(region), Some(part), RGN_OR);
                let _ = DeleteObject(part.into());
            }
            // The system owns the region once it's set.
            if SetWindowRgn(self.hwnd, Some(region), true) == 0 {
                let _ = DeleteObject(region.into());
                bail!("SetWindowRgn failed: {:?}", GetLastError());
            }
        }
        Ok(())
    }

    /// Makes a click on the window leave the keyboard with the game: a game that loses focus makes
    /// other overlays react (PoE Overlay II opens its Session Recap). `WS_EX_NOACTIVATE` alone
    /// isn't enough: `gpui_windows` answers every `WM_MOUSEACTIVATE` with `MA_ACTIVATE` itself
    /// (`events.rs`), so the window procedure is wrapped ([`overlay_proc`]) to answer
    /// `MA_NOACTIVATE`. Deferred like [`Self::set_bounds`]: the style change sends
    /// `WM_STYLECHANGED` synchronously.
    pub fn set_no_activate(&self) -> Result<()> {
        self.wrap(|wrapped| wrapped.no_activate = true)?;
        unsafe {
            let ex_style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
            // See `set_click_through` for why the last error is cleared first.
            SetLastError(WIN32_ERROR(0));
            if SetWindowLongPtrW(
                self.hwnd,
                GWL_EXSTYLE,
                (ex_style | WS_EX_NOACTIVATE.0) as isize,
            ) == 0
                && GetLastError().0 != 0
            {
                bail!(
                    "SetWindowLongPtrW(GWL_EXSTYLE) failed: {:?}",
                    GetLastError()
                );
            }
        }
        Ok(())
    }

    /// Lets GPUI paint the window only in bursts: once the app says what it shows has changed
    /// ([`Self::open_paints`]), while the mouse is on it, and once moved, resized or shown --
    /// else a paint once each `PAINT_TRICKLE_MS`. `gpui_windows` invalidates every window of the
    /// app on each refresh of the display (`platform.rs`'s `begin_vsync_thread`), so each
    /// visible one would be drawn 60 to 165 times a second whether or not it has anything new --
    /// the XP overlay's plates are up all the while the game is played, for words that change
    /// every few seconds -- and would wake the UI thread as often; `redraw_filter`, installed at
    /// start, keeps the refreshes from even asking outside a burst.
    pub fn gate_paints(&self) -> Result<()> {
        let now = unsafe { GetTickCount64() };
        self.wrap(|wrapped| {
            wrapped.gate = Some(PaintGate {
                open_until: now + PAINT_BURST_MS,
                last_passed: 0,
            });
        })
    }

    /// Opens a burst of a [`Self::gate_paints`] window's paints, and asks for the first: what it
    /// shows has changed.
    pub fn open_paints(&self) {
        let now = unsafe { GetTickCount64() };
        let gated = WRAPPED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get_mut(&(self.hwnd.0 as isize))
            .and_then(|wrapped| wrapped.gate.as_mut())
            .map(|gate| gate.open_until = now + PAINT_BURST_MS)
            .is_some();
        if gated {
            let _ = unsafe { InvalidateRect(Some(self.hwnd), None, false) };
        }
    }

    /// Puts [`overlay_proc`] in front of GPUI's window procedure, once, and has `edit` set what
    /// it does for this window.
    fn wrap(&self, edit: impl FnOnce(&mut Wrapped)) -> Result<()> {
        let wrapper: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = overlay_proc;
        let wrapper = wrapper as usize as isize;
        let mut wrapped = WRAPPED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = wrapped.get_mut(&(self.hwnd.0 as isize)) {
            edit(entry);
            return Ok(());
        }
        let gpui_proc = unsafe { GetWindowLongPtrW(self.hwnd, GWLP_WNDPROC) };
        if gpui_proc == 0 {
            bail!("GetWindowLongPtrW(GWLP_WNDPROC) failed: {:?}", unsafe {
                GetLastError()
            });
        }
        let mut entry = Wrapped {
            gpui_proc,
            no_activate: false,
            gate: None,
            min_size: None,
        };
        edit(&mut entry);
        // Recorded before the swap: the wrapper looks it up for the very next message.
        wrapped.insert(self.hwnd.0 as isize, entry);
        drop(wrapped);
        unsafe { SetWindowLongPtrW(self.hwnd, GWLP_WNDPROC, wrapper) };
        Ok(())
    }

    /// Gives the window the keyboard: a [`Self::set_no_activate`] window whose click took none,
    /// when something in it is to be typed into. GPUI's own `activate_window` won't do -- its
    /// first call also puts the window back where it was created (`set_window_placement`). Call
    /// it right after the click, whose input event lets this process take the foreground;
    /// deferred like [`Self::set_bounds`], since the activation sends `WM_ACTIVATE` and
    /// `WM_SETFOCUS` synchronously.
    pub fn activate(&self) {
        // Both return what was active or focused before, not an error.
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
        }
    }

    /// Makes `size` -- a client area's width and height, in pixels at 96 DPI -- the least the
    /// player can size the window to, in place of the one it was created with, which
    /// `gpui_windows` keeps for good: the settings window's grows and shrinks with the UI scale.
    /// Deferred like [`Self::set_bounds`], ahead of a [`Self::zoom`] that shrinks the window below
    /// the least it had.
    pub fn set_min_size(&self, size: (f32, f32)) -> Result<()> {
        self.wrap(|wrapped| wrapped.min_size = Some(size))
    }

    /// Scales the window's client area by `factor` about the pointer -- or about its middle, the
    /// pointer elsewhere -- so what's under the pointer stays there: the settings window growing
    /// or shrinking with the UI scale keeps the stepper that changed it under the pointer. Kept
    /// on the monitor's work area; a maximized or minimized window keeps its size. Deferred like
    /// [`Self::set_bounds`]: `SetWindowPos` sends `WM_SIZE` synchronously.
    pub fn zoom(&self, factor: f64) -> Result<()> {
        if unsafe { IsZoomed(self.hwnd) }.as_bool() || unsafe { IsIconic(self.hwnd) }.as_bool() {
            return Ok(());
        }
        let client = client_rect_on_screen(self.hwnd).context("the window has no client area")?;
        let mut frame = RECT::default();
        unsafe { GetWindowRect(self.hwnd, &mut frame) }.context("GetWindowRect failed")?;
        let mut pointer = POINT::default();
        let (anchor_x, anchor_y) = if unsafe { GetCursorPos(&mut pointer) }.is_ok()
            && (client.x..client.x + client.width).contains(&pointer.x)
            && (client.y..client.y + client.height).contains(&pointer.y)
        {
            (pointer.x, pointer.y)
        } else {
            (client.x + client.width / 2, client.y + client.height / 2)
        };
        let scaled = |length: i32| (f64::from(length) * factor).round() as i32;
        let mut zoomed = PhysicalRect {
            x: anchor_x - scaled(anchor_x - client.x),
            y: anchor_y - scaled(anchor_y - client.y),
            width: scaled(client.width),
            height: scaled(client.height),
        };
        if let Some(work) = work_area(self.hwnd) {
            zoomed.width = zoomed.width.min(work.width);
            zoomed.height = zoomed.height.min(work.height);
            zoomed.x = zoomed.x.min(work.x + work.width - zoomed.width).max(work.x);
            zoomed.y = zoomed
                .y
                .min(work.y + work.height - zoomed.height)
                .max(work.y);
        }
        // What the window has around its client area: `gpui_windows` keeps the resize borders.
        let (left, top) = (client.x - frame.left, client.y - frame.top);
        let right = frame.right - (client.x + client.width);
        let bottom = frame.bottom - (client.y + client.height);
        unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                zoomed.x - left,
                zoomed.y - top,
                zoomed.width + left + right,
                zoomed.height + top + bottom,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        .context("SetWindowPos(zoom) failed")
    }
}

/// The work area -- the monitor less the taskbar -- of the monitor `hwnd` is on.
fn work_area(hwnd: HWND) -> Option<PhysicalRect> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    let work = info.rcWork;
    Some(PhysicalRect {
        x: work.left,
        y: work.top,
        width: work.right - work.left,
        height: work.bottom - work.top,
    })
}

/// Puts `size` -- a client area in pixels at 96 DPI ([`Win32Overlay::set_min_size`]) -- in `info`,
/// the `MINMAXINFO` of `hwnd`'s `WM_GETMINMAXINFO`, as the least size of the whole window at its
/// DPI. A minimized window's rects say nothing of its frame, so its answer stays GPUI's.
fn set_min_track_size(hwnd: HWND, info: &mut MINMAXINFO, (width, height): (f32, f32)) {
    if unsafe { IsIconic(hwnd) }.as_bool() {
        return;
    }
    let mut frame = RECT::default();
    let (Some(client), Ok(())) = (client_rect_on_screen(hwnd), unsafe {
        GetWindowRect(hwnd, &mut frame)
    }) else {
        return;
    };
    let scale = dpi_to_scale(unsafe { GetDpiForWindow(hwnd) });
    let physical = |length: f32| (f64::from(length) * scale).round() as i32;
    info.ptMinTrackSize = POINT {
        x: physical(width) + (frame.right - frame.left) - client.width,
        y: physical(height) + (frame.bottom - frame.top) - client.height,
    };
}

/// The window procedure [`Win32Overlay::wrap`] puts in front of GPUI's: `MA_NOACTIVATE` for a
/// [`Win32Overlay::set_no_activate`] window, and a [`Win32Overlay::gate_paints`] window's paints
/// only in bursts -- a paint outside one is marked done, and the display's next refresh asks
/// again, unless `redraw_filter` drops that refresh's ask. Everything else goes to GPUI's
/// procedure, the registry unlocked first: GPUI's may send messages to another wrapped window of
/// the app. GPUI's answer to `WM_GETMINMAXINFO` then gets a [`Win32Overlay::set_min_size`]
/// window's least size, and the window's entry goes with its last message.
unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let (gpui_proc, no_activate, swallowed, min_size) = {
        let mut wrapped = WRAPPED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = wrapped.get_mut(&(hwnd.0 as isize)) else {
            return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        };
        let mut swallowed = false;
        if let Some(gate) = entry.gate.as_mut() {
            let now = unsafe { GetTickCount64() };
            match message {
                WM_PAINT => {
                    if gate.due(now) {
                        gate.last_passed = now;
                    } else {
                        swallowed = true;
                    }
                }
                WM_MOUSEMOVE | WM_MOUSELEAVE | WM_MOUSEWHEEL | WM_LBUTTONDOWN | WM_LBUTTONUP
                | WM_SIZE | WM_WINDOWPOSCHANGED | WM_SHOWWINDOW | WM_DPICHANGED => {
                    gate.open_until = now + PAINT_BURST_MS;
                }
                _ => {}
            }
        }
        (
            entry.gpui_proc,
            entry.no_activate,
            swallowed,
            entry.min_size,
        )
    };
    if message == WM_MOUSEACTIVATE && no_activate {
        return LRESULT(MA_NOACTIVATE as isize);
    }
    if swallowed {
        let _ = unsafe { ValidateRect(Some(hwnd), None) };
        return LRESULT(0);
    }
    // SAFETY: the value `GWLP_WNDPROC` held before the swap, a window procedure of this window.
    let gpui_proc: WNDPROC = unsafe { std::mem::transmute::<isize, WNDPROC>(gpui_proc) };
    let answer = unsafe { CallWindowProcW(gpui_proc, hwnd, message, wparam, lparam) };
    match message {
        WM_GETMINMAXINFO => {
            if let Some(size) = min_size {
                // SAFETY: a `WM_GETMINMAXINFO`'s `lparam` points at the `MINMAXINFO` it fills.
                let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
                set_min_track_size(hwnd, info, size);
            }
        }
        WM_NCDESTROY => {
            WRAPPED
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&(hwnd.0 as isize));
        }
        _ => {}
    }
    answer
}
