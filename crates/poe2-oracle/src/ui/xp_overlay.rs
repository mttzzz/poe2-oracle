//! The XP overlay: two plates inlaid in the game's HUD, in the top rails of the panels either side
//! of the experience bar (`overlay_layout::hud_rails`), so it reads as part of the game's own
//! interface, not as something laid over the game:
//!
//! - the flask panel's rail: how much of the level is earned, how fast the character levels and
//!   how much play is left to the next level (`crate::xp_tracker`) -- `64,8 % ◆ +12,4 %/ч · до
//!   75 ур. 2 ч 50 мин` -- and, at its right end, the gear that opens the settings. In a pause (a
//!   town or hideout, or five minutes without a gain) the rate and the time to level would pass
//!   for current ones, so the plate dims and says how long the pause has lasted instead:
//!   `64,8 % ◆ пауза · 12 мин`;
//! - the skill panel's rail, while the player keeps the map timer on and there is a run to show:
//!   the map's time and experience next to the average map time -- `карта 4:07 +1,2 % · ср.
//!   6:30`, and `последняя карта 9:00 +3,66 %`, dimmed, once the run is over.
//!
//! A plate says as much as fits its rail, measured in its own typeface: the full wording, else the
//! shorter one (`xp_tracker::Wording`), else -- the level plate -- the shorter one without the
//! percent; once shortened, it stays so while its state lasts, not flipping as a number gains or
//! loses a digit. The words are the interface language's: `64.8% ◆ +12.4%/h · level 75 in 2h
//! 50m` in English.
//!
//! A task samples every two seconds, off the UI thread: the game log's new lines
//! (`platform::client_log`), then the bar's pixels (`platform::xp_bar`) -- in that order, so a
//! level-up line is in before the wrap on the bar it explains. The plates show while the bar
//! itself is on screen -- the HUD is, then -- and through a price check, whose panel spans the
//! bar's middle; each unless the price-check panel covers the plate itself or the setting is off
//! ([`XpOverlay::set_cover`]). Their size is the game's, not the app's interface scale: at any
//! game height a plate is its rail's size, and its words the HUD's.
//!
//! Each plate is its own opaque window (a transparent `PopUp` background still tints the game
//! behind it, see `Win32Overlay::set_shown`) in the HUD's colours (`ui::theme`'s `HUD_*`): a
//! near-black slot under the rail's lip, rimmed with bronze below, the values cream, the words
//! saying what they are muted, the rate in the HUD's gold, a small diamond between the parts. The
//! level plate takes clicks, for its gear, but never the keyboard; the map plate lets them
//! through to the game.

use std::time::{Duration, Instant};

use gpui::{
    App, AsyncApp, Bounds, Context, Div, Entity, Font, Global, Hsla, IntoElement, MouseButton,
    MouseDownEvent, Pixels, Render, SharedString, TextRun, WeakEntity, Window, WindowBounds,
    WindowKind, WindowOptions, div, linear_color_stop, linear_gradient, point, prelude::*, px, rgb,
    size,
};

use crate::i18n::{self, Lang};
use crate::overlay_layout::{PhysicalRect, hud_rails};
use crate::platform::client_log::{self, ClientLog};
use crate::platform::win32::Win32Overlay;
use crate::platform::xp_bar::{self, BarSample};
use crate::price_check::PriceCheckApp;
use crate::settings::Settings;
use crate::ui::fonts;
use crate::ui::style::{diamond, ease_hover, ease_state};
use crate::ui::theme::{
    BASE_REM_SIZE, HUD_GOLD, HUD_LABEL, HUD_RIM_LIGHT, HUD_RIM_SHADE, HUD_SLOT_BOTTOM,
    HUD_SLOT_TOP, HUD_TEXT, blend, rems_from_px,
};
use crate::xp_tracker::{
    Activity, MapStatus, RunState, Word, Wording, XpStatus, XpTracker, map_words, parse_log_line,
    pause_words, percent_words, rate_words,
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
/// The gear's column at the level plate's right end, and the glyph's size: Segoe UI Symbol draws
/// its gear thin, so a size up on the words.
const GEAR_WIDTH: f32 = 16.;
const GEAR_SIZE: f32 = 14.;
/// The rim's thickness.
const RIM: f32 = 1.;

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

/// Where the level plate is on screen now, in physical pixels -- `None` while it is hidden: the
/// tour (`ui::tour`) points its spotlight at it.
#[derive(Clone, Copy, Default)]
pub struct XpLineOnScreen(pub Option<PhysicalRect>);

impl Global for XpLineOnScreen {}

#[derive(Clone, Copy)]
enum Plate {
    Level,
    Map,
}

/// A plate's platform window and what was last applied to it.
#[derive(Default)]
struct PlateWindow {
    /// Resolved on the window's first render, when the platform window exists.
    overlay: Option<Win32Overlay>,
    /// `None` until the first sync.
    last_bounds: Option<PhysicalRect>,
    last_shown: Option<bool>,
}

impl PlateWindow {
    /// Moves the window to `rect` and shows it, or hides it for `None`; returns whether anything
    /// changed. Deferred: `SetWindowPos`/`ShowWindow` send `WM_SIZE`/`WM_SHOWWINDOW` synchronously
    /// into GPUI's own window state.
    fn sync<T: 'static>(&mut self, rect: Option<PhysicalRect>, cx: &mut Context<T>) -> bool {
        let Some(overlay) = self.overlay else {
            return false;
        };
        let bounds = rect.filter(|rect| self.last_bounds != Some(*rect));
        let shown = (self.last_shown != Some(rect.is_some())).then_some(rect.is_some());
        if bounds.is_none() && shown.is_none() {
            return false;
        }
        if bounds.is_some() {
            self.last_bounds = bounds;
        }
        self.last_shown = Some(rect.is_some());
        cx.spawn(async move |_, _| {
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

/// The level plate's window's root view, and the overlay's state: the tracker, what it says, and
/// both plates' platform windows.
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
    map: PlateWindow,
    /// The level plate's wording: paused, rated, with the percent, in what language and room.
    level_fit: Fit<(bool, bool, bool, Lang, Pixels)>,
}

/// The map plate's window's root view: it shows what the [`XpOverlay`] knows.
pub struct MapPlate {
    xp: Entity<XpOverlay>,
    attached: bool,
    /// The plate's wording: for which state of the run, with or without the average, in what
    /// language and room.
    fit: Fit<(RunState, bool, Lang, Pixels)>,
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

/// Opens the plates' windows -- hidden until the bar is on screen -- and starts sampling. The
/// level plate's window owns the returned view; keep the handle only to call
/// [`XpOverlay::set_cover`] and [`XpOverlay::set_options`].
pub fn open(
    options: XpOverlayOptions,
    app: WeakEntity<PriceCheckApp>,
    cx: &mut App,
) -> anyhow::Result<Entity<XpOverlay>> {
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
                map: PlateWindow::default(),
                level_fit: Fit::default(),
            }
        })
    })?;
    let view = window.entity(cx)?;
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
    Ok(view)
}

fn window_options() -> WindowOptions {
    WindowOptions {
        // Placeholder: the first sample puts the window in its rail, then shows it.
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(230.), px(16.)),
        ))),
        titlebar: None,
        kind: WindowKind::PopUp,
        is_movable: false,
        focus: false,
        show: false,
        ..Default::default()
    }
}

/// Feeds the tracker until the window closes.
async fn sample_forever(view: WeakEntity<XpOverlay>, cx: &mut AsyncApp) {
    let start = Instant::now();
    let mut log: Option<ClientLog> = None;
    loop {
        cx.background_executor().timer(SAMPLE_INTERVAL).await;
        let (history, events, sample, still_open) = cx
            .background_executor()
            .spawn(async move {
                let mut log = log;
                let mut history = Vec::new();
                // Retried every sample until the game runs: the log is found through its process.
                if log.is_none()
                    && let Some((opened, replayed)) =
                        ClientLog::open(client_log::HISTORY_BYTES, parse_log_line)
                {
                    log = Some(opened);
                    history = replayed;
                }
                let events = log
                    .as_mut()
                    .map(|log| log.poll(parse_log_line))
                    .unwrap_or_default();
                (history, events, xp_bar::sample(), log)
            })
            .await;
        log = still_open;
        let at = start.elapsed();
        let Some(view) = view.upgrade() else {
            return;
        };
        view.update(cx, |view, cx| {
            view.tracker.restore(history, at);
            for event in events {
                view.tracker.on_log_event(event, at);
            }
            view.tracker
                .on_sample(sample.as_ref().and_then(|sample| sample.fill), at);
            let status = view.tracker.status();
            // A moved or rescaled game moves the plates and resizes their words.
            let place = |sample: &Option<BarSample>| {
                sample
                    .as_ref()
                    .map(|sample| (sample.client, sample.dpi_scale))
            };
            let moved = place(&view.sample) != place(&sample);
            view.sample = sample;
            if status != view.status || moved {
                view.status = status;
                cx.notify();
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
        cx.notify();
        self.sync_windows(cx);
    }

    /// The map plate's run, when the player wants it and there is one to show.
    fn map_status(&self) -> Option<MapStatus> {
        self.options.map_timer.then_some(self.status.map).flatten()
    }

    fn paused(&self) -> bool {
        matches!(self.status.activity, Activity::Paused { .. })
    }

    /// The level plate's wordings, longest first: in full, short, then short without the
    /// percent.
    fn level_wordings(&self) -> Vec<Vec<Vec<Word>>> {
        let status = self.status;
        let percent = self
            .options
            .show_percent
            .then_some(status.fraction)
            .flatten()
            .map(percent_words);
        let rate = |wording| match status.activity {
            Activity::Playing => rate_words(&status, wording),
            Activity::Paused { elapsed, .. } => pause_words(elapsed),
        };
        let with_percent = |rate: Vec<Word>| percent.iter().cloned().chain([rate]).collect();
        vec![
            with_percent(rate(Wording::Full)),
            with_percent(rate(Wording::Short)),
            vec![rate(Wording::Short)],
        ]
    }

    /// The map plate's wordings, longest first.
    fn map_wordings(&self, map: &MapStatus) -> Vec<Vec<Vec<Word>>> {
        let paused = self.paused();
        [Wording::Full, Wording::Short]
            .map(|wording| vec![map_words(map, paused, wording)])
            .into()
    }

    /// Takes a plate's platform window once it exists: frameless, and the level plate never
    /// taking the keyboard -- a click on its gear leaves it with the game -- while the map plate
    /// lets clicks through.
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
                Plate::Level => overlay.set_no_activate(),
                Plate::Map => overlay.set_click_through(true),
            };
            if let Err(err) = styled {
                log::warn!("{err:#}");
            }
        })
        .detach();
        match plate {
            Plate::Level => self.level.overlay = Some(overlay),
            Plate::Map => self.map.overlay = Some(overlay),
        }
        self.sync_windows(cx);
    }

    /// Brings the plates' windows in line with the latest sample. Called from the sampling task,
    /// `set_cover`, `set_options` and a window's first render, not from `render` alone: a hidden
    /// GPUI window is never redrawn (see `app::PriceCheckRoot::sync_window`), so a render-only
    /// sync could never show it again.
    fn sync_windows(&mut self, cx: &mut Context<Self>) {
        let rails = self.sample.as_ref().map(|sample| hud_rails(sample.client));
        // Shown while the bar is readable, and on while something of ours leaves it unreadable
        // (`platform::xp_bar` refuses a covered bar) with the HUD in view: the price panel, over
        // the bar's middle, or the tour's dim while it spotlights the level plate.
        let readable = self.status.bar_visible
            || self.cover.panel.is_some()
            || crate::ui::tour::holds_xp_line(cx);
        let cover = self.cover;
        let clear = |rect: &PhysicalRect| {
            !cover.off && readable && cover.panel.is_none_or(|panel| !panel.intersects(rect))
        };
        let level = rails.map(|rails| rails.flask).filter(clear);
        let map = rails
            .map(|rails| rails.skill)
            .filter(|rect| self.map_status().is_some() && clear(rect));
        if self.level.sync(level, cx) {
            cx.set_global(XpLineOnScreen(level));
        }
        self.map.sync(map, cx);
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
        let Some(sample) = self.sample.clone() else {
            return div().into_any_element();
        };
        window.set_rem_size(hud_rem_size(&sample));
        let font = plate_font(window);
        let room = room(
            hud_rails(sample.client).flask,
            &sample,
            2. * PADDING_X + PART_GAP + GEAR_WIDTH,
            window,
        );
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
        let app = self.app.clone();
        ease_state("playing", !self.paused(), slot(), move |slot, lit| {
            slot.child(words(parts.clone(), &font, Tones::at(lit)))
                .child(gear(app.clone()))
        })
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
        let (Some(sample), Some(map)) = (xp.sample.clone(), xp.map_status()) else {
            return div().into_any_element();
        };
        window.set_rem_size(hud_rem_size(&sample));
        let font = plate_font(window);
        let room = room(
            hud_rails(sample.client).skill,
            &sample,
            2. * PADDING_X,
            window,
        );
        let key = (
            map.state,
            map.average.is_some() && !xp.paused(),
            i18n::lang(),
            room,
        );
        let parts = self
            .fit
            .choose(key, xp.map_wordings(&map), room, &font, window);
        // Dimmed once the character has left the run.
        let running = map.state == RunState::Running;
        ease_state("running", running, slot(), move |slot, lit| {
            slot.child(words(parts.clone(), &font, Tones::at(lit)))
        })
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
/// pixels: less `reserved` HUD pixels for its ends and gear.
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
        let dim = |color: u32| blend(HUD_SLOT_BOTTOM, color, 0.65);
        Tones {
            value: blend(HUD_LABEL, HUD_TEXT, lit),
            label: blend(dim(HUD_LABEL), HUD_LABEL, lit),
            rate: blend(HUD_LABEL, HUD_GOLD, lit),
            separator: blend(HUD_RIM_LIGHT, HUD_GOLD, 0.5 * lit),
        }
    }
}

/// A plate's slot: the near-black of the HUD's recessed plates, rimmed like one -- in shade along
/// the top, under the rail's lip, lit bronze along the bottom, between the two at the ends -- its
/// contents in a row.
fn slot() -> Div {
    let across = |edge: Div, color: u32| {
        edge.absolute()
            .left_0()
            .right_0()
            .h(rems_from_px(RIM))
            .bg(rgb(color))
    };
    let down = |edge: Div| {
        edge.absolute()
            .top_0()
            .bottom_0()
            .w(rems_from_px(RIM))
            .bg(rgb(blend(HUD_RIM_SHADE, HUD_RIM_LIGHT, 0.5)))
    };
    div()
        .relative()
        .size_full()
        .flex()
        .items_center()
        .px(rems_from_px(PADDING_X))
        .bg(linear_gradient(
            180.,
            linear_color_stop(rgb(HUD_SLOT_TOP), 0.),
            linear_color_stop(rgb(HUD_SLOT_BOTTOM), 1.),
        ))
        .child(down(div().left_0()))
        .child(down(div().right_0()))
        .child(across(div().top_0(), HUD_RIM_SHADE))
        .child(across(div().bottom_0(), HUD_RIM_LIGHT))
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

/// The level plate's gear: it opens the settings, lighting from the words' muted tone to the
/// HUD's gold under the pointer. Drawn from Segoe UI Symbol, whose gear is a flat glyph that
/// takes the colour -- the fallback would otherwise find Segoe UI Emoji's grey picture.
fn gear(app: WeakEntity<PriceCheckApp>) -> impl IntoElement {
    let gear = div()
        .id("settings")
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .w(rems_from_px(GEAR_WIDTH))
        .h_full()
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
