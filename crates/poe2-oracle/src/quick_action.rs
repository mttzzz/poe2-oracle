//! What a quick action types into the game: the text that goes on the clipboard and the keys
//! around its paste -- EE2's `typeInChat` and `stashSearch` (`main/src/shortcuts/text-box.ts`),
//! the way PoE2 takes typed text reliably: a paste, not keystrokes per character, so any
//! language's text arrives whatever the keyboard layout. Plain data, built and tested on every
//! target; `platform::synth_input::press_keys` plays the keys on Windows.

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
}
