//! When a gated window's paints go through to GPUI (`win32::Win32Overlay::gate_paints`, on
//! Windows), and when `gpui_windows`' vsync thread may sleep (`vsync_park`). Kept apart from the
//! Windows calls that feed them, so they build and are tested on every target.
//!
//! `gpui_windows` asks every window of the app for a paint on each refresh of the display, 60 to
//! 165 times a second, and each ask it hands on wakes the UI thread -- whether or not the window
//! has anything new to draw. A gated window takes them only while what it shows may be changing:
//! while it has the keyboard, and for a burst once it changed -- the app said so, or it was moved,
//! resized or shown -- or the pointer or a key did something on it; otherwise once a trickle, so
//! whatever made it dirty unforeseen still shows within one and a half trickles
//! ([`TRICKLE_MS`]). A pointer at rest on it keeps nothing going: a hover's look and a tooltip
//! come within a burst of the last move.
//!
//! Every gate's trickle ticks on one clock ([`trickle_after`]), so the quiet windows' paints fall
//! on the same refreshes. In between, with no shown window that wants a paint at each refresh,
//! the vsync thread sleeps ([`vsync`]): the XP overlay's three plates, up the whole time the game
//! is played, wake it twice a second instead of at every refresh.

/// A trickle, in milliseconds: how often a gated window's paint goes through outside a burst.
pub const TRICKLE_MS: u64 = 500;

/// The longest the vsync thread sleeps with no trickle to wake for, in milliseconds: it bounds a
/// wake that never came.
pub const MAX_PARK_MS: u64 = 1_000;

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
    /// Until when paints go through, the last burst's end.
    open_until: u64,
    /// When the last paint went through: the trickle's next comes half a trickle or more after it.
    last_passed: u64,
    /// The window has the keyboard.
    active: bool,
}

impl PaintGate {
    /// A gate whose bursts last `burst_ms`, in a burst from `now`: the window is new, and drawn
    /// once shown.
    pub fn new(burst_ms: u64, now: u64) -> PaintGate {
        PaintGate {
            burst_ms,
            open_until: now + burst_ms,
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
        if self.active || now < self.open_until {
            Wants::EachRefresh
        } else {
            Wants::Trickle {
                at: trickle_after(self.last_passed),
            }
        }
    }

    /// Takes a paint at `now`: whether it goes through to GPUI. One that does is the trickle's
    /// last.
    pub fn paint(&mut self, now: u64) -> bool {
        let due = self.due(now);
        if due {
            self.last_passed = now;
        }
        due
    }

    /// Takes `event` at `now`. Every event but a hide opens a burst: the app's transitions run
    /// past the change that set them off, and a hover or a focus eases out after it ends.
    pub fn note(&mut self, event: GateEvent, now: u64) {
        match event {
            GateEvent::Input | GateEvent::Changed => {}
            GateEvent::Activated(active) => self.active = active,
            GateEvent::Hidden => {
                // Out of sight, it may lose the keyboard without being told.
                self.active = false;
                return;
            }
        }
        self.open_until = now + self.burst_ms;
    }
}

/// What a shown window wants of the display's refreshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wants {
    /// A paint at each: its paints aren't gated, or go through all the while -- it has the
    /// keyboard, or is in a burst.
    EachRefresh,
    /// Its trickle's paint, from `at` on.
    Trickle { at: u64 },
}

impl Wants {
    /// Whether a paint at `now` would go through.
    pub fn due(self, now: u64) -> bool {
        match self {
            Wants::EachRefresh => true,
            Wants::Trickle { at } => now >= at,
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
/// wants a paint at each refresh, or a trickle's paint that no refresh has asked for since it came
/// due; otherwise it sleeps until the first trickle comes due, [`MAX_PARK_MS`] at the most. A
/// refresh that asked no window at all says nothing of what's shown -- there's no window yet, or
/// its asks go past `redraw_filter` -- so it refreshes.
pub fn vsync(windows: impl IntoIterator<Item = Option<Wants>>, asked: u64, now: u64) -> Vsync {
    let mut windows = windows.into_iter().peekable();
    if windows.peek().is_none() {
        return Vsync::Refresh;
    }
    let mut until = now + MAX_PARK_MS;
    for wants in windows.flatten() {
        let next = match wants {
            Wants::EachRefresh => return Vsync::Refresh,
            Wants::Trickle { at } if at > now => at,
            Wants::Trickle { at } if asked < at => return Vsync::Refresh,
            // Asked for since it came due, its paint is on its way to the window: the trickle's
            // next comes after that one, which goes through now at the earliest.
            Wants::Trickle { .. } => trickle_after(now),
        };
        until = until.min(next);
    }
    Vsync::Park { until }
}

/// When the trickle lets a paint through after one at `painted`: at the first tick of its clock --
/// every [`TRICKLE_MS`] of `GetTickCount64`, the same for every gate -- half a trickle or more
/// on, so a paint that just went through, a burst's last, isn't followed by another for nothing.
fn trickle_after(painted: u64) -> u64 {
    (painted + TRICKLE_MS / 2).div_ceil(TRICKLE_MS) * TRICKLE_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    const BURST: u64 = 400;

    /// A gate whose opening burst and first trickle are long past at `now`, the last paint
    /// through at `now` too: quiet from here.
    fn settled(burst_ms: u64, now: u64) -> PaintGate {
        let mut gate = PaintGate::new(burst_ms, 0);
        assert!(gate.paint(now));
        gate
    }

    #[test]
    fn a_quiet_window_paints_once_a_trickle() {
        let mut gate = settled(BURST, 10_000);
        // A 60 Hz display's asks until the trickle is due.
        assert!((1..32).all(|frame| !gate.due(10_000 + frame * 16)));
        assert!(gate.paint(10_000 + TRICKLE_MS));
        assert!(!gate.paint(10_000 + TRICKLE_MS + 16));
    }

    #[test]
    fn asks_that_are_turned_down_leave_the_trickle_alone() {
        let mut gate = settled(BURST, 10_000);
        for frame in 1..30 {
            assert!(!gate.paint(10_000 + frame * 16));
        }
        assert!(gate.paint(10_000 + TRICKLE_MS));
    }

    #[test]
    fn a_change_lets_every_paint_through_for_the_windows_own_burst() {
        let mut plate = settled(BURST, 10_000);
        let mut panel = settled(2_000, 10_000);
        plate.note(GateEvent::Changed, 10_100);
        panel.note(GateEvent::Changed, 10_100);
        assert!(plate.paint(10_116) && plate.paint(10_132) && plate.paint(10_499));
        assert!(!plate.due(10_500 + 1));
        assert!(panel.paint(11_000) && panel.paint(12_099));
        assert!(!panel.due(12_100 + 1));
    }

    #[test]
    fn the_pointer_keeps_it_painting_a_burst_past_its_last_move_and_no_longer() {
        let mut gate = settled(BURST, 10_000);
        gate.note(GateEvent::Input, 10_100);
        gate.note(GateEvent::Input, 10_300);
        assert!(gate.paint(10_699));
        // At rest on the window since.
        assert!(!gate.due(10_700 + 1));
    }

    #[test]
    fn the_keyboard_keeps_it_painting_until_it_goes() {
        let mut gate = settled(BURST, 10_000);
        gate.note(GateEvent::Activated(true), 10_100);
        assert!(gate.paint(20_000));
        gate.note(GateEvent::Activated(false), 20_000);
        assert!(gate.paint(20_399));
        assert!(!gate.due(20_400 + 1));
    }

    #[test]
    fn hiding_ends_a_focus_whose_loss_never_came() {
        let mut gate = settled(BURST, 10_000);
        gate.note(GateEvent::Activated(true), 10_100);
        assert!(gate.paint(10_450));
        gate.note(GateEvent::Hidden, 10_460);
        assert!(!gate.due(10_600));
    }

    #[test]
    fn quiet_windows_trickle_on_the_same_ticks() {
        // Their last paints went through at different times of the same trickle.
        let plates = [10_010, 10_120, 10_250].map(|painted| settled(BURST, painted));
        let tick = Wants::Trickle { at: 10_500 };
        assert!(plates.iter().all(|plate| plate.wants(10_300) == tick));
    }

    #[test]
    fn a_trickle_paint_comes_half_a_trickle_or_more_after_the_last() {
        // Painted just past half a trickle before a tick: the tick after it.
        let gate = settled(BURST, 10_251);
        assert!(!gate.due(10_500) && !gate.due(10_999));
        assert!(gate.due(11_000));
    }

    #[test]
    fn a_window_that_wants_each_refresh_keeps_the_vsync_thread_going() {
        let windows = [
            None,
            Some(Wants::Trickle { at: 10_500 }),
            Some(Wants::EachRefresh),
        ];
        assert_eq!(vsync(windows, 10_000, 10_016), Vsync::Refresh);
    }

    #[test]
    fn quiet_windows_let_it_sleep_until_the_first_trickle() {
        let windows = [
            Some(Wants::Trickle { at: 11_000 }),
            None,
            Some(Wants::Trickle { at: 10_500 }),
        ];
        assert_eq!(
            vsync(windows, 10_016, 10_020),
            Vsync::Park { until: 10_500 }
        );
    }

    #[test]
    fn one_wake_a_trickle_serves_every_quiet_window() {
        let mut plates = [10_010, 10_120, 10_250].map(|painted| settled(BURST, painted));
        let shown =
            |plates: &[PaintGate; 3], now| plates.clone().map(|plate| Some(plate.wants(now)));
        assert_eq!(
            vsync(shown(&plates, 10_260), 10_260, 10_260),
            Vsync::Park { until: 10_500 }
        );
        // Woken at the tick, the thread asked for their paints, which are yet to come: asleep
        // till the next.
        assert!(plates.iter().all(|plate| plate.due(10_500)));
        assert_eq!(
            vsync(shown(&plates, 10_501), 10_500, 10_501),
            Vsync::Park { until: 11_000 }
        );
        for plate in &mut plates {
            assert!(plate.paint(10_502));
        }
        assert_eq!(
            vsync(shown(&plates, 10_503), 10_500, 10_503),
            Vsync::Park { until: 11_000 }
        );
    }

    #[test]
    fn a_trickle_due_since_the_last_ask_brings_a_refresh() {
        let plate = Some(Wants::Trickle { at: 10_500 });
        assert_eq!(vsync([plate], 10_490, 10_505), Vsync::Refresh);
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
