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
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use gpui::{
    App, Bounds, Context, DisplayId, Entity, Focusable, IntoElement, Render, Task, TitlebarOptions,
    Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, canvas, div,
    point, prelude::*, px, size,
};
use gpui_platform::application;
use http_client::HttpClient;
use reqwest_client::ReqwestClient;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::KillTimer;

use crate::brand;
use crate::check_profile::Spent;
use crate::diagnostics;
use crate::i18n::{self, Lang};
use crate::launch::{Knock, Launch};
use crate::logging;
use crate::login;
use crate::overlay_layout::{PanelResize, PanelWindow, PhysicalRect};
use crate::platform::instance::{self, Request};
use crate::platform::taskbar::{self, ButtonEvent, TaskbarButton};
use crate::platform::win32::Win32Overlay;
use crate::platform::{
    autostart, check_clock, d3d_threading, game_config, game_window, gpu_memory, paint_census,
    redraw_filter, vsync_park,
};
use crate::presence::{self, Shows, Step, Tries};
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

/// A server that goes quiet this long -- before its answer or in the middle of it -- fails the
/// request, and the caller's own retry or error takes over (the catalog load retries), instead of
/// "Loading…" for the rest of the run. Each part of the answer that arrives starts it anew, so a
/// long download that keeps coming -- an update's installer, an hour of the exchange's record
/// (2.7 MB) -- isn't cut short. Until the answer begins, though, it runs from the request's start,
/// an upload included: reports go through a client of their own (`report::send`).
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the price panel's paints all go through once it changed, was shown or the player
/// touched it -- then one a safety net till the next (`PANEL_SAFETY_NET`,
/// `Win32Overlay::gate_paints_for`): past its transitions, and its listings' icons coming in after
/// them. Left up while the player is elsewhere, it was drawn at every refresh of the display for
/// nothing new.
const PANEL_PAINT_BURST: Duration = Duration::from_secs(2);
/// How often the price panel's paint goes through outside its bursts: a tenth of the other gated
/// windows' safety net (`paint_gate::SAFETY_NET_MS`). A listing's tooltip draws its item's art as
/// it comes in, which tells the tooltip's own view and not the panel's, whose changes open its
/// bursts (`sync_window`): a picture that came in after a burst still shows within this and a
/// half. Hidden, the panel isn't asked for paints at all, and it shows only while it's used.
const PANEL_SAFETY_NET: Duration = Duration::from_millis(500);

/// Wraps `Entity<PriceCheckApp>` with the platform-window state that has to follow it: the
/// `Win32Overlay` handle (resolved once the real platform window exists), what was last applied to
/// it, and where the app shows itself -- the tray icon, the taskbar button.
struct PriceCheckRoot {
    inner: Entity<PriceCheckApp>,
    overlay: Option<Win32Overlay>,
    /// Last applied click-through bit; `None` until the first sync so it always applies once
    /// (`Win32Overlay` can't read the bit back).
    last_click_through: Option<bool>,
    /// The window's size and place: the panel's rect while shown, a pixel while hidden.
    size: PanelWindow,
    /// Last applied OS-window visibility; `None` until the first sync.
    last_shown: Option<bool>,
    /// EE2's inventory-side placement, used until the first check picks its own, and the UI
    /// scale it was sized for (the panel's width follows the scale).
    default_bounds: Option<PhysicalRect>,
    default_bounds_scale: Option<f32>,
    was_visible: bool,
    /// The tray icon and the taskbar button (`sync_presence`).
    presence: Presence,
    /// A change `sync_presence` makes is under way; how the last ones went, and the wait for the
    /// next try once one fell short.
    presence_changing: bool,
    presence_tries: Tries,
    presence_retry: Option<Task<()>>,
    /// The hotkey and the interface language the tray was last worded for; `None` until the first
    /// sync, and for a new tray.
    tray_words: Option<(Hotkey, Lang)>,
    /// The XP overlay, opened while the setting allows it (see `sync_xp`).
    xp: Option<Entity<XpOverlay>>,
    /// An overlay open is under way (the setting was just turned on), so it isn't started twice.
    xp_opening: bool,
    /// Last cover and options handed to the XP overlay; `None` until the first sync.
    last_xp_cover: Option<XpCover>,
    last_xp_options: Option<XpOverlayOptions>,
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
                // clicked into (`ui::panel::filters`). It paints only while it may be changing
                // (`gate_paints_for`): `sync_window` says when it does.
                cx.spawn(async move |_, _| {
                    if let Err(err) = overlay.remove_frame() {
                        log::warn!("{err:#}");
                    }
                    if let Err(err) = overlay.set_no_activate() {
                        log::warn!("{err:#}");
                    }
                    if let Err(err) = overlay.gate_paints_for(PANEL_PAINT_BURST, PANEL_SAFETY_NET) {
                        log::warn!("{err:#}");
                    }
                })
                .detach();
                // A price check under way times the panel's frames and resizes.
                overlay.time_for_checks();
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
        // Shown only when the player asked (`visible`): for an item, or for why there is none yet
        // (the catalog loading, or its first load failed -- `run_price_check`).
        let want_shown = visible;
        let shown = (self.last_shown != Some(want_shown)).then_some(want_shown);
        // In place before it shows, down to a pixel once hidden (`PanelWindow`).
        let (size, resize) = self.size.next(visible, placement.or(self.default_bounds));
        // Not while the settings window is what took over: it has the focus the player gave it.
        let focus_game = self.was_visible && !visible && !settings_open && overlay.is_foreground();

        self.last_click_through = Some(want_click_through);
        self.size = size;
        self.last_shown = Some(want_shown);
        self.was_visible = visible;

        // Each change of the panel's state, and each frame it draws, lets its paints through a
        // while longer: its transitions run, its icons come in, and then it's left alone.
        overlay.open_paints();

        if click_through.is_none() && resize.is_none() && shown.is_none() && !focus_game {
            return;
        }
        // One foreground task, applied in this order so the window is already in place and
        // clickable (or not) when it appears. Never applied synchronously: this runs inside
        // `render`/an `observe` callback, and `SetWindowPos`/`ShowWindow` send `WM_SIZE`/
        // `WM_SHOWWINDOW` synchronously into GPUI's own window state.
        cx.spawn(async move |this, cx| {
            if let Some(enabled) = click_through
                && let Err(err) = overlay.set_click_through(enabled)
            {
                log::warn!("set_click_through({enabled}) failed: {err:?}");
            }
            // Both timed for a price check under way: back from a pixel, placing makes a new swap
            // chain and textures, and showing draws GPUI's first frame (`WM_SHOWWINDOW`).
            if let Some(PanelResize::Place(rect)) = resize {
                let start = check_clock::now_thread();
                if let Err(err) = overlay.set_bounds(rect) {
                    log::warn!("{err:#}");
                }
                let end = check_clock::now_thread();
                check_clock::with(|check| check.placed(Spent::between(start, end)));
            }
            if let Some(shown) = shown {
                let start = check_clock::now_thread();
                overlay.set_shown(shown);
                let end = check_clock::now_thread();
                if shown {
                    check_clock::with(|check| check.shown(Spent::between(start, end), end));
                }
            }
            // Once out of sight: its one-pixel frame shows nothing.
            let shrunk = resize == Some(PanelResize::Shrink)
                && overlay
                    .shrink()
                    .inspect_err(|err| log::warn!("{err:#}"))
                    .is_ok();
            if focus_game {
                game_window::focus_game();
            }
            if shown == Some(false) {
                // Hidden, the check is over, what hiding cost included.
                check_clock::finish();
            }
            // What the shrink let go of is freed once the GPU is done with the pixel's frame
            // (`gpu_memory`) -- unless the panel is back by then.
            if shrunk {
                cx.background_executor()
                    .timer(gpu_memory::RELEASE_AFTER)
                    .await;
                let hidden = this
                    .read_with(cx, |root, _| matches!(root.size, PanelWindow::Shrunk(_)))
                    .unwrap_or(false);
                if hidden {
                    gpu_memory::release("price panel hidden");
                }
            }
        })
        .detach();
    }

    /// Shows the app where `Settings::app_icon` says -- the tray icon, the taskbar button or both
    /// -- once the player changes it (`Presence::settle`). Spawned, and all of it done outside any
    /// update: this runs inside `render`/an `observe` callback, and the button takes an open
    /// settings window over or gives it back (`TaskbarButton::show`), sending GPUI's procedure
    /// messages. A change made meanwhile follows once this one is done.
    ///
    /// What can't show is tried again after a wait (`presence::Tries`), and meanwhile what showed
    /// stays: the app never disappears.
    fn sync_presence(&mut self, cx: &mut Context<Self>) {
        let wanted = self.inner.read(cx).settings.app_icon;
        if self.presence_changing
            || self.presence.shows() == Shows::wanted(wanted)
            || !self.presence_tries.due(wanted)
        {
            return;
        }
        self.presence_changing = true;
        cx.spawn(async move |this, cx| {
            let Ok((mut presence, mut tries, hotkey)) = this.update(cx, |root, cx| {
                let hotkey = root.inner.read(cx).settings.hotkey;
                (
                    std::mem::take(&mut root.presence),
                    std::mem::take(&mut root.presence_tries),
                    hotkey,
                )
            }) else {
                return;
            };
            let shows = presence.settle(wanted, hotkey, &mut tries);
            let wait = tries.ended(wanted, shows);
            this.update(cx, |root, cx| {
                root.presence = presence;
                root.presence_tries = tries;
                root.presence_changing = false;
                root.presence_retry = wait.map(|wait| Self::retry_presence(wait, cx));
                root.tray_words = None;
                root.sync_presence(cx);
                root.sync_tray(cx);
            })
            .ok();
        })
        .detach();
    }

    /// `sync_presence` once `wait` is over: the wait after a change fell short (`Tries::ended`).
    fn retry_presence(wait: Duration, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            this.update(cx, |root, cx| {
                root.presence_retry = None;
                root.presence_tries.waited();
                root.sync_presence(cx);
            })
            .ok();
        })
    }

    /// Words the tray in the interface language and names the current hotkey in its tooltip --
    /// the only place it's shown besides the panel's own hint.
    fn sync_tray(&mut self, cx: &mut Context<Self>) {
        let words = (self.inner.read(cx).settings.hotkey, i18n::lang());
        if self.tray_words == Some(words) {
            return;
        }
        if let Some(tray) = &self.presence.tray {
            tray.word(words.0);
        }
        self.tray_words = Some(words);
    }

    /// Opens the XP overlay -- at the first sync, or once the setting is turned on -- and tells it
    /// what covers its plates: the setting off, or the price panel while it's shown. Turning the
    /// setting off only hides them; the sampling stops with the next launch, which never opens
    /// the overlay. The player's XP options follow it whenever they change.
    ///
    /// The overlay hears of each only once it changes: this runs as the panel draws, and an update
    /// of the overlay from there would count it among what the panel's window shows -- each of its
    /// redraws, the map clock's every sample in a map, would draw a frame of the panel.
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
                    root.last_xp_options = None;
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
            if self.last_xp_options != Some(options) {
                xp.update(cx, |xp, cx| xp.set_options(options, cx));
                self.last_xp_options = Some(options);
            }
        }
    }
}

impl Render for PriceCheckRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A frame of the panel: a price check under way counts it (`check_clock`).
        check_clock::rendered();
        self.ensure_overlay(window, cx);
        self.sync_window(cx);
        // The player's UI scale: the panel sizes its text and layout in rems, so all of it
        // follows.
        let scale = self.inner.read(cx).settings.ui_scale;
        window.set_rem_size(px(BASE_REM_SIZE * scale));
        // The panel's paint ends with this, the last of the window's root: a price check under way
        // splits the frame's CPU there (`check_clock::painted`).
        div()
            .size_full()
            .child(self.inner.clone())
            .child(canvas(|_, _, _| {}, |_, (), _, _| check_clock::painted()).absolute())
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

/// `tray-icon`'s timer on the icon's hidden window that looks every 15 ms whether the pointer has
/// left the icon, to report its leaving: its id in `tray-icon` 0.25 (`WM_USER_LEAVE_TIMER_ID`).
/// The crate sets it at each move of the pointer over the icon and stops it only from a look that
/// still has a move in hand, and its first look with the pointer still on the icon uses the move
/// up: a pointer that then leaves without another move over the icon -- up into the icon's menu,
/// which opens right above it -- leaves the timer waking the UI thread about 64 times a second for
/// the rest of the run. The app has no use for the leaving, so [`serve_presence`] stops the timer
/// at each move, right after the crate sets it.
const TRAY_LEAVE_TIMER: usize = 6007;

/// The tray icon's hidden window, for [`stop_tray_leave_timer`]; 0 until the icon exists. A run
/// makes one at most (`Presence::tray`).
static TRAY_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Stops [`TRAY_LEAVE_TIMER`]. On GPUI's main thread, which runs the tray's hidden window.
fn stop_tray_leave_timer() {
    let window = TRAY_WINDOW.load(Ordering::Relaxed);
    if window != 0 {
        // Fails harmlessly while no such timer runs.
        let _ = unsafe { KillTimer(Some(HWND(window as *mut _)), TRAY_LEAVE_TIMER) };
    }
}

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
        TRAY_WINDOW.store(icon.window_handle() as isize, Ordering::Relaxed);
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
    fn show(&mut self, shown: bool) -> anyhow::Result<()> {
        if self.shown != shown {
            self.icon.set_visible(shown)?;
            self.shown = shown;
        }
        Ok(())
    }
}

/// Where the app shows itself: its tray icon and its taskbar button.
#[derive(Default)]
struct Presence {
    /// Made the first time it's to show, and only hidden and shown again after that: Windows
    /// remembers whether the player keeps it by the clock or under the ^ arrow by the icon's
    /// number within the run, which a new icon wouldn't have.
    tray: Option<Tray>,
    /// While it shows.
    button: Option<TaskbarButton>,
}

impl Presence {
    fn shows(&self) -> Shows {
        Shows {
            tray: self.tray.as_ref().is_some_and(|tray| tray.shown),
            button: self.button.is_some(),
        }
    }

    /// Shows the app where `wanted` says, as far as it can (`presence::settle`), and gives what
    /// shows then. On GPUI's main thread and, once its windows are open, outside any update
    /// (`TaskbarButton::show`). Each kind of failure is logged once, not with every try
    /// (`Tries::news`).
    fn settle(&mut self, wanted: AppIcon, hotkey: Hotkey, tries: &mut Tries) -> Shows {
        presence::settle(wanted, self.shows(), |step| {
            let taken = self.take(step, hotkey);
            if tries.news(step, taken.is_ok()) {
                match &taken {
                    Ok(()) => log::info!("{step} worked after all"),
                    Err(err) => log::warn!("{step} failed: {err:#}"),
                }
            }
            taken.is_ok()
        })
    }

    /// Takes `step`; a new tray icon names `hotkey` in its tooltip.
    fn take(&mut self, step: Step, hotkey: Hotkey) -> anyhow::Result<()> {
        match step {
            Step::ShowTray => match &mut self.tray {
                Some(tray) => tray.show(true)?,
                None => self.tray = Some(Tray::new(hotkey)?),
            },
            Step::ShowButton => self.button = Some(TaskbarButton::show()?),
            Step::HideTray => {
                if let Some(tray) = &mut self.tray {
                    tray.show(false)?;
                }
            }
            Step::RemoveButton => {
                if let Some(button) = self.button.take() {
                    button.remove();
                }
            }
        }
        Ok(())
    }
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
    // pointer's moves over the icon wake nothing -- each move's timer stopped as it's set
    // (`TRAY_LEAVE_TIMER`), the handler called right after, on the same thread.
    let icon_ask = ask.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } => {
            let _ = icon_ask.try_send(Ask::Settings);
        }
        TrayIconEvent::Move { .. } => stop_tray_leave_timer(),
        _ => {}
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
            // GPUI destroys the window from a task of its own; what it let go of is freed once
            // the device flushes (`gpu_memory`).
            cx.background_executor()
                .timer(gpu_memory::RELEASE_AFTER)
                .await;
            gpu_memory::release("window closed");
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
    // Before GPUI starts: its vsync thread loads the `RedrawWindow` and `DwmFlush` imports once,
    // before its loop. The park on this thread, GPUI's UI thread, whose windows' shows wake it.
    redraw_filter::install();
    vsync_park::install();
    // Before GPUI starts too: it makes its Direct3D device while the application is built, and
    // that device, like the lip watcher's, goes without the graphics driver's threads.
    d3d_threading::install();
    // The clocks price checks are timed on: the counter's rate is measured from here.
    check_clock::start();
    // On this thread, GPUI's UI thread, and only with `POE2_ORACLE_PAINT_CENSUS=1`.
    paint_census::start();
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
        ReqwestClient::proxy_user_agent_and_read_timeout(
            None,
            crate::brand::USER_AGENT,
            Some(READ_TIMEOUT),
        )
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

            // Before the app holds a hotkey of its own: whatever holds a combination the quick
            // actions press is another program. Быстрые действия warns of it when opened.
            let actions = &inner.read(cx).settings.quick_actions;
            let combos = crate::quick_action::combos(actions);
            for key in crate::platform::synth_input::taken_combos(combos, &[]) {
                log::warn!(
                    "{}, which quick actions press, is taken by another program",
                    key.label()
                );
            }
            price_check::register_hotkeys(cx, inner.clone())
                .expect("failed to set up the price-check hotkey and Esc hook");
            // Before any window opens: the settings and report windows open under the taskbar
            // button while it shows. When the player's choice can't show, the other one stands in
            // until a later try shows it (`sync_presence`).
            serve_presence(cx, &inner);
            let (app_icon, hotkey) = {
                let settings = &inner.read(cx).settings;
                (settings.app_icon, settings.hotkey)
            };
            let mut presence = Presence::default();
            let mut presence_tries = Tries::default();
            let shows = presence.settle(app_icon, hotkey, &mut presence_tries);
            let presence_wait = presence_tries.ended(app_icon, shows);
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
                    let presence_retry =
                        presence_wait.map(|wait| PriceCheckRoot::retry_presence(wait, cx));
                    PriceCheckRoot {
                        inner,
                        overlay: None,
                        last_click_through: None,
                        size: PanelWindow::Opened,
                        last_shown: None,
                        default_bounds: None,
                        default_bounds_scale: None,
                        was_visible: false,
                        presence,
                        presence_changing: false,
                        presence_tries,
                        presence_retry,
                        tray_words: None,
                        xp: None,
                        xp_opening: false,
                        last_xp_cover: None,
                        last_xp_options: None,
                    }
                })
            })
            .unwrap();
        });
}
