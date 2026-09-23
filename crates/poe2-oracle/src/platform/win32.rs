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
    GetLastError, HWND, LPARAM, LRESULT, SetLastError, WIN32_ERROR, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, GWL_EXSTYLE, GWL_STYLE, GWLP_WNDPROC, GetForegroundWindow, GetWindowLongPtrW,
    HWND_NOTOPMOST, HWND_TOPMOST, MA_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, WM_MOUSEACTIVATE, WNDPROC, WS_CAPTION, WS_EX_CLIENTEDGE,
    WS_EX_DLGMODALFRAME, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_STATICEDGE, WS_EX_TRANSPARENT,
    WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};

use crate::overlay_layout::PhysicalRect;

/// GPUI's own window procedure of each window [`Win32Overlay::set_no_activate`] wrapped, by the
/// window's handle value.
static WRAPPED_PROCS: LazyLock<Mutex<HashMap<isize, isize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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

    /// Makes a click on the window leave the keyboard with the game: a game that loses focus makes
    /// other overlays react (PoE Overlay II opens its Session Recap). `WS_EX_NOACTIVATE` alone
    /// isn't enough: `gpui_windows` answers every `WM_MOUSEACTIVATE` with `MA_ACTIVATE` itself
    /// (`events.rs`), so the window procedure is wrapped to answer `MA_NOACTIVATE` and hand
    /// everything else to GPUI's. Deferred like [`Self::set_bounds`]: the style change sends
    /// `WM_STYLECHANGED` synchronously.
    pub fn set_no_activate(&self) -> Result<()> {
        let wrapper: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
            no_activate_proc;
        let wrapper = wrapper as usize as isize;
        unsafe {
            let gpui_proc = GetWindowLongPtrW(self.hwnd, GWLP_WNDPROC);
            if gpui_proc == wrapper {
                return Ok(());
            }
            if gpui_proc == 0 {
                bail!(
                    "GetWindowLongPtrW(GWLP_WNDPROC) failed: {:?}",
                    GetLastError()
                );
            }
            // Recorded before the swap: the wrapper looks it up for the very next message.
            WRAPPED_PROCS
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(self.hwnd.0 as isize, gpui_proc);
            SetWindowLongPtrW(self.hwnd, GWLP_WNDPROC, wrapper);
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
}

/// The window procedure [`Win32Overlay::set_no_activate`] puts in front of GPUI's.
unsafe extern "system" fn no_activate_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_MOUSEACTIVATE {
        return LRESULT(MA_NOACTIVATE as isize);
    }
    let gpui_proc = WRAPPED_PROCS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&(hwnd.0 as isize))
        .copied()
        .unwrap_or(0);
    // SAFETY: the value `GWLP_WNDPROC` held before the swap, a window procedure of this window.
    let gpui_proc: WNDPROC = unsafe { std::mem::transmute::<isize, WNDPROC>(gpui_proc) };
    unsafe { CallWindowProcW(gpui_proc, hwnd, message, wparam, lparam) }
}
