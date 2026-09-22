//! Win32 platform helper: the Windows half of what becomes a small `Platform` trait later. The
//! X11 half (`src/platform/x11.rs`) is the reference implementation this mirrors; see it for the
//! shared rationale (`Window::set_input_region` covers Wayland only, never X11 or Windows, so
//! both platforms need a raw escape hatch for click-through).
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
//!   creation time (`gpui_windows/src/window.rs:490`) -- genuinely native, real always-on-top,
//!   no raw code needed for the static case (unlike X11, where override-redirect popups are not
//!   WM-stacked at all and the EWMH hint is only a best-effort ask). `set_always_on_top` here
//!   exists only so this module's shape matches `X11Overlay`'s for the runtime-toggle case, via
//!   `SetWindowPos` with the `HWND_TOPMOST`/`HWND_NOTOPMOST` sentinel handles.
//!
//! Frameless + transparent-background + popup window kind need no raw code at all here either,
//! same as X11: `WindowOptions { kind: WindowKind::PopUp, titlebar: None,
//! window_background: WindowBackgroundAppearance::Transparent, .. }`.
//!
//! Unverified: written and checked line-by-line against the real `windows` 0.62.2 crate source
//! (constant values, function signatures) downloaded into this project's lane, but never
//! actually run on Windows -- this workstation cross-compiles for `x86_64-pc-windows-gnu` from
//! Linux and has no Windows execution target. See `POC_FINDINGS.md` for what that does and does
//! not establish.

use anyhow::{Context as _, Result, bail};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{GetLastError, HWND, SetLastError, WIN32_ERROR};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SetWindowLongPtrW, SetWindowPos, WS_EX_LAYERED, WS_EX_TRANSPARENT,
};

/// A raw Win32 handle to a single GPUI window.
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

    /// The raw `HWND` value, as an integer, for diagnostics/logging.
    pub fn window_id(&self) -> isize {
        self.hwnd.0 as isize
    }

    /// Toggles mouse-transparency via `WS_EX_LAYERED | WS_EX_TRANSPARENT` on `GWL_EXSTYLE`.
    ///
    /// `enabled = true` adds both bits: `WS_EX_TRANSPARENT` removes the window from hit-testing
    /// (pointer input falls through to whatever is behind it), `WS_EX_LAYERED` is required
    /// alongside it for the window to keep compositing correctly once click-through is active.
    /// `enabled = false` clears only `WS_EX_TRANSPARENT`, leaving `WS_EX_LAYERED` set -- harmless
    /// to leave on a DirectComposition-rendered window (`gpui_windows` already sets
    /// `WS_EX_NOREDIRECTIONBITMAP` for its own GPU compositing path), and simpler than tracking
    /// whether this module was the one that first turned it on.
    pub fn set_click_through(&self, enabled: bool) -> Result<()> {
        // SetWindowLongPtrW's return is the *previous* value; 0 is ambiguous between "no error"
        // (the previous style really was 0) and failure. Clear the last-error code first and
        // only treat a 0 return as a real error if GetLastError also reports one afterward --
        // the standard pattern Microsoft's own docs recommend for this API.
        unsafe { SetLastError(WIN32_ERROR(0)) };
        let current = unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) } as u32;
        let updated = if enabled {
            current | WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0
        } else {
            current & !WS_EX_TRANSPARENT.0
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

    /// Runtime always-on-top toggle via `SetWindowPos`'s `HWND_TOPMOST`/`HWND_NOTOPMOST`
    /// sentinel handles. Not needed for this POC's static case -- see the module doc comment --
    /// provided so this type's shape matches `X11Overlay`'s.
    pub fn set_always_on_top(&self, enabled: bool) -> Result<()> {
        let insert_after = if enabled { HWND_TOPMOST } else { HWND_NOTOPMOST };
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(insert_after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
        .context("SetWindowPos(HWND_TOPMOST/HWND_NOTOPMOST) failed")
    }
}
