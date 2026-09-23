//! A one-line text field for the settings window. GPUI ships none (its `examples/input.rs` is a
//! ~700-line editor with IME, selection and a caret anywhere); this one covers what a chat
//! command, a stash search string or a league name needs: typing (the keyboard layout's own
//! characters, from `Keystroke::key_char`), Backspace, Ctrl+V pasting, Ctrl+A selecting
//! everything so the next key replaces it, and Enter or Esc finishing. The caret stays at the end.

use gpui::{
    Context, FocusHandle, Focusable, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    Render, SharedString, Window, div, prelude::*, px, rgb,
};

use crate::ui::theme::{BG_BUTTON_HOVER, BG_CONTROL, BORDER, GOLD, TEXT, TEXT_DIM, TEXT_MUTED};

/// Longer than any chat command or search string the game accepts (its stash search takes 250).
const MAX_CHARS: usize = 250;

pub struct TextField {
    text: String,
    placeholder: SharedString,
    focus_handle: FocusHandle,
    /// Everything is selected: the next character or paste replaces it, Backspace clears it.
    all_selected: bool,
}

impl TextField {
    pub fn new(
        text: impl Into<String>,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> TextField {
        TextField {
            text: text.into(),
            placeholder: placeholder.into(),
            focus_handle: cx.focus_handle(),
            all_selected: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// What the empty field shows until focused.
    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>) {
        self.placeholder = placeholder.into();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        // Every key is the field's while it has focus: Esc must not also cancel the window.
        cx.stop_propagation();
        match keystroke.key.as_str() {
            "enter" | "escape" | "tab" => {
                self.all_selected = false;
                window.blur(cx);
            }
            "backspace" => {
                if std::mem::take(&mut self.all_selected) {
                    self.text.clear();
                } else {
                    self.text.pop();
                }
            }
            "a" if modifiers.control => self.all_selected = !self.text.is_empty(),
            "v" if modifiers.control => {
                let pasted = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .unwrap_or_default();
                // One line: a pasted search string often ends with a newline.
                self.insert(pasted.lines().next().unwrap_or_default().trim());
            }
            _ if modifiers.control || modifiers.alt || modifiers.platform => {}
            _ => {
                if let Some(typed) = keystroke.key_char.as_deref() {
                    self.insert(typed);
                }
            }
        }
        cx.notify();
    }

    fn insert(&mut self, typed: &str) {
        if std::mem::take(&mut self.all_selected) {
            self.text.clear();
        }
        for character in typed.chars().filter(|character| !character.is_control()) {
            if self.text.chars().count() >= MAX_CHARS {
                break;
            }
            self.text.push(character);
        }
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let focus = self.focus_handle.clone();
        let empty = self.text.is_empty();
        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|field, event: &KeyDownEvent, window, cx| {
                field.handle_key(event, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |field, _event: &MouseDownEvent, window, cx| {
                    field.all_selected = false;
                    focus.focus(window, cx);
                    cx.notify();
                }),
            )
            .flex()
            .flex_1()
            .min_w_0()
            .h(px(26.))
            .items_center()
            .px(px(8.))
            .rounded_xs()
            .bg(rgb(BG_CONTROL))
            .border_1()
            .border_color(rgb(if focused { GOLD } else { BORDER }))
            .when(!focused, |this| {
                this.hover(|style| style.border_color(rgb(TEXT_DIM)))
            })
            .cursor_text()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .when(self.all_selected, |this| this.bg(rgb(BG_BUTTON_HOVER)))
                    .text_color(rgb(if empty { TEXT_MUTED } else { TEXT }))
                    .child(if empty && !focused {
                        self.placeholder.clone()
                    } else {
                        SharedString::from(self.text.clone())
                    }),
            )
            .when(focused, |this| {
                this.child(div().flex_none().w(px(1.)).h(px(14.)).bg(rgb(GOLD)))
            })
    }
}
