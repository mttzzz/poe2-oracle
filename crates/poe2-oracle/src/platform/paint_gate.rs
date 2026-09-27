//! When a gated window's paints go through to GPUI (`win32::Win32Overlay::gate_paints`, on
//! Windows), and when `gpui_windows`' vsync thread may sleep (`vsync_park`). Kept apart from the
//! Windows calls that feed them, so they build and are tested on every target.
//!
//! `gpui_windows` asks every window of the app for a paint on each refresh of the display, 60 to
//! 165 times a second, and each ask it hands on wakes the UI thread -- whether or not the window
//! has anything new to draw. A gated window takes them only while what it shows may be changing:
//! while it has the keyboard, and for a burst once it changed -- the app said so, or it was moved,
//! resized or shown -- or the pointer or a key did something on it. A pointer at rest on it keeps
//! nothing going: a hover's look and a tooltip come within a burst of the last move. A still
//! window, whose changes are drawn in one frame -- the XP overlay's plates -- takes its next
//! paints for a change instead ([`OnChange::Paints`]), whenever they come, and a burst only for
//! the pointer, the keyboard, or its app's word that a change eases ([`PaintGate::open`]). In play
//! the plates' words change at most of their samples, two seconds apart, and with a burst for each
//! change the UI thread was woken 44 to 58 times a second as they updated (measured 2026-09-27 on
//! the test machine in real play): a burst is a paint at each of the display's refreshes for 400
//! ms, where the change is drawn at the first.
//!
//! Otherwise a paint goes through once a safety net ([`SAFETY_NET_MS`], five seconds; the price
//! panel keeps a shorter one), so a change nobody told the gate of still shows within one and a
//! half. GPUI draws a window only once something in it changed, so the safety net's paint of one
//! with nothing new draws nothing: it costs the UI thread the wake alone. Every gate's safety net
//! ticks on one clock ([`safety_net_after`]), so the quiet windows' paints fall on the same
//! refreshes. In between, with no shown window that wants a paint at each refresh, the vsync
//! thread sleeps till the next of them ([`vsync`]): the XP overlay's three plates, up the whole
//! time the game is played, wake it once in five seconds while what they say stays the same.

/// A safety net, in milliseconds: how often a gated window's paint goes through outside its
/// bursts, whatever it shows -- the first half a safety net or more after the last paint. A gate
/// with a shorter one ([`PaintGate::new`]) takes a whole part of this one, a half or a tenth, so
/// every tick of this one is a tick of its too.
pub const SAFETY_NET_MS: u64 = 5_000;

/// The longest the vsync thread sleeps with no window shown, in milliseconds: it bounds a wake
/// that never came -- a window shown without the thread being told. A shown window's next safety
/// net's paint ends a sleep in time on its own.
pub const MAX_PARK_MS: u64 = SAFETY_NET_MS;

/// The paints a change owes a still window ([`OnChange::Paints`]): the one that draws it, and one
/// more, for a draw put off -- `gpui_windows` defers a window's draw that comes while another is
/// under way to the display's next refresh (`events.rs`' `draw_window`).
pub const PAINTS_OWED: u8 = 2;

/// What a change to a gated window ([`GateEvent::Changed`]) lets through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnChange {
    /// A burst: the window's changes run on in transitions, or come in without a word to it --
    /// the price panel's.
    Burst,
    /// Its next [`PAINTS_OWED`] paints, whenever they come -- one owed while it's hidden goes
    /// through once it's shown: a still window, whose app opens a burst itself for a change that
    /// eases.
    Paints,
}

/// What happened to a gated window, as far as its paints go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateEvent {
    /// The pointer or a key did something on it: moved, left, pressed, wheeled, touched, typed.
    Input,
    /// It got the keyboard (`true`) or lost it.
    Activated(bool),
    /// What it shows changed: the app said so, or it was moved, resized or shown, or its DPI or
    /// frame changed.
    Changed,
    /// It was hidden: whatever had the keyboard in it is done.
    Hidden,
}

/// One gated window's paints. Times are milliseconds on one clock (`GetTickCount64`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintGate {
    /// How long a burst lasts.
    burst_ms: u64,
    /// Its safety net: how often a paint goes through outside the bursts.
    safety_net_ms: u64,
    /// What a change lets through.
    on_change: OnChange,
    /// Until when paints go through, the last burst's end.
    open_until: u64,
    /// Paints owed by a change ([`OnChange::Paints`]) and yet to go through.
    owed: u8,
    /// When the last paint went through: the safety net's next comes half a safety net or more
    /// after it.
    last_passed: u64,
    /// The window has the keyboard.
    active: bool,
}

impl PaintGate {
    /// A gate whose bursts last `burst_ms`, whose safety net is `safety_net_ms` --
    /// [`SAFETY_NET_MS`] or a whole part of it -- and whose changes let `on_change` through, in a
    /// burst from `now`: the window is new, and drawn once shown.
    pub fn new(burst_ms: u64, safety_net_ms: u64, on_change: OnChange, now: u64) -> PaintGate {
        debug_assert!(
            safety_net_ms > 0 && SAFETY_NET_MS.is_multiple_of(safety_net_ms),
            "a gate's safety net is a whole part of SAFETY_NET_MS"
        );
        PaintGate {
            burst_ms,
            safety_net_ms,
            on_change,
            open_until: now + burst_ms,
            owed: 0,
            last_passed: 0,
            active: false,
        }
    }

    /// Whether a paint at `now` would go through.
    pub fn due(&self, now: u64) -> bool {
        self.wants(now).due(now)
    }

    /// What the window wants of the display's refreshes at `now`.
    pub fn wants(&self, now: u64) -> Wants {
        if self.active || now < self.open_until || self.owed > 0 {
            Wants::EachRefresh
        } else {
            Wants::SafetyNet {
                at: safety_net_after(self.last_passed, self.safety_net_ms),
                every: self.safety_net_ms,
            }
        }
    }

    /// Takes a paint at `now`: whether it goes through to GPUI. One that does is the safety net's
    /// last, and pays one that's owed.
    pub fn paint(&mut self, now: u64) -> bool {
        let due = self.due(now);
        if due {
            self.last_passed = now;
            self.owed = self.owed.saturating_sub(1);
        }
        due
    }

    /// Takes `event` at `now`. Every event but a hide opens a burst -- the app's transitions run
    /// past the change that set them off, and a hover or a focus eases out after it ends -- but a
    /// still window's change ([`OnChange::Paints`]), which owes it its next paints.
    pub fn note(&mut self, event: GateEvent, now: u64) {
        match event {
            GateEvent::Input => {}
            GateEvent::Changed if self.on_change == OnChange::Paints => {
                self.owed = PAINTS_OWED;
                return;
            }
            GateEvent::Changed => {}
            GateEvent::Activated(active) => self.active = active,
            GateEvent::Hidden => {
                // Out of sight, it may lose the keyboard without being told.
                self.active = false;
                return;
            }
        }
        self.open(now);
    }

    /// Opens a burst at `now`, whatever the gate's changes let through: the app's word that what
    /// the window shows eases from here on.
    pub fn open(&mut self, now: u64) {
        self.open_until = now + self.burst_ms;
    }
}

/// What a shown window wants of the display's refreshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wants {
    /// A paint at each: its paints aren't gated, or go through all the while -- it has the
    /// keyboard, is in a burst, or is owed paints for a change.
    EachRefresh,
    /// Its safety net's paint, from `at` on: a safety net `every` milliseconds long.
    SafetyNet { at: u64, every: u64 },
}

impl Wants {
    /// Whether a paint at `now` would go through.
    pub fn due(self, now: u64) -> bool {
        match self {
            Wants::EachRefresh => true,
            Wants::SafetyNet { at, .. } => now >= at,
        }
    }
}

/// What `gpui_windows`' vsync thread does next (`vsync_park`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vsync {
    /// Waits for the display's next refresh, then asks every window for a paint.
    Refresh,
    /// Sleeps until `until` -- unless woken: a window shown, a gate opened -- then asks.
    Park { until: u64 },
}

/// What the vsync thread does next, at `now`. `windows` are what each window its last refresh
/// asked for a paint, at `asked`, wanted of the refreshes then: `None` for one hidden or
/// minimized -- what changed since wakes the thread (`vsync_park`). It refreshes while one of them
/// wants a paint at each refresh, or a safety net's paint that no refresh has asked for since it
/// came due; otherwise it sleeps until the first safety net's paint comes due, however far off --
/// up to one and a half safety nets after a burst -- and with none shown, [`MAX_PARK_MS`]. A
/// refresh that asked no window at all says nothing of what's shown -- there's no window yet, or
/// its asks go past `redraw_filter` -- so it refreshes.
pub fn vsync(windows: impl IntoIterator<Item = Option<Wants>>, asked: u64, now: u64) -> Vsync {
    let mut windows = windows.into_iter().peekable();
    if windows.peek().is_none() {
        return Vsync::Refresh;
    }
    let mut until: Option<u64> = None;
    for wants in windows.flatten() {
        let next = match wants {
            Wants::EachRefresh => return Vsync::Refresh,
            Wants::SafetyNet { at, .. } if at > now => at,
            Wants::SafetyNet { at, .. } if asked < at => return Vsync::Refresh,
            // Asked for since it came due, its paint is on its way to the window: the safety
            // net's next comes after that one, which goes through now at the earliest.
            Wants::SafetyNet { every, .. } => safety_net_after(now, every),
        };
        until = Some(until.map_or(next, |until| until.min(next)));
    }
    Vsync::Park {
        until: until.unwrap_or(now + MAX_PARK_MS),
    }
}

/// When a safety net `every` milliseconds long lets a paint through after one at `painted`: at
/// the first tick of its clock -- every `every` of `GetTickCount64`, the same clock for every
/// gate -- half a safety net or more on, so a paint that just went through, a burst's last, isn't
/// followed by another for nothing.
fn safety_net_after(painted: u64, every: u64) -> u64 {
    (painted + every / 2).div_ceil(every) * every
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The price panel's safety net, a tenth of the others'.
    const PANEL_NET: u64 = SAFETY_NET_MS / 10;

    /// A gated window whose opening burst is long past at `now`, its last paint through at `now`
    /// too: quiet from here.
    fn quiet(burst_ms: u64, safety_net_ms: u64, on_change: OnChange, now: u64) -> PaintGate {
        let mut gate = PaintGate::new(burst_ms, safety_net_ms, on_change, 0);
        assert!(gate.paint(now));
        gate
    }

    /// An XP plate -- a still window -- quiet since `now`.
    fn plate(now: u64) -> PaintGate {
        quiet(400, SAFETY_NET_MS, OnChange::Paints, now)
    }

    /// A toast, quiet since `now`: its changes open bursts.
    fn toast(now: u64) -> PaintGate {
        quiet(400, SAFETY_NET_MS, OnChange::Burst, now)
    }

    /// The price panel, quiet since `now`.
    fn panel(now: u64) -> PaintGate {
        quiet(2_000, PANEL_NET, OnChange::Burst, now)
    }

    /// From when `wants` lets the window's next paint through; `None` for each refresh's.
    fn next_paint(wants: Wants) -> Option<u64> {
        match wants {
            Wants::EachRefresh => None,
            Wants::SafetyNet { at, .. } => Some(at),
        }
    }

    #[test]
    fn a_quiet_window_paints_once_a_safety_net() {
        let mut plate = plate(10_000);
        // A 60 Hz display's asks until its safety net's paint is due.
        assert!((1..SAFETY_NET_MS / 16).all(|frame| !plate.due(10_000 + frame * 16)));
        assert!(plate.paint(10_000 + SAFETY_NET_MS));
        assert!(!plate.paint(10_000 + SAFETY_NET_MS + 16));
    }

    #[test]
    fn asks_that_are_turned_down_leave_the_safety_net_alone() {
        let mut plate = plate(10_000);
        for frame in 1..SAFETY_NET_MS / 16 {
            assert!(!plate.paint(10_000 + frame * 16));
        }
        assert!(plate.paint(10_000 + SAFETY_NET_MS));
    }

    #[test]
    fn a_change_lets_every_paint_through_for_the_windows_own_burst() {
        let (mut toast, mut panel) = (toast(10_000), panel(10_000));
        toast.note(GateEvent::Changed, 10_100);
        panel.note(GateEvent::Changed, 10_100);
        assert!(toast.paint(10_116) && toast.paint(10_132) && toast.paint(10_499));
        assert!(!toast.due(10_500 + 1));
        assert!(panel.paint(11_000) && panel.paint(12_099));
        assert!(!panel.due(12_100 + 1));
    }

    #[test]
    fn a_change_to_a_still_window_owes_it_its_next_two_paints_and_no_burst() {
        let mut plate = plate(10_000);
        plate.note(GateEvent::Changed, 10_100);
        assert_eq!(plate.wants(10_100), Wants::EachRefresh);
        assert!(plate.paint(10_116) && plate.paint(10_132));
        // Well within the burst a change opens for a toast.
        assert!(!plate.due(10_148));
        assert_eq!(next_paint(plate.wants(10_148)), Some(15_000));
    }

    #[test]
    fn paints_owed_while_hidden_go_through_once_shown() {
        let mut plate = plate(10_000);
        plate.note(GateEvent::Changed, 10_100);
        plate.note(GateEvent::Hidden, 10_110);
        // No refresh asks a hidden window for a paint (`redraw_filter`): the first asks after it's
        // shown, seconds later and before its safety net's, draw what changed meanwhile.
        assert!(plate.paint(13_000) && plate.paint(13_016));
        assert!(!plate.due(13_032));
    }

    #[test]
    fn the_apps_word_that_a_change_eases_opens_a_still_windows_burst() {
        let mut plate = plate(10_000);
        plate.note(GateEvent::Changed, 11_000);
        plate.open(11_000);
        // An ease's frames, at the 30 a second GPUI keeps a window without the keyboard to.
        assert!((0..12).all(|frame| plate.paint(11_000 + frame * 33)));
        assert!(!plate.due(11_401));
    }

    #[test]
    fn paints_owed_keep_the_vsync_thread_going_until_they_went_through() {
        let mut plate = plate(10_000);
        plate.note(GateEvent::Changed, 10_100);
        assert_eq!(
            vsync([Some(plate.wants(10_100))], 10_000, 10_100),
            Vsync::Refresh
        );
        assert!(plate.paint(10_116) && plate.paint(10_132));
        assert_eq!(
            vsync([Some(plate.wants(10_133))], 10_132, 10_133),
            Vsync::Park { until: 15_000 }
        );
    }

    #[test]
    fn the_pointer_keeps_it_painting_a_burst_past_its_last_move_and_no_longer() {
        let mut plate = plate(10_000);
        plate.note(GateEvent::Input, 10_100);
        plate.note(GateEvent::Input, 10_300);
        assert!(plate.paint(10_699));
        // At rest on the window since.
        assert!(!plate.due(10_700 + 1));
    }

    #[test]
    fn the_keyboard_keeps_it_painting_until_it_goes() {
        let mut plate = plate(10_000);
        plate.note(GateEvent::Activated(true), 10_100);
        // Long past the burst the focus opened, still every refresh's paint.
        assert!((0..600).all(|frame| plate.paint(10_600 + frame * 16)));
        plate.note(GateEvent::Activated(false), 20_200);
        assert!(plate.paint(20_599));
        assert!(!plate.due(20_600 + 1));
    }

    #[test]
    fn hiding_ends_a_focus_whose_loss_never_came() {
        let mut plate = plate(10_000);
        plate.note(GateEvent::Activated(true), 10_100);
        assert!(plate.paint(10_450));
        plate.note(GateEvent::Hidden, 10_460);
        assert!(!plate.due(10_600));
    }

    #[test]
    fn quiet_windows_paint_on_the_same_ticks() {
        // Their last paints went through at different times before the same tick.
        let plates = [10_010, 11_200, 12_400].map(plate);
        assert!(
            plates
                .iter()
                .all(|plate| next_paint(plate.wants(12_500)) == Some(15_000))
        );
        // The panel's safety net is a part of theirs: that tick is one of its own too.
        assert_eq!(next_paint(panel(14_600).wants(14_700)), Some(15_000));
    }

    #[test]
    fn a_safety_net_paint_comes_half_a_safety_net_or_more_after_the_last() {
        // Painted less than half a safety net before a tick: the tick after it.
        let plate = plate(12_501);
        assert!(!plate.due(15_000) && !plate.due(19_999));
        assert!(plate.due(20_000));
    }

    #[test]
    fn a_window_that_wants_each_refresh_keeps_the_vsync_thread_going() {
        let windows = [
            None,
            Some(plate(10_000).wants(10_016)),
            Some(Wants::EachRefresh),
        ];
        assert_eq!(vsync(windows, 10_000, 10_016), Vsync::Refresh);
    }

    #[test]
    fn quiet_windows_let_it_sleep_until_the_first_safety_net() {
        let windows = [
            Some(Wants::SafetyNet {
                at: 20_000,
                every: SAFETY_NET_MS,
            }),
            None,
            Some(Wants::SafetyNet {
                at: 15_000,
                every: SAFETY_NET_MS,
            }),
        ];
        assert_eq!(
            vsync(windows, 12_016, 12_020),
            Vsync::Park { until: 15_000 }
        );
    }

    #[test]
    fn it_sleeps_all_the_way_to_the_next_paint_due() {
        // A burst's last paint went through less than half a safety net before a tick: nothing is
        // due for nearly one and a half safety nets.
        let plate = plate(12_501);
        assert_eq!(
            vsync([Some(plate.wants(12_520))], 12_516, 12_520),
            Vsync::Park { until: 20_000 }
        );
    }

    #[test]
    fn one_wake_a_safety_net_serves_every_quiet_window() {
        let mut plates = [10_010, 11_200, 12_400].map(plate);
        let shown =
            |plates: &[PaintGate; 3], now| plates.clone().map(|plate| Some(plate.wants(now)));
        assert_eq!(
            vsync(shown(&plates, 12_500), 12_500, 12_500),
            Vsync::Park { until: 15_000 }
        );
        // Woken at the tick, the thread asked for their paints, which are yet to come: asleep
        // till the next.
        assert!(plates.iter().all(|plate| plate.due(15_000)));
        assert_eq!(
            vsync(shown(&plates, 15_001), 15_000, 15_001),
            Vsync::Park { until: 20_000 }
        );
        for plate in &mut plates {
            assert!(plate.paint(15_002));
        }
        assert_eq!(
            vsync(shown(&plates, 15_003), 15_000, 15_003),
            Vsync::Park { until: 20_000 }
        );
    }

    #[test]
    fn a_shorter_safety_net_wakes_it_on_its_own_ticks() {
        // The panel up by the plates: asked for at its tick, its paint is on its way, and the
        // thread sleeps till its next tick, well before the plates'.
        let windows = [
            Some(Wants::SafetyNet {
                at: 12_500,
                every: PANEL_NET,
            }),
            Some(Wants::SafetyNet {
                at: 15_000,
                every: SAFETY_NET_MS,
            }),
        ];
        assert_eq!(
            vsync(windows, 12_500, 12_501),
            Vsync::Park { until: 13_000 }
        );
    }

    #[test]
    fn a_safety_net_due_since_the_last_ask_brings_a_refresh() {
        let plate = Some(Wants::SafetyNet {
            at: 15_000,
            every: SAFETY_NET_MS,
        });
        assert_eq!(vsync([plate], 14_990, 15_005), Vsync::Refresh);
    }

    #[test]
    fn with_nothing_shown_it_sleeps_the_longest() {
        assert_eq!(
            vsync([None, None], 10_000, 10_016),
            Vsync::Park {
                until: 10_016 + MAX_PARK_MS
            }
        );
    }

    #[test]
    fn a_refresh_that_asked_no_window_keeps_it_going() {
        assert_eq!(vsync([], 10_000, 10_016), Vsync::Refresh);
    }
}
