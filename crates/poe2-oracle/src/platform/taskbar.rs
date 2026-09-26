//! The app's permanent taskbar button, when `Settings::app_icon` asks for one: native Win32, since
//! GPUI's windows come and go while the button stays. Its window, the anchor, is never seen:
//!
//! - It is visible -- the taskbar only has buttons for visible windows -- but off every screen
//!   (`OFF_SCREEN`, where `WM_WINDOWPOSCHANGING` keeps it), 1x1, layered at zero opacity and
//!   click-through (`WS_EX_LAYERED | WS_EX_TRANSPARENT`), so it covers no pixel of any monitor,
//!   the game's included. It has no title bar, system menu or minimize box, never paints and runs
//!   no timer: it costs nothing while nobody touches it.
//! - `WS_EX_APPWINDOW` gives it the button, with the exe's icon (resource 1, as `build.rs` embeds
//!   it and GPUI's windows use it) and the title «PoE2 Oracle».
//! - A click on the button -- or picking the app in Alt+Tab or Task View -- makes it the
//!   foreground window (the taskbar and Alt+Tab call `SetForegroundWindow`), and its `WM_ACTIVATE`
//!   asks the app to open or raise the settings ([`ButtonEvent::Activated`]).
//!   `WS_EX_NOACTIVATE` keeps Windows from activating it on its own: it is skipped when the active
//!   window hides or closes, so closing the settings window never opens them again.
//! - «Закрыть окно» in the button's menu sends `WM_SYSCOMMAND`/`SC_CLOSE` (`WM_CLOSE` from
//!   elsewhere): the app quits ([`ButtonEvent::Close`]), as from the tray's «Выход».
//! - The hover thumbnail of an empty window would be a blank rectangle: DWM shows the window's
//!   icon in its place (`DWMWA_FORCE_ICONIC_REPRESENTATION`, without a bitmap of the app's own),
//!   and hovering the thumbnail peeks at nothing (`DWMWA_DISALLOW_PEEK`).
//!
//! GPUI gives its `WindowKind::Normal` windows -- the settings and report windows -- a button of
//! their own (`WS_EX_APPWINDOW`). While this one shows, they hang on the anchor instead: owned by
//! it (`GWLP_HWNDPARENT`) and without `WS_EX_APPWINDOW`. An owned window gets no button, and its
//! owner's button stands for it, lit while it is active. A window opened under the button is taken
//! over before it is first shown ([`under_button`]); one already open when the button comes or
//! goes is taken over or given back at once, `ITaskbarList` dropping or adding its own button.
//! Windows hides an owner's windows while the owner is minimized, so the anchor ignores
//! `SC_MINIMIZE`: with no minimize box, the taskbar leaves it alone too.
//!
//! Alt+Tab lists the anchor as «PoE2 Oracle» with the icon while neither the settings nor the
//! report window is open, and picking it opens the settings; an open one stands in for it, as the
//! last active window its owner has.

use std::cell::Cell;
use std::sync::LazyLock;

use anyhow::{Context as _, Result, bail};
use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_DISALLOW_PEEK, DWMWA_EXCLUDED_FROM_PEEK, DWMWA_FORCE_ICONIC_REPRESENTATION,
    DwmSetWindowAttribute,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{ITaskbarList, TaskbarList};
use windows::Win32::UI::WindowsAndMessaging::{
    CWPSTRUCT, CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, EnumThreadWindows,
    GW_OWNER, GWL_EXSTYLE, GWL_STYLE, GWLP_HWNDPARENT, GetSystemMetrics, GetWindow,
    GetWindowLongPtrW, GetWindowThreadProcessId, HC_ACTION, HHOOK, HICON, IMAGE_ICON,
    IsWindowVisible, LR_DEFAULTSIZE, LR_SHARED, LWA_ALPHA, LoadImageW, RegisterClassExW, SC_CLOSE,
    SC_MINIMIZE, SM_CXSMICON, SW_SHOWNOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowsHookExW, ShowWindow,
    UnhookWindowsHookEx, WA_INACTIVE, WH_CALLWNDPROC, WINDOWPOS, WM_ACTIVATE, WM_CLOSE, WM_CREATE,
    WM_SYSCOMMAND, WM_WINDOWPOSCHANGING, WNDCLASSEXW, WS_CHILD, WS_EX_APPWINDOW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, PCWSTR, w};

const CLASS: PCWSTR = w!("PoE2Oracle.Taskbar");
/// Where the anchor sits: off every screen, where Windows puts minimized windows.
const OFF_SCREEN: i32 = -32000;

/// What the player did with the button; the receiving end is [`events`]'.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEvent {
    /// A click on the button, or the app picked in Alt+Tab or Task View.
    Activated,
    /// «Закрыть окно» in the button's menu.
    Close,
}

static EVENTS: LazyLock<(
    async_channel::Sender<ButtonEvent>,
    async_channel::Receiver<ButtonEvent>,
)> = LazyLock::new(async_channel::unbounded);

thread_local! {
    /// The anchor of the button that shows, for [`under_button`] and its hook.
    static ANCHOR: Cell<Option<HWND>> = const { Cell::new(None) };
}

/// What every button the app shows reports, for the app's whole run.
pub fn events() -> async_channel::Receiver<ButtonEvent> {
    EVENTS.1.clone()
}

/// The taskbar button: it shows while this lives.
pub struct TaskbarButton {
    anchor: HWND,
}

impl TaskbarButton {
    /// Puts the app's button on the taskbar, and the app's open windows that have a button of
    /// their own under it. On GPUI's main thread, whose message loop drives the anchor, and outside
    /// any GPUI update: taking a window over sends its procedure `WM_STYLECHANGED`, and the
    /// taskbar asks it for its title and icon.
    pub fn show() -> Result<TaskbarButton> {
        let module = unsafe { GetModuleHandleW(PCWSTR::null()) }.context("GetModuleHandleW")?;
        let instance = HINSTANCE::from(module);
        register_class(instance)?;
        let anchor = unsafe {
            CreateWindowExW(
                WS_EX_APPWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
                CLASS,
                w!("PoE2 Oracle"),
                WS_POPUP,
                OFF_SCREEN,
                OFF_SCREEN,
                1,
                1,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .context("CreateWindowExW")?;
        // From here on, dropping it takes the anchor down.
        let button = TaskbarButton { anchor };
        unsafe { SetLayeredWindowAttributes(anchor, COLORREF(0), 0, LWA_ALPHA) }
            .context("SetLayeredWindowAttributes")?;
        let on = BOOL::from(true);
        for attribute in [
            DWMWA_FORCE_ICONIC_REPRESENTATION,
            DWMWA_DISALLOW_PEEK,
            DWMWA_EXCLUDED_FROM_PEEK,
        ] {
            let set = unsafe {
                DwmSetWindowAttribute(
                    anchor,
                    attribute,
                    (&raw const on).cast(),
                    size_of::<BOOL>() as u32,
                )
            };
            if let Err(err) = set {
                log::warn!("DwmSetWindowAttribute({attribute:?}) on the taskbar anchor: {err}");
            }
        }
        // Returns the previous visibility, not an error.
        let _ = unsafe { ShowWindow(anchor, SW_SHOWNOACTIVATE) };
        ANCHOR.set(Some(anchor));
        take_over_open_windows(anchor);
        Ok(button)
    }

    /// Takes the button off the taskbar, and gives the windows under it their own buttons back.
    /// Outside any GPUI update, as [`TaskbarButton::show`].
    pub fn remove(self) {
        give_back(self.anchor, true);
    }
}

impl Drop for TaskbarButton {
    fn drop(&mut self) {
        if ANCHOR.get() == Some(self.anchor) {
            ANCHOR.set(None);
        }
        // `DestroyWindow` takes an owner's windows along, and GPUI's own must go GPUI's way: they
        // stop hanging on the anchor first.
        give_back(self.anchor, false);
        let _ = unsafe { DestroyWindow(self.anchor) };
    }
}

/// Runs `open`, which opens one of the app's own windows, so that while the button shows the new
/// window is created under it: taken over at its `WM_CREATE`, before GPUI first shows it -- within
/// `open_window`, and no option of GPUI's names an owner -- so a button of its own never appears.
pub fn under_button<R>(open: impl FnOnce() -> R) -> R {
    if ANCHOR.get().is_none() {
        return open();
    }
    let hook = unsafe {
        SetWindowsHookExW(
            WH_CALLWNDPROC,
            Some(take_over_created),
            None,
            GetCurrentThreadId(),
        )
    }
    .inspect_err(|err| log::warn!("a window opens with a taskbar button of its own: {err}"))
    .ok()
    .map(Hook);
    let opened = open();
    drop(hook);
    opened
}

/// The [`under_button`] hook, removed when it drops -- a panicking `open` included.
struct Hook(HHOOK);

impl Drop for Hook {
    fn drop(&mut self) {
        let _ = unsafe { UnhookWindowsHookEx(self.0) };
    }
}

/// [`under_button`]'s hook: every message sent to a window of GPUI's main thread passes it first.
unsafe extern "system" fn take_over_created(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for `HC_ACTION`, `lparam` points at the message being sent.
        let sent = unsafe { &*(lparam.0 as *const CWPSTRUCT) };
        if sent.message == WM_CREATE
            && let Some(anchor) = ANCHOR.get()
            && has_own_button(sent.hwnd, anchor)
        {
            take_over(sent.hwnd, anchor);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn register_class(instance: HINSTANCE) -> Result<()> {
    // Shared: loaded once for the process, never destroyed.
    let icon = |size: i32, flags| {
        unsafe {
            LoadImageW(
                Some(instance),
                PCWSTR(1 as _),
                IMAGE_ICON,
                size,
                size,
                flags,
            )
        }
        .map(|handle| HICON(handle.0))
        .unwrap_or_default()
    };
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(anchor_proc),
        hInstance: instance,
        hIcon: icon(0, LR_DEFAULTSIZE | LR_SHARED),
        hIconSm: icon(unsafe { GetSystemMetrics(SM_CXSMICON) }, LR_SHARED),
        lpszClassName: CLASS,
        ..Default::default()
    };
    // A button shown again after the player took it away finds the class the first one registered.
    if unsafe { RegisterClassExW(&class) } == 0
        && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
    {
        bail!("RegisterClassExW failed: {:?}", unsafe { GetLastError() });
    }
    Ok(())
}

/// The anchor's procedure: activation opens the settings, a close quits, and the anchor stays off
/// every screen and never minimized.
unsafe extern "system" fn anchor_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // `lparam`: the window that had the activation.
        WM_ACTIVATE
            if (wparam.0 & 0xFFFF) as u32 != WA_INACTIVE
                && !handed_on(HWND(lparam.0 as *mut core::ffi::c_void)) =>
        {
            let _ = EVENTS.0.try_send(ButtonEvent::Activated);
        }
        // The low four bits are the system's own.
        WM_SYSCOMMAND => match (wparam.0 & 0xFFF0) as u32 {
            SC_CLOSE => return close(),
            SC_MINIMIZE => return LRESULT(0),
            _ => {}
        },
        WM_CLOSE => return close(),
        // Whatever moves it -- a monitor that came or went, say -- it stays where nobody sees it.
        WM_WINDOWPOSCHANGING => {
            // SAFETY: `lparam` points at the change to be made, which this may edit.
            let change = unsafe { &mut *(lparam.0 as *mut WINDOWPOS) };
            if !change.flags.contains(SWP_NOMOVE) {
                (change.x, change.y) = (OFF_SCREEN, OFF_SCREEN);
            }
            if !change.flags.contains(SWP_NOSIZE) {
                (change.cx, change.cy) = (1, 1);
            }
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// Not destroyed here: the app quits, and its end takes the anchor along.
fn close() -> LRESULT {
    let _ = EVENTS.0.try_send(ButtonEvent::Close);
    LRESULT(0)
}

/// Whether `previous`, the window the anchor took the activation from, is one of this app's that
/// just hid or closed: Windows handed the activation on, and the player asked for nothing.
/// `WS_EX_NOACTIVATE` should keep that from ever happening.
fn handed_on(previous: HWND) -> bool {
    if previous.is_invalid() {
        return false;
    }
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(previous, Some(&mut process)) };
    process == std::process::id() && !unsafe { IsWindowVisible(previous) }.as_bool()
}

/// Whether `hwnd`, a window of GPUI's main thread, has a taskbar button of its own: a top-level
/// window with `WS_EX_APPWINDOW` -- GPUI's settings and report windows -- that isn't the anchor.
fn has_own_button(hwnd: HWND, anchor: HWND) -> bool {
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    hwnd != anchor && style & WS_CHILD.0 == 0 && ex_style & WS_EX_APPWINDOW.0 != 0
}

/// Hangs `hwnd` on the anchor, without a button of its own.
fn take_over(hwnd: HWND, anchor: HWND) {
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, anchor.0 as isize);
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style & !(WS_EX_APPWINDOW.0 as isize));
    }
}

/// Takes over the app's windows that already have a button of their own, and has the taskbar drop
/// those buttons.
fn take_over_open_windows(anchor: HWND) {
    let taken: Vec<HWND> = thread_windows()
        .into_iter()
        .filter(|&hwnd| has_own_button(hwnd, anchor))
        .collect();
    for &hwnd in &taken {
        take_over(hwnd, anchor);
    }
    retab(&taken, false);
}

/// Lets go of the windows hanging on `anchor`: with `restore`, they get their own buttons back;
/// without it they only stop hanging on it, for its destruction.
fn give_back(anchor: HWND, restore: bool) {
    let owned: Vec<HWND> = thread_windows()
        .into_iter()
        .filter(|&hwnd| unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok_and(|owner| owner == anchor))
        .collect();
    for &hwnd in &owned {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, 0);
            if restore {
                let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_APPWINDOW.0 as isize);
            }
        }
    }
    if restore {
        retab(&owned, true);
    }
}

/// The top-level windows of this thread, GPUI's main thread.
fn thread_windows() -> Vec<HWND> {
    unsafe extern "system" fn collect(hwnd: HWND, list: LPARAM) -> BOOL {
        // SAFETY: `list` is the vector `EnumThreadWindows` was handed below, alive for its call.
        unsafe { &mut *(list.0 as *mut Vec<HWND>) }.push(hwnd);
        BOOL::from(true)
    }
    let mut windows = Vec::new();
    // False once the thread has no window left to list, which isn't an error.
    let _ = unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(collect),
            LPARAM(&raw mut windows as isize),
        )
    };
    windows
}

/// Has the taskbar add or drop the buttons of `hwnds`' visible windows now: it reads a window's
/// styles only as the window shows.
fn retab(hwnds: &[HWND], add: bool) {
    let visible: Vec<HWND> = hwnds
        .iter()
        .copied()
        .filter(|&hwnd| unsafe { IsWindowVisible(hwnd) }.as_bool())
        .collect();
    if visible.is_empty() {
        return;
    }
    // COM is ready on GPUI's main thread: GPUI initializes OLE there.
    let result = unsafe {
        CoCreateInstance::<_, ITaskbarList>(&TaskbarList, None, CLSCTX_INPROC_SERVER).and_then(
            |list| {
                list.HrInit()?;
                visible.iter().try_for_each(|&hwnd| {
                    if add {
                        list.AddTab(hwnd)
                    } else {
                        list.DeleteTab(hwnd)
                    }
                })
            },
        )
    };
    if let Err(err) = result {
        log::warn!("updating the taskbar's buttons failed: {err}");
    }
}
