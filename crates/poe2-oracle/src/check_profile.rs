//! What one price check costs, step by step: the log line a live measurement reads
//! (`platform::check_clock` reads the clocks on Windows and keeps the check under way; this module
//! adds the readings up and words them, and builds -- and is tested -- everywhere).
//!
//! A reading takes three clocks: the wall clock; the CPU cycles of the UI thread, where every step
//! of a check runs but the network itself -- the copy's wait, the parse, the trade site's answers
//! read and parsed, and GPUI's layout and drawing of each frame; and the cycles of the whole
//! process, whose other threads carry the network, the images' decoding and the graphics driver's
//! work. Windows counts cycles on the time-stamp counter (`QueryThreadCycleTime`,
//! `QueryProcessCycleTime`), whose rate [`CycleRate`] measures against the wall clock, so the line
//! says CPU as time.
//!
//! A check runs from the key's press until its panel settles: its search answered, or none made,
//! and [`SETTLE`] without a frame since -- or until the panel hides or the next check starts.
//!
//! A frame's UI-thread CPU is split where the panel's own paint ends: its drawing -- GPUI's render,
//! layout, prepaint and paint of the panel -- and its presenting -- a tooltip's paint, the scene
//! sorted and the renderer's upload, draws and `Present`. The panel draws in parts GPUI keeps from
//! frame to frame (`ui::panel::part`): the line says how many of the parts laid out in its frames
//! were drawn afresh, the rest drawn from their last frame.

use std::fmt::Write as _;
use std::ops::AddAssign;
use std::time::Duration;

/// How long a check stays open after its last frame once its search has answered: the listings'
/// currency icons and the item's art come in after the rows, each drawing a frame of its own.
pub const SETTLE: Duration = Duration::from_millis(1500);

/// One reading of the clocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reading {
    /// Wall time, from an origin the reader keeps.
    pub wall: Duration,
    /// The UI thread's CPU cycles so far.
    pub thread: u64,
    /// The whole process's CPU cycles so far; `None` where only the thread's were read.
    pub process: Option<u64>,
}

/// What a step took: wall time, and the UI thread's CPU cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Spent {
    pub wall: Duration,
    pub cycles: u64,
}

impl Spent {
    /// From `start` to `end`, both read on the UI thread.
    pub fn between(start: Reading, end: Reading) -> Spent {
        Spent {
            wall: end.wall.saturating_sub(start.wall),
            cycles: end.thread.saturating_sub(start.thread),
        }
    }
}

impl AddAssign for Spent {
    fn add_assign(&mut self, other: Spent) {
        self.wall += other.wall;
        self.cycles += other.cycles;
    }
}

/// Something a check does any number of times: frames drawn, paints with nothing to draw, resizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    pub count: u32,
    pub spent: Spent,
}

impl Tally {
    fn add(&mut self, spent: Spent) {
        self.count += 1;
        self.spent += spent;
    }
}

/// The time-stamp counter's rate: its ticks a second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CycleRate(f64);

impl CycleRate {
    /// How far apart two readings of the counter must be to say its rate to a part in a thousand
    /// or better: the wall clock reads to a tenth of a microsecond.
    pub const MIN_SPAN: Duration = Duration::from_millis(100);

    /// The rate between two `(wall time, counter)` readings: `None` if they're less than
    /// [`Self::MIN_SPAN`] apart, or the counter didn't move forward.
    pub fn between(earlier: (Duration, u64), later: (Duration, u64)) -> Option<CycleRate> {
        let span = later.0.checked_sub(earlier.0)?;
        let ticks = later.1.checked_sub(earlier.1)?;
        (span >= Self::MIN_SPAN && ticks > 0).then(|| CycleRate(ticks as f64 / span.as_secs_f64()))
    }

    /// How long `cycles` ran for at this rate.
    pub fn time(self, cycles: u64) -> Duration {
        Duration::from_secs_f64(cycles as f64 / self.0)
    }
}

/// A message GPUI answered for the panel's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelMessage {
    /// `WM_PAINT`: a frame, if anything in it changed.
    Paint,
    /// `WM_SIZE`: the swap chain and the paths' textures made anew at the window's new size.
    Size,
    /// `WM_SHOWWINDOW`: shown, the window's first frame drawn before it appears.
    Show,
}

/// One price check's steps.
#[derive(Debug, Clone)]
pub struct CheckProfile {
    /// The run's how-manieth check.
    number: u64,
    /// The key's press.
    start: Reading,
    /// From the press until the game's text was in hand, the player's clipboard back.
    copied: Option<Spent>,
    /// The parse, the filters and the route, and the panel told to show.
    parsed: Option<Spent>,
    /// The window placed and sized for the panel (`SetWindowPos`).
    placed: Option<Spent>,
    /// The window shown (`ShowWindow`), its first frame drawn inside.
    shown: Option<(Spent, Duration)>,
    /// The first frame after the parse, and when it was done, from the press.
    first_frame: Option<(Spent, Duration)>,
    frames: Tally,
    idle_paints: Tally,
    resizes: Tally,
    /// The trade search's and the listings' fetch's answers, from the request's start.
    search: Option<Duration>,
    fetch: Option<Duration>,
    /// When the search's outcome was in the panel, from the press.
    results: Option<Duration>,
    /// A search is under way.
    searching: bool,
    /// When the check last did something that shows: the parse, a frame, the outcome.
    last_change: Duration,
    /// The frames' UI-thread CPU up to the end of the panel's paint, and after it.
    drawing: Spent,
    presenting: Spent,
    /// The panel's parts laid out in the frames, and those of them drawn afresh.
    parts_placed: u64,
    parts_drawn: u64,
}

impl CheckProfile {
    /// The `number`th check, its key pressed at `start`.
    pub fn new(number: u64, start: Reading) -> CheckProfile {
        CheckProfile {
            number,
            start,
            copied: None,
            parsed: None,
            placed: None,
            shown: None,
            first_frame: None,
            frames: Tally::default(),
            idle_paints: Tally::default(),
            resizes: Tally::default(),
            search: None,
            fetch: None,
            results: None,
            searching: false,
            last_change: start.wall,
            drawing: Spent::default(),
            presenting: Spent::default(),
            parts_placed: 0,
            parts_drawn: 0,
        }
    }

    /// Whether the game's text came in: a check that got that far is worth a line.
    pub fn copied_text(&self) -> bool {
        self.copied.is_some()
    }

    /// The game's text in hand at `at`.
    pub fn copied(&mut self, at: Reading) {
        self.copied = Some(Spent::between(self.start, at));
        self.last_change = at.wall;
    }

    /// Parsed, from `start` to `end`.
    pub fn parsed(&mut self, start: Reading, end: Reading) {
        self.parsed = Some(Spent::between(start, end));
        self.last_change = end.wall;
    }

    /// The window placed.
    pub fn placed(&mut self, spent: Spent) {
        self.placed = Some(spent);
    }

    /// The window shown, `ShowWindow` returning at `end`.
    pub fn shown(&mut self, spent: Spent, end: Reading) {
        self.shown = Some((spent, end.wall.saturating_sub(self.start.wall)));
    }

    /// GPUI answered `message` for the panel's window in `spent`, done at `end`; `drew` if it drew
    /// a frame meanwhile.
    pub fn panel_message(&mut self, message: PanelMessage, spent: Spent, drew: bool, end: Reading) {
        match (message, drew) {
            (PanelMessage::Size, _) => self.resizes.add(spent),
            (PanelMessage::Paint | PanelMessage::Show, true) => {
                self.frames.add(spent);
                if self.first_frame.is_none() && self.parsed.is_some() {
                    self.first_frame = Some((spent, end.wall.saturating_sub(self.start.wall)));
                }
                self.last_change = end.wall;
            }
            (PanelMessage::Paint, false) => self.idle_paints.add(spent),
            (PanelMessage::Show, false) => {}
        }
    }

    /// A frame's CPU split where the panel's paint ended: its `drawing` up to there, its
    /// `presenting` after.
    pub fn frame_split(&mut self, drawing: Spent, presenting: Spent) {
        self.drawing += drawing;
        self.presenting += presenting;
    }

    /// A frame laid out `placed` of the panel's parts and drew `drawn` of them afresh.
    pub fn parts(&mut self, placed: u64, drawn: u64) {
        self.parts_placed += placed;
        self.parts_drawn += drawn;
    }

    /// A search set off: the check isn't settled till its outcome is in.
    pub fn searching(&mut self) {
        self.searching = true;
    }

    /// The trade search answered after `wall`; only the check's first search counts.
    pub fn searched(&mut self, wall: Duration) {
        self.search.get_or_insert(wall);
    }

    /// The listings' fetch answered after `wall`; only the check's first counts.
    pub fn fetched(&mut self, wall: Duration) {
        self.fetch.get_or_insert(wall);
    }

    /// The search's outcome in the panel at `at`.
    pub fn results(&mut self, at: Reading) {
        self.searching = false;
        self.results
            .get_or_insert(at.wall.saturating_sub(self.start.wall));
        self.last_change = at.wall;
    }

    /// Whether the check is over at `now`: parsed, its search's outcome in -- or none made -- and
    /// nothing drawn for [`SETTLE`].
    pub fn settled(&self, now: Duration) -> bool {
        self.parsed.is_some() && !self.searching && now >= self.last_change + SETTLE
    }

    /// The check's line, ending at `end`: the times from the press, then each step's wall time and
    /// the UI thread's CPU, in milliseconds -- CPU only with the counter's `rate` -- and
    /// `gpu_memory`, the process's dedicated GPU memory in bytes, when known.
    pub fn summary(
        &self,
        end: Reading,
        rate: Option<CycleRate>,
        gpu_memory: Option<u64>,
    ) -> String {
        let ms = |time: Duration| time.as_secs_f64() * 1000.;
        let cpu = |cycles: u64| match rate {
            Some(rate) => format!("{:.1}", ms(rate.time(cycles))),
            None => "?".to_owned(),
        };
        let step = |spent: Spent| format!("{:.1}/{}", ms(spent.wall), cpu(spent.cycles));

        let mut line = format!(
            "price check {} timing (ms, wall/UI-thread CPU):",
            self.number
        );
        let mut marks = Vec::new();
        if let Some((_, at)) = self.shown {
            marks.push(format!("shown at {:.0}", ms(at)));
        }
        if let Some((_, at)) = self.first_frame {
            marks.push(format!("first frame at {:.0}", ms(at)));
        }
        if let Some(at) = self.results {
            marks.push(format!("results at {:.0}", ms(at)));
        }
        if !marks.is_empty() {
            let _ = write!(line, " {};", marks.join(", "));
        }

        let mut steps = Vec::new();
        let named = [
            ("copy", self.copied),
            ("parse", self.parsed),
            ("place", self.placed),
            ("show", self.shown.map(|(spent, _)| spent)),
            ("first frame", self.first_frame.map(|(spent, _)| spent)),
        ];
        for (name, spent) in named {
            if let Some(spent) = spent {
                steps.push(format!("{name} {}", step(spent)));
            }
        }
        if let Some(search) = self.search {
            steps.push(format!("search {:.0}", ms(search)));
        }
        if let Some(fetch) = self.fetch {
            steps.push(format!("fetch {:.0}", ms(fetch)));
        }
        if !steps.is_empty() {
            let _ = write!(line, " {};", steps.join(", "));
        }

        let said =
            |name: &str, tally: Tally| format!("{} {name} {}", tally.count, step(tally.spent));
        let mut tallies = Vec::new();
        if self.frames.count > 0 {
            let mut frames = said("frames", self.frames);
            if self.drawing.cycles + self.presenting.cycles > 0 {
                let _ = write!(
                    frames,
                    " (drawing {}, presenting {})",
                    cpu(self.drawing.cycles),
                    cpu(self.presenting.cycles)
                );
            }
            tallies.push(frames);
        }
        for (name, tally) in [("idle paints", self.idle_paints), ("resizes", self.resizes)] {
            if tally.count > 0 {
                tallies.push(said(name, tally));
            }
        }
        if !tallies.is_empty() {
            let _ = write!(line, " {};", tallies.join(", "));
        }
        if self.parts_placed > 0 {
            let _ = write!(
                line,
                " {} of {} panel parts drawn afresh;",
                self.parts_drawn, self.parts_placed
            );
        }

        let _ = write!(
            line,
            " CPU over {:.0}: UI thread {}",
            ms(end.wall.saturating_sub(self.start.wall)),
            cpu(end.thread.saturating_sub(self.start.thread))
        );
        if let (Some(start), Some(end)) = (self.start.process, end.process) {
            let _ = write!(line, ", process {}", cpu(end.saturating_sub(start)));
        }
        if let Some(bytes) = gpu_memory {
            let _ = write!(
                line,
                "; GPU memory {:.1} MiB",
                bytes as f64 / (1024. * 1024.)
            );
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test machine's counter: 3.418 GHz.
    const RATE_HZ: u64 = 3_418_000_000;

    fn at(ms: u64) -> Reading {
        Reading {
            wall: Duration::from_millis(ms),
            thread: ms * 1_000_000,
            process: Some(ms * 2_000_000),
        }
    }

    fn frame(profile: &mut CheckProfile, done_at_ms: u64) {
        let end = at(done_at_ms);
        let spent = Spent::between(at(done_at_ms - 5), end);
        profile.panel_message(PanelMessage::Paint, spent, true, end);
    }

    #[test]
    fn counter_ticks_read_as_time_at_the_rate_measured_against_the_wall_clock() {
        let rate = CycleRate::between(
            (Duration::from_secs(10), 1_000),
            (Duration::from_secs(12), 1_000 + 2 * RATE_HZ),
        )
        .unwrap();
        let millisecond = rate.time(RATE_HZ / 1000);
        assert!(
            millisecond.abs_diff(Duration::from_millis(1)) < Duration::from_nanos(10),
            "{millisecond:?}"
        );
    }

    #[test]
    fn readings_too_close_or_out_of_order_say_no_rate() {
        let start = (Duration::from_secs(1), 5_000);
        let soon = (
            Duration::from_secs(1) + CycleRate::MIN_SPAN / 2,
            5_000 + RATE_HZ,
        );
        assert_eq!(CycleRate::between(start, soon), None);
        let earlier = (Duration::ZERO, 1_000);
        assert_eq!(CycleRate::between(start, earlier), None);
        let stuck = (Duration::from_secs(5), 5_000);
        assert_eq!(CycleRate::between(start, stuck), None);
    }

    #[test]
    fn a_check_waiting_on_its_search_never_settles() {
        let mut profile = CheckProfile::new(1, at(0));
        profile.copied(at(15));
        profile.parsed(at(15), at(18));
        profile.searching();
        frame(&mut profile, 40);
        assert!(!profile.settled(Duration::from_secs(60)));
    }

    #[test]
    fn a_check_settles_only_once_its_last_frame_is_settle_old() {
        let mut profile = CheckProfile::new(1, at(0));
        profile.copied(at(15));
        profile.parsed(at(15), at(18));
        profile.searching();
        profile.results(at(700));
        // The listings' icons came in after the rows.
        frame(&mut profile, 900);
        let settle_ms = SETTLE.as_millis() as u64;
        assert!(!profile.settled(Duration::from_millis(700 + settle_ms)));
        assert!(profile.settled(Duration::from_millis(900 + settle_ms)));
    }

    #[test]
    fn a_problem_with_no_search_settles_after_its_frames() {
        let mut profile = CheckProfile::new(1, at(0));
        profile.copied(at(15));
        profile.parsed(at(15), at(16));
        frame(&mut profile, 30);
        let settle_ms = SETTLE.as_millis() as u64;
        assert!(profile.settled(Duration::from_millis(30 + settle_ms)));
    }

    #[test]
    fn the_first_frame_is_the_first_drawn_after_the_parse() {
        let mut profile = CheckProfile::new(1, at(0));
        // The panel, open on the last item, drew a hover while the game copied.
        frame(&mut profile, 10);
        profile.copied(at(15));
        profile.parsed(at(15), at(18));
        // Shown: GPUI draws the window's first frame inside `ShowWindow`.
        let end = at(40);
        profile.panel_message(PanelMessage::Show, Spent::between(at(30), end), true, end);
        profile.shown(Spent::between(at(29), at(41)), at(41));
        frame(&mut profile, 60);
        let line = profile.summary(at(3000), None, None);
        assert!(line.contains("first frame at 40"), "{line}");
        assert!(line.contains("shown at 41"), "{line}");
        assert!(line.contains("3 frames"), "{line}");
    }

    #[test]
    fn the_line_says_each_step_as_wall_time_and_cpu_time() {
        let rate = CycleRate(1e9);
        let mut profile = CheckProfile::new(7, at(0));
        profile.copied(at(16));
        profile.parsed(at(16), at(19));
        profile.searching();
        profile.searched(Duration::from_millis(400));
        profile.fetched(Duration::from_millis(230));
        // A second search the player made before the check settled doesn't replace the first.
        profile.searched(Duration::from_millis(900));
        profile.results(at(700));
        let line = profile.summary(at(2900), Some(rate), Some(150 * 1024 * 1024));
        // 3 ms of wall time and 3,000,000 cycles at 1 GHz.
        assert!(line.contains("parse 3.0/3.0"), "{line}");
        assert!(line.contains("search 400, fetch 230"), "{line}");
        assert!(line.contains("results at 700"), "{line}");
        // 2,900 ms of the thread's cycles and 5,800 ms of the process's.
        assert!(line.contains("UI thread 2900.0, process 5800.0"), "{line}");
        assert!(line.contains("GPU memory 150.0 MiB"), "{line}");
        assert!(!line.contains("place"), "no window was placed: {line}");
    }

    #[test]
    fn the_frames_say_their_drawing_and_presenting_and_the_parts_drawn_afresh() {
        let rate = CycleRate(1e9);
        let mut profile = CheckProfile::new(3, at(0));
        profile.copied(at(15));
        profile.parsed(at(15), at(18));
        // A frame that drew the panel's 25 parts afresh, then one that drew one of them.
        for (done, drawn) in [(40, 25), (60, 1)] {
            frame(&mut profile, done);
            profile.frame_split(
                Spent::between(at(done - 5), at(done - 2)),
                Spent::between(at(done - 2), at(done)),
            );
            profile.parts(25, drawn);
        }
        let line = profile.summary(at(3000), Some(rate), None);
        assert!(
            line.contains("2 frames 10.0/10.0 (drawing 6.0, presenting 4.0)"),
            "{line}"
        );
        assert!(line.contains("26 of 50 panel parts drawn afresh"), "{line}");
    }
}
