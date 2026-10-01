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
//! game is in front and not minimised, and a moment after, the lips are watched frame by frame
//! (`platform::lip_watch`) -- twenty times a second while the player moves the mouse or presses
//! keys, thinning out to every two seconds once they keep still (`platform::lip_schedule`) -- so
//! a plate steps aside the moment a tooltip covers its rail and comes back the moment it goes,
//! and the bar is read off the same frames, every half second at the most: the sampler takes
//! that reading, and reads no pixels itself. Otherwise the sampler's look decides,
//! a single miss let pass: behind another window the game still shows a tooltip under the
//! cursor, and its plate steps aside two to four seconds later, back within two once it goes.
//! Out of the front, though, no experience comes in: the sampler looks at the bar only every ten
//! seconds then, the tracker carrying its reading over the samples between, and at the next
//! sample after a look that saw it change or couldn't read it (`XpTracker::unattended_look_due`).
//! A minimised game is read nothing off, and a bar or a rail another window covers isn't copied
//! at all (`xp_bar::shows_the_game`), nor the bar with the pointer on it (`xp_bar::bar_hovered`).
//! The price-check panel hides only a plate it covers, and the setting both
//! ([`XpOverlay::set_cover`]). Their size is the game's, not the app's interface scale: at any
//! game height a plate is its rail's width, and its words the HUD's.
//!
//! An update's restart doesn't start the plates over: the app's old copy leaves its tracker
//! ([`carry_over`]), and the new one carries on with it, the log's lines since taken up.
//!
//! Nor does a restart of the app, or a login, forget which character is playing, which the game's
//! log names only at a level-up: the level book (`xp_tracker::LevelBook`, `paths::xp_levels_file`)
//! keeps each recent character's level and where its bar stood, so the first reading of the bar
//! says who it is and the level plate names the next level again. It is read when the overlay
//! opens, and filled from far back in the game's log when it is empty
//! (`ClientLog::latest_levels`); the sampler writes it at each level-up, soon after a change of
//! area, a logout or a new level's first position on the bar, and otherwise once a minute at most
//! while the bar moves (`XpTracker::book_to_save`), so that an app ended without a word loses
//! little; it is written once more when the app quits.
//!
//! Each plate is its own window that lets clicks through to the game -- the plates stand over the
//! game's world. The gear is a window of its own at the level plate's right end, the one place
//! that takes a click, and never the keyboard. The plates are drawn pixel by pixel in the HUD's
//! own materials (`crate::plate_art`): the rails' cap molding along the top, a dark face, a thin
//! seam where they sit on the rail; at its globe a plate runs on over the world to the globe's
//! frame, and at its other end it curls down onto the tip of the game's volute. Each window shows
//! its part of that art and is shaped to exactly its pixels (a window region), so nothing of the
//! world shows between a plate and the HUD, and nothing of the HUD is covered. The values are in
//! the HUD's cream, the words saying what they are muted, the rate in its gold, a small diamond
//! between the parts.
//!
//! A plate window keeps on the GPU only what it draws with. GPUI gives each window a sprite atlas
//! of its own, whose texture pages are 1024 pixels square at the least: an image in it takes a
//! 4 MiB page of colour, ClearType words another, grayscale words a 1 MiB page of coverage. So
//! the art and the diamonds are quads, not images (`plate_art::Fill`, `style::quad_diamond`), and
//! the window is transparent to GPUI -- which draws the words of any window that isn't opaque in
//! grayscale, as the price panel's are -- yet opaque to Windows: GPUI's transparent backdrop
//! would tint the game behind it (`Win32Overlay::set_shown`), so it's taken off again
//! (`Win32Overlay::clear_backdrop`), and the art covers every pixel the window shows. That's 1 MiB
//! of atlas a window, where an image and ClearType took 8.

use std::fs;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use gpui::{
    App, AsyncApp, Bounds, Context, Div, Entity, Font, Global, Hsla, IntoElement, MouseButton,
    MouseDownEvent, Pixels, Render, SharedString, TextRun, WeakEntity, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, canvas, div, point,
    prelude::*, px, rgb, size,
};
use windows::Win32::System::SystemInformation::GetTickCount64;

use crate::i18n::{self, Lang};
use crate::overlay_layout::{PhysicalRect, hud_rails};
use crate::paths;
use crate::plate_art::{self, ArtSlice, Fill, PlateArt};
use crate::platform::client_log::{self, ClientLog};
use crate::platform::game_window;
use crate::platform::lip_watch::{LipReport, LipWatch};
use crate::platform::win32::Win32Overlay;
use crate::platform::xp_bar::{self, BarSample, RailsSeen};
use crate::price_check::PriceCheckApp;
use crate::settings::{InterfaceLanguage, Settings};
use crate::ui::fonts;
use crate::ui::style::{ease_hover, ease_state, quad_diamond};
use crate::ui::theme::{
    BASE_REM_SIZE, HUD_DIVIDER, HUD_GOLD, HUD_LABEL, HUD_TEXT, blend, rems_from_px,
};
use crate::xp_tracker::{
    Activity, BarLook, LevelBook, MapStatus, RunState, Word, Wording, XpStatus, XpTracker,
    level_parts, log_time, map_words, parse_log_line, parse_timed_log_line,
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
    /// The interface language the player picked, which the plates are worded in (`i18n::lang`):
    /// another has them worded anew at once.
    pub language: InterfaceLanguage,
}

impl XpOverlayOptions {
    pub fn from_settings(settings: &Settings) -> XpOverlayOptions {
        XpOverlayOptions {
            show_percent: settings.xp_show_percent,
            map_timer: settings.xp_map_timer,
            rate_window_minutes: settings.xp_rate_window_minutes,
            language: settings.interface_language,
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

/// The overlay while it's open, for [`carry_over`].
struct Tracking(WeakEntity<XpOverlay>);

impl Global for Tracking {}

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

/// A plate window's part of the art: where the window goes, what of it shows, and its pixels as
/// rects of one colour, relative to the window, which it paints as quads ([`plate`]).
struct Slice {
    rect: PhysicalRect,
    shown: Vec<PhysicalRect>,
    fills: Arc<[Fill]>,
}

impl Slice {
    fn cut(art: &PlateArt, rect: PhysicalRect) -> Slice {
        let ArtSlice { fills, shown } = art.slice(rect);
        Slice {
            rect,
            shown,
            fills: fills.into(),
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
    /// What the plates said when they were last drawn anew ([`XpOverlay::redraw`]); `None` before
    /// the first time.
    drawn: Option<Shown>,
    /// The level plate's wording: paused, rated, with the percent, in what language and room.
    level_fit: Fit<(bool, bool, bool, Lang, Pixels)>,
    /// Whether the level plate's words were last drawn lit -- play, not a pause -- `None` if its
    /// last frame drew none ([`eases`]).
    level_lit: Option<bool>,
    /// The sampler's look at the rails, every two seconds.
    rails: RailPresence,
    /// The frame-by-frame look at the rails and the bar while the game is in front
    /// (`platform::lip_watch`): its handle, `None` if its thread couldn't start, whose latest
    /// reading of the bar's fill the sampler takes instead of reading the screen itself
    /// ([`LipWatch::fill`]); the game it was last given; and the rails it saw last, `None` while
    /// it isn't watching.
    lip_watch: Option<LipWatch>,
    watched: Option<PhysicalRect>,
    lips: Option<RailsSeen>,
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
    /// Whether its words were last drawn lit -- a run under way -- `None` if its last frame drew
    /// none ([`eases`]).
    lit: Option<bool>,
}

/// What the plates say ([`XpOverlay::shown`]), and in which language -- which picks their face
/// too: a plate is drawn anew only when its part of this changes.
struct Shown {
    lang: Lang,
    level: Vec<Vec<Vec<Word>>>,
    playing: bool,
    map: Option<(Vec<Vec<Vec<Word>>>, RunState)>,
}

impl Shown {
    /// Whether the level plate, and the map plate, say otherwise in `self` than in `drawn`. The
    /// map's words dim with a pause too, and lose the average.
    fn differs(&self, drawn: &Shown) -> (bool, bool) {
        let reworded = self.lang != drawn.lang;
        (
            reworded || self.level != drawn.level || self.playing != drawn.playing,
            reworded || self.map != drawn.map,
        )
    }
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
        cx.new(|cx| {
            let mut tracker = XpTracker::new();
            tracker.set_rate_window(options.rate_window_minutes);
            tracker.set_book(load_levels());
            // The sampler writes the book as the tracker has it due, not at every change: the
            // app's end writes what came after.
            cx.on_app_quit(|overlay: &mut XpOverlay, _| {
                if let Some(book) = overlay.tracker.book_to_save(None) {
                    write_levels(&book);
                }
                std::future::ready(())
            })
            .detach();
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
                drawn: None,
                level_fit: Fit::default(),
                level_lit: None,
                rails: RailPresence::new(),
                lip_watch,
                watched: None,
                lips: None,
            }
        })
    })?;
    let view = window.entity(cx)?;
    cx.set_global(Tracking(view.downgrade()));
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
                lit: None,
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
                view.update(cx, |view, cx| {
                    view.on_lips(report);
                    // Reports that came meanwhile -- the UI thread was busy -- are taken with it,
                    // and the plates brought in line with the latest once.
                    while let Ok(later) = reports.try_recv() {
                        view.on_lips(later);
                    }
                    view.sync_windows(cx);
                });
            }
        })
        .detach();
        // The watcher watches only while the game is in front: it's told of each new foreground
        // window, and polls for none.
        match game_window::watch_foreground() {
            Ok(changes) => {
                let weak = view.downgrade();
                cx.spawn(async move |cx| {
                    while let Ok(foreground) = changes.recv().await {
                        let Some(view) = weak.upgrade() else {
                            return;
                        };
                        view.update(cx, |view, _| {
                            if let Some(watch) = &view.lip_watch {
                                watch.foreground(foreground);
                            }
                        });
                    }
                })
                .detach();
            }
            Err(err) => log::warn!("{err:#}"),
        }
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
        // For GPUI only, whose words in a window that isn't opaque are grayscale: a 1 MiB atlas
        // page, not ClearType's 4 MiB. Opaque to Windows once attached (`XpOverlay::attach`).
        window_background: WindowBackgroundAppearance::Transparent,
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

/// Leaves the tracker for the app's copy an update's restart starts, which carries on with it
/// (`XpTracker::carried`): the rate, the time to the level and the map runs go on where they were
/// instead of starting over. Call right before quitting for the update; without the overlay open,
/// there's nothing to leave.
pub fn carry_over(cx: &App) {
    let Some(view) = cx
        .try_global::<Tracking>()
        .and_then(|open| open.0.upgrade())
    else {
        return;
    };
    let path = paths::xp_carry_file();
    let written = view
        .read(cx)
        .tracker
        .carry()
        .context("writing the XP tracker")
        .and_then(|json| {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
            }
            fs::write(&path, json).with_context(|| format!("writing {}", path.display()))
        });
    match written {
        Ok(()) => log::info!("xp: the tracker left for the next start to carry on with"),
        Err(err) => log::warn!("leaving the XP tracker for the next start failed: {err:#}"),
    }
}

/// The tracker the app's last copy left ([`carry_over`]), taken: read and deleted, so it's
/// carried on with once at most.
fn take_carried() -> Option<Vec<u8>> {
    let path = paths::xp_carry_file();
    let json = match fs::read(&path) {
        Ok(json) => Some(json),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
        Err(err) => {
            log::warn!("reading {} failed: {err}", path.display());
            None
        }
    };
    if let Err(err) = fs::remove_file(&path) {
        log::warn!("deleting {} failed: {err}", path.display());
    }
    json
}

/// The level book the last run left ([`LevelBook::load`]): empty when there is none.
fn load_levels() -> LevelBook {
    LevelBook::load(&paths::xp_levels_file())
}

/// Writes the level book ([`LevelBook::save`]). A failure is for the log, not the player: the
/// game's log fills the book again.
fn write_levels(book: &LevelBook) {
    let path = paths::xp_levels_file();
    if let Err(err) = book.save(&path) {
        log::warn!("writing {} failed: {err}", path.display());
    }
}

/// Feeds the tracker until the window closes.
async fn sample_forever(view: WeakEntity<XpOverlay>, cx: &mut AsyncApp) {
    let mut log: Option<ClientLog> = None;
    loop {
        cx.background_executor().timer(SAMPLE_INTERVAL).await;
        // While the lip watcher watches, its looks read the bar and the rails: no blit here.
        // Otherwise the rails' lips every time, and the bar while the game is in front -- out of
        // it, only when the tracker's reading is due a look (`XpTracker::unattended_look_due`).
        // The level book comes with it: when the tracker has it due to be written, and whether it
        // has nothing yet, for the game's log to fill.
        let Ok((read_pixels, unattended, book, seeding)) = view.update(cx, |view, _| {
            let now = uptime();
            (
                view.lips.is_none(),
                view.tracker.unattended_look_due(now),
                view.tracker.book_to_save(Some(now)),
                view.tracker.book_is_empty(),
            )
        }) else {
            return;
        };
        let (history, carried, seeds, events, sample, still_open) = cx
            .background_executor()
            .spawn(async move {
                let mut log = log;
                let mut history = Vec::new();
                let mut carried = None;
                let mut seeds = Vec::new();
                if let Some(book) = book {
                    write_levels(&book);
                }
                // Retried every sample until the game runs: the log is found through its process.
                if log.is_none()
                    && let Some((opened, replayed)) =
                        ClientLog::open(client_log::HISTORY_BYTES, parse_timed_log_line)
                {
                    // An empty level book is filled from far back in the log, which the tail read
                    // above doesn't reach: a level at 94 takes days.
                    if seeding {
                        match opened.latest_levels() {
                            Ok(latest) => seeds = latest,
                            Err(err) => {
                                log::warn!("reading the game log's level-ups failed: {err}")
                            }
                        }
                    }
                    log = Some(opened);
                    history = replayed;
                    // What an update's restart left, carried on with instead of the tail alone.
                    carried = take_carried();
                }
                let events = log
                    .as_mut()
                    .map(|log| log.poll(parse_log_line))
                    .unwrap_or_default();
                let sample = xp_bar::sample(read_pixels, |in_front| in_front || unattended);
                (history, carried, seeds, events, sample, log)
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
            // Before the tail is replayed, so that the levels it names refine these.
            view.tracker.seed_book(seeds);
            match carried.and_then(|json| XpTracker::carried(&json, at)) {
                Some(mut tracker) => {
                    log::info!("xp: carrying on with the tracker from before the restart");
                    tracker.set_rate_window(view.options.rate_window_minutes);
                    tracker.take_book_from(&mut view.tracker);
                    tracker.catch_up(history, at);
                    view.tracker = tracker;
                }
                None => view.tracker.restore(history, at),
            }
            for event in events {
                view.tracker.on_log_event(event, at);
            }
            // The watcher's latest reading while it watches -- it may have stopped since the look
            // above left the pixels to it, and then the tracker carries its last reading this
            // once.
            let look = match &sample {
                _ if view.lips.is_some() => view
                    .lip_watch
                    .as_ref()
                    .and_then(LipWatch::fill)
                    .map_or(BarLook::Unreadable, BarLook::Read),
                Some(sample) => sample.bar,
                // No game, or a minimised one.
                None => BarLook::Unreadable,
            };
            // A change the tracker starts holding: while debugging, the pixels it was read from
            // go to a file, off this thread.
            if let Some(suspect) = view.tracker.on_sample(look, at)
                && let Some(rows) = xp_bar::last_rows()
            {
                cx.background_executor()
                    .spawn(async move { xp_bar::snapshot(&rows, suspect) })
                    .detach();
            }
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
            // but a plate is drawn anew only when what it says changes, or where.
            view.status = status;
            view.redraw(moved, cx);
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

    /// Takes over the player's saved options: what to show, the rate's averaging window, the
    /// language the plates are worded in.
    pub fn set_options(&mut self, options: XpOverlayOptions, cx: &mut Context<Self>) {
        if options == self.options {
            return;
        }
        self.tracker.set_rate_window(options.rate_window_minutes);
        self.options = options;
        self.status = self.tracker.status();
        self.redraw(false, cx);
        self.sync_windows(cx);
    }

    /// What the plates say, as far as drawing them goes: their words and their language, whether
    /// the level plate shows play or a pause, and whether the map is still being run.
    fn shown(&self) -> Shown {
        Shown {
            lang: i18n::lang(),
            level: self.level_wordings(),
            playing: !self.paused(),
            map: self
                .map_status()
                .map(|map| (self.map_wordings(&map), map.state)),
        }
    }

    /// Draws anew the plates that say otherwise than when they were last drawn -- every one once
    /// the game `moved` or was rescaled, which moves their art and resizes their words. Their
    /// windows paint only when what they show changes (`Win32Overlay::gate_still_paints`), so
    /// each plate drawn anew has its window's next paints go through -- a pair, not a burst: the
    /// words change in one frame, and the one change that eases opens a burst as it's drawn
    /// ([`eases`]) -- and the others' stay shut: the map's clock running on redraws neither the
    /// level plate nor the gear. The notice reaches every plate's view -- each reads this one --
    /// and a window whose paints stay shut draws its unchanged plate once more at its next safety
    /// net's paint.
    fn redraw(&mut self, moved: bool, cx: &mut Context<Self>) {
        let shown = self.shown();
        let (level, map) = match &self.drawn {
            Some(drawn) if !moved => shown.differs(drawn),
            _ => (true, true),
        };
        let gear = moved || self.drawn.is_none();
        if !(level || map || gear) {
            return;
        }
        self.drawn = Some(shown);
        cx.notify();
        for (window, changed) in [(&self.level, level), (&self.gear, gear), (&self.map, map)] {
            if changed && let Some(overlay) = window.overlay {
                overlay.repaint();
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

    /// Takes a plate's platform window once it exists: frameless, opaque to Windows -- GPUI took
    /// it for transparent (`window_options`) -- painting only when what it shows changes
    /// (`redraw`), and letting clicks through to the game -- but for the gear's, which takes
    /// clicks and never the keyboard, so a click on it leaves the keyboard with the game.
    fn attach(&mut self, plate: Plate, overlay: Win32Overlay, cx: &mut Context<Self>) {
        if let Err(err) = overlay.disable_dwm_frame() {
            log::warn!("{err:#}");
        }
        // Deferred out of `render`, which calls this, since these calls may send the window
        // messages; spawned before the sync's first task, so the window is ready by the time it's
        // shown.
        cx.spawn(async move |_, _| {
            for done in [overlay.clear_backdrop(), overlay.remove_frame()] {
                if let Err(err) = done {
                    log::warn!("{err:#}");
                }
            }
            let styled = match plate {
                Plate::Level | Plate::Map => overlay.set_click_through(true),
                Plate::Gear => overlay.set_no_activate(),
            };
            if let Err(err) = styled.and_then(|()| overlay.gate_still_paints()) {
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

    /// Takes the lip watcher's word on the rails, which the plates are brought in line with at
    /// once (`sync_windows`): a tooltip over a rail takes its plate down as soon as a look reads
    /// it, and back when it goes. The watcher says only when that changes; the bar's fill waits
    /// for the next sample, which takes the watcher's latest reading.
    fn on_lips(&mut self, report: LipReport) {
        log::debug!("lip watch: {report:?}");
        match report {
            LipReport::Seen(rails) => self.lips = Some(rails),
            // The sampler takes over from the watcher's last look, not from its own older ones.
            LipReport::Idle => {
                if let Some(seen) = self.lips.take() {
                    self.rails.seed(seen);
                }
            }
        }
    }

    /// Brings the plates' windows in line with the latest looks at the game. Called from the
    /// sampling task, the lip watcher's reports, `set_cover`, `set_options` and a window's first
    /// render, not from `render` alone: a hidden GPUI window is never redrawn (see
    /// `app::PriceCheckRoot::sync_window`), so a render-only sync could never show it again.
    fn sync_windows(&mut self, cx: &mut Context<Self>) {
        // Drawn again only when the game moves or resizes.
        let game = self.sample.as_ref().map(|sample| sample.client);
        if self.art.as_ref().map(|art| art.game) != game {
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
        // While the game is in front the watcher's frame-by-frame look decides; otherwise the
        // sampler's. The level plate also shows while the tour spotlights it: the tour's dim
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
        // The words ease between a pause's dimmed look and play's over the frames from this one
        // (`style::ease_state`), which a change's pair of paints wouldn't draw: a burst of them.
        let lit = (self.sample.is_some() && self.art.is_some()).then(|| !self.paused());
        if eases(&mut self.level_lit, lit)
            && let Some(overlay) = self.level.overlay
        {
            overlay.open_paints();
        }
        let (Some(sample), Some(art)) = (self.sample.clone(), self.art.as_ref()) else {
            return div().into_any_element();
        };
        let (rect, fills) = (art.level.rect, art.level.fills.clone());
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
        plate(fills)
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
        let (rect, fills) = (art.gear.rect, art.gear.fills.clone());
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
        plate(fills)
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
        // The words dim as the character leaves the run, over the frames from this one
        // (`style::ease_state`), which a change's pair of paints wouldn't draw: a burst of them.
        let lit = xp
            .map_status()
            .filter(|_| xp.sample.is_some() && xp.art.is_some())
            .map(|map| map.state == RunState::Running);
        if eases(&mut self.lit, lit)
            && let Some(overlay) = xp.map.overlay
        {
            overlay.open_paints();
        }
        let (Some(sample), Some(map), Some(art)) =
            (xp.sample.clone(), xp.map_status(), xp.art.as_ref())
        else {
            return div().into_any_element();
        };
        let (rect, fills) = (art.map.rect, art.map.fills.clone());
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
        plate(fills)
            .child(ease_state("running", running, run, move |run, lit| {
                run.child(words(parts.clone(), &font, Tones::at(lit)))
            }))
            .into_any_element()
    }
}

/// Whether a plate's words ease into another look from the frame being drawn: they're drawn `lit`
/// -- playing, a run under way -- turned from how they were last drawn, `drawn`, which then takes
/// this frame's look; `None` for a frame without them. Words drawn anew after a frame without
/// them take their look at once, as `style::ease_state`'s channel starts afresh.
fn eases(drawn: &mut Option<bool>, lit: Option<bool>) -> bool {
    let before = std::mem::replace(drawn, lit);
    matches!((before, lit), (Some(before), Some(lit)) if before != lit)
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

/// A plate window's root: its part of the plates' art (`Arts`), pixel for pixel -- `fills`
/// relative to the window's top left corner, in physical pixels -- painted as quads: an image
/// would take a texture page of the window's sprite atlas. One layer holds them all, the fills
/// being apart, so each takes no search for what it's drawn over: a few hundred at 4K.
fn plate(fills: Arc<[Fill]>) -> Div {
    div().relative().size_full().child(
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let scale = window.scale_factor();
                let logical = |length: i32| px(length as f32 / scale);
                window.paint_layer(bounds, |window| {
                    for fill in fills.iter() {
                        let rect = fill.rect;
                        window.paint_quad(gpui::fill(
                            Bounds::new(
                                bounds.origin + point(logical(rect.x), logical(rect.y)),
                                size(logical(rect.width), logical(rect.height)),
                            ),
                            rgb(fill.colour),
                        ));
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
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
            line = line.child(quad_diamond(SEPARATOR_DIAMOND, tones.separator));
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
