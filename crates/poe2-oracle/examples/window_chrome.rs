//! Capability 1 POC: frameless + transparent-background + popup (`WS_EX_TOOLWINDOW |
//! WS_EX_TOPMOST` on Windows) window, with a runtime click-through toggle bound to a *global*
//! Ctrl+E hotkey.
//!
//! Frameless/transparent/popup needs no raw platform code -- it is declarative via
//! `WindowOptions` (see `build_window_options` below, copied from GPUI's own
//! `examples/window_positioning.rs`). Click-through is not declarative on Windows
//! (`Window::set_input_region` is a Wayland-only no-op there too), so this example drives the
//! platform escape hatch directly: `WS_EX_LAYERED`/`WS_EX_TRANSPARENT` via
//! `poe2_oracle::platform::win32::Win32Overlay`.
//!
//! The toggle is a *global* hotkey (`global-hotkey` crate), not a window-focused `on_key_down`:
//! confirmed as a real bug on real hardware (manual test, see `POC_FINDINGS.md`) that once
//! click-through is ON, mouse clicks -- and in practice keyboard focus too, since whatever the
//! user clicks through to (game, browser) becomes the new OS foreground window -- no longer
//! reach this window at all, so a window-scoped key handler can never fire again to turn
//! click-through back OFF. A global hotkey is delivered via `WM_HOTKEY` to the registering
//! thread's queue regardless of which window currently has focus, so it stays reachable in
//! every state.
//!
//! `Code::KeyE` registers Windows virtual-key `VK_E`, not a layout-specific character. Windows
//! keeps the alphabetic VK codes tied to physical key position (not the letter/glyph a layout
//! types there) specifically so shortcuts keep working across layouts -- confirmed empirically
//! on the real Windows box with the active layout switched to Russian, see `POC_FINDINGS.md`.
//!
//! Windows-only: this project ships to Windows exclusively for now (see `POC_FINDINGS.md`'s
//! "Windows" section for why the earlier Linux/X11 spike's platform module and Xvfb harness
//! were dropped rather than dual-maintained).

use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
use gpui::{
    App, Bounds, Context, MouseButton, MouseDownEvent, Render, SharedString, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, prelude::*,
    px, rgb, size,
};
use gpui_platform::application;
use poe2_oracle::platform::win32::Win32Overlay;
use std::cell::RefCell;
use std::time::Duration;

/// Fixed screen-space geometry (arbitrary but stable, in case a future harness wants it).
const WINDOW_BOUNDS: (f32, f32, f32, f32) = (100., 100., 420., 320.);

struct WindowChrome {
    click_through: bool,
    /// Resolved lazily on first render, once the platform window actually exists.
    overlay: RefCell<Option<Win32Overlay>>,
    /// Held only for its RAII unregister-on-drop; never read after construction. Per the
    /// `global_hotkey` crate's own docs, this must be created on the same OS thread that runs
    /// the platform's message loop -- true here, since `application().run` blocks that thread
    /// (the one `main` called it from) for the whole app lifetime.
    _hotkey_manager: GlobalHotKeyManager,
}

impl WindowChrome {
    fn ensure_overlay(&self, window: &Window) {
        if self.overlay.borrow().is_some() {
            return;
        }
        match Win32Overlay::from_window(window) {
            Ok(overlay) => {
                println!("WIN32_OVERLAY_WINDOW_ID={}", overlay.window_id());
                *self.overlay.borrow_mut() = Some(overlay);
            }
            Err(err) => eprintln!("window_chrome: Win32Overlay::from_window failed: {err:?}"),
        }
    }

    fn apply_click_through(&self) {
        let overlay = self.overlay.borrow();
        let Some(overlay) = overlay.as_ref() else {
            eprintln!("window_chrome: no overlay yet, cannot toggle click-through");
            return;
        };
        if let Err(err) = overlay.set_click_through(self.click_through) {
            eprintln!("window_chrome: set_click_through({}) failed: {err:?}", self.click_through);
        }
    }
}

impl Render for WindowChrome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_overlay(window);

        let (bg, state_text) = if self.click_through {
            (rgb(0x1c5e2e), "click-through: ON")
        } else {
            (rgb(0x6e1c1c), "click-through: OFF")
        };

        div()
            .id("root")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _event: &MouseDownEvent, _window, _cx| {
                    // Observable proof that a click reached window_chrome itself (expected only
                    // while click-through is OFF -- once the SHAPE input region is emptied, this
                    // must NOT fire, and the click must reach whatever is behind instead).
                    println!("WINDOW_CHROME_GOT_CLICK");
                }),
            )
            .font_family("DejaVu Sans")
            .flex()
            .flex_col()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .bg(bg)
            .text_color(rgb(0xffffff))
            .child(div().text_lg().child(SharedString::from(state_text)))
            .child(div().text_sm().child("Press Ctrl+E to toggle click-through"))
    }
}

fn build_window_options() -> WindowOptions {
    let (x, y, w, h) = WINDOW_BOUNDS;
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(x), px(y)),
            size(px(w), px(h)),
        ))),
        titlebar: None,
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::PopUp,
        is_movable: false,
        ..Default::default()
    }
}

fn main() {
    // Must be created on this thread -- see the `global_hotkey` crate's own docs and the module
    // doc comment above: this is the thread `application().run` pumps the platform's message
    // loop on for the whole app lifetime.
    let hotkey_manager =
        GlobalHotKeyManager::new().expect("window_chrome: GlobalHotKeyManager::new failed");
    let toggle_hotkey = HotKey::new(Some(Modifiers::CONTROL), Code::KeyE);
    hotkey_manager
        .register(toggle_hotkey)
        .expect("window_chrome: failed to register Ctrl+E global hotkey");

    application().run(move |cx: &mut App| {
        cx.open_window(build_window_options(), |window, cx| {
            // `titlebar: None` above means the window has no OS-drawn titlebar to source a
            // title from; set one explicitly so the window is identifiable (taskbar, Alt+Tab).
            window.set_window_title("Oracle POC — window_chrome");
            let view = cx.new(|_cx| WindowChrome {
                click_through: false,
                overlay: RefCell::new(None),
                _hotkey_manager: hotkey_manager,
            });

            // `global_hotkey`'s receiver is a plain thread-safe `crossbeam_channel::Receiver`
            // with a non-blocking `try_recv`, not a future -- polling it on a timer from GPUI's
            // own foreground executor is simpler and more robust than bridging through an extra
            // OS thread + channel, and 30ms latency is imperceptible for a hotkey toggle.
            let weak = view.downgrade();
            cx.spawn(async move |cx| {
                loop {
                    if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                        if event.id() == toggle_hotkey.id() && event.state() == HotKeyState::Pressed {
                            let Some(view) = weak.upgrade() else { return };
                            view.update(cx, |state, cx| {
                                state.click_through = !state.click_through;
                                state.apply_click_through();
                                // Observable, harness-greppable proof independent of screenshot
                                // capture.
                                println!("CLICK_THROUGH_STATE={}", state.click_through);
                                cx.notify();
                            });
                        }
                    }
                    cx.background_executor().timer(Duration::from_millis(30)).await;
                }
            })
            .detach();

            view
        })
        .unwrap();
        cx.activate(true);
    });
}
