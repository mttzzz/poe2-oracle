//! The XP overlay: one line above PoE2's experience bar -- `+12,4 %/ч · до 75 ур. 1 ч 32 мин
//! игры`, how fast the character levels and how much play is left to the next level
//! (`crate::xp_tracker`) -- and, as the player chooses, how much of the level is earned and the
//! current map's timer: `64,8 % · +12,4 %/ч · до 75 ур. 1 ч 32 мин игры · карта 4:07 +1,2 % ·
//! ср. 6:30`. In a pause (a town or hideout, or five minutes without a gain) the rate and the
//! time to level would pass for current ones, so the line dims and says how long the pause has
//! lasted instead: `64,8 % · пауза · 12 мин · последняя карта 9:00 +3,66 %`. The words are the
//! interface language's (`xp_tracker::rate_words` and the rest): `64.8% · +12.4%/h · level 75 in
//! 1h 32m of play · map 4:07 +1.2% · avg 6:30` in English.
//!
//! A task samples every two seconds, off the UI thread: the game log's new lines
//! (`platform::client_log`), then the bar's pixels (`platform::xp_bar`) -- in that order, so a
//! level-up line is in before the wrap on the bar it explains. The window is always click-through
//! and never focused, sits centred just above the bar (`XpBarGeometry::overlay_rect`), and is
//! shown only while the bar itself is on screen and the price-check panel is closed
//! ([`XpOverlay::set_suppressed`]): the panel can span the bar's middle.
//!
//! The window is opaque and sized to what it shows: a transparent `PopUp` background still tints
//! the game behind it (see `Win32Overlay::set_shown`), which an overlay that stays up for whole
//! mapping sessions can't afford. It follows the interface scale like the price panel.
//!
//! It wears the game-styled look the other windows share (`ui::style`): a near-black bar lit warm
//! from the top, in the thin double gold frame -- too low for corner ornaments, so a diamond sits
//! at the middle of each end instead -- with a small diamond between the parts. The rate is gold,
//! the values bright and the words saying what they are dim; a pause dims each a step, ornaments
//! included, easing there and back over `style::TRANSITION`.

use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, AsyncApp, Bounds, Context, Div, Entity, FontWeight, Global, IntoElement,
    Render, SharedString, WeakEntity, Window, WindowBounds, WindowKind, WindowOptions, div, point,
    prelude::*, px, rgb, size,
};

use crate::overlay_layout::PhysicalRect;
use crate::platform::client_log::{self, ClientLog};
use crate::platform::win32::Win32Overlay;
use crate::platform::xp_bar::{self, BarSample};
use crate::settings::Settings;
use crate::ui::style::{bar_frame, diamond, ease_state, title_gradient};
use crate::ui::theme::{
    BASE_REM_SIZE, BORDER_GOLD, GOLD, GOLD_LIGHT, TEXT, TEXT_DIM, TEXT_MUTED, blend, rems_from_px,
};
use crate::xp_tracker::{
    Activity, MapStatus, RunState, Word, XpStatus, XpTracker, map_words, parse_log_line,
    pause_words, percent_words, rate_words,
};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
/// The window's logical height at 100 % scale.
const HEIGHT: f32 = 26.;
/// Logical widths at 100 % scale: each part's longest wording plus the frame's padding -- where
/// its end diamonds sit -- and the diamond between parts: `+123 %/ч · до 100 ур. 23 ч 59 мин
/// игры` for the rate and `пауза · 23 ч 59 мин` in its place in a pause; `карта 1:23:45 +12,5 %`
/// for the map, with `последняя ` before it for the last one and ` · ср. 12:34` after it outside
/// a pause. The English words fit every part with room to spare -- in Segoe UI's metrics (its
/// stand-in Selawik), `+123%/h · next level in 23h 59m of play` is 248, `paused · 23h 59m` 110,
/// `map 1:23:45 +12.5%` 126, `last ` 25 and ` · avg 12:34` 70.
const PADDING_WIDTH: f32 = 24.;
const SEPARATOR_WIDTH: f32 = 16.;
const PERCENT_WIDTH: f32 = 64.;
const RATE_WIDTH: f32 = 275.;
const PAUSE_WIDTH: f32 = 138.;
const MAP_WIDTH: f32 = 152.;
const LAST_MAP_WIDTH: f32 = 79.;
const AVERAGE_WIDTH: f32 = 78.;
/// The space between a part's words, between the parts and their diamond, and the diamond's size,
/// at 100 % scale.
const WORD_GAP: f32 = 4.;
const PART_GAP: f32 = 6.;
const SEPARATOR_DIAMOND: f32 = 4.;

/// What the overlay shows and how big, from the player's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XpOverlayOptions {
    pub show_percent: bool,
    pub map_timer: bool,
    pub rate_window_minutes: u16,
    pub ui_scale: f32,
}

impl XpOverlayOptions {
    pub fn from_settings(settings: &Settings) -> XpOverlayOptions {
        XpOverlayOptions {
            show_percent: settings.xp_show_percent,
            map_timer: settings.xp_map_timer,
            rate_window_minutes: settings.xp_rate_window_minutes,
            ui_scale: settings.ui_scale,
        }
    }
}

/// Where the XP line is on screen now, in physical pixels -- `None` while it is hidden: the tour
/// (`ui::tour`) points its spotlight at it.
#[derive(Clone, Copy, Default)]
pub struct XpLineOnScreen(pub Option<PhysicalRect>);

impl Global for XpLineOnScreen {}

/// The overlay window's root view: the tracker, what it says, and the platform window state that
/// follows it.
pub struct XpOverlay {
    tracker: XpTracker,
    status: XpStatus,
    options: XpOverlayOptions,
    /// The latest look at the game; the window's rect follows from it and what the line shows.
    sample: Option<BarSample>,
    suppressed: bool,
    /// Resolved on the first render, when the platform window exists.
    overlay: Option<Win32Overlay>,
    /// What was last applied to the platform window; `None` until the first sync.
    last_bounds: Option<PhysicalRect>,
    last_shown: Option<bool>,
}

/// Opens the overlay window -- hidden until the bar is on screen -- and starts sampling. The
/// window owns the returned view; keep the handle only to call [`XpOverlay::set_suppressed`] and
/// [`XpOverlay::set_options`].
pub fn open(options: XpOverlayOptions, cx: &mut App) -> anyhow::Result<Entity<XpOverlay>> {
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
                suppressed: false,
                overlay: None,
                last_bounds: None,
                last_shown: None,
            }
        })
    })?;
    let view = window.entity(cx)?;
    let weak = view.downgrade();
    cx.spawn(async move |cx| sample_forever(weak, cx).await)
        .detach();
    Ok(view)
}

fn window_options() -> WindowOptions {
    WindowOptions {
        // Placeholder: the first sample places the window above the bar, then shows it.
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(RATE_WIDTH), px(HEIGHT)),
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
            view.sample = sample;
            if status != view.status {
                view.status = status;
                cx.notify();
            }
            view.sync_window(cx);
        });
    }
}

impl XpOverlay {
    /// Hides the overlay while `suppressed` is true -- pass whether the price-check window is
    /// shown, whenever that changes.
    pub fn set_suppressed(&mut self, suppressed: bool, cx: &mut Context<Self>) {
        self.suppressed = suppressed;
        self.sync_window(cx);
    }

    /// Takes over the player's saved options: what to show, the rate's averaging window, the
    /// scale.
    pub fn set_options(&mut self, options: XpOverlayOptions, cx: &mut Context<Self>) {
        if options == self.options {
            return;
        }
        self.tracker.set_rate_window(options.rate_window_minutes);
        self.options = options;
        self.status = self.tracker.status();
        cx.notify();
        self.sync_window(cx);
    }

    /// The map part, when the player wants it and there is a map run to show.
    fn map_status(&self) -> Option<MapStatus> {
        self.options.map_timer.then_some(self.status.map).flatten()
    }

    fn paused(&self) -> bool {
        matches!(self.status.activity, Activity::Paused { .. })
    }

    /// The line's logical width at 100 % scale for what it shows now.
    fn content_width(&self) -> f32 {
        let paused = self.paused();
        let mut width = PADDING_WIDTH + if paused { PAUSE_WIDTH } else { RATE_WIDTH };
        if self.options.show_percent && self.status.fraction.is_some() {
            width += SEPARATOR_WIDTH + PERCENT_WIDTH;
        }
        if let Some(map) = self.map_status() {
            width += SEPARATOR_WIDTH + MAP_WIDTH;
            if map.state == RunState::Last {
                width += LAST_MAP_WIDTH;
            }
            if !paused && map.average.is_some() {
                width += AVERAGE_WIDTH;
            }
        }
        width
    }

    /// The window's rect for the latest look at the game: the line's size at the game's DPI and
    /// the player's scale, centred just above the bar.
    fn placement(&self) -> Option<PhysicalRect> {
        let sample = self.sample.as_ref()?;
        let scale = sample.dpi_scale * f64::from(self.options.ui_scale);
        let width = (f64::from(self.content_width()) * scale).round() as i32;
        let height = (f64::from(HEIGHT) * scale).round() as i32;
        Some(sample.geometry.as_ref()?.overlay_rect(width, height))
    }

    fn ensure_overlay(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.overlay.is_some() {
            return;
        }
        match Win32Overlay::from_window(window) {
            Ok(overlay) => {
                if let Err(err) = overlay.disable_dwm_frame() {
                    log::warn!("{err:#}");
                }
                // Spawned before `sync_window`'s first task, so the window is frameless and
                // click-through by the time it is first shown.
                cx.spawn(async move |_, _| {
                    if let Err(err) = overlay.remove_frame() {
                        log::warn!("{err:#}");
                    }
                    if let Err(err) = overlay.set_click_through(true) {
                        log::warn!("set_click_through failed: {err:?}");
                    }
                })
                .detach();
                self.overlay = Some(overlay);
                self.sync_window(cx);
            }
            Err(err) => log::warn!("Win32Overlay::from_window failed: {err:?}"),
        }
    }

    /// Brings the platform window in line with the latest sample. Called from the sampling task,
    /// `set_suppressed` and `set_options`, not only from `render`: a hidden GPUI window is never
    /// redrawn (see `app::PriceCheckRoot::sync_window`), so a render-only sync could never show
    /// it again.
    fn sync_window(&mut self, cx: &mut Context<Self>) {
        let Some(overlay) = self.overlay else {
            return;
        };
        let placement = self.placement();
        // Shown while the bar is readable -- or while the tour spotlights the line, whose dim
        // covers the bar (`platform::xp_bar` then refuses to read it).
        let readable = self.status.bar_visible || crate::ui::tour::holds_xp_line(cx);
        let want_shown = !self.suppressed && readable && placement.is_some();
        let bounds = placement.filter(|rect| self.last_bounds != Some(*rect));
        let shown = (self.last_shown != Some(want_shown)).then_some(want_shown);
        if bounds.is_none() && shown.is_none() {
            return;
        }
        cx.set_global(XpLineOnScreen(placement.filter(|_| want_shown)));
        if bounds.is_some() {
            self.last_bounds = bounds;
        }
        self.last_shown = Some(want_shown);
        // Deferred: `SetWindowPos`/`ShowWindow` send `WM_SIZE`/`WM_SHOWWINDOW` synchronously into
        // GPUI's own window state.
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
    }
}

/// The line's colours, `lit` (0 to 1) of the way from its dimmed pause look to its play look: the
/// values, the words saying what they are, the rate, and the diamonds between the parts.
#[derive(Clone, Copy)]
struct Tones {
    value: u32,
    label: u32,
    rate: u32,
    separator: u32,
}

impl Tones {
    fn at(lit: f32) -> Tones {
        Tones {
            value: blend(TEXT_DIM, TEXT, lit),
            label: blend(TEXT_MUTED, TEXT_DIM, lit),
            rate: blend(TEXT_DIM, GOLD_LIGHT, lit),
            separator: blend(BORDER_GOLD, GOLD, 0.6 * lit),
        }
    }
}

/// A word -- or a value read as one, `1 ч 32 мин` -- in `color`.
fn word(text: impl Into<SharedString>, color: u32) -> Div {
    div().text_color(rgb(color)).child(text.into())
}

/// A part of the line (`xp_tracker::rate_words` and the rest): its words in a row, a space apart,
/// each in its tone -- the rate semibold, the dot between groups muted.
fn part(words: Vec<Word>, tones: Tones) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(rems_from_px(WORD_GAP))
        .children(words.into_iter().map(|each| match each {
            Word::Rate(text) => word(text, tones.rate).font_weight(FontWeight::SEMIBOLD),
            Word::Value(text) => word(text, tones.value),
            Word::Label(text) => word(text, tones.label),
            Word::Dot => word("·", TEXT_MUTED),
        }))
        .into_any_element()
}

impl Render for XpOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_overlay(window, cx);
        window.set_rem_size(px(BASE_REM_SIZE * self.options.ui_scale));
        let status = self.status;
        let percent = self
            .options
            .show_percent
            .then_some(status.fraction)
            .flatten();
        let map = self.map_status();
        let paused = self.paused();
        let surface = div()
            .relative()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(title_gradient())
            .text_size(rems_from_px(14.))
            .whitespace_nowrap();
        ease_state("playing", !paused, surface, move |surface, lit| {
            let tones = Tones::at(lit);
            let mut parts: Vec<AnyElement> = Vec::new();
            if let Some(fraction) = percent {
                parts.push(part(percent_words(fraction), tones));
            }
            let rate = match status.activity {
                Activity::Playing => rate_words(&status),
                Activity::Paused { elapsed, .. } => pause_words(elapsed),
            };
            parts.push(part(rate, tones));
            if let Some(map) = map {
                // Dimmed once the character has left the run.
                let tones = if map.state == RunState::Running {
                    tones
                } else {
                    Tones::at(0.)
                };
                parts.push(part(map_words(&map, paused), tones));
            }
            let mut line = div().flex().items_center().gap(rems_from_px(PART_GAP));
            for (index, part) in parts.into_iter().enumerate() {
                if index > 0 {
                    line = line.child(diamond(SEPARATOR_DIAMOND, tones.separator));
                }
                line = line.child(part);
            }
            surface.child(line).child(bar_frame(lit))
        })
    }
}
