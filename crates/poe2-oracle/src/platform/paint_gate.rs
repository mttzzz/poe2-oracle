//! When a gated window's paints go through to GPUI (`win32::Win32Overlay::gate_paints`, on
//! Windows). Kept apart from the Windows calls that feed it, so it builds and is tested on every
//! target.
//!
//! `gpui_windows` asks every window of the app for a paint on each refresh of the display, 60 to
//! 165 times a second, and each ask it hands on wakes the UI thread -- whether or not the window
//! has anything new to draw. A gated window takes them only while what it shows may be changing:
//! while it has the keyboard, and for a burst once it changed -- the app said so, or it was moved,
//! resized or shown -- or the pointer or a key did something on it; otherwise once a trickle, so
//! whatever made it dirty unforeseen still shows within [`TRICKLE_MS`]. A pointer at rest on it
//! keeps nothing going: a hover's look and a tooltip come within a burst of the last move.

/// How often a gated window's paint goes through outside a burst, in milliseconds.
pub const TRICKLE_MS: u64 = 500;

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
    /// When the last paint went through: the trickle counts from it.
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
        self.active || now < self.open_until || now.saturating_sub(self.last_passed) >= TRICKLE_MS
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
}
