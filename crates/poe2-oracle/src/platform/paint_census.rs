//! What the UI thread is woken for, counted -- off unless the app starts with
//! `POE2_ORACLE_PAINT_CENSUS=1` in its environment, and then logged every ten seconds: a live look
//! at what the app costs while it idles over the game.
//!
//! Measured 2026-09-26 on the test machine, the game behind another window and the cursor over
//! it, the UI thread was woken 195 times a second -- 12 on 2026-09-24. The thread's counters say
//! how often, not what for. A report says, per second over the last ten, first what
//! `gpui_windows`' vsync thread did (`vsync_park`): how many of the display's refreshes it waited
//! for, how many times it slept instead -- how many of those a wake cut short -- and how much of
//! the time it slept; nothing but zeros means the park isn't in. Then for each window of the UI
//! thread -- its handle, title and class, whether it's shown, its size:
//!
//! - `asked`: the display refreshes' asks for a paint (the vsync thread, through `redraw_filter`)
//!   and what the filter made of them: `passed` on to user32, which wakes the thread for a
//!   `WM_PAINT`, or dropped as `hidden` (or minimized) or `gated` (a gated window's paint not
//!   due). Windows shown and no asks at all would mean the vsync thread goes past the filter:
//!   asleep, it still asks once a second at the least;
//! - `paints`: the `WM_PAINT`s a gated window (`win32::Win32Overlay::gate_paints`) let through
//!   `to GPUI`, and those it `swallowed`;
//! - `gate`: what else reached a gated window's gate -- the pointer, the keyboard, moves, sizes,
//!   shows -- and `app` for the app's own word that it changed (`Win32Overlay::open_paints`);
//! - `taken`: what the thread took off its queue for the window -- posted messages, input,
//!   `WM_PAINT`, `WM_TIMER` -- under «the thread» for those posted to no window;
//! - `sent`: what other threads and programs sent the window, each a wake too.
//!
//! The report reads the windows without sending them a message, from a thread of its own: the
//! census wakes the UI thread for nothing of its own, its hooks only add to the wakes it counts.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::ffi::c_void;
use std::fmt::Write as _;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CWPSTRUCT, CallNextHookEx, GetClassNameW, GetWindowRect, HC_ACTION, InternalGetWindowText,
    IsWindowVisible, MSG, PM_REMOVE, SetWindowsHookExW, WH_CALLWNDPROC, WH_GETMESSAGE, WM_ACTIVATE,
    WM_APP, WM_CHAR, WM_DPICHANGED, WM_GETOBJECT, WM_HOTKEY, WM_INPUT, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCHITTEST, WM_PAINT,
    WM_SETCURSOR, WM_SHOWWINDOW, WM_SIZE, WM_TIMER, WM_USER, WM_WINDOWPOSCHANGED,
    WM_WINDOWPOSCHANGING,
};

/// How often the census reports.
const REPORT_EVERY: Duration = Duration::from_secs(10);

/// What [`opened`] is given for the app's own word that a window's content changed: no message
/// is 0.
pub const BY_APP: u32 = 0;

/// Whether the census counts: the environment's word, read once.
static ON: LazyLock<bool> = LazyLock::new(|| {
    std::env::var_os("POE2_ORACLE_PAINT_CENSUS").is_some_and(|value| value == "1")
});

/// The counts since the last report, by window handle value (0 for none) and what was counted.
static COUNTS: LazyLock<Mutex<HashMap<(isize, Tally), u64>>> = LazyLock::new(Default::default);

/// What the vsync thread did since the last report.
static VSYNC: Mutex<Vsync> = Mutex::new(Vsync::NONE);

/// What the vsync thread did (`vsync_park`).
#[derive(Debug, Clone, Copy)]
struct Vsync {
    /// The display's refreshes it waited for.
    refreshes: u64,
    /// How many times it slept instead, and how many of those a wake cut short.
    parks: u64,
    woken: u64,
    /// How long it slept in all.
    parked: Duration,
}

impl Vsync {
    const NONE: Vsync = Vsync {
        refreshes: 0,
        parks: 0,
        woken: 0,
        parked: Duration::ZERO,
    };
}

/// What a display refresh's ask for a paint came to in `redraw_filter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ask {
    /// Handed on to user32.
    Passed,
    /// Dropped: the window is hidden or minimized.
    Hidden,
    /// Dropped: the window's paints are gated, and its next isn't due.
    Gated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Tally {
    Asked(Ask),
    /// A gated window's `WM_PAINT`: through to GPUI, or swallowed.
    Painted(bool),
    /// Anything else a gated window's gate took, by message; [`BY_APP`] for the app's word.
    Gate(u32),
    /// A message the thread took off its queue.
    Taken(u32),
    /// A message another thread sent.
    Sent(u32),
}

/// Counts a display refresh's ask for a paint of `hwnd`.
pub fn asked(hwnd: HWND, ask: Ask) {
    if *ON {
        count(hwnd.0 as isize, Tally::Asked(ask));
    }
}

/// Counts a gated window's `WM_PAINT`, `passed` to GPUI or swallowed.
pub fn painted(hwnd: HWND, passed: bool) {
    if *ON {
        count(hwnd.0 as isize, Tally::Painted(passed));
    }
}

/// Counts `message` taken by a gated window's gate, or [`BY_APP`] for the app's word.
pub fn opened(hwnd: HWND, message: u32) {
    if *ON {
        count(hwnd.0 as isize, Tally::Gate(message));
    }
}

/// Counts a refresh of the display the vsync thread waited for.
pub fn refreshed() {
    if *ON {
        vsync().refreshes += 1;
    }
}

/// Counts a sleep of the vsync thread that lasted `slept`, cut short by a wake or not.
pub fn parked(slept: Duration, woken: bool) {
    if *ON {
        let mut vsync = vsync();
        vsync.parks += 1;
        vsync.woken += u64::from(woken);
        vsync.parked += slept;
    }
}

fn vsync() -> MutexGuard<'static, Vsync> {
    VSYNC
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn count(hwnd: isize, tally: Tally) {
    *COUNTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entry((hwnd, tally))
        .or_default() += 1;
}

/// Starts the census, if the environment asks for it, on this thread: the UI thread, before
/// GPUI runs on it. Its hooks last as long as the thread.
pub fn start() {
    if !*ON {
        return;
    }
    let thread = unsafe { GetCurrentThreadId() };
    let hooked = unsafe { SetWindowsHookExW(WH_GETMESSAGE, Some(on_taken), None, thread) }
        .and_then(|_| unsafe { SetWindowsHookExW(WH_CALLWNDPROC, Some(on_sent), None, thread) });
    if let Err(err) = hooked {
        log::warn!("paint census: hooking the UI thread failed, it counts paints alone: {err}");
    }
    let reporter = thread::Builder::new()
        .name("paint-census".into())
        .spawn(report_forever);
    match reporter {
        Ok(_) => log::info!("paint census: on, a report every {REPORT_EVERY:?}"),
        Err(err) => log::warn!("paint census: its reporter didn't start: {err}"),
    }
}

/// `WH_GETMESSAGE`: a message the thread takes off its queue.
unsafe extern "system" fn on_taken(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam.0 & PM_REMOVE.0 as usize != 0 {
        // SAFETY: a `WH_GETMESSAGE` hook's `lparam` points at the `MSG` taken.
        let taken = unsafe { &*(lparam.0 as *const MSG) };
        count(taken.hwnd.0 as isize, Tally::Taken(taken.message));
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// `WH_CALLWNDPROC`: a message sent to a window of the thread, counted when another thread sent
/// it -- then `wparam` is 0.
unsafe extern "system" fn on_sent(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam.0 == 0 {
        // SAFETY: a `WH_CALLWNDPROC` hook's `lparam` points at the `CWPSTRUCT` sent.
        let sent = unsafe { &*(lparam.0 as *const CWPSTRUCT) };
        count(sent.hwnd.0 as isize, Tally::Sent(sent.message));
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn report_forever() {
    let mut since = Instant::now();
    loop {
        thread::sleep(REPORT_EVERY);
        let counts = std::mem::take(&mut *COUNTS.lock().unwrap_or_else(|p| p.into_inner()));
        let vsync = std::mem::replace(&mut *vsync(), Vsync::NONE);
        let seconds = since.elapsed().as_secs_f64();
        since = Instant::now();
        log::info!("{}", report(counts, vsync, seconds));
    }
}

/// The report of `vsync` and `counts` over `seconds`: a line per window, the busiest first.
fn report(counts: HashMap<(isize, Tally), u64>, vsync: Vsync, seconds: f64) -> String {
    let mut windows: HashMap<isize, Vec<(Tally, u64)>> = HashMap::new();
    for ((hwnd, tally), count) in counts {
        windows.entry(hwnd).or_default().push((tally, count));
    }
    let mut windows: Vec<_> = windows.into_iter().collect();
    windows.sort_by_key(|(_, tallies)| Reverse(tallies.iter().map(|&(_, n)| n).sum::<u64>()));
    let per_second = |count: u64| count as f64 / seconds;
    let mut out = format!(
        "paint census, per second over {seconds:.1} s; the vsync thread: refreshes {:.1}, \
         sleeps {:.1} ({:.1} cut short), asleep {:.1} % of the time:",
        per_second(vsync.refreshes),
        per_second(vsync.parks),
        per_second(vsync.woken),
        vsync.parked.as_secs_f64() / seconds * 100.0,
    );
    if windows.is_empty() {
        out += " nothing";
    }
    for (hwnd, mut tallies) in windows {
        tallies.sort_by_key(|&(tally, count)| (label(tally).0, Reverse(count)));
        let _ = write!(out, "\n  {}", describe(hwnd));
        let mut group = "";
        for (tally, count) in tallies {
            let (_, of, what) = label(tally);
            if of == group {
                out += ", ";
            } else {
                out += if group.is_empty() { ": " } else { "; " };
                out += of;
                out += " ";
                group = of;
            }
            let _ = write!(out, "{what} {:.1}", per_second(count));
        }
    }
    out
}

/// Where a count goes in its window's line: its group's place and name, and what it counts.
fn label(tally: Tally) -> (u8, &'static str, String) {
    match tally {
        Tally::Asked(Ask::Passed) => (0, "asked", "passed".into()),
        Tally::Asked(Ask::Hidden) => (0, "asked", "hidden".into()),
        Tally::Asked(Ask::Gated) => (0, "asked", "gated".into()),
        Tally::Painted(true) => (1, "paints", "to GPUI".into()),
        Tally::Painted(false) => (1, "paints", "swallowed".into()),
        Tally::Gate(message) => (2, "gate", message_name(message)),
        Tally::Taken(message) => (3, "taken", message_name(message)),
        Tally::Sent(message) => (4, "sent", message_name(message)),
    }
}

fn message_name(message: u32) -> String {
    let name = match message {
        BY_APP => "app",
        WM_PAINT => "WM_PAINT",
        WM_TIMER => "WM_TIMER",
        WM_MOUSEMOVE => "WM_MOUSEMOVE",
        // `WM_MOUSELEAVE`, which the `windows` crate files under `Win32_UI_Controls`.
        0x02A3 => "WM_MOUSELEAVE",
        WM_MOUSEWHEEL => "WM_MOUSEWHEEL",
        WM_LBUTTONDOWN => "WM_LBUTTONDOWN",
        WM_LBUTTONUP => "WM_LBUTTONUP",
        WM_KEYDOWN => "WM_KEYDOWN",
        WM_KEYUP => "WM_KEYUP",
        WM_CHAR => "WM_CHAR",
        WM_HOTKEY => "WM_HOTKEY",
        WM_INPUT => "WM_INPUT",
        WM_ACTIVATE => "WM_ACTIVATE",
        WM_SIZE => "WM_SIZE",
        WM_SHOWWINDOW => "WM_SHOWWINDOW",
        WM_WINDOWPOSCHANGING => "WM_WINDOWPOSCHANGING",
        WM_WINDOWPOSCHANGED => "WM_WINDOWPOSCHANGED",
        WM_DPICHANGED => "WM_DPICHANGED",
        WM_NCHITTEST => "WM_NCHITTEST",
        WM_SETCURSOR => "WM_SETCURSOR",
        WM_GETOBJECT => "WM_GETOBJECT",
        // `gpui_windows`' own (`events.rs`): a foreground task's wake, a key through its
        // accelerator.
        _ if message == WM_USER + 3 => "WM_USER+3 (GPUI task)",
        _ if message == WM_USER + 8 => "WM_USER+8 (GPUI key)",
        _ if (WM_USER..WM_APP).contains(&message) => {
            return format!("WM_USER+{}", message - WM_USER);
        }
        _ if (WM_APP..0xC000).contains(&message) => return format!("WM_APP+{}", message - WM_APP),
        _ => return format!("{message:#06x}"),
    };
    name.to_owned()
}

/// `hwnd` as a report names it -- read without sending it a message, which would wake the UI
/// thread.
fn describe(hwnd: isize) -> String {
    if hwnd == 0 {
        return "the thread".to_owned();
    }
    let window = HWND(hwnd as *mut c_void);
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(window, &mut rect) }.is_err() {
        return format!("{hwnd:#x} (gone)");
    }
    let mut title = [0u16; 80];
    let title_len = unsafe { InternalGetWindowText(window, &mut title) }.max(0) as usize;
    let mut class = [0u16; 80];
    let class_len = unsafe { GetClassNameW(window, &mut class) }.max(0) as usize;
    let shown = if unsafe { IsWindowVisible(window) }.as_bool() {
        "shown"
    } else {
        "hidden"
    };
    format!(
        "{hwnd:#x} «{}» [{}] {shown} {}x{}",
        String::from_utf16_lossy(&title[..title_len]),
        String::from_utf16_lossy(&class[..class_len]),
        rect.right - rect.left,
        rect.bottom - rect.top,
    )
}
