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
//! The tray icon is the app's only visible "running" signal, and its menu opens the settings and
//! quits: a `PopUp` window has no taskbar button.

use std::sync::Arc;

use gpui::{
    App, Bounds, Context, DisplayId, Entity, Focusable, IntoElement, Render, TitlebarOptions,
    Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point,
    prelude::*, px, size,
};
use gpui_platform::application;
use http_client::HttpClient;
use reqwest_client::ReqwestClient;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

use crate::brand;
use crate::bug_report;
use crate::diagnostics;
use crate::live_search::{self, LiveCard, LiveSearches};
use crate::logging;
use crate::login;
use crate::overlay_layout::PhysicalRect;
use crate::paths;
use crate::platform::instance::{self, Request};
use crate::platform::win32::Win32Overlay;
use crate::platform::{autostart, game_config, game_window};
use crate::price_check::{self, BootstrapState, PriceCheckApp};
use crate::session::{self, SessionHttpClient};
use crate::settings::{self, Hotkey};
use crate::ui::fonts;
use crate::ui::settings_view::{Intro, SettingsView};
use crate::ui::theme::BASE_REM_SIZE;
use crate::ui::trade_overlay::{self, TradeOverlay, TradeOverlayOptions};
use crate::ui::xp_overlay::{self, XpOverlay, XpOverlayOptions};
use crate::updates::Updates;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Wraps `Entity<PriceCheckApp>` with the platform-window state that has to follow it: the
/// `Win32Overlay` handle (resolved once the real platform window exists), what was last applied to
/// it, and the tray icon.
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
    tray: TrayIcon,
    /// The hotkey the tray tooltip names; `None` until the first sync.
    tray_hotkey: Option<Hotkey>,
    /// The XP overlay, opened while the setting allows it (see `sync_xp`).
    xp: Option<Entity<XpOverlay>>,
    /// An overlay open is under way (the setting was just turned on), so it isn't started twice.
    xp_opening: bool,
    /// Last suppression handed to the XP overlay; `None` until the first sync.
    last_xp_suppressed: Option<bool>,
    /// The trade overlay, opened once a search is watched (see `sync_trade`).
    trade: Option<Entity<TradeOverlay>>,
    trade_opening: bool,
    /// Last suppression handed to the trade overlay; `None` until the first sync.
    last_trade_suppressed: Option<bool>,
    /// Live search's cards, which the trade overlay shows (`live_search::init`).
    live_cards: async_channel::Receiver<LiveCard>,
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
        self.sync_tray(cx);
        self.sync_xp(cx);
        self.sync_trade(cx);
        let Some(overlay) = self.overlay else {
            return;
        };
        let (visible, placement, settings_open, ui_scale) = {
            let state = self.inner.read(cx);
            (
                state.visible,
                state.placement,
                state.settings_window().is_some(),
                state.settings.ui_scale,
            )
        };
        if self.default_bounds_scale != Some(ui_scale) {
            self.default_bounds = game_window::default_panel_rect(ui_scale);
            self.default_bounds_scale = Some(ui_scale);
        }

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

    /// Names the current hotkey in the tray tooltip -- the only place it's shown besides the
    /// panel's own hint.
    fn sync_tray(&mut self, cx: &mut Context<Self>) {
        let hotkey = self.inner.read(cx).settings.hotkey;
        if self.tray_hotkey == Some(hotkey) {
            return;
        }
        if let Err(err) = self.tray.set_tooltip(Some(tray_tooltip(hotkey))) {
            log::warn!("{err:#}");
        }
        self.tray_hotkey = Some(hotkey);
    }

    /// Hides the XP overlay while the price window is shown (the panel spans the bar's middle) or
    /// the setting is off, and opens it -- at the first sync, or once the setting is turned on.
    /// Turning it off only hides it; its sampling stops with the next launch, which never opens it.
    /// The player's XP options follow it whenever they change.
    fn sync_xp(&mut self, cx: &mut Context<Self>) {
        let (enabled, price_shown, options) = {
            let state = self.inner.read(cx);
            (
                state.settings.xp_overlay,
                state.visible,
                XpOverlayOptions::from_settings(&state.settings),
            )
        };
        if enabled && self.xp.is_none() && !self.xp_opening {
            // Spawned: this runs from `render`, where no window may be opened.
            self.xp_opening = true;
            cx.spawn(async move |this, cx| {
                let opened = cx.update(|cx| xp_overlay::open(options, cx));
                this.update(cx, |root, cx| {
                    root.xp_opening = false;
                    match opened {
                        Ok(xp) => root.xp = Some(xp),
                        Err(err) => log::warn!("the XP overlay is unavailable: {err:#}"),
                    }
                    root.last_xp_suppressed = None;
                    root.sync_xp(cx);
                })
                .ok();
            })
            .detach();
        }
        let suppressed = price_shown || !enabled;
        if let Some(xp) = &self.xp {
            if self.last_xp_suppressed != Some(suppressed) {
                xp.update(cx, |xp, cx| xp.set_suppressed(suppressed, cx));
                self.last_xp_suppressed = Some(suppressed);
            }
            xp.update(cx, |xp, cx| xp.set_options(options, cx));
        }
    }

    /// Opens the trade overlay once a search is watched -- live search's cards show in it -- and
    /// hides it while the price window is shown. The player's options follow it whenever they
    /// change.
    fn sync_trade(&mut self, cx: &mut Context<Self>) {
        let (price_shown, options) = {
            let state = self.inner.read(cx);
            (
                state.visible,
                TradeOverlayOptions::from_settings(&state.settings),
            )
        };
        let watching = cx
            .try_global::<LiveSearches>()
            .is_some_and(|live| live.count() > 0);
        if watching && self.trade.is_none() && !self.trade_opening {
            // Spawned: this runs from `render`, where no window may be opened.
            self.trade_opening = true;
            let app = self.inner.downgrade();
            let live_cards = self.live_cards.clone();
            cx.spawn(async move |this, cx| {
                let opened = cx.update(|cx| trade_overlay::open(options, app, live_cards, cx));
                this.update(cx, |root, cx| {
                    root.trade_opening = false;
                    match opened {
                        Ok(trade) => root.trade = Some(trade),
                        Err(err) => log::warn!("the trade overlay is unavailable: {err:#}"),
                    }
                    root.last_trade_suppressed = None;
                    root.sync_trade(cx);
                })
                .ok();
            })
            .detach();
        }
        if let Some(trade) = &self.trade {
            if self.last_trade_suppressed != Some(price_shown) {
                trade.update(cx, |trade, cx| trade.set_suppressed(price_shown, cx));
                self.last_trade_suppressed = Some(price_shown);
            }
            trade.update(cx, |trade, cx| trade.set_options(options, cx));
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
    format!("PoE2 Oracle — проверка цены: {hotkey}")
}

/// Notification-area icon with "Настройки", the update entry (`crate::updates`), "Сообщить об
/// ошибке" (`bug_report::report_bug`) and "Выход".
/// Created on GPUI's main thread, whose message loop also drives the tray's hidden window; menu
/// clicks come through `MenuEvent`'s handler into a channel a task here awaits.
fn build_tray(cx: &mut App, app: &Entity<PriceCheckApp>) -> anyhow::Result<TrayIcon> {
    // Before the menu exists: `muda` settles on its channel for good with the first event.
    let (click_tx, clicks) = async_channel::unbounded();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = click_tx.try_send(event);
    }));
    let open_settings_item = MenuItem::new("Настройки", true, None);
    let settings_id = open_settings_item.id().clone();
    let update_item = Updates::menu_item();
    let update_id = update_item.id().clone();
    let report_item = MenuItem::new("Сообщить об ошибке", true, None);
    let report_id = report_item.id().clone();
    let quit = MenuItem::new("Выход", true, None);
    let quit_id = quit.id().clone();
    let menu = Menu::new();
    menu.append(&open_settings_item)?;
    menu.append(&update_item)?;
    menu.append(&report_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit)?;
    let tray = TrayIconBuilder::new()
        .with_tooltip(tray_tooltip(app.read(cx).settings.hotkey))
        .with_icon(tray_icon_image())
        .with_menu(Box::new(menu))
        .build()?;

    let updates = Updates::new(update_item, cx.http_client());
    let check_after_launch = app.read(cx).settings.check_updates;
    let app = app.downgrade();
    cx.spawn(async move |cx| {
        if check_after_launch {
            updates.check_after_launch(cx);
        }
        while let Ok(event) = clicks.recv().await {
            if event.id == quit_id {
                cx.update(|cx| cx.quit());
                return;
            }
            if event.id == settings_id
                && let Some(app) = app.upgrade()
            {
                cx.update(|cx| open_settings(&app, false, cx));
            }
            if event.id == update_id {
                updates.clicked(cx);
            }
            if event.id == report_id
                && let Some(app) = app.upgrade()
            {
                cx.update(|cx| {
                    let (summary, language) = {
                        let state = app.read(cx);
                        (state.diagnostics_summary(), state.item_language())
                    };
                    bug_report::report_bug(summary, language, cx);
                });
            }
        }
    })
    .detach();

    Ok(tray)
}

/// The settings window's size, logical px, as the owner approved it on the style mockup; and
/// the least it can be resized to.
const SETTINGS_SIZE: (f32, f32) = (1100., 720.);
const SETTINGS_MIN_SIZE: (f32, f32) = (900., 600.);

/// Opens the settings window (`ui::settings_view`) -- or brings the open one forward -- centred
/// on the monitor the game is on. The price panel steps aside while it's open: the price-check
/// hotkey belongs to the window's recorder then, so no check could bring the panel back, and one
/// left up would cover part of the window. `welcome` heads it with the first-launch welcome; the
/// setup problems (`diagnostics::setup_problems`) head it always.
///
/// Autostart is read from the registry first: that is where it lives -- the installer and Task
/// Manager change it too -- so the window shows what Windows will do.
pub fn open_settings(app: &Entity<PriceCheckApp>, welcome: bool, cx: &mut App) {
    if let Some(handle) = app.read(cx).settings_window()
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        log::info!("settings window brought forward");
        return;
    }
    app.update(cx, |state, _| {
        state.settings.autostart = autostart::autostart_enabled();
    });
    let intro = Intro {
        welcome,
        problems: diagnostics::setup_problems(&game_config::read()),
    };
    // `gpui_windows` names a display by its monitor handle; one it doesn't list falls back to
    // the primary display.
    let display = game_window::game_monitor()
        .map(DisplayId::new)
        .filter(|&display| cx.find_display(display).is_some());
    let (width, height) = SETTINGS_SIZE;
    let (min_width, min_height) = SETTINGS_MIN_SIZE;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            display,
            size(px(width), px(height)),
            cx,
        ))),
        // Transparent: the view draws its own title bar and frame.
        titlebar: Some(TitlebarOptions {
            title: Some("PoE2 Oracle — настройки".into()),
            appears_transparent: true,
            ..Default::default()
        }),
        kind: WindowKind::Normal,
        display_id: display,
        window_min_size: Some(size(px(min_width), px(min_height))),
        focus: true,
        show: true,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        let app = app.clone();
        let view = cx.new(|cx| SettingsView::new(app, intro, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
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

/// Opens the settings window with the welcome once the catalogs are in (or failed): its league
/// list comes with them.
fn welcome_when_ready(app: &Entity<PriceCheckApp>, cx: &mut App) {
    let mut pending = true;
    cx.observe(app, move |app, cx| {
        if pending && !matches!(app.read(cx).bootstrap, BootstrapState::Loading) {
            pending = false;
            // Not from inside the notification that reported it.
            cx.defer(move |cx| open_settings(&app, true, cx));
        }
    })
    .detach();
}

/// Serves what other processes ask through `instance`'s door: the installer's quit, a second
/// launch's settings.
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
                    cx.update(|cx| cx.quit());
                    return;
                }
                Request::ShowSettings => {
                    if let Some(app) = app.upgrade() {
                        cx.update(|cx| open_settings(&app, false, cx));
                    }
                }
            }
        }
    })
    .detach();
}

/// Runs the app until the player quits from the tray.
pub fn run() {
    // Before any window exists: per-monitor-v2 DPI awareness, so every Win32 coordinate this app
    // reads or sets (game window, cursor, SetWindowPos) is in physical pixels and GPUI renders at
    // the monitor's real scale instead of being bitmap-stretched. Fails harmlessly if a manifest
    // already set the process's awareness.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    // Before the log: `logging::init` starts a new file, and a second copy must leave the running
    // one's log alone. A second copy started by autostart leaves quietly; one the player started
    // has the running copy open its settings.
    let autostarted = std::env::args().any(|arg| arg == autostart::AUTOSTART_ARG);
    let requests = match instance::claim(autostarted) {
        Ok(None) => return,
        claimed => claimed,
    };
    logging::init();
    let requests = requests.unwrap_or_else(|err| {
        log::warn!("the single-copy check failed, running anyway: {err:#}");
        None
    });

    // The player's pathofexile.com session, from the Credential Manager: the HTTP client adds it
    // to the trade sites' requests, and to theirs only (`session`).
    let trade_session = session::load();
    let inner_client: Arc<dyn HttpClient> =
        Arc::new(ReqwestClient::user_agent(USER_AGENT).expect("failed to build HTTP client"));
    application()
        .with_http_client(Arc::new(SessionHttpClient::new(
            inner_client.clone(),
            trade_session.clone(),
        )))
        .run(|cx: &mut App| {
            if let Err(err) = fonts::register(cx) {
                log::warn!("nameplate fonts unavailable: {err:#}");
            }
            session::init(trade_session, inner_client, cx);
            let http_client: Arc<dyn HttpClient> = cx.http_client();
            // No settings file yet: the first launch. Its defaults are saved at once, so the
            // welcome shows this once only.
            let first_launch = paths::settings_file().is_some_and(|path| !path.exists());
            let settings = settings::load();
            crate::i18n::apply(settings.interface_language);
            if first_launch && let Err(err) = settings::save(&settings) {
                log::warn!("saving the first settings failed: {err:#}");
            }
            let inner = price_check::create_app(cx, http_client, settings);
            login::init(&inner, cx);
            let live_cards = live_search::init(&inner, USER_AGENT, cx);
            if first_launch {
                welcome_when_ready(&inner, cx);
            }

            price_check::register_hotkeys(cx, inner.clone())
                .expect("failed to set up the price-check hotkey and Esc hook");
            let tray = build_tray(cx, &inner).expect("failed to create the tray icon");
            if let Some(requests) = requests {
                serve_instance_requests(cx, &inner, requests);
            }

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
                        tray_hotkey: None,
                        xp: None,
                        xp_opening: false,
                        last_xp_suppressed: None,
                        trade: None,
                        trade_opening: false,
                        last_trade_suppressed: None,
                        live_cards,
                    }
                })
            })
            .unwrap();
        });
}
