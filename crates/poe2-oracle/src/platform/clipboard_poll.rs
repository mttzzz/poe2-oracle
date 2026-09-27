//! Reads the item text PoE2 puts on the clipboard in answer to its copy-item combo, then puts back
//! what the player had copied, via GPUI's own native clipboard
//! (`App::read_from_clipboard`/`write_to_clipboard`) -- not any third-party clipboard crate (see
//! `poe2-oracle/Cargo.toml`'s own comment on why `arboard` is deliberately not a dependency: a
//! second clipboard client would just race GPUI's own for clipboard ownership). Only what GPUI
//! can't express goes to the Win32 clipboard directly, from the same thread GPUI's own calls run
//! on: the privacy marks of what the player copied ([`holds_private_content`]) and of a quick
//! action's text ([`write_private_text`]).
//!
//! The sequence is ported from `exiled-exchange-2/main/src/shortcuts/HostClipboard.ts`, its
//! budget too (`POLL_LIMIT`, 500 ms: [`ANSWER_TIMEOUT`]); its 48 ms between reads is 5 ms between
//! looks at the clipboard's change count here ([`POLL_INTERVAL`], see [`poll_item_clipboard`]).
//! A timeout returns `None` silently -- no error -- matching the reference's own empty `.catch`.
//!
//! The restore is the same file's `readItemText` with EE2's `restoreClipboard` setting on: opt-in
//! there (off by default), always on here, so a price check never leaves item text on the
//! player's clipboard. As in EE2 it happens in the poll tick that read the answer, not after
//! `RESTORE_AFTER` (120ms): that delay serves EE2's paste flows (`restoreShortly`), where the game
//! still has to read what EE2 put on the clipboard -- here the reader is this module, and it has
//! already read.
//!
//! What the player copied goes back only if its source didn't mark it private. A password manager
//! (KeePass, Bitwarden, 1Password) marks a copied password to stay out of Windows' clipboard
//! history and cloud clipboard, and GPUI would write it back as plain text, into both. Such content
//! is never captured ([`Saved::Private`]): the check or quick action replaces it, and the clipboard
//! is emptied afterwards -- the password leaves the clipboard, as the manager's own timer would
//! take it, and is copied again when needed.
//!
//! Verified live 2026-09-22 against a real Russian PoE2 client on the test machine: hovering an
//! inventory item and sending `Ctrl+Alt+C` puts `Класс предмета: ...` text on the clipboard,
//! which is exactly what this poll now waits for.
//!
//! [`paste_restoring`] is the other direction, for quick actions: text the game pastes, and the
//! player's clipboard back after EE2's `RESTORE_AFTER`.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use gpui::{AsyncApp, ClipboardEntry, ClipboardItem};
use item_parser::looks_like_item_text;
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::core::{Owned, PCWSTR, w};

/// How long the game has to read what a paste put on the clipboard before the player's own
/// content goes back -- EE2's `RESTORE_AFTER`. A game lagging past it pastes the restored text.
const RESTORE_AFTER: Duration = Duration::from_millis(120);

/// How often a check looks for the game's answer to the copy combo: at the clipboard's change
/// count, which doesn't open the clipboard, reading the clipboard itself only once the count
/// moves. EE2 reads the clipboard every 48 ms, so its answer waited 24 ms on average past the
/// game's copy, 48 ms at worst.
const POLL_INTERVAL: Duration = Duration::from_millis(5);
/// How long the game has to answer the copy combo: EE2's `POLL_LIMIT`. No item under the cursor
/// runs this out.
const ANSWER_TIMEOUT: Duration = Duration::from_millis(500);

/// The marks a program puts on content it copied that nobody should keep -- a password manager
/// on a password. This one, present at all, asks every program watching the clipboard to leave
/// the content alone.
static EXCLUDE_FROM_MONITORING: LazyLock<u32> =
    LazyLock::new(|| register_format(w!("ExcludeClipboardContentFromMonitorProcessing")));
/// A DWORD: 0 keeps the content out of Windows' clipboard history (Win+V).
static CAN_INCLUDE_IN_HISTORY: LazyLock<u32> =
    LazyLock::new(|| register_format(w!("CanIncludeInClipboardHistory")));
/// A DWORD: 0 keeps the content off the cloud clipboard, which syncs it to the player's other
/// devices.
static CAN_UPLOAD_TO_CLOUD: LazyLock<u32> =
    LazyLock::new(|| register_format(w!("CanUploadToCloudClipboard")));

/// `name`'s clipboard format; 0 -- which no clipboard holds -- if Windows can't register it.
fn register_format(name: PCWSTR) -> u32 {
    // SAFETY: `name` is a static NUL-terminated string.
    unsafe { RegisterClipboardFormatW(name) }
}

/// What the clipboard held before a check or a quick action used it.
enum Saved {
    /// What GPUI read of it -- `None` for nothing, an empty clipboard among others -- which goes
    /// back as far as GPUI can write it.
    Content(Option<ClipboardItem>),
    /// Content its source marked private ([`holds_private_content`]): not read, and never
    /// written back -- the clipboard is emptied instead.
    Private,
}

impl Saved {
    fn capture(cx: &mut AsyncApp) -> Saved {
        if holds_private_content() {
            Saved::Private
        } else {
            Saved::Content(read_clipboard(cx))
        }
    }

    fn restore(self, cx: &mut AsyncApp) {
        match self {
            Saved::Content(content) => set_clipboard(cx, content),
            Saved::Private => set_clipboard(cx, None),
        }
    }
}

/// Whether the clipboard holds content its source marked private: with
/// [`EXCLUDE_FROM_MONITORING`], or with 0 for [`CAN_INCLUDE_IN_HISTORY`] or
/// [`CAN_UPLOAD_TO_CLOUD`]. A mark whose value can't be read counts as a 0.
fn holds_private_content() -> bool {
    let present = |format: u32| {
        // SAFETY: a plain query; it needs no open clipboard.
        unsafe { IsClipboardFormatAvailable(format) }.is_ok()
    };
    if present(*EXCLUDE_FROM_MONITORING) {
        return true;
    }
    let marks = [*CAN_INCLUDE_IN_HISTORY, *CAN_UPLOAD_TO_CLOUD];
    if !marks.into_iter().any(present) {
        return false;
    }
    // SAFETY: opened by this thread and closed right below.
    if unsafe { OpenClipboard(None) }.is_err() {
        // Another program has it open at this instant.
        return true;
    }
    let private = marks
        .into_iter()
        .filter(|&format| present(format))
        .any(|format| read_dword(format).is_none_or(|value| value == 0));
    // SAFETY: this thread opened it above.
    let _ = unsafe { CloseClipboard() };
    private
}

/// The DWORD the clipboard, open on this thread, holds as `format`.
fn read_dword(format: u32) -> Option<u32> {
    // SAFETY: the handle is the clipboard's, valid while it stays open; the block is read only
    // between its lock and unlock, and only as far as its size.
    unsafe {
        let memory = HGLOBAL(GetClipboardData(format).ok()?.0);
        if GlobalSize(memory) < size_of::<u32>() {
            return None;
        }
        let data = GlobalLock(memory);
        if data.is_null() {
            return None;
        }
        let value = data.cast::<u32>().read_unaligned();
        let _ = GlobalUnlock(memory);
        Some(value)
    }
}

/// Puts `text` on the clipboard marked to stay out of Windows' clipboard history and cloud
/// clipboard: a quick action's text is there for the game to paste, not for the player to keep.
fn write_private_text(text: &str) -> windows::core::Result<()> {
    let text: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: opened by this thread, written while open, and closed on every path.
    unsafe {
        OpenClipboard(None)?;
        let written = EmptyClipboard().and_then(|()| set_data(u32::from(CF_UNICODETEXT.0), &text));
        if written.is_ok() {
            for format in [*CAN_INCLUDE_IN_HISTORY, *CAN_UPLOAD_TO_CLOUD] {
                // Best effort: a mark that fails only lets the text into the history or the cloud.
                let _ = set_data(format, &[0u32]);
            }
        }
        let _ = CloseClipboard();
        written
    }
}

/// Puts `data` on the clipboard, open on this thread, as `format`.
fn set_data<T: Copy>(format: u32, data: &[T]) -> windows::core::Result<()> {
    // SAFETY: the block holds exactly `data`, copied in between its lock and unlock; `Owned` frees
    // it on a failure, and once the clipboard has taken it, it is the clipboard's to free.
    unsafe {
        let memory = Owned::new(GlobalAlloc(GMEM_MOVEABLE, size_of_val(data))?);
        let target = GlobalLock(*memory);
        if target.is_null() {
            return Err(windows::core::Error::from_thread());
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), target.cast::<T>(), data.len());
        let _ = GlobalUnlock(*memory);
        SetClipboardData(format, Some(HANDLE(memory.0)))?;
        std::mem::forget(memory);
    }
    Ok(())
}

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
/// the synth-input mechanism), waits for item text to appear on the clipboard, for up to
/// `ANSWER_TIMEOUT`, and restores what the clipboard held before the call.
///
/// Mirrors EE2's `HostClipboard.readItemText` step for step: the clipboard is captured first and,
/// only if it already holds item text (a leftover from an earlier copy, which would otherwise be
/// mistaken for this check's answer), emptied -- an empty clipboard is then what gets restored,
/// like EE2's `textBefore = ""`. "Item text" is [`looks_like_item_text`] -- every game client
/// language, not just English: a Russian client's `Класс предмета:` answer used to be rejected
/// here, so the poll timed out on every check and the hotkey appeared to do nothing.
///
/// The wait is EE2's but finer (`POLL_INTERVAL`): every 5 ms the clipboard's change count, and
/// the clipboard itself once the count has moved -- again while it can't be read (the game still
/// has it open, or it was emptied before the answer is written). Windows' timer ticks every
/// 15.6 ms unless a process asks for finer, so the wait asks for 1 ms ticks while it lasts. The
/// log says how long the game took to answer.
///
/// The capture goes back as far as GPUI can write it: text, and images as PNG, GIF, JPEG or SVG --
/// never as a bitmap, so an app that pastes only bitmaps won't see a restored screenshot.
/// A copied file list, rich text's HTML/RTF and app-private formats are lost; the game's own copy
/// already replaced them. (EE2 captures text only.) Content marked private isn't captured, and the
/// clipboard is left empty after the answer instead. On a timeout EE2 restores as well; here only
/// when the clipboard no longer reads as the check left it. If the game copied nothing (no item
/// under the cursor), the player's clipboard is still intact, and rewriting it would only strip
/// the formats GPUI can't write back. A clipboard GPUI reads as nothing counts as untouched.
pub async fn poll_item_clipboard(cx: &mut AsyncApp, send_copy: impl FnOnce()) -> Option<String> {
    let saved = match Saved::capture(cx) {
        Saved::Content(Some(item)) if item_text(&item).is_some() => {
            set_clipboard(cx, None);
            Saved::Content(None)
        }
        other => other,
    };

    let _fine_ticks = FineTimerTicks::new();
    // The count as this check left the clipboard. 0: the count can't be read here (no clipboard
    // access for this window station), and the clipboard is read at every look instead.
    let mut seen = unsafe { GetClipboardSequenceNumber() };
    let sending = Instant::now();
    send_copy();
    let sent = Instant::now();

    loop {
        cx.background_executor().timer(POLL_INTERVAL).await;
        let count = unsafe { GetClipboardSequenceNumber() };
        if count == 0 || count != seen {
            let current = read_clipboard(cx);
            if let Some(text) = current.as_ref().and_then(item_text) {
                let answered = sent.elapsed();
                let restoring = Instant::now();
                saved.restore(cx);
                // The two ends of the copy that are the app's own: sending the combo (its key
                // releases' gap included) and giving the player's clipboard back.
                log::info!(
                    "item text from the game in {} ms (combo sent in {:.1} ms, clipboard given \
                     back in {:.1} ms)",
                    answered.as_millis(),
                    (sent - sending).as_secs_f64() * 1000.,
                    restoring.elapsed().as_secs_f64() * 1000.
                );
                return Some(text.to_owned());
            }
            // Something else, readable: the next change is the one to read.
            if current.is_some() {
                seen = count;
            }
        }
        if sent.elapsed() >= ANSWER_TIMEOUT {
            // Private content the game didn't replace is still there as it was.
            let current = read_clipboard(cx);
            if let Saved::Content(saved) = saved
                && current.is_some()
                && current != saved
            {
                set_clipboard(cx, saved);
            }
            return None;
        }
    }
}

/// Windows' timer at 1 ms ticks while this lives (`timeBeginPeriod`): a 5 ms timer otherwise
/// fires on the default 15.6 ms tick. Since Windows 10 2004 the finer ticks are this process's
/// only, not the whole system's.
struct FineTimerTicks;

impl FineTimerTicks {
    fn new() -> Self {
        unsafe { timeBeginPeriod(1) };
        FineTimerTicks
    }
}

impl Drop for FineTimerTicks {
    fn drop(&mut self) {
        unsafe { timeEndPeriod(1) };
    }
}

/// Puts `text` on the clipboard ([`write_private_text`]), runs `paste` (expected to synthesize
/// the keys that make the game paste it, e.g. via `synth_input::press_keys`), and after
/// [`RESTORE_AFTER`] puts back what the clipboard held -- EE2's `HostClipboard.restoreShortly`
/// with `restoreClipboard` on. Only if the clipboard still holds `text`: whatever the player
/// copied in the meantime stays. Content marked private isn't put back; the clipboard is emptied.
///
/// Nothing is pasted when `text` can't go on the clipboard: the keys would paste what is there
/// instead -- the player's own copy, a password even -- and send it to the chat.
pub async fn paste_restoring(cx: &mut AsyncApp, text: &str, paste: impl FnOnce()) {
    let saved = Saved::capture(cx);
    if let Err(err) = write_private_text(text) {
        log::warn!("putting a quick action's text on the clipboard failed, nothing typed: {err}");
        return;
    }
    paste();
    cx.background_executor().timer(RESTORE_AFTER).await;
    if read_clipboard(cx).and_then(|item| item.text()).as_deref() == Some(text) {
        saved.restore(cx);
    }
}
