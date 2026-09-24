//! The XP overlay: two plates set on the game's HUD, on top of the rails of the panels either
//! side of the experience bar (`overlay_layout::hud_rails`). The rails themselves are the game's
//! gauges -- rage and stun fill them -- so the plates stand on them, never over them, built of the
//! rails' own materials so they read as part of the game's interface:
//!
//! - over the flask panel: how much of the level is earned, how fast the character levels and
//!   how much play is left to the next level (`crate::xp_tracker`) -- `64,8 % ◆ +12,4 %/ч · до
//!   75 ур. 2 ч 50 мин` -- and, at its right end, the gear that opens the settings. In a pause (a
//!   town or hideout, or five minutes without a gain) the rate and the time to level would pass
//!   for current ones, so the plate dims and says only how much of the level is earned: `64,8 %`;
//! - over the skill panel, while the player keeps the map timer on and there is a run to show:
//!   the map's time and experience next to the average map time -- `карта 4:07 +1,2 % · ср.
//!   6:30`, and `последняя карта 9:00 +3,66 %`, dimmed, once the run is over.
//!
//! A plate says as much as fits it, measured in its own typeface: the full wording, else the
//! shorter one (`xp_tracker::Wording`), else -- the level plate -- the shorter one without the
//! percent; once shortened, it stays so while its state lasts, not flipping as a number gains or
//! loses a digit. The words are the interface language's: `64.8% ◆ +12.4%/h · level 75 in 2h
//! 50m` in English.
//!
//! A task samples every two seconds, off the UI thread: the game log's new lines
//! (`platform::client_log`), then the bar's pixels and the rails' lips (`platform::xp_bar`) -- in
//! that order, so a level-up line is in before the wrap on the bar it explains. A plate shows
//! while its rail is seen where it goes (`overlay_layout::rail_seen`): not over a tooltip, a
//! loading screen, a full-screen panel, another program, or a HUD laid out otherwise. While the
//! player is at the game -- it's in front, or the cursor is over it -- the lips are watched frame
//! by frame (`platform::lip_watch`), so a plate steps aside the moment a tooltip covers its rail
//! and comes back the moment it goes, and the bar is read off the same frames every half second:
//! the sampler takes that reading, and reads no pixels itself. Otherwise the sampler's look
//! decides, a single miss let pass. The price-check panel hides only a plate it
//! covers, and the setting both ([`XpOverlay::set_cover`]). Their size is the game's, not the
//! app's interface scale: at any game height a plate is its rail's width, and its words the
//! HUD's.
//!
//! Each plate is its own opaque window (a transparent `PopUp` background still tints the game
//! behind it, see `Win32Overlay::set_shown`) that lets clicks through to the game -- the plates
//! stand over the game's world. The gear is a window of its own at the level plate's right end,
//! the one place that takes a click, and never the keyboard. The plates are drawn pixel by pixel
//! in the HUD's own materials (`crate::plate_art`): the rails' cap molding along the top, a dark
//! face, a thin seam where they sit on the rail; at its globe a plate runs on over the world to
//! the globe's frame, and at its other end it curls down onto the tip of the game's volute. Each
//! window shows its part of that art and is shaped to exactly its pixels (a window region), so
//! nothing of the world shows between a plate and the HUD, and nothing of the HUD is covered.
//! The values are in the HUD's cream, the words saying what they are muted, the rate in its
//! gold, a small diamond between the parts.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, AsyncApp, Bounds, Context, Div, Entity, Font, Global, Hsla, ImageSource, IntoElement,
    MouseButton, MouseDownEvent, ObjectFit, Pixels, Render, RenderImage, SharedString, TextRun,
    WeakEntity, Window, WindowBounds, WindowKind, WindowOptions, div, img, point, prelude::*, px,
    rgb, size,
};
use image::Frame;
use windows::Win32::System::SystemInformation::GetTickCount64;

use crate::i18n::{self, Lang};
use crate::overlay_layout::{PhysicalRect, hud_rails};
use crate::plate_art::{self, ArtSlice, PlateArt};
use crate::platform::client_log::{self, ClientLog};
use crate::platform::lip_watch::{LipReport, LipWatch};
use crate::platform::win32::Win32Overlay;
use crate::platform::xp_bar::{self, BarSample, RailsSeen};
use crate::price_check::PriceCheckApp;
use crate::settings::Settings;
use crate::ui::fonts;
use crate::ui::style::{diamond, ease_hover, ease_state};
use crate::ui::theme::{
    BASE_REM_SIZE, HUD_DIVIDER, HUD_GOLD, HUD_LABEL, HUD_TEXT, blend, rems_from_px,
};
use crate::xp_tracker::{
    Activity, MapStatus, RunState, Word, Wording, XpStatus, XpTracker, level_parts, log_time,
    map_words, parse_log_line, parse_timed_log_line,
};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

// The plates' lengths are in HUD pixels: a 1080-row game's pixels, which the plates' rem size
// turns into the game's own at its height (`hud_rem_size`).
const HUD_UNIT_HEIGHT: f64 = 1080.0;
/// The words' size: about the HUD's own small text, the charm counts beside the flasks.
const TEXT_SIZE: f32 = 12.;
/// Room between a plate's ends and its words.
const PADDING_X: f32 = 7.;
/// The space between a part's words, and between the parts and the diamond between them; the
/// diamond's size.
const WORD_GAP: f32 = 3.;
const PART_GAP: f32 = 5.;
const SEPARATOR_DIAMOND: f32 = 4.;
/// The gear glyph's size: Segoe UI Symbol draws its gear thin, so a size up on the words.
const GEAR_SIZE: f32 = 14.;
/// The cap molding's height: the art's rows of it (`plate_art::CAP`, a 2160-row game's) halved.
/// The words sit on the face under it.
const CAP_HEIGHT: f32 = plate_art::CAP.len() as f32 / 2.;
/// The line between the level plate's words and its gear.
const DIVIDER: f32 = 0.5;

/// What the plates show, from the player's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XpOverlayOptions {
    pub show_percent: bool,
    pub map_timer: bool,
    pub rate_window_minutes: u16,
}

impl XpOverlayOptions {
    pub fn from_settings(settings: &Settings) -> XpOverlayOptions {
        XpOverlayOptions {
            show_percent: settings.xp_show_percent,
            map_timer: settings.xp_map_timer,
            rate_window_minutes: settings.xp_rate_window_minutes,
        }
    }
}

/// What keeps the plates off the screen besides the game: the setting turned off, and the price
/// panel while it is shown -- its rect then -- which hides a plate it covers.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct XpCover {
    pub off: bool,
    pub panel: Option<PhysicalRect>,
}

/// Where the level plate is on screen now, its gear included, in physical pixels -- `None` while
/// it is hidden: the tour (`ui::tour`) points its spotlight at it.
#[derive(Clone, Copy, Default)]
pub struct XpLineOnScreen(pub Option<PhysicalRect>);

impl Global for XpLineOnScreen {}

#[derive(Clone, Copy)]
enum Plate {
    Level,
    Gear,
    Map,
}

/// A plate's platform window and what was last applied to it.
#[derive(Default)]
struct PlateWindow {
    /// Resolved on the window's first render, when the platform window exists.
    overlay: Option<Win32Overlay>,
    /// `None` until the first sync.
    last_bounds: Option<PhysicalRect>,
    last_region: Option<Vec<PhysicalRect>>,
    last_shown: Option<bool>,
}

impl PlateWindow {
    /// Moves the window to its part of the art, shaped to it, and shows it, or hides it for
    /// `None`; returns whether anything changed. Deferred: `SetWindowRgn`/`SetWindowPos`/
    /// `ShowWindow` send `WM_WINDOWPOSCHANGED`/`WM_SIZE`/`WM_SHOWWINDOW` synchronously into
    /// GPUI's own window state. The shape goes first, so a window never shows unshaped.
    fn sync<T: 'static>(&mut self, slice: Option<&Slice>, cx: &mut Context<T>) -> bool {
        let Some(overlay) = self.overlay else {
            return false;
        };
        let visible = slice.is_some();
        let (bounds, region) = match slice {
            Some(slice) => (
                Some(slice.rect).filter(|rect| self.last_bounds != Some(*rect)),
                Some(&slice.shown)
                    .filter(|shown| self.last_region.as_ref() != Some(*shown))
                    .cloned(),
            ),
            None => (None, None),
        };
        let shown = (self.last_shown != Some(visible)).then_some(visible);
        if bounds.is_none() && region.is_none() && shown.is_none() {
            return false;
        }
        if bounds.is_some() {
            self.last_bounds = bounds;
        }
        if region.is_some() {
            self.last_region.clone_from(&region);
        }
        self.last_shown = Some(visible);
        cx.spawn(async move |_, _| {
            if let Some(region) = region
                && let Err(err) = overlay.set_region(&region)
            {
                log::warn!("{err:#}");
            }
            if let Some(rect) = bounds
                && let Err(err) = overlay.set_bounds(rect)
            {
                log::warn!("{err:#}");
            }
            if let Some(shown) = shown {
                overlay.set_shown(shown);
            }
        })
        .detach();
        true
    }
}

/// The plates' pixels (`plate_art`), drawn for one place and size of the game and cut into their
/// windows' parts: the level plate's from the life globe's frame to its gear, the gear's with the
/// flask plate's inner end, and the map plate's whole.
struct Arts {
    game: PhysicalRect,
    level: Slice,
    gear: Slice,
    map: Slice,
    /// The flask plate's whole art, gear and ends included: what the tour spotlights.
    line: PhysicalRect,
}

/// A plate window's part of the art: where the window goes, what of it shows, and its pixels.
struct Slice {
    rect: PhysicalRect,
    shown: Vec<PhysicalRect>,
    image: Arc<RenderImage>,
}

impl Slice {
    fn cut(art: &PlateArt, rect: PhysicalRect) -> Slice {
        let ArtSlice { image, shown } = art.slice(rect);
        Slice {
            rect,
            shown,
            image: Arc::new(RenderImage::new([Frame::new(image)])),
        }
    }
}

impl Arts {
    fn draw(game: PhysicalRect) -> Arts {
        let rails = hud_rails(game);
        let flask = PlateArt::flask(&rails, game.height);
        let skill = PlateArt::skill(&rails, game.height);
        let (_, gear) = split_gear(rails.flask);
        let line = flask.bounds;
        let level = PhysicalRect {
            width: gear.x - line.x,
            height: rails.flask.height,
            ..line
        };
        let gear = PhysicalRect {
            x: gear.x,
            width: line.x + line.width - gear.x,
            ..line
        };
        Arts {
            game,
            level: Slice::cut(&flask, level),
            gear: Slice::cut(&flask, gear),
            map: Slice::cut(&skill, skill.bounds),
            line,
        }
    }

    /// Takes the pixels out of every window's sprite atlas, once they are drawn no more.
    fn drop_images(self, cx: &mut App) {
        for slice in [self.level, self.gear, self.map] {
            cx.drop_image(slice.image, None);
        }
    }
}

/// The level plate's window's root view, and the overlay's state: the tracker, what it says, and
/// the plates' platform windows.
pub struct XpOverlay {
    tracker: XpTracker,
    status: XpStatus,
    options: XpOverlayOptions,
    /// The latest look at the game: where its HUD is, and at what DPI.
    sample: Option<BarSample>,
    cover: XpCover,
    /// For the gear, which opens the app's settings.
    app: WeakEntity<PriceCheckApp>,
    level: PlateWindow,
    gear: PlateWindow,
    map: PlateWindow,
    /// The plates' pixels, for the place and size of the game they were drawn for.
    art: Option<Arts>,
    /// The level plate's wording: paused, rated, with the percent, in what language and room.
    level_fit: Fit<(bool, bool, bool, Lang, Pixels)>,
    /// The sampler's look at the rails, every two seconds.
    rails: RailPresence,
    /// The frame-by-frame look at the rails and the bar while the player is at the game
    /// (`platform::lip_watch`): its handle, `None` if its thread couldn't start; the game it was
    /// last given; and what it saw last -- the rails, `None` while it isn't watching, and the
    /// bar's fill, which the sampler takes instead of reading the screen itself.
    lip_watch: Option<LipWatch>,
    watched: Option<PhysicalRect>,
    lips: Option<RailsSeen>,
    watched_fill: Option<f64>,
}

/// The gear's window, at the level plate's right end: what the [`XpOverlay`] knows decides where
/// it is and how big.
pub struct GearPlate {
    xp: Entity<XpOverlay>,
    attached: bool,
}

/// The map plate's window's root view: it shows what the [`XpOverlay`] knows.
pub struct MapPlate {
    xp: Entity<XpOverlay>,
    attached: bool,
    /// The plate's wording: for which state of the run, with or without the average, in what
    /// language and room.
    fit: Fit<(RunState, bool, Lang, Pixels)>,
}

/// What the plates say ([`XpOverlay::shown`]): a new sample redraws them only when it changes.
#[derive(PartialEq)]
struct Shown {
    level: Vec<Vec<Vec<Word>>>,
    playing: bool,
    map: Option<(Vec<Vec<Vec<Word>>>, RunState)>,
}

/// Which of a plate's wordings -- longest first -- it shows, and in what state. The choice sticks
/// while the plate says the same kind of thing: once it had to shorten its words it keeps the
/// shorter ones until the state changes, rather than flipping back and forth as a number gains
/// or loses a digit.
struct Fit<K> {
    key: Option<K>,
    index: usize,
}

impl<K> Default for Fit<K> {
    fn default() -> Self {
        Fit {
            key: None,
            index: 0,
        }
    }
}

impl<K: PartialEq> Fit<K> {
    /// The wording for a plate in state `key`: the longest of `wordings` that fits `room` in
    /// `font`, but no longer than the one it showed in the same state; the last if none fits.
    fn choose(
        &mut self,
        key: K,
        mut wordings: Vec<Vec<Vec<Word>>>,
        room: Pixels,
        font: &Font,
        window: &Window,
    ) -> Vec<Vec<Word>> {
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.index = 0;
        }
        let last = wordings.len() - 1;
        self.index = (self.index.min(last)..last)
            .find(|&index| laid_width(&wordings[index], font, window) <= room)
            .unwrap_or(last);
        wordings.swap_remove(self.index)
    }
}

/// Opens the plates' windows -- hidden until their rails are on screen -- and starts sampling.
/// The level plate's window owns the returned view; keep the handle only to call
/// [`XpOverlay::set_cover`] and [`XpOverlay::set_options`].
pub fn open(
    options: XpOverlayOptions,
    app: WeakEntity<PriceCheckApp>,
    cx: &mut App,
) -> anyhow::Result<Entity<XpOverlay>> {
    let (lip_watch, lip_reports) = match LipWatch::start() {
        Ok((watch, reports)) => (Some(watch), Some(reports)),
        Err(err) => {
            log::warn!("{err:#}");
            (None, None)
        }
    };
    let window = cx.open_window(window_options(), |window, cx| {
        window.set_window_title("PoE2 Oracle — XP");
        cx.new(|_| {
            let mut tracker = XpTracker::new();
            tracker.set_rate_window(options.rate_window_minutes);
            XpOverlay {
                tracker,
                status: XpStatus::default(),
                options,
                sample: None,
                cover: XpCover::default(),
                app,
                level: PlateWindow::default(),
                gear: PlateWindow::default(),
                map: PlateWindow::default(),
                art: None,
                level_fit: Fit::default(),
                rails: RailPresence::new(),
                lip_watch,
                watched: None,
                lips: None,
                watched_fill: None,
            }
        })
    })?;
    let view = window.entity(cx)?;
    let xp = view.clone();
    cx.open_window(window_options(), |window, cx| {
        window.set_window_title("PoE2 Oracle — gear");
        cx.new(|cx| {
            cx.observe(&xp, |_, _, cx| cx.notify()).detach();
            GearPlate {
                xp,
                attached: false,
            }
        })
    })?;
    let xp = view.clone();
    cx.open_window(window_options(), |window, cx| {
        window.set_window_title("PoE2 Oracle — Map");
        cx.new(|cx| {
            cx.observe(&xp, |_, _, cx| cx.notify()).detach();
            MapPlate {
                xp,
                attached: false,
                fit: Fit::default(),
            }
        })
    })?;
    let weak = view.downgrade();
    cx.spawn(async move |cx| sample_forever(weak, cx).await)
        .detach();
    if let Some(reports) = lip_reports {
        let weak = view.downgrade();
        cx.spawn(async move |cx| {
            while let Ok(report) = reports.recv().await {
                let Some(view) = weak.upgrade() else {
                    return;
                };
                view.update(cx, |view, cx| view.on_lips(report, cx));
            }
        })
        .detach();
    }
    Ok(view)
}

fn window_options() -> WindowOptions {
    WindowOptions {
        // Placeholder: the first sample puts the window on its rail, then shows it.
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(230.), px(20.)),
        ))),
        titlebar: None,
        kind: WindowKind::PopUp,
        is_movable: false,
        focus: false,
        show: false,
        ..Default::default()
    }
}

/// The overlay's clock: time since Windows started (`GetTickCount64`), the clock the game's log
/// stamps its lines with -- so the log's tail read at start falls into place on it
/// (`xp_tracker::log_time`).
fn uptime() -> Duration {
    Duration::from_millis(unsafe { GetTickCount64() })
}

/// Feeds the tracker until the window closes.
async fn sample_forever(view: WeakEntity<XpOverlay>, cx: &mut AsyncApp) {
    let mut log: Option<ClientLog> = None;
    loop {
        cx.background_executor().timer(SAMPLE_INTERVAL).await;
        // While the lip watcher watches, its looks read the bar and the rails: no blit here.
        let Ok(read_pixels) = view.read_with(cx, |view, _| view.lips.is_none()) else {
            return;
        };
        let (history, events, sample, still_open) = cx
            .background_executor()
            .spawn(async move {
                let mut log = log;
                let mut history = Vec::new();
                // Retried every sample until the game runs: the log is found through its process.
                if log.is_none()
                    && let Some((opened, replayed)) =
                        ClientLog::open(client_log::HISTORY_BYTES, parse_timed_log_line)
                {
                    log = Some(opened);
                    history = replayed;
                }
                let events = log
                    .as_mut()
                    .map(|log| log.poll(parse_log_line))
                    .unwrap_or_default();
                (history, events, xp_bar::sample(read_pixels), log)
            })
            .await;
        log = still_open;
        let at = uptime();
        let Some(view) = view.upgrade() else {
            return;
        };
        view.update(cx, |view, cx| {
            let history = history
                .into_iter()
                .map(|(tick, event)| (log_time(tick, at), event));
            view.tracker.restore(history, at);
            for event in events {
                view.tracker.on_log_event(event, at);
            }
            // The watcher's reading while it watches -- it may have stopped since the look above
            // left the pixels to it, and then the tracker keeps its last reading this once.
            let fill = if view.lips.is_some() {
                view.watched_fill
            } else {
                sample.as_ref().and_then(|sample| sample.fill)
            };
            view.tracker.on_sample(fill, at);
            let status = view.tracker.status();
            // A moved or rescaled game moves the plates and resizes their words.
            let place = |sample: &Option<BarSample>| {
                sample
                    .as_ref()
                    .map(|sample| (sample.client, sample.dpi_scale))
            };
            let moved = place(&view.sample) != place(&sample);
            // No game is no rails; a look that left the rails to the watcher says nothing.
            let rails = match &sample {
                Some(sample) => sample.rails,
                None => Some(RailsSeen::default()),
            };
            if let Some(rails) = rails {
                view.rails.note(rails);
            }
            view.sample = sample;
            // The status moves on with every sample -- a pause's length, a rate's last digits --
            // but the plates are redrawn only when what they say changes, or where.
            let shown = view.shown();
            view.status = status;
            if moved || view.shown() != shown {
                view.repaint(cx);
            }
            view.sync_windows(cx);
        });
    }
}

impl XpOverlay {
    /// Takes what keeps the plates off the screen now; pass it whenever it changes.
    pub fn set_cover(&mut self, cover: XpCover, cx: &mut Context<Self>) {
        self.cover = cover;
        self.sync_windows(cx);
    }

    /// Takes over the player's saved options: what to show, the rate's averaging window.
    pub fn set_options(&mut self, options: XpOverlayOptions, cx: &mut Context<Self>) {
        if options == self.options {
            return;
        }
        self.tracker.set_rate_window(options.rate_window_minutes);
        self.options = options;
        self.status = self.tracker.status();
        self.repaint(cx);
        self.sync_windows(cx);
    }

    /// What the plates say, as far as drawing them goes: their words, whether the level plate
    /// shows play or a pause, and whether the map is still being run.
    fn shown(&self) -> Shown {
        Shown {
            level: self.level_wordings(),
            playing: !self.paused(),
            map: self
                .map_status()
                .map(|map| (self.map_wordings(&map), map.state)),
        }
    }

    /// Redraws the plates, whose words or art changed: their windows paint only in bursts
    /// (`Win32Overlay::gate_paints`), so each opens one as its view is notified.
    fn repaint(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        for window in [&self.level, &self.gear, &self.map] {
            if let Some(overlay) = window.overlay {
                overlay.open_paints();
            }
        }
    }

    /// The map plate's run, when the player wants it and there is one to show.
    fn map_status(&self) -> Option<MapStatus> {
        self.options.map_timer.then_some(self.status.map).flatten()
    }

    fn paused(&self) -> bool {
        matches!(self.status.activity, Activity::Paused { .. })
    }

    /// The level plate's wordings, longest first: in full, short, then short without the
    /// percent -- in a pause, the percent alone (`xp_tracker::level_parts`).
    fn level_wordings(&self) -> Vec<Vec<Vec<Word>>> {
        let status = &self.status;
        if self.paused() {
            return vec![level_parts(status, true, Wording::Full)];
        }
        let show_percent = self.options.show_percent;
        vec![
            level_parts(status, show_percent, Wording::Full),
            level_parts(status, show_percent, Wording::Short),
            level_parts(status, false, Wording::Short),
        ]
    }

    /// The map plate's wordings, longest first.
    fn map_wordings(&self, map: &MapStatus) -> Vec<Vec<Vec<Word>>> {
        let paused = self.paused();
        [Wording::Full, Wording::Short]
            .map(|wording| vec![map_words(map, paused, wording)])
            .into()
    }

    /// Takes a plate's platform window once it exists: frameless, painting only in bursts
    /// (`repaint`), and letting clicks through to the game -- but for the gear's, which takes
    /// clicks and never the keyboard, so a click on it leaves the keyboard with the game.
    fn attach(&mut self, plate: Plate, overlay: Win32Overlay, cx: &mut Context<Self>) {
        if let Err(err) = overlay.disable_dwm_frame() {
            log::warn!("{err:#}");
        }
        // Spawned before the sync's first task, so the window is ready by the time it's shown.
        cx.spawn(async move |_, _| {
            if let Err(err) = overlay.remove_frame() {
                log::warn!("{err:#}");
            }
            let styled = match plate {
                Plate::Level | Plate::Map => overlay.set_click_through(true),
                Plate::Gear => overlay.set_no_activate(),
            };
            if let Err(err) = styled.and_then(|()| overlay.gate_paints()) {
                log::warn!("{err:#}");
            }
        })
        .detach();
        match plate {
            Plate::Level => self.level.overlay = Some(overlay),
            Plate::Gear => self.gear.overlay = Some(overlay),
            Plate::Map => self.map.overlay = Some(overlay),
        }
        self.sync_windows(cx);
    }

    /// Takes the lip watcher's word on the rails and puts the plates in line with it at once:
    /// a tooltip over a rail takes its plate down in the frame it appears, and back when it goes.
    /// The bar's fill waits for the next sample, which the tracker takes it from.
    fn on_lips(&mut self, report: LipReport, cx: &mut Context<Self>) {
        log::debug!("lip watch: {report:?}");
        match report {
            LipReport::Seen { rails, fill } => {
                self.lips = Some(rails);
                self.watched_fill = fill;
            }
            // The sampler takes over from the watcher's last look, not from its own older ones.
            LipReport::Idle => {
                self.watched_fill = None;
                if let Some(seen) = self.lips.take() {
                    self.rails.seed(seen);
                }
            }
        }
        self.sync_windows(cx);
    }

    /// Brings the plates' windows in line with the latest looks at the game. Called from the
    /// sampling task, the lip watcher's reports, `set_cover`, `set_options` and a window's first
    /// render, not from `render` alone: a hidden GPUI window is never redrawn (see
    /// `app::PriceCheckRoot::sync_window`), so a render-only sync could never show it again.
    fn sync_windows(&mut self, cx: &mut Context<Self>) {
        // Drawn again only when the game moves or resizes.
        let game = self.sample.as_ref().map(|sample| sample.client);
        if self.art.as_ref().map(|art| art.game) != game {
            if let Some(old) = self.art.take() {
                old.drop_images(cx);
            }
            self.art = game.map(Arts::draw);
        }
        // The lip watcher follows the game while the overlay is on.
        let target = game.filter(|_| !self.cover.off);
        if target != self.watched {
            self.watched = target;
            if let Some(watch) = &self.lip_watch {
                watch.watch(target);
            }
        }
        let cover = self.cover;
        let clear = |rect: &PhysicalRect| {
            !cover.off && cover.panel.is_none_or(|panel| !panel.intersects(rect))
        };
        // While the player is at the game the watcher's frame-by-frame look decides; otherwise
        // the sampler's. The level plate also shows while the tour spotlights it: the tour's dim
        // covers the lip until its hole is cut around the plate.
        let seen = self.lips.unwrap_or(RailsSeen {
            flask: self.rails.flask(),
            skill: self.rails.skill(),
        });
        let flask = seen.flask || crate::ui::tour::holds_xp_line(cx);
        let skill = seen.skill && self.map_status().is_some();
        let art = self.art.as_ref();
        let line = art.filter(|art| flask && clear(&art.line));
        let map = art
            .filter(|art| skill && clear(&art.map.rect))
            .map(|art| &art.map);
        if self.level.sync(line.map(|art| &art.level), cx) {
            cx.set_global(XpLineOnScreen(line.map(|art| art.line)));
        }
        self.gear.sync(line.map(|art| &art.gear), cx);
        self.map.sync(map, cx);
    }
}

/// The level plate's window and its gear's, side by side on the flask rail: the gear a square
/// at the right end.
fn split_gear(line: PhysicalRect) -> (PhysicalRect, PhysicalRect) {
    let side = line.height.min(line.width);
    (
        PhysicalRect {
            width: line.width - side,
            ..line
        },
        PhysicalRect {
            x: line.x + line.width - side,
            width: side,
            ..line
        },
    )
}

/// Whether each rail is taken for on screen by the sampler's look, every two seconds: seen in the
/// latest sample, or missed only once since -- a moment's cover over its lip, a tooltip passing,
/// doesn't blink its plate. Taken for off screen until first seen.
struct RailPresence {
    /// Samples in a row that missed each rail.
    flask_misses: u8,
    skill_misses: u8,
}

impl RailPresence {
    /// Missed samples in a row that take a rail's plate down.
    const MISSES_TO_HIDE: u8 = 2;

    fn new() -> Self {
        RailPresence {
            flask_misses: Self::MISSES_TO_HIDE,
            skill_misses: Self::MISSES_TO_HIDE,
        }
    }

    fn note(&mut self, seen: RailsSeen) {
        let next = |misses: u8, seen: bool| if seen { 0 } else { misses.saturating_add(1) };
        self.flask_misses = next(self.flask_misses, seen.flask);
        self.skill_misses = next(self.skill_misses, seen.skill);
    }

    /// Starts again from `seen`, the lip watcher's last look: each rail on screen or off it for
    /// good, till the samples say otherwise.
    fn seed(&mut self, seen: RailsSeen) {
        let misses = |seen: bool| if seen { 0 } else { Self::MISSES_TO_HIDE };
        self.flask_misses = misses(seen.flask);
        self.skill_misses = misses(seen.skill);
    }

    fn flask(&self) -> bool {
        self.flask_misses < Self::MISSES_TO_HIDE
    }

    fn skill(&self) -> bool {
        self.skill_misses < Self::MISSES_TO_HIDE
    }
}

impl Render for XpOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.level.overlay.is_none() {
            match Win32Overlay::from_window(window) {
                Ok(overlay) => self.attach(Plate::Level, overlay, cx),
                Err(err) => log::warn!("Win32Overlay::from_window failed: {err:?}"),
            }
        }
        let (Some(sample), Some(art)) = (self.sample.clone(), self.art.as_ref()) else {
            return div().into_any_element();
        };
        let (rect, image) = (art.level.rect, art.level.image.clone());
        window.set_rem_size(hud_rem_size(&sample));
        let font = plate_font(window);
        let (level, _) = split_gear(hud_rails(sample.client).flask);
        let room = room(level, &sample, 2. * PADDING_X, window);
        let status = self.status;
        let key = (
            self.paused(),
            status.rate_per_hour.is_some(),
            self.options.show_percent && status.fraction.is_some(),
            i18n::lang(),
            room,
        );
        let wordings = self.level_wordings();
        let parts = self.level_fit.choose(key, wordings, room, &font, window);
        // The art runs on over the gap to the life globe; the words keep to the plate's run.
        let run = run_box(level, rect, window);
        plate(image, rect, window)
            .child(ease_state(
                "playing",
                !self.paused(),
                run,
                move |run, lit| run.child(words(parts.clone(), &font, Tones::at(lit))),
            ))
            .into_any_element()
    }
}

impl Render for GearPlate {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.attached {
            match Win32Overlay::from_window(window) {
                Ok(overlay) => {
                    self.attached = true;
                    self.xp
                        .update(cx, |xp, cx| xp.attach(Plate::Gear, overlay, cx));
                }
                Err(err) => log::warn!("Win32Overlay::from_window failed: {err:?}"),
            }
        }
        let xp = self.xp.read(cx);
        let (Some(sample), Some(art)) = (xp.sample.clone(), xp.art.as_ref()) else {
            return div().into_any_element();
        };
        let (rect, image) = (art.gear.rect, art.gear.image.clone());
        window.set_rem_size(hud_rem_size(&sample));
        let app = xp.app.clone();
        let (_, square) = split_gear(hud_rails(sample.client).flask);
        // The line between the words and the gear, on the face only.
        let divider = div()
            .absolute()
            .left_0()
            .top(rems_from_px(CAP_HEIGHT + 3.))
            .bottom(rems_from_px(3.))
            .w(rems_from_px(DIVIDER))
            .bg(rgb(HUD_DIVIDER));
        plate(image, rect, window)
            .child(
                run_box(square, rect, window)
                    .justify_center()
                    .px_0()
                    .child(divider)
                    .child(gear(app)),
            )
            .into_any_element()
    }
}

impl Render for MapPlate {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.attached {
            match Win32Overlay::from_window(window) {
                Ok(overlay) => {
                    self.attached = true;
                    self.xp
                        .update(cx, |xp, cx| xp.attach(Plate::Map, overlay, cx));
                }
                Err(err) => log::warn!("Win32Overlay::from_window failed: {err:?}"),
            }
        }
        let xp = self.xp.read(cx);
        let (Some(sample), Some(map), Some(art)) =
            (xp.sample.clone(), xp.map_status(), xp.art.as_ref())
        else {
            return div().into_any_element();
        };
        let (rect, image) = (art.map.rect, art.map.image.clone());
        window.set_rem_size(hud_rem_size(&sample));
        let font = plate_font(window);
        let skill = hud_rails(sample.client).skill;
        let room = room(skill, &sample, 2. * PADDING_X, window);
        let key = (
            map.state,
            map.average.is_some() && !xp.paused(),
            i18n::lang(),
            room,
        );
        let parts = self
            .fit
            .choose(key, xp.map_wordings(&map), room, &font, window);
        // Dimmed once the character has left the run. The art runs on past the rail's end and
        // over the gap to the mana globe; the words keep to the plate's run.
        let running = map.state == RunState::Running;
        let run = run_box(skill, rect, window);
        plate(image, rect, window)
            .child(ease_state("running", running, run, move |run, lit| {
                run.child(words(parts.clone(), &font, Tones::at(lit)))
            }))
            .into_any_element()
    }
}

/// The rem size that makes a HUD pixel the game's own at its height, in the window's logical
/// pixels.
fn hud_rem_size(sample: &BarSample) -> Pixels {
    let hud_scale = f64::from(sample.client.height) / HUD_UNIT_HEIGHT;
    px((f64::from(BASE_REM_SIZE) * hud_scale / sample.dpi_scale) as f32)
}

/// The width a plate `rect` wide (physical pixels) leaves its words, in the window's logical
/// pixels: less `reserved` HUD pixels for its ends.
fn room(rect: PhysicalRect, sample: &BarSample, reserved: f32, window: &Window) -> Pixels {
    px((f64::from(rect.width) / sample.dpi_scale) as f32)
        - rems_from_px(reserved).to_pixels(window.rem_size())
}

/// The plates' typeface: the game-styled one of the interface language, as the HUD's own words
/// are the game's face.
fn plate_font(window: &Window) -> Font {
    let face = fonts::interface_font();
    Font {
        family: face.family.into(),
        weight: face.weight,
        ..window.text_style().font()
    }
}

/// How wide [`words`] lays `parts` out.
fn laid_width(parts: &[Vec<Word>], font: &Font, window: &Window) -> Pixels {
    let rem = window.rem_size();
    let hud = |length: f32| rems_from_px(length).to_pixels(rem);
    let text_size = hud(TEXT_SIZE);
    let mut width = px(0.);
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            width += hud(2. * PART_GAP + SEPARATOR_DIAMOND);
        }
        for (at, word) in part.iter().enumerate() {
            if at > 0 {
                width += hud(WORD_GAP);
            }
            let text = word.text();
            let run = TextRun {
                len: text.len(),
                font: font.clone(),
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            width += window
                .text_system()
                .shape_line(SharedString::from(text.to_owned()), text_size, &[run], None)
                .width;
        }
    }
    width
}

/// The words' colours, `lit` (0 to 1) of the way from the dimmed look -- a pause, a run that is
/// over -- to the look of play: the values, the words saying what they are, the rate, and the
/// diamonds between the parts.
#[derive(Clone, Copy)]
struct Tones {
    value: u32,
    label: u32,
    rate: u32,
    separator: u32,
}

impl Tones {
    fn at(lit: f32) -> Tones {
        let dim = |color: u32| blend(plate_art::FACE_BOTTOM, color, 0.65);
        Tones {
            value: blend(HUD_LABEL, HUD_TEXT, lit),
            label: blend(dim(HUD_LABEL), HUD_LABEL, lit),
            rate: blend(HUD_LABEL, HUD_GOLD, lit),
            separator: blend(dim(HUD_GOLD), HUD_GOLD, lit),
        }
    }
}

/// A plate window's root: its part of the plates' art (`Arts`), pixel for pixel -- `rect` is
/// where the window is, in physical pixels, and the image as many pixels.
fn plate(image: Arc<RenderImage>, rect: PhysicalRect, window: &Window) -> Div {
    let logical = |length: i32| px(length as f32 / window.scale_factor());
    div().relative().size_full().child(
        img(ImageSource::Render(image))
            .absolute()
            .top_0()
            .left_0()
            .w(logical(rect.width))
            .h(logical(rect.height))
            .object_fit(ObjectFit::Fill),
    )
}

/// The box a plate window's contents go in, in a row: over `run` -- the part of a rail the plate
/// stands on, in physical pixels on screen -- of the window at `rect`, on the face below the cap
/// molding, with room at its ends.
fn run_box(run: PhysicalRect, rect: PhysicalRect, window: &Window) -> Div {
    let logical = |length: i32| px(length as f32 / window.scale_factor());
    div()
        .absolute()
        .left(logical(run.x - rect.x))
        .top(logical(run.y - rect.y))
        .w(logical(run.width))
        .h(logical(run.height))
        .flex()
        .items_center()
        .pt(rems_from_px(CAP_HEIGHT))
        .px(rems_from_px(PADDING_X))
}

/// `parts` centred in the room left of anything after them, a diamond between them, each word in
/// its tone in the plates' face.
fn words(parts: Vec<Vec<Word>>, font: &Font, tones: Tones) -> Div {
    let mut line = div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .gap(rems_from_px(PART_GAP))
        .font_family(font.family.clone())
        .font_weight(font.weight)
        .text_size(rems_from_px(TEXT_SIZE))
        .whitespace_nowrap();
    for (index, part) in parts.into_iter().enumerate() {
        if index > 0 {
            line = line.child(diamond(SEPARATOR_DIAMOND, tones.separator));
        }
        line = line.child(
            div()
                .flex()
                .items_center()
                .gap(rems_from_px(WORD_GAP))
                .children(part.iter().map(|word| {
                    let color = match word {
                        Word::Rate(_) => tones.rate,
                        Word::Value(_) => tones.value,
                        Word::Label(_) | Word::Dot => tones.label,
                    };
                    div()
                        .text_color(rgb(color))
                        .child(SharedString::from(word.text().to_owned()))
                })),
        );
    }
    line
}

/// The gear: it opens the settings, lighting from the words' muted tone to the HUD's gold under
/// the pointer. Drawn from Segoe UI Symbol, whose gear is a flat glyph that takes the colour --
/// the fallback would otherwise find Segoe UI Emoji's grey picture.
fn gear(app: WeakEntity<PriceCheckApp>) -> impl IntoElement {
    let gear = div()
        .id("settings")
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .font_family("Segoe UI Symbol")
        .text_size(rems_from_px(GEAR_SIZE))
        .cursor_pointer()
        .child("⚙")
        .on_mouse_down(
            MouseButton::Left,
            move |_: &MouseDownEvent, _window, cx: &mut App| {
                if let Some(app) = app.upgrade() {
                    crate::app::open_settings(&app, cx);
                }
            },
        );
    ease_hover("settings", gear, |gear, hover| {
        gear.text_color(rgb(blend(HUD_LABEL, HUD_GOLD, hover)))
    })
}
