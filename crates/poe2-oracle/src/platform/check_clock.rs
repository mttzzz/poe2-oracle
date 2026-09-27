//! The clocks a price check's steps are timed on, and the check under way, which `check_profile`
//! adds up and words for the log. Every step it times runs on the UI thread -- the hotkey's task,
//! the search's, and the panel's window procedure, inside which GPUI draws the panel's frames --
//! so the check lives in a thread-local there, which the window procedure reaches without GPUI.
//!
//! The CPU is counted in cycles of the time-stamp counter: what `QueryThreadCycleTime` and
//! `QueryProcessCycleTime` count on every processor with an invariant counter -- all that run
//! Windows 10 and 11 -- and what the live measurements divide by the counter's rate too. The rate
//! is measured here against the wall clock, from the app's start ([`start`]) on.

use std::cell::{Cell, RefCell};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LRESULT};
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentThread};
use windows::Win32::System::WindowsProgramming::{QueryProcessCycleTime, QueryThreadCycleTime};
use windows::Win32::UI::WindowsAndMessaging::{WM_PAINT, WM_SHOWWINDOW, WM_SIZE};

use crate::check_profile::{CheckProfile, CycleRate, PanelMessage, Reading, Spent};
use crate::platform::gpu_memory;

/// The wall clock's origin and the time-stamp counter then, which its rate is measured from.
static ORIGIN: LazyLock<(Instant, u64)> = LazyLock::new(|| (Instant::now(), time_stamp()));

thread_local! {
    /// The check under way.
    static CHECK: RefCell<Option<CheckProfile>> = const { RefCell::new(None) };
    /// The checks so far.
    static CHECKS: Cell<u64> = const { Cell::new(0) };
    /// The price panel's window handle; 0 until it's known.
    static PANEL: Cell<isize> = const { Cell::new(0) };
    /// The panel's frames so far: its root view renders once a frame ([`rendered`]).
    static RENDERS: Cell<u64> = const { Cell::new(0) };
}

/// Sets the clocks' origin: call as the app starts, so the counter's rate is known by the first
/// check.
pub fn start() {
    LazyLock::force(&ORIGIN);
}

/// The time-stamp counter.
#[cfg(target_arch = "x86_64")]
#[allow(unused_unsafe)]
fn time_stamp() -> u64 {
    // SAFETY: reads the counter, which every x86-64 processor has.
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// No counter to read: the line gives no CPU times ([`CycleRate::between`] says no rate).
#[cfg(not(target_arch = "x86_64"))]
fn time_stamp() -> u64 {
    0
}

/// The counter's rate, measured from the clocks' origin.
fn rate() -> Option<CycleRate> {
    let (origin, origin_stamp) = *ORIGIN;
    CycleRate::between(
        (Duration::ZERO, origin_stamp),
        (origin.elapsed(), time_stamp()),
    )
}

fn thread_cycles() -> u64 {
    let mut cycles = 0;
    // SAFETY: the pseudo handle of this thread, and a place for the count.
    let _ = unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut cycles) };
    cycles
}

/// The process's cycles; `None` if Windows can't say.
fn process_cycles() -> Option<u64> {
    let mut cycles = 0;
    // SAFETY: the pseudo handle of this process, and a place for the count.
    unsafe { QueryProcessCycleTime(GetCurrentProcess(), &mut cycles) }.ok()?;
    Some(cycles)
}

/// The clocks now, the whole process's cycles too.
fn now() -> Reading {
    Reading {
        wall: ORIGIN.0.elapsed(),
        thread: thread_cycles(),
        process: process_cycles(),
    }
}

/// The wall clock and this thread's cycles now: what each step reads.
pub fn now_thread() -> Reading {
    Reading {
        wall: ORIGIN.0.elapsed(),
        thread: thread_cycles(),
        process: None,
    }
}

/// A price check's key was pressed: a check starts, and one still under way ends first.
pub fn begin() {
    finish();
    let number = CHECKS.get() + 1;
    CHECKS.set(number);
    CHECK.set(Some(CheckProfile::new(number, now())));
}

/// Has the check under way, if any, take `step`.
pub fn with(step: impl FnOnce(&mut CheckProfile)) {
    CHECK.with_borrow_mut(|check| {
        if let Some(check) = check {
            step(check);
        }
    });
}

/// Ends the check under way: its line goes to the log, if the game's text came in.
pub fn finish() {
    let Some(check) = CHECK.take() else {
        return;
    };
    if check.copied_text() {
        log::info!("{}", check.summary(now(), rate(), gpu_memory::dedicated()));
    }
}

/// The price panel's window: its messages are timed while a check is under way ([`timed`]).
pub fn watch_panel(hwnd: HWND) {
    PANEL.set(hwnd.0 as isize);
}

/// The panel's root view rendered: GPUI is drawing a frame of it.
pub fn rendered() {
    RENDERS.set(RENDERS.get() + 1);
}

/// Whether `message` to `hwnd` is one of the panel's that a check under way times.
pub fn timed(hwnd: HWND, message: u32) -> Option<PanelMessage> {
    let panel = PANEL.get();
    if panel == 0 || hwnd.0 as isize != panel {
        return None;
    }
    let message = match message {
        WM_PAINT => PanelMessage::Paint,
        WM_SIZE => PanelMessage::Size,
        WM_SHOWWINDOW => PanelMessage::Show,
        _ => return None,
    };
    CHECK
        .with_borrow(|check| check.is_some())
        .then_some(message)
}

/// Times `answer`, GPUI's answer to the panel's `message`, for the check under way -- and ends the
/// check if it has settled since.
pub fn answer_panel(message: PanelMessage, answer: impl FnOnce() -> LRESULT) -> LRESULT {
    let renders = RENDERS.get();
    let start = now_thread();
    let answered = answer();
    let end = now_thread();
    let drew = RENDERS.get() != renders;
    let mut settled = false;
    with(|check| {
        check.panel_message(message, Spent::between(start, end), drew, end);
        settled = check.settled(end.wall);
    });
    if settled {
        finish();
    }
    answered
}
