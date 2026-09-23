//! A one-line text field for the settings window. GPUI ships none (its `examples/input.rs` is a
//! ~700-line editor with IME, selection and a caret anywhere); this one covers what a chat
//! command, a stash search string or a league name needs: typing (the keyboard layout's own
//! characters, from `Keystroke::key_char`), Backspace, Ctrl+V pasting, and Ctrl+A selecting
//! everything so the next key replaces it. The caret stays at the end.
//!
//! What was typed applies when the player leaves the field -- Enter, Tab, a click elsewhere, or
//! the window losing the keyboard: the field emits [`Committed`] when its text differs from the
//! one last committed. Esc puts that one back and leaves. It looks like `ui::style`'s controls: a
//! dark field whose edge warms to gold under the pointer and glows gold while it has the keyboard.

use gpui::{
    Context, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, Render, SharedString, Subscription, Window, div, prelude::*, px, rgb,
};

use crate::ui::style::{CONTROL_HEIGHT, CONTROL_RADIUS, ease_hover, ease_state, glow};
use crate::ui::theme::{
    BG_BUTTON_HOVER, BG_FIELD, BORDER_FIELD, GOLD, TEXT, TEXT_MUTED, blend, rems_from_px,
};

/// Longer than any chat command or search string the game accepts (its stash search takes 250).
const MAX_CHARS: usize = 250;

pub struct TextField {
    text: String,
    /// The text as last committed: what Esc puts back.
    committed: String,
    /// What the empty field shows until focused, worded each time it's shown: a placeholder in
    /// the app's words follows the interface language.
    placeholder: fn() -> &'static str,
    focus_handle: FocusHandle,
    /// Everything is selected: the next character or paste replaces it, Backspace clears it.
    all_selected: bool,
    /// Commits the text when the field loses the keyboard.
    _blur: Subscription,
}

/// The player left the field with a text other than the one last committed:
/// [`TextField::text`] has it.
pub struct Committed;

impl EventEmitter<Committed> for TextField {}

impl TextField {
    pub fn new(
        text: impl Into<String>,
        placeholder: fn() -> &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TextField {
        let text = text.into();
        let focus_handle = cx.focus_handle();
        let blur = cx.on_blur(&focus_handle, window, |field, _window, cx| field.commit(cx));
        TextField {
            committed: text.clone(),
            text,
            placeholder,
            focus_handle,
            all_selected: false,
            _blur: blur,
        }
    }

    /// The text as it reads now, committed or not.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Puts `text` in the field as committed, in place of whatever it held.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.text = text.into();
        self.committed.clone_from(&self.text);
        self.all_selected = false;
        cx.notify();
    }

    /// What the empty field shows until focused.
    pub fn set_placeholder(&mut self, placeholder: fn() -> &'static str) {
        self.placeholder = placeholder;
    }

    /// Takes the text as it reads, telling the owner when it changed.
    fn commit(&mut self, cx: &mut Context<Self>) {
        self.all_selected = false;
        if self.text != self.committed {
            self.committed.clone_from(&self.text);
            cx.emit(Committed);
        }
        cx.notify();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        // Every key is the field's while it has focus: Esc must not also close the window.
        cx.stop_propagation();
        match keystroke.key.as_str() {
            // Leaving commits (the blur subscription).
            "enter" | "tab" => window.blur(cx),
            "escape" => {
                self.text.clone_from(&self.committed);
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
        let empty = self.text.is_empty();
        let shown: SharedString = if empty && !focused {
            (self.placeholder)().into()
        } else {
            self.text.clone().into()
        };
        let field = div()
            .id("field")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|field, event: &KeyDownEvent, window, cx| {
                field.handle_key(event, window, cx);
            }))
            // The press itself moves the keyboard here (`track_focus`).
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|field, _event: &MouseDownEvent, _window, cx| {
                    field.all_selected = false;
                    cx.notify();
                }),
            )
            .relative()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .h(rems_from_px(CONTROL_HEIGHT))
            .px(rems_from_px(10.))
            .rounded(rems_from_px(CONTROL_RADIUS))
            .bg(rgb(BG_FIELD))
            .border_1()
            .text_size(rems_from_px(13.))
            .cursor_text()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .when(self.all_selected, |this| this.bg(rgb(BG_BUTTON_HOVER)))
                    .text_color(rgb(if empty { TEXT_MUTED } else { TEXT }))
                    .child(shown),
            )
            .when(focused, |this| {
                this.child(
                    div()
                        .flex_none()
                        .w(px(1.))
                        .h(rems_from_px(15.))
                        .bg(rgb(GOLD)),
                )
            })
            // The keyboard's gold ring, easing in and out over the edge.
            .child(ease_state(
                "focus",
                focused,
                div()
                    .absolute()
                    .inset_0()
                    .rounded(rems_from_px(CONTROL_RADIUS))
                    .border_1()
                    .border_color(rgb(GOLD)),
                |ring, on| ring.opacity(on).shadow(glow(GOLD, 0.6 * on)),
            ));
        ease_hover("field", field, |field, hover| {
            field
                .border_color(rgb(blend(BORDER_FIELD, GOLD, 0.7 * hover)))
                .shadow(glow(GOLD, 0.4 * hover))
        })
    }
}
