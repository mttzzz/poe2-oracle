//! What a quick action types into the game: the text that goes on the clipboard and the keys
//! around its paste -- EE2's `typeInChat` and `stashSearch` (`main/src/shortcuts/text-box.ts`),
//! the way PoE2 takes typed text reliably: a paste, not keystrokes per character, so any
//! language's text arrives whatever the keyboard layout. And what it never types -- the chat
//! commands that destroy something or change it for good ([`denied_command`]) -- and how often:
//! once per press of its key ([`KEY_PRESSES`]). Plain data, built and tested on every target;
//! `platform::synth_input::press_keys` plays the keys on Windows.

use std::sync::atomic::{AtomicU32, Ordering};

use crate::settings::{QuickAction, QuickActionKind};

/// EE2's stand-in for the last player who whispered: `@last спасибо` answers them,
/// `/invite @last` names them in a command, and `@last` alone opens a whisper to them. The
/// game's Ctrl+Enter opens the chat already addressed to that player (`@name `), which is what
/// fills the name in.
pub const LAST_WHISPER: &str = "@last";

/// The characters the chat input, opened with Enter, trades the channel it opened in for when
/// one comes first -- global, party, whisper, trade, guild and command: EE2's `AUTO_CLEAR`. Any
/// other text replaces the input's contents instead (Ctrl+A before the paste).
const CHANNEL_PREFIXES: [char; 6] = ['#', '%', '@', '$', '&', '/'];

/// One synthesized keystroke.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Enter,
    /// Opens the chat addressed to the last player who whispered.
    CtrlEnter,
    CtrlA,
    /// Focuses the open stash's (or a vendor's) search box, its text selected.
    CtrlF,
    CtrlV,
    Home,
    Delete,
}

/// A quick action's input: `clipboard`, if any, goes on the clipboard, then `keys` are pressed
/// -- among them the Ctrl+V that pastes it.
#[derive(PartialEq, Eq, Debug)]
pub struct Typing {
    pub clipboard: Option<String>,
    pub keys: Vec<Key>,
}

/// How `action` gets into the game.
pub fn typing(action: &QuickAction) -> Typing {
    let text = action.text.as_str();
    match action.kind {
        QuickActionKind::StashSearch => pasted(text, vec![Key::CtrlF], vec![Key::Enter]),
        QuickActionKind::ChatCommand => chat_typing(text),
    }
}

fn chat_typing(text: &str) -> Typing {
    if text == LAST_WHISPER {
        // The whisper opens and waits for the player's own words.
        return Typing {
            clipboard: None,
            keys: vec![Key::CtrlEnter],
        };
    }
    if let Some(message) = text
        .strip_prefix(LAST_WHISPER)
        .and_then(|rest| rest.strip_prefix(' '))
    {
        return pasted(message, vec![Key::CtrlEnter], vec![Key::Enter]);
    }
    if let Some(command) = text
        .strip_suffix(LAST_WHISPER)
        .filter(|command| command.ends_with(' '))
    {
        // The chat opens as `@name `; Home and Delete leave `name `, and the paste goes in front.
        // EE2 presses Home twice: the first one only focuses the input with a controller.
        let before = vec![Key::CtrlEnter, Key::Home, Key::Home, Key::Delete];
        return pasted(command, before, vec![Key::Enter]);
    }
    let mut before = vec![Key::Enter];
    if !text.starts_with(CHANNEL_PREFIXES) {
        before.push(Key::CtrlA);
    }
    pasted(text, before, vec![Key::Enter])
}

fn pasted(text: &str, before: Vec<Key>, after: Vec<Key>) -> Typing {
    let mut keys = before;
    keys.push(Key::CtrlV);
    keys.extend(after);
    Typing {
        clipboard: Some(text.to_owned()),
        keys,
    }
}

/// A chat command no quick action sends: it destroys something or changes it for good, and a
/// mis-pressed key must not do that. VibeTools fences its chat hotkeys the same way
/// (`DENY_COMMANDS` in its `main.js`: `/destroy` and `/clear_ignore_list`); the rest come from
/// the game's command list (poe2wiki.net/wiki/Chat).
pub struct DeniedCommand {
    /// As the game's command list writes it; matched ignoring case.
    pub command: &'static str,
    /// What the settings window says under an action that would send it.
    pub warning: &'static str,
}

static DENIED_COMMANDS: [DeniedCommand; 4] = [
    // "destroys the item on your cursor (be careful!)" -- the wiki; VibeTools denies it.
    DeniedCommand {
        command: "/destroy",
        warning: "Команда /destroy уничтожает предмет — быстрые действия её не отправляют",
    },
    // "removes all accounts from your ignore list" -- the wiki; VibeTools denies it.
    DeniedCommand {
        command: "/clear_ignore_list",
        warning: "Команда /clear_ignore_list очищает весь список игнорируемых — быстрые действия \
                  её не отправляют",
    },
    // Destroys the race reward unique on the cursor for an account-bound skin that "cannot be
    // traded to other players or turned back into the original item" (poewiki.net/wiki/Chat);
    // in PoE2 since 0.1.1e.
    DeniedCommand {
        command: "/convertracereward",
        warning: "Команда /convertracereward уничтожает предмет, превращая его в облик — быстрые \
                  действия её не отправляют",
    },
    // Resets the Atlas; the game takes it only when no map is left to run (0.1.1c).
    DeniedCommand {
        command: "/ResetAtlas",
        warning: "Команда /ResetAtlas сбрасывает атлас — быстрые действия её не отправляют",
    },
];

/// The denied command `text` would send, if any: a line's first word once a channel's sign and a
/// whisper's addressee are off its front -- `%/destroy` and `@last /destroy` count too, in any
/// case and between any spaces. Stash searches are held to it as well: they are pasted and sent
/// with Enter like a chat line, and with the chat open they would be one.
pub fn denied_command(text: &str) -> Option<&'static DeniedCommand> {
    text.lines().find_map(|line| {
        let word = command_word(line)?;
        DENIED_COMMANDS
            .iter()
            .find(|denied| denied.command.eq_ignore_ascii_case(word))
    })
}

/// A line's first word, after any channel signs at its front -- a whisper's with its addressee.
fn command_word(line: &str) -> Option<&str> {
    let mut rest = line.trim_start();
    loop {
        let mut chars = rest.chars();
        rest = match chars.next() {
            // A whisper: the addressee, then the message.
            Some('@') => chars
                .as_str()
                .split_once(char::is_whitespace)
                .map_or("", |(_, message)| message),
            Some(sign) if sign != '/' && CHANNEL_PREFIXES.contains(&sign) => chars.as_str(),
            _ => return rest.split_whitespace().next(),
        }
        .trim_start();
    }
}

/// The keyboard's presses of each key, so that one press of a quick action's key types once. A
/// held key fires its hotkey again: typing lifts the hotkey's keys first
/// (`synth_input::press_keys`), which to the system is the key let go, so the keyboard's
/// auto-repeat, once it starts, counts as a new press. Only the keyboard knows whether the player
/// really let go: the low-level keyboard hook (`platform::esc_hook`) reports each key it sees go
/// down or up, and what programs type -- this one's lift included -- doesn't count.
pub struct KeyPresses {
    /// Per virtual-key code: how many times the key went down, times two, plus [`DOWN`] while it
    /// is down -- a held key's value names its press.
    presses: [AtomicU32; 256],
    /// Per virtual-key code: the press that typed last.
    typed: [AtomicU32; 256],
}

/// The bit of a [`KeyPresses`] value that is set while the key is down.
const DOWN: u32 = 1;

/// Every key's presses: the keyboard hook records them, `game_chat` claims them.
pub static KEY_PRESSES: KeyPresses = KeyPresses::new();

impl KeyPresses {
    const fn new() -> KeyPresses {
        KeyPresses {
            presses: [const { AtomicU32::new(0) }; 256],
            typed: [const { AtomicU32::new(0) }; 256],
        }
    }

    /// A key going down (`down`) or up as the keyboard hook sees it, `injected` when a program
    /// typed it. Only the hook's thread calls this.
    pub fn record(&self, vk: u16, down: bool, injected: bool) {
        if injected {
            return;
        }
        let Some(presses) = self.presses.get(usize::from(vk)) else {
            return;
        };
        let value = presses.load(Ordering::Relaxed);
        let next = match (down, value & DOWN != 0) {
            // The auto-repeat of a held key: the same press.
            (true, true) => return,
            (true, false) => value.wrapping_add(2) | DOWN,
            (false, _) => value & !DOWN,
        };
        presses.store(next, Ordering::Relaxed);
    }

    /// Whether the hotkey of `vk` firing may type: the first fire for the press of `vk` that is
    /// down may, a later one for the same press may not. A key not seen down -- let go by now, or
    /// pressed by a program -- has no press to repeat, and always may.
    pub fn claim(&self, vk: u16) -> bool {
        let index = usize::from(vk);
        let (Some(presses), Some(typed)) = (self.presses.get(index), self.typed.get(index)) else {
            return true;
        };
        let press = presses.load(Ordering::Relaxed);
        press & DOWN == 0 || typed.swap(press, Ordering::Relaxed) != press
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Key::*;

    fn typed(kind: QuickActionKind, text: &str) -> (Option<String>, Vec<Key>) {
        let action = QuickAction {
            kind,
            text: text.to_owned(),
            hotkey: None,
        };
        let Typing { clipboard, keys } = typing(&action);
        (clipboard, keys)
    }

    #[test]
    fn chat_texts_follow_ee2s_sequences() {
        let chat = |text| typed(QuickActionKind::ChatCommand, text);
        let pasted = |text: &str| Some(text.to_owned());
        // A leading channel character takes over the chat's channel by itself.
        assert_eq!(
            chat("/hideout"),
            (pasted("/hideout"), vec![Enter, CtrlV, Enter])
        );
        assert_eq!(chat("%go"), (pasted("%go"), vec![Enter, CtrlV, Enter]));
        // Anything else replaces whatever the input opened with.
        assert_eq!(
            chat("всем привет"),
            (pasted("всем привет"), vec![Enter, CtrlA, CtrlV, Enter])
        );
        assert_eq!(
            chat("@last спасибо"),
            (pasted("спасибо"), vec![CtrlEnter, CtrlV, Enter])
        );
        assert_eq!(
            chat("/invite @last"),
            (
                pasted("/invite "),
                vec![CtrlEnter, Home, Home, Delete, CtrlV, Enter]
            )
        );
        assert_eq!(chat("@last"), (None, vec![CtrlEnter]));
        // Only the whole word is the placeholder: these whisper players named "lastochka" and
        // end on someone else's name.
        assert_eq!(
            chat("@lastochka привет"),
            (pasted("@lastochka привет"), vec![Enter, CtrlV, Enter])
        );
        assert_eq!(
            chat("/invite x@last"),
            (pasted("/invite x@last"), vec![Enter, CtrlV, Enter])
        );
    }

    #[test]
    fn a_stash_search_pastes_into_the_search_box() {
        assert_eq!(
            typed(QuickActionKind::StashSearch, "\"rare\" \"ilvl: 8\""),
            (
                Some("\"rare\" \"ilvl: 8\"".to_owned()),
                vec![CtrlF, CtrlV, Enter]
            )
        );
    }

    #[test]
    fn dangerous_commands_are_denied_however_they_are_written() {
        for (text, command) in [
            ("/destroy", "/destroy"),
            ("  /DESTROY  ", "/destroy"),
            ("/Destroy now", "/destroy"),
            // Behind a channel's sign, or a whisper's addressee.
            ("%/destroy", "/destroy"),
            ("# /destroy", "/destroy"),
            ("@last /destroy", "/destroy"),
            ("%@last /destroy", "/destroy"),
            // On any line of a text with several.
            ("всем привет\n/destroy", "/destroy"),
            ("/clear_ignore_list", "/clear_ignore_list"),
            ("/convertracereward", "/convertracereward"),
            ("/resetatlas", "/ResetAtlas"),
        ] {
            let denied = denied_command(text).map(|denied| denied.command);
            assert_eq!(denied, Some(command), "{text:?}");
        }
    }

    #[test]
    fn other_texts_are_not_denied() {
        for text in [
            "/hideout",
            "/invite @last",
            "@last спасибо",
            "@last",
            "",
            // Only the whole command counts, and only as the first word.
            "/destroyer",
            "/clear",
            "не пиши /destroy",
        ] {
            let denied = denied_command(text).map(|denied| denied.command);
            assert_eq!(denied, None, "{text:?}");
        }
    }

    /// F5's virtual-key code.
    const F5: u16 = 0x74;

    #[test]
    fn a_held_key_types_once_per_press() {
        let keys = KeyPresses::new();
        let keyboard = |down| keys.record(F5, down, false);
        keyboard(true);
        assert!(keys.claim(F5));
        // The action lifts the key as far as the system knows, and the auto-repeat of the key the
        // player still holds fires the hotkey again.
        keys.record(F5, false, true);
        keyboard(true);
        keyboard(true);
        assert!(!keys.claim(F5));
        keyboard(true);
        assert!(!keys.claim(F5));
        // Let go and pressed again: a new press types.
        keyboard(false);
        keyboard(true);
        assert!(keys.claim(F5));
    }

    #[test]
    fn a_key_not_seen_down_always_types() {
        let keys = KeyPresses::new();
        // Pressed by a program, not on the keyboard.
        keys.record(F5, true, true);
        assert!(keys.claim(F5));
        assert!(keys.claim(F5));
        // A tap let go before its hotkey's fire got here.
        keys.record(F5, true, false);
        keys.record(F5, false, false);
        assert!(keys.claim(F5));
        assert!(keys.claim(F5));
    }
}
