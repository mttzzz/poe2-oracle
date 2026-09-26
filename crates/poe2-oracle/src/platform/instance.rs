//! One PoE2 Oracle per Windows session, with a door the others knock on.
//!
//! A second launch -- the Start menu shortcut while autostart already runs the app -- finds the
//! first by a named mutex, asks it for what the launch was for (`crate::launch`'s knock) and exits:
//! two copies would show two tray icons and contend for the hotkeys. The knock opens the running
//! copy's settings -- what launching it again most likely means -- with the welcome over them when
//! the installer's finish page started the launch. A second launch by autostart itself, or by an
//! update's restart, exits without asking.
//!
//! The first copy keeps a hidden window of a known class, the door. Besides the knock, `WM_CLOSE`
//! to it quits the app the normal way, its tray icon removed. That is how the installer
//! closes a running copy (`packaging/installer.nsi`): GPUI leaves its message loop on no window
//! message, so without the door the installer could only force-close the app, and a force-closed
//! app leaves its tray icon behind until the mouse passes over it.
//!
//! A game data update restarts the app itself ([`relaunch`]): the new copy starts while the old
//! one is still quitting, so it waits for that one to be gone ([`wait_for_exit`]) before it
//! claims the session.

use std::process::Command;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WAIT_TIMEOUT,
    WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CreateMutexW, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
    WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, FindWindowW, PostMessageW,
    RegisterClassExW, WM_APP, WM_CLOSE, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, PWSTR, w};

use crate::launch::{AFTER_ARG, Knock};

/// The door's window class: `packaging/installer.nsi` finds the window by it.
const DOOR_CLASS: PCWSTR = w!("PoE2Oracle.Instance");
/// Held for the process's life; `Local\` scopes it to this Windows session.
const MUTEX_NAME: PCWSTR = w!("Local\\PoE2Oracle.Instance");
/// Knocks: asks the running copy to open its settings. Its `WPARAM` is the knock's word
/// (`Knock::word`); a copy from before the welcome sent 0, the settings alone.
const WM_SHOW_SETTINGS: u32 = WM_APP + 1;
/// How long a second launch looks for the door of a first copy that is still starting.
const DOOR_WAIT: Duration = Duration::from_secs(3);

/// What another process asks the running copy for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// A second launch's knock.
    Knock(Knock),
    Quit,
}

static REQUESTS: LazyLock<(
    async_channel::Sender<Request>,
    async_channel::Receiver<Request>,
)> = LazyLock::new(async_channel::unbounded);

/// Claims this session's single copy. `Ok(None)`: another copy runs -- asked for `ask`, if any --
/// and this one should exit. Otherwise the door is open and every request arrives on the returned
/// channel. Call once, on the thread that runs GPUI's message loop: the door's window lives on it.
pub fn claim(ask: Option<Knock>) -> Result<Option<async_channel::Receiver<Request>>> {
    // Never closed: the handle is the claim, and the process's end releases it.
    unsafe { CreateMutexW(None, false, MUTEX_NAME) }.context("CreateMutexW")?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        if let Some(ask) = ask {
            knock(ask);
        }
        return Ok(None);
    }
    open_door()?;
    Ok(Some(REQUESTS.1.clone()))
}

/// Asks the running copy for `ask`, giving a copy that is still starting `DOOR_WAIT` to open its
/// door.
fn knock(ask: Knock) {
    let deadline = Instant::now() + DOOR_WAIT;
    loop {
        let door = unsafe { FindWindowW(DOOR_CLASS, PCWSTR::null()) }
            .ok()
            .filter(|hwnd| !hwnd.is_invalid());
        if let Some(door) = door {
            // The player just launched this process, so it may hand the foreground on: the
            // settings window must come up in front, not flash in the taskbar.
            let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
            let _ = unsafe {
                PostMessageW(Some(door), WM_SHOW_SETTINGS, WPARAM(ask.word()), LPARAM(0))
            };
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
        WM_SHOW_SETTINGS => Request::Knock(Knock::from_word(wparam.0)),
        _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    };
    let _ = REQUESTS.0.try_send(request);
    LRESULT(0)
}

/// How long a copy [`relaunch`] started waits for the one that started it to quit, before it
/// closes that one: the time the installer gives a running copy.
const RELAUNCH_WAIT: Duration = Duration::from_secs(10);

/// Starts this exe again, to take over once this process has quit ([`AFTER_ARG`]): a game data
/// update's restart, the new copy loading the new pack. Quit the app the normal way as soon as
/// this returns `Ok`.
pub fn relaunch() -> Result<()> {
    let exe = std::env::current_exe().context("finding this exe")?;
    Command::new(&exe)
        .arg(AFTER_ARG)
        .arg(std::process::id().to_string())
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))?;
    Ok(())
}

/// Waits for the process `pid` -- the copy that started this one with [`relaunch`] -- to quit, so
/// that [`claim`] finds the session free instead of a copy still running: at most
/// [`RELAUNCH_WAIT`], after which a copy of this exe still there is closed, as the installer
/// closes one that lingers. Returns whether it had to be.
pub fn wait_for_exit(pid: u32) -> bool {
    let access = PROCESS_SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION;
    // Gone already.
    let Ok(process) = (unsafe { OpenProcess(access, false, pid) }) else {
        return false;
    };
    let wait = RELAUNCH_WAIT.as_millis() as u32;
    let closed = unsafe { WaitForSingleObject(process, wait) } == WAIT_TIMEOUT
        && runs_this_exe(process)
        && unsafe { TerminateProcess(process, 1) }.is_ok();
    if closed {
        // Until it's gone, and its claim with it.
        let _ = unsafe { WaitForSingleObject(process, wait) };
    }
    let _ = unsafe { CloseHandle(process) };
    closed
}

/// Whether `process` runs this very exe: a process id is only a number, which Windows hands to
/// another process once its own has ended.
fn runs_this_exe(process: HANDLE) -> bool {
    let mut path = [0u16; 1024];
    let mut len = path.len() as u32;
    let named = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut len,
        )
    };
    if named.is_err() {
        return false;
    }
    let theirs = String::from_utf16_lossy(&path[..len as usize]);
    std::env::current_exe().is_ok_and(|ours| ours.to_string_lossy().eq_ignore_ascii_case(&theirs))
}
