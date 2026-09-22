//! Capability 1 POC: frameless + transparent-background + popup (`WS_EX_TOOLWINDOW |
//! WS_EX_TOPMOST` on Windows) window, with a runtime click-through toggle bound to the "t" key.
//!
//! Frameless/transparent/popup needs no raw platform code -- it is declarative via
//! `WindowOptions` (see `build_window_options` below, copied from GPUI's own
//! `examples/window_positioning.rs`). Click-through is not declarative on Windows
//! (`Window::set_input_region` is a Wayland-only no-op there too), so this example drives the
//! platform escape hatch directly: `WS_EX_LAYERED`/`WS_EX_TRANSPARENT` via
//! `poe2_oracle::platform::win32::Win32Overlay`.
//!
//! Windows-only: this project ships to Windows exclusively for now (see `POC_FINDINGS.md`'s
//! "Windows" section for why the earlier Linux/X11 spike's platform module and Xvfb harness
//! were dropped rather than dual-maintained).

use gpui::{
    App, Bounds, Context, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent, Render,
    SharedString, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
    div, point, prelude::*, px, rgb, size,
};
use gpui_platform::application;
use poe2_oracle::platform::win32::Win32Overlay;
use std::cell::RefCell;

/// Fixed screen-space geometry (arbitrary but stable, in case a future harness wants it).
const WINDOW_BOUNDS: (f32, f32, f32, f32) = (100., 100., 420., 320.);

struct WindowChrome {
    focus_handle: FocusHandle,
    click_through: bool,
    /// Resolved lazily on first render, once the platform window actually exists.
    overlay: RefCell<Option<Win32Overlay>>,
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
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if event.keystroke.key == "t" {
                    this.click_through = !this.click_through;
                    this.apply_click_through();
                    // Observable, harness-greppable proof independent of screenshot capture.
                    println!("CLICK_THROUGH_STATE={}", this.click_through);
                    cx.notify();
                }
            }))
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
            .child(div().text_sm().child("Press T to toggle click-through"))
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
    application().run(|cx: &mut App| {
        cx.open_window(build_window_options(), |window, cx| {
            // `titlebar: None` above means the window has no OS-drawn titlebar to source a
            // title from; set one explicitly so the window is identifiable (taskbar, Alt+Tab).
            window.set_window_title("Oracle POC — window_chrome");
            cx.new(|cx| WindowChrome {
                focus_handle: {
                    let handle = cx.focus_handle();
                    handle.focus(window, cx);
                    handle
                },
                click_through: false,
                overlay: RefCell::new(None),
            })
        })
        .unwrap();
        cx.activate(true);
    });
}
