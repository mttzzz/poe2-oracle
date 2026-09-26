//! Lets `gpui_windows`' vsync thread sleep while no window of the app wants a paint.
//!
//! `gpui_windows` paints on the display's refreshes: its vsync thread (`platform.rs`'s
//! `begin_vsync_thread`) waits for each in `DwmFlush`, then asks every window of the app for a
//! paint (`RedrawWindow`, which `redraw_filter` turns down for the quiet ones). With the XP
//! overlay's plates up the whole time the game is played, quiet and their paints gated, the thread
//! still woke at every refresh for asks the filter turned down: measured 2026-09-26 on the test
//! machine, 63 times a second for 58 ms of CPU a minute, and a thread of the NVIDIA driver serving
//! GPUI's Direct3D device 69 times a second for 17 ms more.
//!
//! So this executable's import of `DwmFlush` is pointed at [`parked_dwm_flush`] too. On the vsync
//! thread it waits for the refresh as before while a shown window wants a paint at each -- one
//! whose paints aren't gated, or a gated one in a burst or with the keyboard -- or a safety net's
//! paint no refresh has asked for yet; otherwise it sleeps until the first safety net's paint
//! comes due, five seconds apart for the quiet plates, and with no window shown five seconds at
//! the most (`paint_gate::vsync`); the refresh's asks go out as it wakes. Whatever makes a window
//! want paints wakes it at once: a gate that opens ([`wake`], from `win32`'s gates), and a window
//! of the UI thread shown or restored (a WinEvent hook on that thread). GPUI checks for a lost
//! Direct3D device before each refresh's asks, so a device lost meanwhile is still found before
//! anything is drawn.
//!
//! [`install`] runs on the UI thread before GPUI starts, as `redraw_filter::install` does and for
//! the same reason -- the vsync thread may read the import slot once, before its loop -- and needs
//! that filter in: the asks it notes tell the park which windows GPUI has.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use anyhow::{Result, bail, ensure};
use windows::Win32::Foundation::{HANDLE, HWND, S_OK, WAIT_OBJECT_0};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentThreadId, SetEvent, WaitForSingleObject,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_SHOW, EVENT_SYSTEM_MINIMIZEEND, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT,
};
use windows::core::{HRESULT, PCWSTR};

use crate::platform::paint_gate::{self, Vsync, Wants};
use crate::platform::{paint_census, redraw_filter};

/// The name `gpui_windows` gives its vsync thread (`platform.rs`'s `begin_vsync_thread`): the
/// thread that sleeps.
const VSYNC_THREAD: &str = "VSyncProvider";

type DwmFlushFn = unsafe extern "system" fn() -> HRESULT;

/// dwmapi's `DwmFlush`: the import slot's value before [`parked_dwm_flush`] took it.
static DWM_FLUSH: AtomicUsize = AtomicUsize::new(0);

/// The auto-reset event a sleeping vsync thread waits on, which [`wake`] sets: its handle's
/// value, if it could be made. It keeps a wake that came while the thread was up for its next
/// sleep, which then ends at once: whatever changed after the refresh the sleep was decided on --
/// a window that wants paints since -- gets the next.
static WAKE_EVENT: LazyLock<Option<usize>> = LazyLock::new(|| {
    unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
        .ok()
        .map(|event| event.0 as usize)
});

/// Whether the park is in: from then on, `redraw_filter` notes each refresh's asks for it, and
/// [`wake`] sets the event.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether a wake came since the vsync thread's last sleep: the event is set, and [`wake`] needn't
/// set it again -- while the thread refreshes, a wake costs no system call.
static WOKEN: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Whether this is the vsync thread.
    static ON_VSYNC_THREAD: bool = std::thread::current().name() == Some(VSYNC_THREAD);
    /// The vsync thread's last refresh's asks for paints, as `redraw_filter` noted them.
    static ASKS: RefCell<Asks> = RefCell::default();
}

/// More asks than a refresh makes -- the app never has that many windows at once. A vsync thread
/// that stopped going through the park, a `gpui_windows` that waits for the refreshes otherwise,
/// leaves its asks untaken, and they stop being noted rather than pile up.
const MAX_ASKS: usize = 64;

/// A refresh's asks for paints.
#[derive(Default)]
struct Asks {
    /// Whether they're noted: on the vsync thread once it went through the park, which takes them
    /// at the next refresh.
    noted: bool,
    /// When they went out (`GetTickCount64`).
    at: u64,
    /// What each window asked wanted of the refreshes then, `None` if hidden or minimized.
    windows: Vec<Option<Wants>>,
}

/// Whether the park is in: tried once, on the first call of [`install`].
static INSTALLED: LazyLock<bool> = LazyLock::new(|| match set_up() {
    Ok(slots) => {
        log::info!("vsync park: {slots} DwmFlush import(s) parked");
        true
    }
    Err(err) => {
        log::warn!("vsync park: {err:#}; GPUI's vsync thread wakes at every refresh");
        false
    }
});

/// Lets the vsync thread sleep while no window wants a paint, from now on; whether it could. Call
/// on the UI thread before GPUI starts (see the module's doc).
pub fn install() -> bool {
    *INSTALLED
}

/// Wakes the vsync thread if it sleeps, or ends its next sleep at once: a window may want paints
/// that it didn't see coming.
pub fn wake() {
    if ACTIVE.load(Ordering::Acquire)
        && !WOKEN.swap(true, Ordering::AcqRel)
        && let Some(event) = *WAKE_EVENT
    {
        let _ = unsafe { SetEvent(HANDLE(event as *mut c_void)) };
    }
}

/// Notes the vsync thread's ask at `now` for a paint of a window, which `wants` that of the
/// refreshes -- `None` hidden or minimized: the next refresh or sleep is decided on the asks.
pub(super) fn asked(wants: Option<Wants>, now: u64) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    ASKS.with_borrow_mut(|asks| {
        if !asks.noted {
            return;
        }
        if asks.windows.len() == MAX_ASKS {
            asks.noted = false;
            asks.windows.clear();
            return;
        }
        asks.at = now;
        asks.windows.push(wants);
    });
}

/// The event the vsync thread sleeps on, the hooks that wake it when a window of this thread is
/// shown or restored, and then the import: how many slots it had.
fn set_up() -> Result<usize> {
    ensure!(
        redraw_filter::install(),
        "the redraw filter isn't in, and the park goes by the asks it notes"
    );
    ensure!(
        WAKE_EVENT.is_some(),
        "CreateEventW failed: no event to sleep on"
    );
    let thread = unsafe { GetCurrentThreadId() };
    let hooks = [EVENT_OBJECT_SHOW, EVENT_SYSTEM_MINIMIZEEND].map(|shown| unsafe {
        SetWinEventHook(
            shown,
            shown,
            None,
            Some(on_shown),
            std::process::id(),
            thread,
            WINEVENT_OUTOFCONTEXT,
        )
    });
    if hooks.iter().any(HWINEVENTHOOK::is_invalid) {
        unhook(hooks);
        bail!("SetWinEventHook failed");
    }
    ACTIVE.store(true, Ordering::Release);
    let park: DwmFlushFn = parked_dwm_flush;
    redraw_filter::point_import("dwmapi.dll", "DwmFlush", park as usize, &DWM_FLUSH).inspect_err(
        |_| {
            ACTIVE.store(false, Ordering::Release);
            unhook(hooks);
        },
    )
}

fn unhook(hooks: [HWINEVENTHOOK; 2]) {
    for hook in hooks.into_iter().filter(|hook| !hook.is_invalid()) {
        let _ = unsafe { UnhookWinEvent(hook) };
    }
}

/// `EVENT_OBJECT_SHOW` and `EVENT_SYSTEM_MINIMIZEEND` of the UI thread, from its message loop: a
/// window of it was shown or restored, and may want paints. GPUI shows its own -- those it opens
/// shown -- and the app its overlays; either way, it's done by the time this runs.
unsafe extern "system" fn on_shown(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _window: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    if object == OBJID_WINDOW.0 && child == CHILDID_SELF as i32 {
        wake();
    }
}

/// Stands in for `DwmFlush` in this executable. On the vsync thread, it waits for the display's
/// next refresh only when a window wants its paint, and otherwise sleeps till one may; on any
/// other thread, it's `DwmFlush`.
unsafe extern "system" fn parked_dwm_flush() -> HRESULT {
    if !ON_VSYNC_THREAD.with(|on| *on) || !ACTIVE.load(Ordering::Acquire) {
        return unsafe { dwm_flush() };
    }
    let mut asks = ASKS.take();
    let flushed = if park(&asks) {
        S_OK
    } else {
        paint_census::refreshed();
        unsafe { dwm_flush() }
    };
    // The next refresh's asks, into the same buffer.
    asks.windows.clear();
    asks.noted = true;
    ASKS.set(asks);
    flushed
}

/// Decides on the refresh after `asks`, and sleeps unless a window wants it: whether it slept.
fn park(asks: &Asks) -> bool {
    let Some(event) = *WAKE_EVENT else {
        return false;
    };
    let now = unsafe { GetTickCount64() };
    let Vsync::Park { until } = paint_gate::vsync(asks.windows.iter().copied(), asks.at, now)
    else {
        return false;
    };
    // One and a half safety nets at the most (`paint_gate::vsync`).
    let timeout = until.saturating_sub(now) as u32;
    paint_census::parking();
    let woken =
        unsafe { WaitForSingleObject(HANDLE(event as *mut c_void), timeout) } == WAIT_OBJECT_0;
    // The wakes so far are taken -- what they woke for is seen by the refresh this sleep ends in
    // -- and the next sets the event again.
    WOKEN.swap(false, Ordering::AcqRel);
    paint_census::parked(woken);
    true
}

/// dwmapi's `DwmFlush`.
unsafe fn dwm_flush() -> HRESULT {
    // SAFETY: stored before the slot was pointed at `parked_dwm_flush`: dwmapi's `DwmFlush`,
    // which has this signature.
    let flush =
        unsafe { std::mem::transmute::<usize, DwmFlushFn>(DWM_FLUSH.load(Ordering::Acquire)) };
    unsafe { flush() }
}
