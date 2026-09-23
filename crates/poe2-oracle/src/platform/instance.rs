//! One PoE2 Oracle per Windows session, with a door the others knock on.
//!
//! A second launch -- the Start menu shortcut while autostart already runs the app -- finds the
//! first by a named mutex, asks it to open its settings (what launching it again most likely
//! means) and exits: two copies would show two tray icons and contend for the hotkeys. A second
//! launch by autostart itself exits without asking.
//!
//! The first copy keeps a hidden window of a known class, the door. Besides the settings request,
//! `WM_CLOSE` to it quits the app the normal way, its tray icon removed. That is how the installer
//! closes a running copy (`packaging/installer.nsi`): GPUI leaves its message loop on no window
//! message, so without the door the installer could only force-close the app, and a force-closed
//! app leaves its tray icon behind until the mouse passes over it.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, FindWindowW, PostMessageW,
    RegisterClassExW, WM_APP, WM_CLOSE, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

/// The door's window class: `packaging/installer.nsi` finds the window by it.
const DOOR_CLASS: PCWSTR = w!("PoE2Oracle.Instance");
/// Held for the process's life; `Local\` scopes it to this Windows session.
const MUTEX_NAME: PCWSTR = w!("Local\\PoE2Oracle.Instance");
/// Asks the running copy to open its settings.
const WM_SHOW_SETTINGS: u32 = WM_APP + 1;
/// How long a second launch looks for the door of a first copy that is still starting.
const DOOR_WAIT: Duration = Duration::from_secs(3);

/// What another process asks the running copy for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    ShowSettings,
    Quit,
}

static REQUESTS: LazyLock<(
    async_channel::Sender<Request>,
    async_channel::Receiver<Request>,
)> = LazyLock::new(async_channel::unbounded);

/// Claims this session's single copy. `Ok(None)`: another copy runs -- asked to open its settings
/// unless `quietly` -- and this one should exit. Otherwise the door is open and every request
/// arrives on the returned channel. Call once, on the thread that runs GPUI's message loop: the
/// door's window lives on it.
pub fn claim(quietly: bool) -> Result<Option<async_channel::Receiver<Request>>> {
    // Never closed: the handle is the claim, and the process's end releases it.
    unsafe { CreateMutexW(None, false, MUTEX_NAME) }.context("CreateMutexW")?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        if !quietly {
            knock();
        }
        return Ok(None);
    }
    open_door()?;
    Ok(Some(REQUESTS.1.clone()))
}

/// Asks the running copy to open its settings, giving a copy that is still starting `DOOR_WAIT`
/// to open its door.
fn knock() {
    let deadline = Instant::now() + DOOR_WAIT;
    loop {
        let door = unsafe { FindWindowW(DOOR_CLASS, PCWSTR::null()) }
            .ok()
            .filter(|hwnd| !hwnd.is_invalid());
        if let Some(door) = door {
            // The player just launched this process, so it may hand the foreground on: the
            // settings window must come up in front, not flash in the taskbar.
            let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
            let _ = unsafe { PostMessageW(Some(door), WM_SHOW_SETTINGS, WPARAM(0), LPARAM(0)) };
            return;
        }
        if Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn open_door() -> Result<()> {
    let module = unsafe { GetModuleHandleW(PCWSTR::null()) }.context("GetModuleHandleW")?;
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(door_proc),
        hInstance: module.into(),
        lpszClassName: DOOR_CLASS,
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        bail!("RegisterClassExW failed: {:?}", unsafe { GetLastError() });
    }
    // Top-level, since FindWindow sees no message-only window, but never shown -- and a tool
    // window, so nothing would list it if it were.
    unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            DOOR_CLASS,
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .context("CreateWindowExW")?;
    Ok(())
}

unsafe extern "system" fn door_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let request = match message {
        // Not destroyed here: the app quits, and its end takes the window along.
        WM_CLOSE => Request::Quit,
        WM_SHOW_SETTINGS => Request::ShowSettings,
        _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    };
    let _ = REQUESTS.0.try_send(request);
    LRESULT(0)
}
