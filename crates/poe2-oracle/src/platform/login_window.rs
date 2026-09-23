//! The sign-in window: pathofexile.com's own login page in Microsoft Edge WebView2 -- the real
//! Edge engine, so Cloudflare's check passes and signing in through Steam works -- hosted in a
//! plain Win32 window of the app's. It is an ordinary window: not topmost, in the taskbar, closed by
//! its × at any moment, the browser's start included.
//!
//! WebView2 lives on the thread that creates it and answers through that thread's message loop:
//! GPUI's main thread, which GPUI has already made an OLE single-threaded apartment. Nothing here
//! waits for the browser: it starts, shows pages and reads cookies in completion handlers the loop
//! dispatches, and what the app needs goes out as [`LoginEvent`]s. After every pathofexile.com
//! page the window reports the `POESESSID` cookies the browser holds for it; the app asks the site
//! about them (`crate::login`) and closes the window once one is signed in.
//!
//! The browser keeps nothing: it runs InPrivate, in a profile folder of its own under
//! [`paths::login_browser_dir`], deleted once the browser's process has exited after the window
//! closes. Whatever a sign-in interrupted harder than that leaves goes at the next sign-in or start
//! ([`wipe_profiles`]).
//!
//! [`open`] and [`bring_forward`] activate the window, which sends window messages at once -- to
//! the GPUI window losing activation too -- so they're called outside any GPUI update, from a
//! task's body. [`close`] only posts.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Context as _, Result, ensure};
use async_channel::Sender;
use http_client::Url;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COLOR, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
    CreateCoreWebView2EnvironmentWithOptions, GetAvailableCoreWebView2BrowserVersionString,
    ICoreWebView2, ICoreWebView2_2, ICoreWebView2Controller, ICoreWebView2Controller2,
    ICoreWebView2CookieList, ICoreWebView2Environment, ICoreWebView2Environment5,
    ICoreWebView2Environment10, ICoreWebView2EnvironmentOptions, ICoreWebView2Settings8,
};
use webview2_com::{
    BrowserProcessExitedEventHandler, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, GetCookiesCompletedHandler,
    NavigationCompletedEventHandler, NewWindowRequestedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{
    E_POINTER, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT,
    WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BLACK_BRUSH, GetMonitorInfoW, GetStockObject, HBRUSH, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetForegroundWindow, HICON,
    IDC_ARROW, IMAGE_ICON, IsIconic, LR_DEFAULTSIZE, LR_SHARED, LoadCursorW, LoadImageW,
    PostMessageW, RegisterClassExW, SIZE_MINIMIZED, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE,
    SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindow, WINDOW_EX_STYLE, WM_CLOSE,
    WM_DESTROY, WM_DPICHANGED, WM_MOVE, WM_SETFOCUS, WM_SIZE, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{HSTRING, Interface, PCWSTR, PWSTR, w};

use crate::paths;
use crate::platform::game_window::dpi_to_scale;

/// Where the window starts.
const LOGIN_PAGE: PCWSTR = w!("https://www.pathofexile.com/login");
/// The site's session cookie.
const SESSION_COOKIE: &str = "POESESSID";
const CLASS_NAME: PCWSTR = w!("PoE2OracleSignIn");
const TITLE: PCWSTR = w!("PoE2 Oracle — вход на pathofexile.com");
/// The window's size before DPI scaling: pathofexile.com lays its pages out about 1000 px wide.
const WIDTH: f64 = 1040.0;
const HEIGHT: f64 = 800.0;
/// What the view shows before the first page paints: the site's own black, not a white flash.
const BACKGROUND: COREWEBVIEW2_COLOR = COREWEBVIEW2_COLOR {
    A: 255,
    R: 0,
    G: 0,
    B: 0,
};

/// What the sign-in window tells the app. No `Debug`: it carries sessions.
pub enum LoginEvent {
    /// A pathofexile.com page finished loading, and the browser holds these `POESESSID` cookies
    /// for it (one, as a rule): the session the site gives every visitor, signed in or not.
    Sessions(Vec<String>),
    /// The browser failed to start or to open the login page; the window closes.
    Failed(String),
    /// The window closed: its ×, [`close`], or a failure.
    Closed,
}

/// The open sign-in window; there's one at most.
struct Login {
    hwnd: HWND,
    /// Tells this window's browser handlers from those of a window closed earlier.
    id: u64,
    events: Sender<LoginEvent>,
    /// The browser's profile folder.
    profile: PathBuf,
    environment: Option<ICoreWebView2Environment>,
    controller: Option<ICoreWebView2Controller>,
}

/// The browser of a closed window, held until its process has exited -- holding it is what brings
/// the news -- when its profile folder can go.
struct Exiting {
    _environment: ICoreWebView2Environment5,
    exited: Rc<Cell<bool>>,
}

thread_local! {
    static LOGIN: RefCell<Option<Login>> = const { RefCell::new(None) };
    static LAST_ID: Cell<u64> = const { Cell::new(0) };
    /// Released at the next sign-in once exited: not from inside their own exit event.
    static EXITING: RefCell<Vec<Exiting>> = const { RefCell::new(Vec::new()) };
}

/// The WebView2 runtime's version; `None` when there is none and the window can't open.
pub fn runtime_version() -> Option<String> {
    let mut version = PWSTR::null();
    // SAFETY: on success `version` is a string the loader allocated; `take_pwstr` frees it.
    unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut version) }.ok()?;
    Some(take_pwstr(version)).filter(|version| !version.is_empty())
}

/// Opens the sign-in window on pathofexile.com's login page, reporting to `events` until it
/// closes. Fails, with no window, when the browser can't be asked to start.
pub fn open(events: Sender<LoginEvent>) -> Result<()> {
    ensure!(
        LOGIN.with_borrow(Option::is_none),
        "the sign-in window is already open"
    );
    EXITING.with_borrow_mut(|exiting| exiting.retain(|browser| !browser.exited.get()));
    wipe_profiles();
    let id = LAST_ID.get() + 1;
    LAST_ID.set(id);
    let profile = paths::login_browser_dir().join(id.to_string());
    let hwnd = create_window()?;
    LOGIN.set(Some(Login {
        hwnd,
        id,
        events,
        profile: profile.clone(),
        environment: None,
        controller: None,
    }));
    let started = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |result, environment| {
            on_environment(
                id,
                result.and_then(|()| environment.ok_or_else(|| E_POINTER.into())),
            );
            Ok(())
        },
    ));
    // SAFETY: the folder string outlives the call; the handler is called later, on this thread.
    let requested = unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &HSTRING::from(profile.as_os_str()),
            None::<&ICoreWebView2EnvironmentOptions>,
            &started,
        )
    };
    if let Err(err) = requested {
        // Nothing started; the window goes unseen, and without a word: nobody listens yet.
        LOGIN.set(None);
        // SAFETY: a window this thread created.
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(err).context("starting WebView2");
    }
    // SAFETY: a window this thread created.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(())
}

/// Brings the open sign-in window, if any, to the front.
pub fn bring_forward() {
    let Some(hwnd) = LOGIN.with_borrow(|login| login.as_ref().map(|login| login.hwnd)) else {
        return;
    };
    // SAFETY: a window this thread created and hasn't destroyed (it would have left `LOGIN`).
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Closes the open sign-in window, if any, as its × would.
pub fn close() {
    if let Some(hwnd) = LOGIN.with_borrow(|login| login.as_ref().map(|login| login.hwnd)) {
        post_close(hwnd);
    }
}

/// Deletes every sign-in browser's profile folder. One whose browser is still exiting keeps its
/// files in use and stays until next time.
pub fn wipe_profiles() {
    wipe(&paths::login_browser_dir());
}

fn wipe(folder: &Path) {
    match std::fs::remove_dir_all(folder) {
        Ok(()) => log::info!("sign-in browser profile deleted"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => log::warn!(
            "deleting the sign-in browser profile {} failed: {err}",
            folder.display()
        ),
    }
}

fn post_close(hwnd: HWND) {
    // SAFETY: posting to a window this thread created; a destroyed one just drops the message.
    let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
}

/// Window `id`'s browser started. It gets its view unless the window closed meanwhile; then
/// releasing it here ends it.
fn on_environment(id: u64, environment: windows::core::Result<ICoreWebView2Environment>) {
    let environment = match environment {
        Ok(environment) => environment,
        Err(err) => return fail(id, &err),
    };
    let Some(hwnd) = LOGIN.with_borrow_mut(|login| {
        let login = login.as_mut().filter(|login| login.id == id)?;
        login.environment = Some(environment.clone());
        Some(login.hwnd)
    }) else {
        return;
    };
    let created = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |result, controller| {
            on_controller(
                id,
                result.and_then(|()| controller.ok_or_else(|| E_POINTER.into())),
            );
            Ok(())
        },
    ));
    // SAFETY: `hwnd` is the live window; the handler is called later, on this thread.
    let requested = unsafe {
        match environment.cast::<ICoreWebView2Environment10>() {
            // InPrivate: the browser writes no cookie or history to its profile, even while open.
            Ok(environment) => {
                environment
                    .CreateCoreWebView2ControllerOptions()
                    .and_then(|options| {
                        options.SetIsInPrivateModeEnabled(true)?;
                        environment
                            .CreateCoreWebView2ControllerWithOptions(hwnd, &options, &created)
                    })
            }
            Err(_) => environment.CreateCoreWebView2Controller(hwnd, &created),
        }
    };
    if let Err(err) = requested {
        fail(id, &err);
    }
}

/// Window `id`'s view is ready: it fills the window and opens the login page -- unless the window
/// closed meanwhile, and then it closes too.
fn on_controller(id: u64, controller: windows::core::Result<ICoreWebView2Controller>) {
    let controller = match controller {
        Ok(controller) => controller,
        Err(err) => return fail(id, &err),
    };
    let Some(hwnd) = LOGIN.with_borrow_mut(|login| {
        let login = login.as_mut().filter(|login| login.id == id)?;
        login.controller = Some(controller.clone());
        Some(login.hwnd)
    }) else {
        // SAFETY: a view nothing else uses.
        let _ = unsafe { controller.Close() };
        return;
    };
    if let Err(err) = show_login_page(id, hwnd, &controller) {
        fail(id, &err);
    }
}

fn show_login_page(
    id: u64,
    hwnd: HWND,
    controller: &ICoreWebView2Controller,
) -> windows::core::Result<()> {
    let page_loaded = NavigationCompletedEventHandler::create(Box::new(move |webview, _| {
        if let Some(webview) = webview {
            report_sessions(id, &webview);
        }
        Ok(())
    }));
    // What a page opens in a new window opens here instead: one window to close, and one browser
    // to wait for before the profile can go.
    let new_window = NewWindowRequestedEventHandler::create(Box::new(|webview, args| {
        let (Some(webview), Some(args)) = (webview, args) else {
            return Ok(());
        };
        let mut uri = PWSTR::null();
        // SAFETY: on success `uri` is a string the browser allocated; `take_pwstr` frees it.
        unsafe {
            args.Uri(&mut uri)?;
            let uri = take_pwstr(uri);
            args.SetHandled(true)?;
            webview.Navigate(&HSTRING::from(uri))
        }
    }));
    // SAFETY: plain calls on live WebView2 objects, on their own thread; the handlers are called
    // later, on this thread.
    unsafe {
        controller.SetBounds(client_rect(hwnd))?;
        if let Ok(controller) = controller.cast::<ICoreWebView2Controller2>() {
            controller.SetDefaultBackgroundColor(BACKGROUND)?;
        }
        let webview = controller.CoreWebView2()?;
        let settings = webview.Settings()?;
        settings.SetAreDevToolsEnabled(false)?;
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetIsWebMessageEnabled(false)?;
        // No SmartScreen: it would send Microsoft the address of every page, all of them
        // pathofexile.com's or those of the services it signs in through.
        if let Ok(settings) = settings.cast::<ICoreWebView2Settings8>() {
            settings.SetIsReputationCheckingRequired(false)?;
        }
        let mut token = 0;
        webview.add_NavigationCompleted(&page_loaded, &mut token)?;
        webview.add_NewWindowRequested(&new_window, &mut token)?;
        webview.Navigate(LOGIN_PAGE)?;
        // Keys go to the page. Fails while the window is minimized: then its next focus does it.
        let _ = controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
    }
    Ok(())
}

/// A page of window `id` finished loading: when it's pathofexile.com's, the app hears which
/// `POESESSID` cookies the browser holds for it.
fn report_sessions(id: u64, webview: &ICoreWebView2) {
    let Some(events) = LOGIN.with_borrow(|login| {
        login
            .as_ref()
            .filter(|login| login.id == id)
            .map(|login| login.events.clone())
    }) else {
        return;
    };
    let mut source = PWSTR::null();
    // SAFETY: on success `source` is a string the browser allocated; `take_pwstr` frees it.
    if unsafe { webview.Source(&mut source) }.is_err() {
        return;
    }
    let page = take_pwstr(source);
    if !is_site_page(&page) {
        return;
    }
    let read = webview.cast::<ICoreWebView2_2>().and_then(|webview| {
        let handler = GetCookiesCompletedHandler::create(Box::new(move |result, cookies| {
            match result
                .and_then(|()| cookies.ok_or_else(|| E_POINTER.into()))
                .and_then(|cookies| session_cookies(&cookies))
            {
                Ok(sessions) => {
                    let _ = events.try_send(LoginEvent::Sessions(sessions));
                }
                Err(err) => log::warn!("reading the sign-in window's cookies failed: {err}"),
            }
            Ok(())
        }));
        // SAFETY: the page string outlives the call; the handler is called later, on this thread.
        unsafe {
            webview
                .CookieManager()?
                .GetCookies(&HSTRING::from(page.as_str()), &handler)
        }
    });
    if let Err(err) = read {
        log::warn!("reading the sign-in window's cookies failed: {err}");
    }
}

/// Whether `page` is on pathofexile.com, over https.
fn is_site_page(page: &str) -> bool {
    Url::parse(page).is_ok_and(|url| {
        url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|host| host == "pathofexile.com" || host.ends_with(".pathofexile.com"))
    })
}

/// The values of the `POESESSID` cookies among `cookies`.
fn session_cookies(cookies: &ICoreWebView2CookieList) -> windows::core::Result<Vec<String>> {
    let mut sessions = Vec::new();
    let mut count = 0;
    // SAFETY: plain reads of a live cookie list; every string they return is the caller's, freed
    // by `take_pwstr`.
    unsafe {
        cookies.Count(&mut count)?;
        for index in 0..count {
            let cookie = cookies.GetValueAtIndex(index)?;
            let mut name = PWSTR::null();
            cookie.Name(&mut name)?;
            if take_pwstr(name) == SESSION_COOKIE {
                let mut value = PWSTR::null();
                cookie.Value(&mut value)?;
                sessions.push(take_pwstr(value));
            }
        }
    }
    Ok(sessions)
}

/// Window `id`'s browser failed: the app hears why, and the window closes.
fn fail(id: u64, error: &windows::core::Error) {
    let Some((hwnd, events)) = LOGIN.with_borrow(|login| {
        login
            .as_ref()
            .filter(|login| login.id == id)
            .map(|login| (login.hwnd, login.events.clone()))
    }) else {
        return;
    };
    log::warn!("the sign-in window's browser failed: {error}");
    let _ = events.try_send(LoginEvent::Failed(error.message()));
    post_close(hwnd);
}

/// The window is closing: the app hears it, and its browser closes, the profile folder deleted
/// once the browser's process has exited. A browser that has no view yet ends as it's released
/// here; its folder goes at the next sign-in or start.
fn teardown(hwnd: HWND) {
    let Some(login) = LOGIN.with_borrow_mut(|login| login.take_if(|login| login.hwnd == hwnd))
    else {
        return;
    };
    let _ = login.events.try_send(LoginEvent::Closed);
    let Some(controller) = login.controller else {
        return;
    };
    if let Some(environment) = login.environment {
        wipe_on_exit(&environment, login.profile);
    }
    // SAFETY: the view is closed once, before its window goes.
    if let Err(err) = unsafe { controller.Close() } {
        log::warn!("closing the sign-in window's browser failed: {err}");
    }
}

/// Deletes `profile` once `environment`'s browser process has exited: its files are in use until
/// then.
fn wipe_on_exit(environment: &ICoreWebView2Environment, profile: PathBuf) {
    let Ok(environment) = environment.cast::<ICoreWebView2Environment5>() else {
        return;
    };
    let exited = Rc::new(Cell::new(false));
    let handler = BrowserProcessExitedEventHandler::create(Box::new({
        let exited = exited.clone();
        move |_, _| {
            if !exited.replace(true) {
                let profile = profile.clone();
                // Off the UI thread: a used profile is hundreds of files.
                std::thread::spawn(move || wipe(&profile));
            }
            Ok(())
        }
    }));
    let mut token = 0;
    // SAFETY: the handler is called later, on this thread.
    match unsafe { environment.add_BrowserProcessExited(&handler, &mut token) } {
        Ok(()) => EXITING.with_borrow_mut(|exiting| {
            exiting.push(Exiting {
                _environment: environment,
                exited,
            })
        }),
        Err(err) => log::warn!("watching the sign-in browser's exit failed: {err}"),
    }
}

fn client_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a valid out pointer; a failed call leaves it empty.
    let _ = unsafe { GetClientRect(hwnd, &mut rect) };
    rect
}

/// The view of window `hwnd`, once it has one.
fn controller(hwnd: HWND) -> Option<ICoreWebView2Controller> {
    LOGIN.with_borrow(|login| {
        login
            .as_ref()
            .filter(|login| login.hwnd == hwnd)
            .and_then(|login| login.controller.clone())
    })
}

/// A top-level window for the view: centered on the monitor of the window in front (the settings
/// window «Войти» was clicked in), `WIDTH` × `HEIGHT` at its scale but no larger than nine tenths
/// of its work area.
fn create_window() -> Result<HWND> {
    // SAFETY: this module's own module handle.
    let instance = HINSTANCE::from(unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW")?);
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        // The exe's icon: resource 1 (build.rs), as on GPUI's windows.
        // SAFETY: loads a shared resource of this exe.
        hIcon: unsafe {
            LoadImageW(
                Some(instance),
                PCWSTR(std::ptr::without_provenance(1)),
                IMAGE_ICON,
                0,
                0,
                LR_DEFAULTSIZE | LR_SHARED,
            )
        }
        .map(|icon| HICON(icon.0))
        .unwrap_or_default(),
        // SAFETY: a system cursor.
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        // SAFETY: a stock brush.
        hbrBackground: HBRUSH(unsafe { GetStockObject(BLACK_BRUSH) }.0),
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // A second sign-in finds the class the first one registered.
    // SAFETY: `class` is fully initialized and its strings are static.
    if unsafe { RegisterClassExW(&class) } == 0
        && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
    {
        return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
    }

    // SAFETY: plain queries; `info` is a valid out pointer with its size set.
    let (area, scale) = unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dpi_x, mut dpi_y) = (0, 0);
        let scale = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y)
            .map_or(1.0, |()| dpi_to_scale(dpi_x));
        (info.rcWork, scale)
    };
    let (area_width, area_height) = (area.right - area.left, area.bottom - area.top);
    let width = ((WIDTH * scale) as i32).min(area_width * 9 / 10);
    let height = ((HEIGHT * scale) as i32).min(area_height * 9 / 10);
    // SAFETY: the class is registered and the strings are static.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            TITLE,
            WS_OVERLAPPEDWINDOW,
            area.left + (area_width - width) / 2,
            area.top + (area_height - height) / 2,
            width,
            height,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .context("CreateWindowExW")
}

/// The sign-in window's procedure: the view follows the window's size, position, focus and DPI,
/// and closing the window closes the view first.
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_SIZE if wparam.0 != SIZE_MINIMIZED as usize => {
            if let Some(controller) = controller(hwnd) {
                // SAFETY: a live view, on its own thread.
                let _ = unsafe { controller.SetBounds(client_rect(hwnd)) };
            }
        }
        // Popups the page opens (a list's options) are placed from the window's position.
        WM_MOVE => {
            if let Some(controller) = controller(hwnd) {
                // SAFETY: a live view, on its own thread.
                let _ = unsafe { controller.NotifyParentWindowPositionChanged() };
            }
        }
        WM_SETFOCUS => {
            if let Some(controller) = controller(hwnd) {
                // SAFETY: a live view, on its own thread.
                let _ =
                    unsafe { controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
            }
        }
        WM_DPICHANGED => {
            // SAFETY: WM_DPICHANGED's `lparam` points at the window's suggested new rect.
            let rect = unsafe { *(lparam.0 as *const RECT) };
            // SAFETY: this window.
            let _ = unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            return LRESULT(0);
        }
        // The default for WM_CLOSE then destroys the window; WM_DESTROY covers any other end.
        WM_CLOSE | WM_DESTROY => teardown(hwnd),
        _ => {}
    }
    // SAFETY: the message as this procedure got it.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
