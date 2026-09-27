//! Esc closes the price-check panel, and is taken away from the game only while the panel is open.
//!
//! A low-level keyboard hook (`WH_KEYBOARD_LL`), not `RegisterHotKey`: a registered hotkey only
//! suppresses the `WM_KEYDOWN` message, while a game reading raw input would still see Esc and
//! close its own inventory panel along with ours. Returning non-zero from a low-level hook drops
//! the keystroke before any application sees it.
//!
//! The same hook tells `quick_action::KEY_PRESSES` of every key it sees go down or up, so that one
//! press of a quick action's key types once. One hook, not a second: each sits in the path of every
//! keystroke in the system.
//!
//! So the hook is in only while it's needed: while the panel is shown ([`set_armed`]), until the
//! Esc that closed it is let go -- its auto-repeats and key-up are taken too, so the game never
//! sees the tail of the press -- and while a quick action's hotkey is held ([`count_presses`]):
//! the game in front, with quick actions that have keys. Otherwise no keystroke of the session
//! goes through this app. Always in, the hook woke its thread for every key the player pressed:
//! measured 2026-09-27 on the test machine in real play, with the panel closed and no quick
//! actions, 25 to 36 times a second for 10 to 16 ms of CPU a minute, for keys only the game had
//! any use for. Putting it in takes a thread message to the hook's thread and a
//! `SetWindowsHookExW` there (the log says how long after the ask it was in), sent as the panel is
//! shown -- before its first frame is drawn, let alone the player's first Esc at it.
//!
//! The hook runs on its own thread with its own message loop: the system calls a low-level hook
//! from the installing thread's message wait and silently skips hooks that take too long, so it
//! must not depend on how busy GPUI's main thread is. The thread waits in `GetMessageW` with the
//! hook in or out, woken only by what the hook is called for and by the thread messages that put
//! it in and take it out. Its shared state is atomics -- its own and `KEY_PRESSES`' -- and the
//! channel its presses go out on.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_SYSKEYDOWN, WM_USER,
};
use windows::core::PCWSTR;

use crate::quick_action::KEY_PRESSES;

/// The panel is open: Esc presses are consumed and reported.
static ARMED: AtomicBool = AtomicBool::new(false);
/// A quick action's hotkey is held: the hook counts the keyboard's presses for it.
static COUNTING: AtomicBool = AtomicBool::new(false);
/// A consumed Esc key is still held: its auto-repeats and key-up are consumed too, even after the
/// panel has closed, so the game never sees the tail of a press that closed the panel.
static SWALLOWING: AtomicBool = AtomicBool::new(false);
/// The hook thread's id once its message queue is there, where [`SYNC`] goes; 0 before.
static THREAD: AtomicU32 = AtomicU32::new(0);
/// Each consumed Esc press; the receiving end is [`install`]'s.
static PRESSES: LazyLock<(async_channel::Sender<()>, async_channel::Receiver<()>)> =
    LazyLock::new(async_channel::unbounded);
/// The clock [`SYNC`] is stamped with, microseconds from its first reading: a message's own time
/// counts in the system timer's ticks, 15.6 ms apart, too coarse for how soon the hook is in.
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// The thread message that has the hook thread put the hook in, or take it out, as it's needed;
/// its `lParam` is when it was posted ([`stamp`]).
const SYNC: u32 = WM_APP;

/// Starts the hook's thread and waits until it takes orders. Call once per process. Every Esc
/// press consumed while armed arrives on the returned channel.
pub fn install() -> Result<async_channel::Receiver<()>> {
    let presses = PRESSES.1.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("esc-hook".into())
        .spawn(move || {
            // A thread's message queue is made by its first ask for a message: made before its id
            // goes out, so no order posted to it is lost. Whatever was ordered before is seen by
            // the first sync, which comes after the id is out.
            let mut msg = MSG::default();
            let _ = unsafe { PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE) };
            THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
            let _ = ready_tx.send(());
            let mut hook = Hook::default();
            let mut asked = None;
            loop {
                hook.sync(asked);
                // Low-level hook callbacks are delivered inside this wait; no windows live on
                // this thread, so there is nothing to dispatch.
                if !unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                    return;
                }
                asked = (msg.message == SYNC).then_some(msg.lParam.0);
            }
        })
        .context("spawning the esc-hook thread")?;
    ready_rx
        .recv()
        .map_err(|_| anyhow!("esc-hook thread exited before it took orders"))?;
    Ok(presses)
}

/// Whether Esc is currently intercepted -- mirror the panel's visibility here. The hook goes in
/// with the panel, and out once it's hidden and the Esc that closed it let go.
pub fn set_armed(armed: bool) {
    if ARMED.swap(armed, Ordering::SeqCst) != armed {
        sync();
    }
}

/// Whether a quick action's hotkey is held: the hook stays in to count the keyboard's presses for
/// it (`quick_action::KEY_PRESSES`).
pub fn count_presses(counting: bool) {
    if COUNTING.swap(counting, Ordering::SeqCst) != counting {
        sync();
    }
}

/// Has the hook's thread put the hook in or take it out, as it's needed now.
fn sync() {
    let thread = THREAD.load(Ordering::SeqCst);
    if thread != 0 {
        let _ = unsafe { PostThreadMessageW(thread, SYNC, WPARAM(0), LPARAM(stamp())) };
    }
}

/// Now on [`EPOCH`]'s clock.
fn stamp() -> isize {
    EPOCH.elapsed().as_micros() as isize
}

/// The hook, on its thread.
#[derive(Default)]
struct Hook {
    hook: Option<HHOOK>,
    /// It went in once: later times are logged only at debug.
    announced: bool,
    /// The last try failed: said once until one works.
    failed: bool,
}

impl Hook {
    /// Puts the hook in, or takes it out, as it's needed now -- asked by a [`SYNC`] posted at
    /// `asked` ([`stamp`]), if one asked.
    fn sync(&mut self, asked: Option<isize>) {
        let needed = ARMED.load(Ordering::SeqCst)
            || COUNTING.load(Ordering::SeqCst)
            || SWALLOWING.load(Ordering::Relaxed);
        match (needed, self.hook) {
            (true, None) => self.put_in(asked),
            (false, Some(hook)) => {
                self.hook = None;
                if let Err(err) = unsafe { UnhookWindowsHookEx(hook) } {
                    log::warn!("esc hook: UnhookWindowsHookEx failed: {err}");
                }
                log::debug!("esc hook: out");
            }
            _ => {}
        }
    }

    fn put_in(&mut self, asked: Option<isize>) {
        // Out, it saw no key let go: none is taken for held from before.
        KEY_PRESSES.forget_held();
        let started = Instant::now();
        let hooked = unsafe { GetModuleHandleW(PCWSTR::null()) }.and_then(|module| unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(module.into()), 0)
        });
        match hooked {
            Ok(hook) => {
                self.hook = Some(hook);
                self.failed = false;
                let took = started.elapsed();
                // The first time a sign in the diagnostics report that it works; later ones only
                // at debug.
                let level = if self.announced {
                    log::Level::Debug
                } else {
                    log::Level::Info
                };
                self.announced = true;
                match asked {
                    Some(asked) => {
                        let since = Duration::from_micros((stamp() - asked).max(0) as u64);
                        log::log!(
                            level,
                            "esc hook: in {since:.1?} after the ask (SetWindowsHookExW {took:.1?})"
                        );
                    }
                    None => log::log!(level, "esc hook: in (SetWindowsHookExW {took:.1?})"),
                }
            }
            Err(err) if !self.failed => {
                log::warn!("esc hook: SetWindowsHookExW(WH_KEYBOARD_LL) failed: {err}");
                self.failed = true;
            }
            Err(_) => {}
        }
    }
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
                if ARMED.load(Ordering::SeqCst) {
                    SWALLOWING.store(true, Ordering::Relaxed);
                    let _ = PRESSES.0.try_send(());
                    return LRESULT(1);
                }
            } else if SWALLOWING.swap(false, Ordering::Relaxed) {
                // The press that closed the panel is over: the hook may go out with it.
                sync();
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
