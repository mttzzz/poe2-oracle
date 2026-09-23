//! The XP overlay: one line above PoE2's experience bar -- `+12,4 %/ч · до 75 ур. 1 ч 32 мин`,
//! how fast the character levels and how much play is left to the next level
//! (`crate::xp_tracker`) -- and, as the player chooses, how much of the level is earned and the
//! current map's timer: `64,8 % · +12,4 %/ч · до 75 ур. 1 ч 32 мин · карта 4:07 +1,2 % · ср. 6:30`.
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

use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, AsyncApp, Bounds, Context, Entity, FontWeight, IntoElement, Render,
    WeakEntity, Window, WindowBounds, WindowKind, WindowOptions, div, point, prelude::*, px, rgb,
    size,
};

use crate::overlay_layout::PhysicalRect;
use crate::platform::client_log::{self, ClientLog};
use crate::platform::win32::Win32Overlay;
use crate::platform::xp_bar::{self, BarSample};
use crate::settings::Settings;
use crate::ui::theme::{
    BASE_REM_SIZE, BG_PANEL, BORDER_GOLD, GOLD, TEXT, TEXT_DIM, TEXT_MUTED, rems_from_px,
};
use crate::xp_tracker::{
    MapStatus, XpStatus, XpTracker, format_clock, format_eta, format_percent, format_rate,
    parse_log_line,
};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
/// The window's logical height at 100 % scale.
const HEIGHT: f32 = 26.;
/// Logical widths at 100 % scale: each part's longest wording plus the frame's padding and the
/// `·` between parts -- `+123 %/ч · до след. ур. 23 ч 59 мин` for the rate,
/// `карта 12:34 +12,5 % · ср. 12:34` for the map.
const PADDING_WIDTH: f32 = 24.;
const SEPARATOR_WIDTH: f32 = 16.;
const PERCENT_WIDTH: f32 = 64.;
const RATE_WIDTH: f32 = 250.;
const MAP_WIDTH: f32 = 214.;

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
            view.tracker.restore(history);
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

    /// The line's logical width at 100 % scale for what it shows now.
    fn content_width(&self) -> f32 {
        let mut width = PADDING_WIDTH + RATE_WIDTH;
        if self.options.show_percent && self.status.fraction.is_some() {
            width += SEPARATOR_WIDTH + PERCENT_WIDTH;
        }
        if self.map_status().is_some() {
            width += SEPARATOR_WIDTH + MAP_WIDTH;
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
        let want_shown = !self.suppressed && self.status.bar_visible && placement.is_some();
        let bounds = placement.filter(|rect| self.last_bounds != Some(*rect));
        let shown = (self.last_shown != Some(want_shown)).then_some(want_shown);
        if bounds.is_none() && shown.is_none() {
            return;
        }
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

    /// `+12,4 %/ч · до 75 ур. 1 ч 32 мин`, or the wait for a first rate.
    fn rate_part(&self) -> AnyElement {
        let status = self.status;
        let Some(rate) = status.rate_per_hour else {
            // The first two minutes of play, before there is a rate to show.
            return div()
                .text_color(rgb(TEXT_DIM))
                .child("замер скорости…")
                .into_any_element();
        };
        let target = match status.level {
            Some(level) => format!("до {} ур.", level + 1),
            None => "до след. ур.".to_owned(),
        };
        let eta = status
            .time_to_level()
            .map_or_else(|| "—".to_owned(), format_eta);
        div()
            .flex()
            .items_center()
            .gap(rems_from_px(6.))
            .child(
                div()
                    .text_color(rgb(GOLD))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format_rate(rate)),
            )
            .child(separator())
            .child(format!("{target} {eta}"))
            .into_any_element()
    }

    /// `карта 4:07 +1,2 % · ср. 6:30`: dimmed while the run waits in the hideout.
    fn map_part(map: MapStatus) -> AnyElement {
        let mut text = format!("карта {}", format_clock(map.time));
        if map.gained > 0.0 {
            text += &format!(" +{} %", format_percent(map.gained));
        }
        div()
            .flex()
            .items_center()
            .gap(rems_from_px(6.))
            .child(
                div()
                    .text_color(rgb(if map.active { TEXT } else { TEXT_DIM }))
                    .child(text),
            )
            .children(map.average.map(|average| {
                div()
                    .text_color(rgb(TEXT_DIM))
                    .child(format!("· ср. {}", format_clock(average)))
            }))
            .into_any_element()
    }
}

fn separator() -> impl IntoElement {
    div().text_color(rgb(TEXT_MUTED)).child("·")
}

impl Render for XpOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_overlay(window, cx);
        window.set_rem_size(px(BASE_REM_SIZE * self.options.ui_scale));
        let mut parts: Vec<AnyElement> = Vec::new();
        if self.options.show_percent
            && let Some(fraction) = self.status.fraction
        {
            parts.push(
                div()
                    .child(format!("{} %", format_percent(fraction)))
                    .into_any_element(),
            );
        }
        parts.push(self.rate_part());
        if let Some(map) = self.map_status() {
            parts.push(Self::map_part(map));
        }
        let mut line = div()
            .flex()
            .items_center()
            .gap(rems_from_px(6.))
            .whitespace_nowrap();
        for (index, part) in parts.into_iter().enumerate() {
            if index > 0 {
                line = line.child(separator());
            }
            line = line.child(part);
        }
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(BG_PANEL))
            .border_1()
            .border_color(rgb(BORDER_GOLD))
            .text_sm()
            .text_color(rgb(TEXT))
            .child(line)
    }
}
