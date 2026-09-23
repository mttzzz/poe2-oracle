//! Reads the item text PoE2 puts on the clipboard in answer to its copy-item combo, then puts back
//! what the player had copied, via GPUI's own native clipboard
//! (`App::read_from_clipboard`/`write_to_clipboard`) -- not any third-party clipboard crate,
//! matching this crate's established convention (see `examples/hotkey_clipboard.rs` and
//! `poe2-oracle/Cargo.toml`'s own comment on why `arboard` is deliberately not a dependency: a
//! second clipboard client would just race GPUI's own for clipboard ownership).
//!
//! Sequence and default timing (`initial_delay`/`poll_interval` = 48ms, `timeout` = 500ms) are
//! ported from `exiled-exchange-2/main/src/shortcuts/HostClipboard.ts`'s real, working
//! `POLL_DELAY`/`POLL_LIMIT` constants: `POLL_DELAY` is used both as the delay before the first
//! read attempt and as the interval between subsequent attempts, and `POLL_LIMIT` is a budget
//! compared against an accumulated poll-interval count each iteration, not a single fixed wait.
//! A timeout returns `None` silently -- no error -- matching the reference's own empty `.catch`.
//!
//! The restore is the same file's `readItemText` with EE2's `restoreClipboard` setting on: opt-in
//! there (off by default), always on here, so a price check never leaves item text on the
//! player's clipboard. As in EE2 it happens in the poll tick that read the answer, not after
//! `RESTORE_AFTER` (120ms): that delay serves EE2's paste flows (`restoreShortly`), where the game
//! still has to read what EE2 put on the clipboard -- here the reader is this module, and it has
//! already read.
//!
//! Verified live 2026-09-22 against a real Russian PoE2 client on the test machine: hovering an
//! inventory item and sending `Ctrl+Alt+C` puts `Класс предмета: ...` text on the clipboard,
//! which is exactly what this poll now waits for.
//!
//! [`paste_restoring`] is the other direction, for quick actions: text the game pastes, and the
//! player's clipboard back after EE2's `RESTORE_AFTER`.

use gpui::{AsyncApp, ClipboardEntry, ClipboardItem};
use item_parser::looks_like_item_text;
use std::time::Duration;

/// How long the game has to read what a paste put on the clipboard before the player's own
/// content goes back -- EE2's `RESTORE_AFTER`. A game lagging past it pastes the restored text.
const RESTORE_AFTER: Duration = Duration::from_millis(120);

/// `None` when GPUI reads nothing: an empty clipboard, one holding no format GPUI reads, or one
/// another process has open at this instant.
fn read_clipboard(cx: &mut AsyncApp) -> Option<ClipboardItem> {
    cx.update(|app| app.read_from_clipboard())
}

/// `None` empties the clipboard: GPUI's Windows backend writes an item with no entries as a bare
/// `EmptyClipboard`, which reads back as `None` -- the same as a captured empty clipboard.
fn set_clipboard(cx: &mut AsyncApp, content: Option<ClipboardItem>) {
    let item = content.unwrap_or(ClipboardItem {
        entries: Vec::new(),
    });
    cx.update(|app| app.write_to_clipboard(item));
}

/// The game's answer, if `item` holds one. Checked in place rather than via
/// `ClipboardItem::text()`, which would copy whatever text the player had on every poll tick.
fn item_text(item: &ClipboardItem) -> Option<&str> {
    item.entries().iter().find_map(|entry| match entry {
        ClipboardEntry::String(string) if looks_like_item_text(string.text()) => {
            Some(string.text().as_str())
        }
        _ => None,
    })
}

/// Invokes `send_copy` (expected to synthesize the game's item-copy combo, e.g. via
/// `synth_input::send_copy_item_combo` -- taken as a parameter so this module stays decoupled from
/// the synth-input mechanism), polls the clipboard until item text appears or `timeout` elapses,
/// and restores what the clipboard held before the call.
///
/// Mirrors EE2's `HostClipboard.readItemText` step for step: the clipboard is captured first and,
/// only if it already holds item text (a leftover from an earlier copy, which would otherwise be
/// mistaken for this check's answer), emptied -- an empty clipboard is then what gets restored,
/// like EE2's `textBefore = ""`. "Item text" is [`looks_like_item_text`] -- every game client
/// language, not just English: a Russian client's `Класс предмета:` answer used to be rejected
/// here, so the poll timed out on every check and the hotkey appeared to do nothing.
///
/// The capture goes back as far as GPUI can write it: text, and images as PNG, GIF, JPEG or SVG --
/// never as a bitmap, so an app that pastes only bitmaps won't see a restored screenshot.
/// A copied file list, rich text's HTML/RTF and app-private formats are lost; the game's own copy
/// already replaced them. (EE2 captures text only.) On a timeout EE2 restores as well; here only
/// when the clipboard no longer reads as the check left it. If the game copied nothing (no item
/// under the cursor), the player's clipboard is still intact, and rewriting it would only strip
/// the formats GPUI can't write back. A clipboard GPUI reads as nothing counts as untouched.
///
/// Callers should pass `initial_delay`/`poll_interval` = 48ms and `timeout` = 500ms, matching
/// the reference's own `POLL_DELAY`/`POLL_LIMIT`.
pub async fn poll_item_clipboard(
    cx: &mut AsyncApp,
    send_copy: impl FnOnce(),
    initial_delay: Duration,
    poll_interval: Duration,
    timeout: Duration,
) -> Option<String> {
    let saved = match read_clipboard(cx) {
        Some(item) if item_text(&item).is_some() => {
            set_clipboard(cx, None);
            None
        }
        other => other,
    };

    send_copy();

    cx.background_executor().timer(initial_delay).await;

    let mut elapsed = Duration::ZERO;
    loop {
        let current = read_clipboard(cx);
        if let Some(text) = current.as_ref().and_then(item_text) {
            set_clipboard(cx, saved);
            return Some(text.to_owned());
        }

        elapsed += poll_interval;
        if elapsed >= timeout {
            if current.is_some() && current != saved {
                set_clipboard(cx, saved);
            }
            return None;
        }
        cx.background_executor().timer(poll_interval).await;
    }
}

/// Puts `text` on the clipboard, runs `paste` (expected to synthesize the keys that make the game
/// paste it, e.g. via `synth_input::press_keys`), and after [`RESTORE_AFTER`] puts back what the
/// clipboard held -- EE2's `HostClipboard.restoreShortly` with `restoreClipboard` on. Only if
/// the clipboard still holds `text`: whatever the player copied in the meantime stays.
pub async fn paste_restoring(cx: &mut AsyncApp, text: &str, paste: impl FnOnce()) {
    let saved = read_clipboard(cx);
    cx.update(|app| app.write_to_clipboard(ClipboardItem::new_string(text.to_owned())));
    paste();
    cx.background_executor().timer(RESTORE_AFTER).await;
    if read_clipboard(cx).and_then(|item| item.text()).as_deref() == Some(text) {
        set_clipboard(cx, saved);
    }
}
