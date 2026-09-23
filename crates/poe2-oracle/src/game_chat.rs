//! Types into the game for the player: the quick actions' hotkeys and the trade overlay's replies.
//! The text goes through the clipboard -- `crate::quick_action` plans the keys around the paste --
//! and the player's own clipboard comes back right after.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::AsyncApp;
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;

use crate::platform::game_window::{self, Foreground};
use crate::platform::{clipboard_poll, synth_input};
use crate::quick_action;
use crate::settings::{QuickAction, QuickActionKind};

/// Set while something types: one at a time, as EE2's `restoreShortly` has it -- the keys of two
/// would interleave, and the second paste would hand the game the first one's text.
static TYPING: AtomicBool = AtomicBool::new(false);

/// Clears [`TYPING`] however the typing ends.
struct Typing;

impl Drop for Typing {
    fn drop(&mut self) {
        TYPING.store(false, Ordering::Release);
    }
}

/// Types `action` into the game. `held` are the keys of the hotkey that fired it, released first
/// (see `synth_input::press_keys`). A click in one of this app's windows made it the foreground:
/// the game gets the keyboard back first. Nothing is typed while something else types or another
/// program is in front -- its keys are its own. Whether it typed.
pub async fn type_action(action: &QuickAction, held: &[VIRTUAL_KEY], cx: &mut AsyncApp) -> bool {
    if TYPING.swap(true, Ordering::AcqRel) {
        return false;
    }
    let _typing = Typing;
    if game_window::foreground() == Foreground::ThisApp && game_window::reclaim_game_focus() {
        cx.background_executor()
            .timer(game_window::FOCUS_SWITCH_DELAY)
            .await;
    }
    if game_window::foreground() != Foreground::Game {
        return false;
    }
    let typing = quick_action::typing(action);
    match typing.clipboard {
        Some(text) => {
            clipboard_poll::paste_restoring(cx, &text, || {
                synth_input::press_keys(held, &typing.keys);
            })
            .await;
        }
        None => synth_input::press_keys(held, &typing.keys),
    }
    true
}

/// Sends `text` to the game's chat, the way a chat-command quick action does.
pub async fn send_chat(text: String, cx: &mut AsyncApp) -> bool {
    type_text(QuickActionKind::ChatCommand, text, cx).await
}

/// Types `text` into the search box of the stash (or whatever the game has open), the way a
/// stash-search quick action does: every word must be somewhere in an item's text.
pub async fn search_stash(text: String, cx: &mut AsyncApp) -> bool {
    type_text(QuickActionKind::StashSearch, text, cx).await
}

async fn type_text(kind: QuickActionKind, text: String, cx: &mut AsyncApp) -> bool {
    let action = QuickAction {
        kind,
        text,
        hotkey: None,
    };
    type_action(&action, &[], cx).await
}
