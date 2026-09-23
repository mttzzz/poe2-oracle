//! Esc closes the price-check panel, and is taken away from the game only while the panel is open.
//!
//! A low-level keyboard hook (`WH_KEYBOARD_LL`), not `RegisterHotKey`: a registered hotkey only
//! suppresses the `WM_KEYDOWN` message, while a game reading raw input would still see Esc and
//! close its own inventory panel along with ours. Returning non-zero from a low-level hook drops
//! the keystroke before any application sees it. While the panel is closed the hook passes every
//! key straight through (`CallNextHookEx`), so Esc behaves normally in the game.
//!
//! The same hook tells `quick_action::KEY_PRESSES` of every key it sees go down or up, so that one
//! press of a quick action's key types once. One hook, not a second: each sits in the path of every
//! keystroke in the system.
//!
//! The hook runs on its own thread with its own message loop: the system calls a low-level hook
//! from the installing thread's message wait and silently skips hooks that take too long, so it
//! must not depend on how busy GPUI's main thread is. Its only shared state is atomics -- its own
//! two and `KEY_PRESSES`' -- and the channel its presses go out on.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use anyhow::{Context as _, Result, anyhow};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    SetWindowsHookExW, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
};
use windows::core::PCWSTR;

use crate::quick_action::KEY_PRESSES;

/// The panel is open: Esc presses are consumed and reported.
static ARMED: AtomicBool = AtomicBool::new(false);
/// A consumed Esc key is still held: its auto-repeats and key-up are consumed too, even after the
/// panel has closed, so the game never sees the tail of a press that closed the panel.
static SWALLOWING: AtomicBool = AtomicBool::new(false);
/// Each consumed Esc press; the receiving end is [`install`]'s.
static PRESSES: LazyLock<(async_channel::Sender<()>, async_channel::Receiver<()>)> =
    LazyLock::new(async_channel::unbounded);

/// Starts the hook thread and waits until the hook is installed. Call once per process. Every
/// Esc press consumed while armed arrives on the returned channel.
pub fn install() -> Result<async_channel::Receiver<()>> {
    let presses = PRESSES.1.clone();
    let (installed_tx, installed_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("esc-hook".into())
        .spawn(move || {
            let installed = unsafe { GetModuleHandleW(PCWSTR::null()) }.and_then(|module| unsafe {
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(module.into()), 0)
            });
            let ok = installed.is_ok();
            let _ = installed_tx.send(installed.map(|_| ()));
            if !ok {
                return;
            }
            // Low-level hook callbacks are delivered inside this wait; no windows live on this
            // thread, so there is nothing to dispatch.
            let mut msg = MSG::default();
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {}
        })
        .context("spawning the esc-hook thread")?;
    installed_rx
        .recv()
        .map_err(|_| anyhow!("esc-hook thread exited before reporting"))?
        .context("SetWindowsHookExW(WH_KEYBOARD_LL) failed")?;
    Ok(presses)
}

/// Whether Esc is currently intercepted -- mirror the panel's visibility here.
pub fn set_armed(armed: bool) {
    ARMED.store(armed, Ordering::Relaxed);
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for `HC_ACTION`, `lparam` points to the event's `KBDLLHOOKSTRUCT`.
        let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let message = wparam.0 as u32;
        let is_down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        if let Ok(vk) = u16::try_from(event.vkCode) {
            KEY_PRESSES.record(vk, is_down, event.flags.contains(LLKHF_INJECTED));
        }
        if event.vkCode == u32::from(VK_ESCAPE.0) {
            if is_down {
                if SWALLOWING.load(Ordering::Relaxed) {
                    return LRESULT(1);
                }
                if ARMED.load(Ordering::Relaxed) {
                    SWALLOWING.store(true, Ordering::Relaxed);
                    let _ = PRESSES.0.try_send(());
                    return LRESULT(1);
                }
            } else if SWALLOWING.swap(false, Ordering::Relaxed) {
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
