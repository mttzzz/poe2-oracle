//! Types a quick action into the game for the player. The text goes through the clipboard --
//! `crate::quick_action` plans the keys around the paste -- and the player's own clipboard comes
//! back right after. A denied command never goes (`quick_action::denied_command`), and one press
//! of the hotkey's key types once (`quick_action::KEY_PRESSES`).

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::AsyncApp;
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;

use crate::platform::game_window::{self, Foreground};
use crate::platform::{clipboard_poll, synth_input};
use crate::quick_action;
use crate::settings::QuickAction;

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
/// (see `synth_input::press_keys`). Nothing is typed while something else types or another program
/// is in front -- its keys are its own -- nor a denied command, nor again for the press of the
/// hotkey's key that already typed: a held key's auto-repeat.
pub async fn type_action(action: &QuickAction, held: &[VIRTUAL_KEY], cx: &mut AsyncApp) {
    if let Some(denied) = quick_action::denied_command(&action.text) {
        log::warn!(
            "quick action refused: {} is a denied command",
            denied.command
        );
        return;
    }
    if TYPING.swap(true, Ordering::AcqRel) {
        return;
    }
    let _typing = Typing;
    if game_window::foreground() != Foreground::Game {
        return;
    }
    if let Some(hotkey) = action.hotkey
        && !quick_action::KEY_PRESSES.claim(hotkey.key.virtual_key())
    {
        log::info!("{hotkey}: a repeat of the held key, not typed again");
        return;
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
}
