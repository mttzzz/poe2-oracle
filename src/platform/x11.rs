//! X11 platform helper: the X11 half of what becomes a small `Platform` trait later. The Win32
//! half (`WS_EX_LAYERED`/`WS_EX_TRANSPARENT`, `RegisterHotKey` only if `global-hotkey`'s Windows
//! backend ever needs a native escape hatch) is out of scope here and belongs to phase-2
//! architecture.
//!
//! This module only covers what GPUI's own X11 backend does not already expose declaratively:
//!
//! - Click-through: `gpui::Window::set_input_region` exists, but reading gpui_linux's X11 window
//!   implementation confirms it is a no-op there (`fn set_input_region(&self, _: ...) {}`, the
//!   `PlatformWindow` trait default) -- only the Wayland backend overrides it, via
//!   `wl_surface::set_input_region`. X11 has no equivalent declarative option, so this toggles the
//!   SHAPE extension's input shape directly.
//! - Always-on-top: `WindowKind::PopUp` already sets `override_redirect` on X11 (confirmed by
//!   reading gpui_linux's window-creation code), which is what makes the window frameless and
//!   unmanaged by a window manager, but override-redirect windows are not WM-stacked at all, so
//!   there is no equivalent to "always on top" for them beyond raw X11 stacking order. This sends
//!   the standard EWMH `_NET_WM_STATE_ABOVE` hint as a best-effort supplement for real window
//!   managers; see the doc comment on `set_always_on_top` for why Xvfb cannot prove it.
//!
//! Frameless + transparent-background + popup window kind need no raw code at all: they are
//! `WindowOptions { kind: WindowKind::PopUp, titlebar: None,
//! window_background: WindowBackgroundAppearance::Transparent, .. }`, exactly as GPUI's own
//! `examples/window_positioning.rs` does it.

use anyhow::{Context as _, Result, bail};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection as _;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ClientMessageEvent, ConnectionExt as _, EventMask};
use x11rb::rust_connection::RustConnection;

/// A raw-X11 handle to a single GPUI window, opened over its own connection to `$DISPLAY`.
///
/// Deliberately does not reuse GPUI's own X11 connection: GPUI does not expose it, and opening a
/// second client connection to the same display for a handful of one-off requests is standard
/// X11 practice (the protocol has no concept of "the same process," every client is just a
/// socket).
pub struct X11Overlay {
    conn: RustConnection,
    window: u32,
    screen_root: u32,
}

impl X11Overlay {
    /// The raw X11 window id this overlay operates on.
    pub fn window_id(&self) -> u32 {
        self.window
    }

    /// Resolves `handle`'s X11 window id and opens a dedicated connection to `$DISPLAY` to
    /// operate on it. Call this only after the window has actually been created (e.g. from
    /// inside the `cx.open_window` callback) -- a handle requested any earlier has no platform
    /// window yet.
    pub fn from_window(handle: &impl HasWindowHandle) -> Result<Self> {
        let raw = handle
            .window_handle()
            .context("window has no platform handle yet -- call from_window after open_window")?
            .as_raw();
        let window = match raw {
            RawWindowHandle::Xcb(h) => h.window.get(),
            RawWindowHandle::Xlib(h) => h.window as u32,
            other => bail!("X11Overlay requires an Xcb/Xlib window handle, got {other:?}"),
        };

        let (conn, screen_num) =
            x11rb::connect(None).context("connecting to $DISPLAY for the platform helper")?;
        let screen_root = conn.setup().roots[screen_num].root;

        Ok(Self {
            conn,
            window,
            screen_root,
        })
    }

    /// Toggles mouse-transparency via the X11 SHAPE extension's input shape.
    ///
    /// `enabled = true` sets an empty input region, so every pointer/touch event passes through
    /// to whatever is behind the window (there is nothing else in this POC's Xvfb session, so
    /// the harness proves it with a dummy target window instead). `enabled = false` resets the
    /// region to the window's default (whole-window) shape via `XShapeMask` with a `None`
    /// pixmap, the standard way to clear a shape back to default.
    pub fn set_click_through(&self, enabled: bool) -> Result<()> {
        if enabled {
            self.conn
                .shape_rectangles(
                    shape::SO::SET,
                    shape::SK::INPUT,
                    xproto::ClipOrdering::UNSORTED,
                    self.window,
                    0,
                    0,
                    &[],
                )
                .context("XShapeRectangles request")?
                .check()
                .context("XShapeRectangles reply")?;
        } else {
            self.conn
                .shape_mask(shape::SO::SET, shape::SK::INPUT, self.window, 0, 0, x11rb::NONE)
                .context("XShapeMask request")?
                .check()
                .context("XShapeMask reply")?;
        }
        self.conn.flush().context("flushing X11 connection")?;
        Ok(())
    }

    /// Sends a `_NET_WM_STATE_ABOVE` client message to the root window: the standard EWMH way
    /// to ask a window manager to keep this window above normal windows
    /// (<https://specifications.freedesktop.org/wm-spec/1.4/ar01s05.html>).
    ///
    /// Best-effort only. A window manager has to be running and choose to honor the hint, and
    /// `WindowKind::PopUp` already marks the window `override_redirect`, which takes it out of
    /// WM-managed stacking entirely on most window managers. Xvfb (this POC's screenshot
    /// harness) runs with no window manager at all, so neither path is meaningfully provable
    /// there -- the POC's actual proof of "on top" is that this is the only window on screen.
    /// See `POC_FINDINGS.md`.
    pub fn set_always_on_top(&self, enabled: bool) -> Result<()> {
        const NET_WM_STATE_REMOVE: u32 = 0;
        const NET_WM_STATE_ADD: u32 = 1;

        let net_wm_state = self.intern_atom("_NET_WM_STATE")?;
        let net_wm_state_above = self.intern_atom("_NET_WM_STATE_ABOVE")?;

        let event = ClientMessageEvent::new(
            32,
            self.window,
            net_wm_state,
            [
                if enabled {
                    NET_WM_STATE_ADD
                } else {
                    NET_WM_STATE_REMOVE
                },
                net_wm_state_above,
                0,
                0,
                0,
            ],
        );
        self.conn
            .send_event(
                false,
                self.screen_root,
                EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
                event,
            )
            .context("sending _NET_WM_STATE_ABOVE client message")?
            .check()
            .context("_NET_WM_STATE_ABOVE reply")?;
        self.conn.flush().context("flushing X11 connection")?;
        Ok(())
    }

    fn intern_atom(&self, name: &str) -> Result<xproto::Atom> {
        Ok(self
            .conn
            .intern_atom(false, name.as_bytes())
            .context("InternAtom request")?
            .reply()
            .context("InternAtom reply")?
            .atom)
    }
}
