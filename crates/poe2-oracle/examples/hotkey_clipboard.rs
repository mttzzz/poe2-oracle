//! Capabilities 2+3 POC: a system-wide global hotkey (Ctrl+Alt+O) that reads the real clipboard
//! and re-renders with its contents.
//!
//! `global-hotkey`'s Windows backend registers the combo via `RegisterHotKey`, so delivery is
//! independent of which window currently has input focus -- it fires system-wide regardless of
//! focus by construction, not something that needs a foil app to demonstrate.
//!
//! Clipboard is read through GPUI's own native `App::read_from_clipboard()`, not `arboard` --
//! see `POC_FINDINGS.md`'s deviation notes.

use gpui::{
    App, Bounds, Context, Render, SharedString, Window, WindowBounds, div, prelude::*, px, rgb,
    size,
};
use gpui_platform::application;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, hotkey::{Code, HotKey, Modifiers}};
use std::time::Duration;

struct HotkeyClipboard {
    /// Held only to keep the OS-level key grab alive for the process lifetime; never read.
    _manager: GlobalHotKeyManager,
    trigger_count: u32,
    clipboard_text: Option<SharedString>,
}

impl Render for HotkeyClipboard {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let body: SharedString = match &self.clipboard_text {
            Some(text) => text.clone(),
            None => SharedString::from("(waiting for Ctrl+Alt+O …)"),
        };

        div()
            .font_family("DejaVu Sans")
            .flex()
            .flex_col()
            .size_full()
            .p_4()
            .gap_2()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xffffff))
            .child(div().text_sm().text_color(rgb(0x9a9a9a)).child(format!(
                "triggers: {}  —  press Ctrl+Alt+O anywhere on screen",
                self.trigger_count
            )))
            .child(div().text_lg().child(body))
    }
}

fn main() {
    let manager = GlobalHotKeyManager::new().expect("failed to create GlobalHotKeyManager");
    let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyO);
    manager
        .register(hotkey)
        .expect("failed to register Ctrl+Alt+O global hotkey");

    application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(520.), px(180.)), cx);
        cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Oracle POC — hotkey_clipboard");
                let view = cx.new(|_cx| HotkeyClipboard {
                    _manager: manager,
                    trigger_count: 0,
                    clipboard_text: None,
                });

                // Non-blocking poll of the crate's global (not `manager`-scoped) event
                // channel -- bridges it onto GPUI's own executor since global-hotkey delivers
                // events on its own background thread outside GPUI's event loop.
                cx.spawn({
                    let view = view.downgrade();
                    async move |cx| {
                        loop {
                            cx.background_executor()
                                .timer(Duration::from_millis(50))
                                .await;
                            let fired = matches!(
                                GlobalHotKeyEvent::receiver().try_recv(),
                                Ok(event) if event.state == global_hotkey::HotKeyState::Pressed
                            );
                            if fired {
                                let Some(view) = view.upgrade() else { break };
                                view.update(cx, |state, cx| {
                                    state.trigger_count += 1;
                                    state.clipboard_text = cx
                                        .read_from_clipboard()
                                        .and_then(|item| item.text())
                                        .map(SharedString::from);
                                    // Observable, harness-greppable proof independent of
                                    // screenshot capture.
                                    println!(
                                        "HOTKEY_FIRED trigger_count={} clipboard={:?}",
                                        state.trigger_count, state.clipboard_text
                                    );
                                    cx.notify();
                                });
                            }
                        }
                    }
                })
                .detach();

                view
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
