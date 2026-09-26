//! The application itself: registers the `Ctrl+E` price-check hotkey and the Esc hook via
//! `crate::price_check`, opens a frameless/transparent/popup overlay window, and renders
//! `crate::price_check::PriceCheckApp` in it. The panel stays open until Esc or its × button --
//! never closed by mouse movement, so the player can move into it and use it. `main.rs` only
//! calls [`run`].
//!
//! The OS-window side of EE2's behaviour lives in the small `PriceCheckRoot` wrapper below, which
//! re-syncs the platform window every time `PriceCheckApp` notifies:
//! - click-through while nothing is shown, interactive while an item is on screen;
//! - moved to EE2's placement for each check (full game height, glued to the inventory or stash
//!   panel), or to where the player dragged it on that side -- `crate::overlay_layout`; a drag
//!   moves it the same way;
//! - no Windows 11 DWM frame (otherwise a permanent outline over the game);
//! - focus handed back to the game when a panel the player clicked into closes.
//!
//! Where the running app shows itself -- its notification-area icon, a taskbar button or both --
//! is the player's choice (`Settings::app_icon`, applied live by `PriceCheckRoot::sync_presence`):
//! a `PopUp` window has no taskbar button, so without them the app would be invisible. A left click
//! on the icon or a click on the button opens the settings; the icon's menu opens them and quits,
//! and so does the button's «Закрыть окно» ([`serve_presence`]).

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, Bounds, Context, DisplayId, Entity, Focusable, IntoElement, Render, TitlebarOptions,
    Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point,
    prelude::*, px, size,
};
use gpui_platform::application;
use http_client::HttpClient;
use reqwest_client::ReqwestClient;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

use crate::brand;
use crate::diagnostics;
use crate::i18n::{self, Lang};
use crate::launch::{Knock, Launch};
use crate::logging;
use crate::login;
use crate::overlay_layout::PhysicalRect;
use crate::platform::instance::{self, Request};
use crate::platform::taskbar::{self, ButtonEvent, TaskbarButton};
use crate::platform::win32::Win32Overlay;
use crate::platform::{autostart, game_config, game_window, redraw_filter};
use crate::price_check::{self, BootstrapState, PriceCheckApp};
use crate::report;
use crate::session::{self, SessionHttpClient};
use crate::settings::{self, AppIcon, Hotkey};
use crate::tr;
use crate::ui::fonts;
use crate::ui::report_view::{self, ReportView};
use crate::ui::settings_view::{self, Intro, SettingsView};
use crate::ui::theme::BASE_REM_SIZE;
use crate::ui::tour;
use crate::ui::welcome;
use crate::ui::xp_overlay::{self, XpCover, XpOverlay, XpOverlayOptions};
use crate::updates;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
/// A server that goes quiet this long -- before its answer or in the middle of it -- fails the
/// request, and the caller's own retry or error takes over (the catalog load retries), instead of
/// "Loading…" for the rest of the run. Each part of the answer that arrives starts it anew, so a
/// long download that keeps coming -- an update's installer, an hour of the exchange's record
/// (2.7 MB) -- isn't cut short. Until the answer begins, though, it runs from the request's start,
/// an upload included: reports go through a client of their own (`report::send`).
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Wraps `Entity<PriceCheckApp>` with the platform-window state that has to follow it: the
/// `Win32Overlay` handle (resolved once the real platform window exists), what was last applied to
/// it, and where the app shows itself -- the tray icon, the taskbar button.
struct PriceCheckRoot {
    inner: Entity<PriceCheckApp>,
    overlay: Option<Win32Overlay>,
    /// Last applied click-through bit; `None` until the first sync so it always applies once
    /// (`Win32Overlay` can't read the bit back).
    last_click_through: Option<bool>,
    last_bounds: Option<PhysicalRect>,
    /// Last applied OS-window visibility; `None` until the first sync.
    last_shown: Option<bool>,
    /// EE2's inventory-side placement, used until the first check picks its own, and the UI
    /// scale it was sized for (the panel's width follows the scale).
    default_bounds: Option<PhysicalRect>,
    default_bounds_scale: Option<f32>,
    was_visible: bool,
    /// The notification-area icon, made the first time `Settings::app_icon` asks for it and
    /// hidden while it doesn't; the taskbar button, while it asks for one (`sync_presence`).
    tray: Option<Tray>,
    taskbar: Option<TaskbarButton>,
    /// Where the app shows itself as of the last change `sync_presence` made, and whether one is
    /// under way.
    app_icon: AppIcon,
    presence_changing: bool,
    /// The hotkey and the interface language the tray was last worded for; `None` until the first
    /// sync, and for a new tray.
    tray_words: Option<(Hotkey, Lang)>,
    /// The XP overlay, opened while the setting allows it (see `sync_xp`).
    xp: Option<Entity<XpOverlay>>,
    /// An overlay open is under way (the setting was just turned on), so it isn't started twice.
    xp_opening: bool,
    /// Last cover handed to the XP overlay; `None` until the first sync.
    last_xp_cover: Option<XpCover>,
}

impl PriceCheckRoot {
    fn ensure_overlay(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.overlay.is_some() {
            return;
        }
        match Win32Overlay::from_window(window) {
            Ok(overlay) => {
                if let Err(err) = overlay.disable_dwm_frame() {
                    log::warn!("{err:#}");
                }
                // Spawned before `sync_window`'s first task, so the frame is gone by the time
                // the window is first placed and shown. A click on the panel leaves the keyboard
                // with the game (`set_no_activate`): its checkboxes, chips and buttons act on
                // the panel, and a game that loses focus makes other overlays react (PoE Overlay
                // II opens its Session Recap). Only a bound input takes the keyboard, when it's
                // clicked into (`ui::panel::filters`).
                cx.spawn(async move |_, _| {
                    if let Err(err) = overlay.remove_frame() {
                        log::warn!("{err:#}");
                    }
                    if let Err(err) = overlay.set_no_activate() {
                        log::warn!("{err:#}");
                    }
                })
                .detach();
                self.overlay = Some(overlay);
            }
            Err(err) => log::warn!("Win32Overlay::from_window failed: {err:?}"),
        }
    }

    /// Brings the OS window in line with `PriceCheckApp`'s state. Runs from the first `render`
    /// (the only place the `Window` handle exists) and from the `observe` callback on every state
    /// change afterwards -- NOT from `render` alone: a hidden GPUI window is never redrawn
    /// (`gpui_windows` draws on `WM_PAINT`, which hidden windows don't get), so a sync that only
    /// ran in `render` could never show the window again once it had hidden it.
    fn sync_window(&mut self, cx: &mut Context<Self>) {
        // Before `sync_xp`, which hands the XP overlay the rect the panel is going to.
        let ui_scale = self.inner.read(cx).settings.ui_scale;
        if self.default_bounds_scale != Some(ui_scale) {
            self.default_bounds = game_window::default_panel_rect(ui_scale);
            self.default_bounds_scale = Some(ui_scale);
        }
        self.sync_presence(cx);
        self.sync_tray(cx);
        self.sync_xp(cx);
        let Some(overlay) = self.overlay else {
            return;
        };
        let (visible, placement, settings_open) = {
            let state = self.inner.read(cx);
            (
                state.visible,
                state.placement,
                state.settings_window().is_some(),
            )
        };

        let want_click_through = !visible;
        let click_through =
            (self.last_click_through != Some(want_click_through)).then_some(want_click_through);
        let wanted_bounds = placement.or(self.default_bounds);
        let bounds = if wanted_bounds != self.last_bounds {
            wanted_bounds
        } else {
            None
        };
        // Shown only when the player asked (`visible`): for an item, or for why there is none yet
        // (the catalog loading, or its first load failed -- `run_price_check`).
        let want_shown = visible;
        let shown = (self.last_shown != Some(want_shown)).then_some(want_shown);
        // Not while the settings window is what took over: it has the focus the player gave it.
        let focus_game = self.was_visible && !visible && !settings_open && overlay.is_foreground();

        self.last_click_through = Some(want_click_through);
        if bounds.is_some() {
            self.last_bounds = bounds;
        }
        self.last_shown = Some(want_shown);
        self.was_visible = visible;

        if click_through.is_none() && bounds.is_none() && shown.is_none() && !focus_game {
            return;
        }
        // One foreground task, applied in this order so the window is already in place and
        // clickable (or not) when it appears. Never applied synchronously: this runs inside
        // `render`/an `observe` callback, and `SetWindowPos`/`ShowWindow` send `WM_SIZE`/
        // `WM_SHOWWINDOW` synchronously into GPUI's own window state.
        cx.spawn(async move |_, _| {
            if let Some(enabled) = click_through
                && let Err(err) = overlay.set_click_through(enabled)
            {
                log::warn!("set_click_through({enabled}) failed: {err:?}");
            }
            if let Some(rect) = bounds
                && let Err(err) = overlay.set_bounds(rect)
            {
                log::warn!("{err:#}");
            }
            if let Some(shown) = shown {
                overlay.set_shown(shown);
            }
            if focus_game {
                game_window::focus_game();
            }
        })
        .detach();
    }

    /// Shows the app where `Settings::app_icon` says -- the tray icon, the taskbar button or both
    /// -- once the player changes it. Spawned, and all of it done outside any update: this runs
    /// inside `render`/an `observe` callback, and the button takes an open settings window over or
    /// gives it back (`TaskbarButton::show`), sending GPUI's procedure messages. What is wanted
    /// shows before what isn't goes, so the app never disappears; a change made meanwhile follows
    /// once this one is done.
    ///
    /// The tray icon, once made, is only hidden and shown again: Windows remembers whether the
    /// player keeps it by the clock or under the ^ arrow by the icon's number within the run,
    /// which a new icon wouldn't have.
    fn sync_presence(&mut self, cx: &mut Context<Self>) {
        let wanted = self.inner.read(cx).settings.app_icon;
        if wanted == self.app_icon || self.presence_changing {
            return;
        }
        self.presence_changing = true;
        cx.spawn(async move |this, cx| {
            let Ok((mut tray, mut button, hotkey)) = this.update(cx, |root, cx| {
                let hotkey = root.inner.read(cx).settings.hotkey;
                (root.tray.take(), root.taskbar.take(), hotkey)
            }) else {
                return;
            };
            if wanted.tray() {
                match &mut tray {
                    Some(tray) => tray.show(true),
                    None => tray = new_tray(hotkey),
                }
            }
            if wanted.taskbar() && button.is_none() {
                button = new_button();
            }
            if !wanted.tray()
                && let Some(tray) = &mut tray
            {
                tray.show(false);
            }
            if !wanted.taskbar()
                && let Some(button) = button.take()
            {
                button.remove();
            }
            this.update(cx, |root, cx| {
                root.tray = tray;
                root.taskbar = button;
                root.app_icon = wanted;
                root.presence_changing = false;
                root.tray_words = None;
                root.sync_presence(cx);
                root.sync_tray(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Words the tray in the interface language and names the current hotkey in its tooltip --
    /// the only place it's shown besides the panel's own hint.
    fn sync_tray(&mut self, cx: &mut Context<Self>) {
        let words = (self.inner.read(cx).settings.hotkey, i18n::lang());
        if self.tray_words == Some(words) {
            return;
        }
        if let Some(tray) = &self.tray {
            tray.word(words.0);
        }
        self.tray_words = Some(words);
    }

    /// Opens the XP overlay -- at the first sync, or once the setting is turned on -- and tells it
    /// what covers its plates: the setting off, or the price panel while it's shown. Turning the
    /// setting off only hides them; the sampling stops with the next launch, which never opens
    /// the overlay. The player's XP options follow it whenever they change.
    fn sync_xp(&mut self, cx: &mut Context<Self>) {
        let (enabled, panel, options) = {
            let state = self.inner.read(cx);
            (
                state.settings.xp_overlay,
                // Where the panel is going, as `sync_window` places it right after this.
                state
                    .visible
                    .then(|| state.placement.or(self.default_bounds))
                    .flatten(),
                XpOverlayOptions::from_settings(&state.settings),
            )
        };
        if enabled && self.xp.is_none() && !self.xp_opening {
            // Spawned: this runs from `render`, where no window may be opened.
            self.xp_opening = true;
            let app = self.inner.downgrade();
            cx.spawn(async move |this, cx| {
                let opened = cx.update(|cx| xp_overlay::open(options, app, cx));
                this.update(cx, |root, cx| {
                    root.xp_opening = false;
                    match opened {
                        Ok(xp) => root.xp = Some(xp),
                        Err(err) => log::warn!("the XP overlay is unavailable: {err:#}"),
                    }
                    root.last_xp_cover = None;
                    root.sync_xp(cx);
                })
                .ok();
            })
            .detach();
        }
        let cover = XpCover {
            off: !enabled,
            panel,
        };
        if let Some(xp) = &self.xp {
            if self.last_xp_cover != Some(cover) {
                xp.update(cx, |xp, cx| xp.set_cover(cover, cx));
                self.last_xp_cover = Some(cover);
            }
            xp.update(cx, |xp, cx| xp.set_options(options, cx));
        }
    }
}

impl Render for PriceCheckRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_overlay(window, cx);
        self.sync_window(cx);
        // The player's UI scale: the panel sizes its text and layout in rems, so all of it
        // follows.
        let scale = self.inner.read(cx).settings.ui_scale;
        window.set_rem_size(px(BASE_REM_SIZE * scale));
        div().size_full().child(self.inner.clone())
    }
}

fn build_window_options() -> WindowOptions {
    WindowOptions {
        // Placeholder only: the first render moves the window to EE2's placement next to the
        // inventory (`PriceCheckRoot::sync_window`), then shows it -- created hidden so it never
        // flashes at this origin.
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(460.), px(700.)),
        ))),
        titlebar: None,
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::PopUp,
        // Neither the system's move nor its resize: the title bar drags the panel sideways itself
        // (`PriceCheckApp::begin_panel_drag`), without activating it, and a resizable window's
        // top edge would answer as a resize border over the title bar.
        is_movable: false,
        is_resizable: false,
        // The overlay must never take keyboard focus from the game on launch.
        focus: false,
        show: false,
        ..Default::default()
    }
}

/// The app's own mark (`crate::brand`), the same one the exe and installer carry.
fn tray_icon_image() -> Icon {
    Icon::from_rgba(
        brand::TRAY_ICON_RGBA.to_vec(),
        brand::TRAY_ICON_SIZE,
        brand::TRAY_ICON_SIZE,
    )
    .expect("the tray RGBA's size is fixed by its array type")
}

fn tray_tooltip(hotkey: Hotkey) -> String {
    tr!("PoE2 Oracle — price check: {hotkey}", hotkey = hotkey)
}

/// The ids the tray menu's entries come back with ([`serve_presence`]).
const TRAY_SETTINGS: &str = "settings";
const TRAY_QUIT: &str = "quit";

/// The notification-area icon and its menu's entries, kept to word them again in another
/// interface language ([`Tray::word`]). Dropping it takes the icon away.
struct Tray {
    icon: TrayIcon,
    settings: MenuItem,
    quit: MenuItem,
    /// Not hidden ([`Tray::show`]).
    shown: bool,
}

impl Tray {
    /// Puts the icon in the notification area, with «Настройки» and «Выход» in its menu and
    /// `hotkey` named in its tooltip. A right click opens the menu, a left click the settings: its
    /// clicks reach [`serve_presence`]. On GPUI's main thread, whose message loop also drives the
    /// tray's hidden window.
    fn new(hotkey: Hotkey) -> anyhow::Result<Tray> {
        // `Tray::word` gives the entries their words.
        let settings = MenuItem::with_id(MenuId::new(TRAY_SETTINGS), "", true, None);
        let quit = MenuItem::with_id(MenuId::new(TRAY_QUIT), "", true, None);
        let menu = Menu::new();
        menu.append(&settings)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit)?;
        let icon = TrayIconBuilder::new()
            .with_icon(tray_icon_image())
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()?;
        let tray = Tray {
            icon,
            settings,
            quit,
            shown: true,
        };
        tray.word(hotkey);
        Ok(tray)
    }

    /// Words the menu in the interface language, and the tooltip naming `hotkey`.
    fn word(&self, hotkey: Hotkey) {
        self.settings.set_text(tr!("Settings"));
        self.quit.set_text(tr!("Quit"));
        if let Err(err) = self.icon.set_tooltip(Some(tray_tooltip(hotkey))) {
            log::warn!("{err:#}");
        }
    }

    /// Shows the icon in the notification area, or hides it there.
    fn show(&mut self, shown: bool) {
        if self.shown == shown {
            return;
        }
        match self.icon.set_visible(shown) {
            Ok(()) => self.shown = shown,
            Err(err) => log::warn!("showing the tray icon ({shown}) failed: {err:#}"),
        }
    }
}

/// A new tray icon; the app runs on without one it can't make.
fn new_tray(hotkey: Hotkey) -> Option<Tray> {
    Tray::new(hotkey)
        .inspect_err(|err| log::warn!("the tray icon is unavailable: {err:#}"))
        .ok()
}

/// A new taskbar button; the app runs on without one it can't make.
fn new_button() -> Option<TaskbarButton> {
    TaskbarButton::show()
        .inspect_err(|err| log::warn!("the taskbar button is unavailable: {err:#}"))
        .ok()
}

/// What the player asks through the tray icon or the taskbar button.
#[derive(Clone, Copy)]
enum Ask {
    Settings,
    Quit,
}

/// Answers the tray icon and the taskbar button for the app's whole run, whichever of them shows
/// and however often the player switches: a left click on the icon, its «Настройки» and a click
/// on the button open the settings (or bring them forward); its «Выход» and the button's «Закрыть
/// окно» quit. Call before the first icon exists: `muda` and `tray-icon` settle on their event
/// handlers for good with their first event.
fn serve_presence(cx: &mut App, app: &Entity<PriceCheckApp>) {
    let (ask, asks) = async_channel::unbounded();
    let menu_ask = ask.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let asked = if event.id == TRAY_SETTINGS {
            Ask::Settings
        } else if event.id == TRAY_QUIT {
            Ask::Quit
        } else {
            return;
        };
        let _ = menu_ask.try_send(asked);
    }));
    // A left click opens the settings once the button is let go; presses, double clicks and the
    // pointer's moves over the icon wake nothing.
    let icon_ask = ask.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            let _ = icon_ask.try_send(Ask::Settings);
        }
    }));
    let button_events = taskbar::events();
    cx.spawn(async move |_| {
        while let Ok(event) = button_events.recv().await {
            let asked = match event {
                ButtonEvent::Activated => Ask::Settings,
                ButtonEvent::Close => Ask::Quit,
            };
            if ask.send(asked).await.is_err() {
                return;
            }
        }
    })
    .detach();

    let app = app.downgrade();
    cx.spawn(async move |cx| {
        while let Ok(asked) = asks.recv().await {
            match asked {
                Ask::Quit => {
                    cx.update(quit);
                    return;
                }
                Ask::Settings => {
                    if let Some(app) = app.upgrade() {
                        cx.update(|cx| open_settings(&app, cx));
                    }
                }
            }
        }
    })
    .detach();
}

/// Quits the app -- from the tray, the taskbar button or the settings window's «Помощь», for the
/// installer (`instance`) or for an update. What the settings window's fields hold, typed but not
/// yet left, applies first (`SettingsView::apply_typed`), as the window's own close would apply
/// it: quitting ends GPUI's message loop without closing any window, so nothing else would.
pub fn quit(cx: &mut App) {
    for settings in cx
        .windows()
        .into_iter()
        .filter_map(|window| window.downcast::<SettingsView>())
    {
        if let Err(err) = settings.update(cx, |view, _, cx| view.apply_typed(cx)) {
            log::warn!("taking the settings window's typed text failed: {err:#}");
        }
    }
    cx.quit();
}

/// Opens the settings window (`ui::settings_view`) -- or brings the open one forward -- centred
/// on the monitor the game is on, as big as the UI scale makes its content. The price panel steps
/// aside while it's open: the price-check hotkey belongs to the window's recorder then, so no
/// check could bring the panel back, and one left up would cover part of the window. The setup
/// problems (`diagnostics::setup_problems`) head it.
///
/// Autostart is read from the registry first: that is where it lives -- the installer and Task
/// Manager change it too -- so the window shows what Windows will do; and the account's private
/// leagues are loaded again.
pub fn open_settings(app: &Entity<PriceCheckApp>, cx: &mut App) {
    if let Some(handle) = app.read(cx).settings_window()
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        log::info!("settings window brought forward");
        return;
    }
    // The player may have joined a private league since they last loaded: the league menus offer
    // the account's own.
    app.update(cx, |state, cx| {
        state.settings.autostart = autostart::autostart_enabled();
        state.refresh_private_leagues(false, cx);
    });
    let intro = Intro {
        problems: diagnostics::setup_problems(&game_config::read()),
    };
    // `gpui_windows` names a display by its monitor handle; one it doesn't list falls back to
    // the primary display.
    let display = game_window::game_monitor()
        .map(DisplayId::new)
        .filter(|&display| cx.find_display(display).is_some());
    let scale = app.read(cx).settings.ui_scale;
    let (width, height) = settings_view::WINDOW_SIZE;
    let (min_width, min_height) = settings_view::WINDOW_MIN_SIZE;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            display,
            size(px(width * scale), px(height * scale)),
            cx,
        ))),
        // Transparent: the view draws its own title bar and frame.
        titlebar: Some(TitlebarOptions {
            title: Some(settings_view::window_title().into()),
            appears_transparent: true,
            ..Default::default()
        }),
        kind: WindowKind::Normal,
        display_id: display,
        window_min_size: Some(size(px(min_width * scale), px(min_height * scale))),
        focus: true,
        show: true,
        ..Default::default()
    };
    // While the taskbar button shows, the window opens under it, without a button of its own.
    let opened = taskbar::under_button(|| {
        cx.open_window(options, |window, cx| {
            let app = app.clone();
            let view = cx.new(|cx| SettingsView::new(app, intro, window, cx));
            window.focus(&view.focus_handle(cx), cx);
            view
        })
    });
    match opened {
        Ok(handle) => {
            log::info!("settings window opened");
            app.update(cx, |state, cx| {
                state.set_settings_window(Some(handle.into()));
                state.visible = false;
                cx.notify();
            });
        }
        Err(err) => log::warn!("opening the settings window failed: {err:#}"),
    }
}

/// Opens the report window (`ui::report_view`) for `request` -- or brings the open one forward,
/// which takes the request over unless it's sending a report or saying why one didn't go
/// (`ReportView::take`) -- centred on the monitor the game is on, as big as the UI scale makes its
/// content. The price panel steps aside either way, as it does for the settings window: while
/// it's shown, its Esc hook takes the Esc the window's text box and the window itself answer.
pub fn open_report(app: &Entity<PriceCheckApp>, request: report::Request, cx: &mut App) {
    let open = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<ReportView>());
    if let Some(handle) = open
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        handle
            .update(cx, |view, window, cx| view.take(request, window, cx))
            .ok();
        log::info!("report window brought forward");
        hide_panel(app, cx);
        return;
    }
    // `gpui_windows` names a display by its monitor handle; one it doesn't list falls back to
    // the primary display.
    let display = game_window::game_monitor()
        .map(DisplayId::new)
        .filter(|&display| cx.find_display(display).is_some());
    let scale = app.read(cx).settings.ui_scale;
    let (width, height) = report_view::WINDOW_SIZE;
    let (min_width, min_height) = report_view::WINDOW_MIN_SIZE;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            display,
            size(px(width * scale), px(height * scale)),
            cx,
        ))),
        // Transparent: the view draws its own title bar and frame.
        titlebar: Some(TitlebarOptions {
            title: Some(report_view::window_title().into()),
            appears_transparent: true,
            ..Default::default()
        }),
        kind: WindowKind::Normal,
        display_id: display,
        window_min_size: Some(size(px(min_width * scale), px(min_height * scale))),
        focus: true,
        show: true,
        ..Default::default()
    };
    let opened = taskbar::under_button(|| {
        cx.open_window(options, |window, cx| {
            let app = app.clone();
            cx.new(|cx| ReportView::new(app, request, window, cx))
        })
    });
    match opened {
        Ok(_) => {
            log::info!("report window opened");
            hide_panel(app, cx);
        }
        Err(err) => log::warn!("opening the report window failed: {err:#}"),
    }
}

/// The price panel steps aside for the report window.
fn hide_panel(app: &Entity<PriceCheckApp>, cx: &mut App) {
    app.update(cx, |state, cx| {
        state.visible = false;
        cx.notify();
    });
}

/// Closes one of the app's own windows -- the settings window, the report window, the tour's.
/// `Window::remove_window` alone lets go of the window at once while `gpui_windows` hides and
/// destroys it only later (`Drop for WindowsWindow`: `ShowWindowAsync`, then `DestroyWindow` from
/// a task), so what Windows reports in between reaches a window GPUI no longer has, and GPUI logs
/// each report as «window not found» at error level. The one every close of an active window sets
/// off is its deactivation: `WM_ACTIVATE` comes synchronously with the hiding, but GPUI passes it
/// on from a task of its own (`events.rs`'s `handle_activate_msg`). So the window is hidden first,
/// while it's still GPUI's, and removed in a task spawned after that: GPUI runs foreground tasks
/// in the order they're spawned (`executor.rs`: "they run in order on the main thread"), so the
/// deactivation's report and the visibility's run first. Hidden and inactive, the window gets no
/// paint and no input until GPUI destroys it. Both outside the update this is called from:
/// `ShowWindow` sends its messages into GPUI's window procedure synchronously.
pub fn close_window(window: &Window, cx: &mut App) {
    let handle = window.window_handle();
    let overlay = Win32Overlay::from_window(window)
        .inspect_err(|err| log::warn!("closing a window without hiding it first: {err:#}"))
        .ok();
    cx.spawn(async move |cx| {
        if let Some(overlay) = overlay {
            overlay.set_shown(false);
        }
        cx.spawn(async move |cx| {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        })
        .detach();
    })
    .detach();
}

/// Starts the tour (`ui::tour`) once the catalogs are in (or failed) -- its first stop is the
/// settings window's league select, whose list comes with them -- and the welcome after an
/// install is gone (`ui::welcome`): the tour follows it, never shows under it.
fn tour_when_ready(app: &Entity<PriceCheckApp>, cx: &mut App) {
    let mut pending = true;
    cx.observe(app, move |app, cx| {
        if pending && !matches!(app.read(cx).bootstrap, BootstrapState::Loading) {
            pending = false;
            // Not from inside the notification that reported it.
            cx.defer(move |cx| welcome::after(cx, move |cx| tour::start(&app, cx)));
        }
    })
    .detach();
}

/// Serves what other processes ask through `instance`'s door: the installer's quit, a second
/// launch's knock -- the settings, with the welcome for one the installer's finish page started.
fn serve_instance_requests(
    cx: &mut App,
    app: &Entity<PriceCheckApp>,
    requests: async_channel::Receiver<Request>,
) {
    let app = app.downgrade();
    cx.spawn(async move |cx| {
        while let Ok(request) = requests.recv().await {
            match request {
                Request::Quit => {
                    log::info!("asked to quit");
                    cx.update(quit);
                    return;
                }
                Request::Knock(knock) => {
                    if let Some(app) = app.upgrade() {
                        cx.update(|cx| match knock {
                            Knock::Settings => open_settings(&app, cx),
                            Knock::Welcome => welcome::show(&app, cx),
                        });
                    }
                }
            }
        }
    })
    .detach();
}

/// Runs the app until the player quits: from the tray, the taskbar button or the settings.
pub fn run() {
    // Before any window exists: per-monitor-v2 DPI awareness, so every Win32 coordinate this app
    // reads or sets (game window, cursor, SetWindowPos) is in physical pixels and GPUI renders at
    // the monitor's real scale instead of being bitmap-stretched. Fails harmlessly if a manifest
    // already set the process's awareness.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    // Before the log: `logging::init` starts a new file, and a second copy must leave the running
    // one's log alone. A second copy asks the running one for what it was started for
    // (`Launch::knock`) and leaves.
    let launch = Launch::parse(std::env::args_os().skip(1));
    // An update's restart: the copy this one replaces is still quitting (`updates`).
    let closed_replaced = launch.after.is_some_and(instance::wait_for_exit);
    let requests = match instance::claim(launch.knock()) {
        Ok(None) => return,
        claimed => claimed,
    };
    logging::init();
    if closed_replaced {
        log::warn!("the copy this one replaces didn't quit in time and was closed");
    }
    // Before GPUI starts: its vsync thread loads the `RedrawWindow` import once, before its loop.
    redraw_filter::install();
    // Before anything reads a game table: the installed data pack's tables, when it's sound and
    // newer than the built-in ones (`data_pack`).
    crate::data_pack::activate();
    let requests = requests.unwrap_or_else(|err| {
        log::warn!("the single-copy check failed, running anyway: {err:#}");
        None
    });

    // The player's pathofexile.com session, from the Credential Manager: the HTTP client adds it
    // to the trade sites' requests, and to theirs only (`session`).
    let trade_session = session::load();
    // Built by reqwest_client's only constructor that takes a read timeout, which also verifies
    // certificates through Windows (rustls-platform-verifier, as Zed does) and offers no ALPN, so
    // it speaks HTTP/1.1. The trade sites, poe2scout, GGG's CDN and GitHub all answer it (checked
    // 2026-09-23). Its answers are read only through `SessionHttpClient`, which reads them the way
    // the read timeout needs.
    let inner_client: Arc<dyn HttpClient> = Arc::new(
        ReqwestClient::proxy_user_agent_and_read_timeout(None, USER_AGENT, Some(READ_TIMEOUT))
            .expect("failed to build HTTP client"),
    );
    application()
        .with_http_client(Arc::new(SessionHttpClient::new(
            inner_client.clone(),
            trade_session.clone(),
        )))
        .run(move |cx: &mut App| {
            if let Err(err) = fonts::register(cx) {
                log::warn!("nameplate fonts unavailable: {err:#}");
            }
            session::init(trade_session, inner_client, cx);
            let http_client: Arc<dyn HttpClient> = cx.http_client();
            let settings = settings::load();
            crate::i18n::apply(settings.interface_language);
            // Until the player finishes or skips it, the tour starts with every launch -- the
            // first one's introduction, after the install's welcome (`tour_when_ready`).
            let start_tour = !settings.tour_done;
            let inner = price_check::create_app(cx, http_client, settings);
            login::init(cx);
            if start_tour {
                tour_when_ready(&inner, cx);
            }

            price_check::register_hotkeys(cx, inner.clone())
                .expect("failed to set up the price-check hotkey and Esc hook");
            // Before any window opens: the settings and report windows open under the taskbar
            // button while it shows, and a failure to show either only costs that one.
            serve_presence(cx, &inner);
            let (app_icon, hotkey) = {
                let settings = &inner.read(cx).settings;
                (settings.app_icon, settings.hotkey)
            };
            let tray = app_icon.tray().then(|| new_tray(hotkey)).flatten();
            let taskbar = app_icon.taskbar().then(new_button).flatten();
            if let Some(requests) = requests {
                serve_instance_requests(cx, &inner, requests);
            }
            // The last run crashed: its report window opens by itself, the crash attached.
            if let Some(crash) = report::recent_crash() {
                open_report(&inner, report::Request::crash(crash), cx);
            }
            // Started by the installer's finish page: the settings open with the welcome over
            // them, which says the install worked and where the app lives from now on.
            if launch.installed {
                welcome::show(&inner, cx);
            }
            // What the last update left to say, and the updater itself: it connects a few
            // seconds on, while the player allows it.
            updates::init(&inner, cx);

            cx.open_window(build_window_options(), |window, cx| {
                window.set_window_title("PoE2 Oracle — Price Check");
                let inner_for_observe = inner.clone();
                cx.new(|cx| {
                    cx.observe(
                        &inner_for_observe,
                        |this: &mut PriceCheckRoot, _inner, cx| {
                            this.sync_window(cx);
                            cx.notify();
                        },
                    )
                    .detach();
                    // A click on the game takes the keyboard from a bound box the player typed
                    // in: the box lets go of it, like any field whose window loses the keyboard.
                    cx.observe_window_activation(
                        window,
                        |this: &mut PriceCheckRoot, window, cx| {
                            if !window.is_window_active() {
                                window.blur(cx);
                                this.inner.update(cx, |app, cx| app.end_bound_edit(cx));
                            }
                        },
                    )
                    .detach();
                    PriceCheckRoot {
                        inner,
                        overlay: None,
                        last_click_through: None,
                        last_bounds: None,
                        last_shown: None,
                        default_bounds: None,
                        default_bounds_scale: None,
                        was_visible: false,
                        tray,
                        taskbar,
                        app_icon,
                        presence_changing: false,
                        tray_words: None,
                        xp: None,
                        xp_opening: false,
                        last_xp_cover: None,
                    }
                })
            })
            .unwrap();
        });
}
