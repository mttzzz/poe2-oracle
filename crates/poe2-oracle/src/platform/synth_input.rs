//! Synthesizes the in-game "copy item to clipboard" key combo (`Ctrl` + the game's configured
//! advanced-mod-description key + `C`) via raw `SendInput` -- mirroring `win32.rs`'s own posture
//! of using the `windows` crate directly rather than a general input-simulation crate.
//! `exiled-exchange-2`'s own equivalent (`main/src/shortcuts/Shortcuts.ts`, using `uiohook-napi`)
//! needs a separate input library only because its Electron main process isn't in-process with a
//! native Win32 API the way this crate already is.
//!
//! Sequence, key ordering, and the ~10ms gap before releasing the held keys are ported from that
//! same file's real, working `pressKeysToCopyItemText` -- its own comment there documents the
//! delay as a workaround for the game dropping release inputs sent with no gap between them.
//!
//! Verified live 2026-09-22 on the test machine: a real user pressing the quick-check hotkey over
//! an inventory item sees the game's advanced (Alt) description flicker -- the synthesized combo
//! does reach PoE2 through `SendInput` for a local, physically-pressed hotkey.
//!
//! Also plays the keys of a quick action ([`press_keys`], planned by `crate::quick_action`): the
//! Enter, Ctrl+V and the like around a paste into the game's chat or stash search.

use std::thread::sleep;
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
    RegisterHotKey, SendInput, UnregisterHotKey, VIRTUAL_KEY, VK_A, VK_C, VK_CONTROL, VK_DELETE,
    VK_F, VK_HOME, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_MENU, VK_RCONTROL, VK_RETURN, VK_RMENU,
    VK_RSHIFT, VK_SHIFT, VK_V,
};

use crate::quick_action::Key;

/// A throwaway id for [`copy_combo_taken`]'s probe registration; any id works for a thread-bound
/// hotkey that is released at once.
const PROBE_HOTKEY_ID: i32 = 0x0C0C;

/// Gap between the `C` tap and releasing the held modifier key(s), matching the reference's own
/// documented workaround for PoE dropping back-to-back release inputs with no delay.
const KEY_RELEASE_GAP: Duration = Duration::from_millis(10);

/// Gap before each keystroke of a quick action, for the same reason: the game drops inputs that
/// arrive back to back.
const KEY_STEP_GAP: Duration = Duration::from_millis(15);

/// Synthesizes the item-copy combo. Blocking/synchronous by design -- called directly from a
/// hotkey-fired context, not from async code.
///
/// `release_first` are the hotkey's own non-modifier keys (`E`), released before anything is
/// pressed -- EE2's `Shortcuts.ts` handler does the same for `keepModKeys` shortcuts. Without it
/// the game sees the still-held `E` alongside the copy combo.
///
/// `keep_mod_keys`: `true` while the player is still physically holding `Ctrl` -- `Ctrl` itself
/// is then never touched, to avoid double-pressing or fighting the real key state. `false` (a
/// quick tap already released it) synthesizes `Ctrl` down/up around the rest of the combo too.
///
/// Never panics and never returns an error: a `SendInput` count mismatch is only logged to
/// stderr, matching this crate's general tolerance (see `win32.rs`) for best-effort platform
/// calls where getting it exactly right isn't worth a `Result` in the signature.
pub fn send_copy_item_combo(
    advanced_mod_desc_key: VIRTUAL_KEY,
    keep_mod_keys: bool,
    release_first: &[VIRTUAL_KEY],
) {
    for &key in release_first {
        send_key(key, false);
    }

    // The game's own configured advanced-mod-desc key can itself be Ctrl (an unusual but valid
    // user configuration) -- avoid pressing/releasing that one physical key twice below.
    let mod_desc_is_ctrl = advanced_mod_desc_key == VK_CONTROL;

    if keep_mod_keys {
        send_key(advanced_mod_desc_key, true);
        tap(VK_C);
        sleep(KEY_RELEASE_GAP);
        send_key(advanced_mod_desc_key, false);
        return;
    }

    send_key(VK_CONTROL, true);
    if !mod_desc_is_ctrl {
        send_key(advanced_mod_desc_key, true);
    }
    tap(VK_C);
    sleep(KEY_RELEASE_GAP);
    if !mod_desc_is_ctrl {
        send_key(advanced_mod_desc_key, false);
    }
    send_key(VK_CONTROL, false);
}

/// Plays a quick action's `keys` into the game. `held` are the keys of the hotkey that fired it,
/// down as far as the game knows (a registered hotkey swallows only its own key, never the
/// modifiers): released first, or the chat's Enter would arrive as Ctrl+Enter. A release of a key
/// that's already up does nothing. Blocking, like [`send_copy_item_combo`]; never fails, only
/// logs.
pub fn press_keys(held: &[VIRTUAL_KEY], keys: &[Key]) {
    for &key in held {
        send_key(key, false);
    }
    for &key in keys {
        sleep(KEY_STEP_GAP);
        match key {
            Key::Enter => tap(VK_RETURN),
            Key::CtrlEnter => with_ctrl(VK_RETURN),
            Key::CtrlA => with_ctrl(VK_A),
            Key::CtrlF => with_ctrl(VK_F),
            Key::CtrlV => with_ctrl(VK_V),
            Key::Home => tap(VK_HOME),
            Key::Delete => tap(VK_DELETE),
        }
    }
}

/// Taps `vk` with Ctrl held, released after [`KEY_RELEASE_GAP`] like the copy combo's.
fn with_ctrl(vk: VIRTUAL_KEY) {
    send_key(VK_CONTROL, true);
    tap(vk);
    sleep(KEY_RELEASE_GAP);
    send_key(VK_CONTROL, false);
}

/// Presses (`down = true`) or releases (`down = false`) one key via a single synthesized
/// `SendInput` event. Home and Delete go as the extended keys they are: without the flag the
/// system reports the number pad's 7 and decimal point.
fn send_key(vk: VIRTUAL_KEY, down: bool) {
    let mut flags = if down {
        KEYBD_EVENT_FLAGS(0)
    } else {
        KEYEVENTF_KEYUP
    };
    if matches!(vk, VK_HOME | VK_DELETE) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        let vk_code = vk.0;
        log::warn!("SendInput: sent {sent}, expected 1 (vk={vk_code:#06x}, down={down})");
    }
}

/// Presses then immediately releases `vk`, with no delay between the two. The deliberate delays
/// are the callers': the gap before releasing held modifiers ([`KEY_RELEASE_GAP`]) and between a
/// quick action's keystrokes ([`KEY_STEP_GAP`]).
fn tap(vk: VIRTUAL_KEY) {
    send_key(vk, true);
    send_key(vk, false);
}

/// Whether another program holds the item-copy combo (`Ctrl` + `advanced_mod_desc_key` + `C`) as
/// a global hotkey. The game then never receives it and every check times out with nothing to
/// show -- EE2's most-reported failure ("nothing happens"), caused by GPU overlays and screen
/// recorders binding that combination. Probed by registering the combo for a moment: the system
/// refuses a combination someone else holds. `false` when it can't tell: an advanced-description
/// key that isn't a modifier makes a combo `RegisterHotKey` can't express.
pub fn copy_combo_taken(advanced_mod_desc_key: VIRTUAL_KEY) -> bool {
    let Some(modifier) = modifier_flag(advanced_mod_desc_key) else {
        return false;
    };
    let modifiers = MOD_CONTROL | modifier | MOD_NOREPEAT;
    // SAFETY: a thread-bound registration (no window), released before returning.
    match unsafe { RegisterHotKey(None, PROBE_HOTKEY_ID, modifiers, u32::from(VK_C.0)) } {
        Ok(()) => {
            let _ = unsafe { UnregisterHotKey(None, PROBE_HOTKEY_ID) };
            false
        }
        Err(_) => true,
    }
}

/// The combo as the player reads it: "Ctrl+Alt+C" for the default advanced-description key.
pub fn copy_combo_label(advanced_mod_desc_key: VIRTUAL_KEY) -> String {
    match modifier_flag(advanced_mod_desc_key) {
        Some(flag) if flag == MOD_ALT => "Ctrl+Alt+C".to_owned(),
        Some(flag) if flag == MOD_SHIFT => "Ctrl+Shift+C".to_owned(),
        _ => "Ctrl+C".to_owned(),
    }
}

fn modifier_flag(key: VIRTUAL_KEY) -> Option<HOT_KEY_MODIFIERS> {
    match key {
        VK_MENU | VK_LMENU | VK_RMENU => Some(MOD_ALT),
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT => Some(MOD_SHIFT),
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => Some(MOD_CONTROL),
        _ => None,
    }
}
