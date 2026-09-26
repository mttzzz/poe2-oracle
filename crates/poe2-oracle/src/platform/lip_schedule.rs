//! The lip watcher's schedule (`platform::lip_watch`, on Windows): when it watches the HUD's
//! rails, and when it next looks at them. Kept apart from the Windows calls that feed it, so it
//! builds and is tested on every target.
//!
//! It watches while the game is in front and not minimised, and for [`LINGER`] after it leaves
//! the front. Nothing is polled for that: Windows reports each change of the foreground window
//! (`game_window::watch_foreground`), and is asked again once a burst of reports has settled
//! ([`SETTLE`]), since a report can land out of step with where the foreground ends up. Behind
//! another window the game still shows a tooltip under the cursor (seen live 2026-09-24), but the
//! plates are then left to the XP sampler's look every two seconds: watching holds a Direct3D
//! device and a duplication of the game's monitor -- on the test machine 17 threads of the
//! graphics driver, and video memory for the monitor's frame, 32 MiB at 4K -- and looks four
//! times a second even while the player keeps still: its thread and the driver's took 0.35 % of a
//! core together, measured 2026-09-26.
//!
//! While it watches it looks every [`LOOK_INTERVAL`] while the player moves the mouse or presses
//! keys -- a tooltip comes or goes only then -- and every [`STILL_LOOK_INTERVAL`] once they have
//! kept still for [`STILL_AFTER`]. In between it sleeps: to the next look while they move, in
//! [`INPUT_POLL`] steps while they keep still, each step asking Windows only when the last input
//! was -- the first move after a still second ends the wait within a step.

use std::time::{Duration, Instant};

/// How long the watcher keeps watching once the game has left the front: the tooltip the player
/// left goes a moment later, and its plate should come back then; and a quick look at another
/// window -- a click into the price panel, an Alt+Tab there and back -- keeps the duplication
/// instead of opening it anew.
pub const LINGER: Duration = Duration::from_secs(2);
/// How long after the last of a burst of foreground reports Windows is asked which window is in
/// front: a report can land out of step with where the foreground settles (a console window's
/// activation was seen reported after the game had already taken the foreground back).
pub const SETTLE: Duration = Duration::from_millis(250);
/// How long after the desktop couldn't be duplicated -- the secure desktop of a UAC prompt,
/// another program's exclusive fullscreen -- it's tried again.
pub const RETRY_AFTER: Duration = Duration::from_secs(3);
/// The least time between two looks at the desktop while the player moves the mouse or presses
/// keys, and once they've kept still for `STILL_AFTER`. The game presents far more often: a look
/// at each of its frames cost 4 % of a core, measured 2026-09-24 on the test machine at 77 frames
/// a second.
pub const LOOK_INTERVAL: Duration = Duration::from_millis(25);
pub const STILL_LOOK_INTERVAL: Duration = Duration::from_millis(250);
pub const STILL_AFTER: Duration = Duration::from_secs(1);
/// How often a still player is asked after: when their last input was (`GetLastInputInfo`). To
/// be woken by the input itself instead would take a low-level mouse hook -- every mouse report
/// of the session passed through this app before the game gets it -- or raw input to a window of
/// the watcher's own, which Windows allows one of per process and kind of device, so it would
/// take GPUI's place should GPUI ever register one.
pub const INPUT_POLL: Duration = Duration::from_millis(100);

/// Whether the watcher watches: while it's given a game, the game is in front and not minimised
/// or left the front less than `LINGER` ago, and the desktop wasn't found impossible to duplicate
/// in the last `RETRY_AFTER`.
#[derive(Debug, Default)]
pub struct WhenToWatch {
    /// A game to watch is given.
    game: bool,
    /// The game is in front and not minimised, as last reported or asked.
    front: bool,
    /// When it last left the front.
    left: Option<Instant>,
    /// When Windows is to be asked which window is in front: `SETTLE` after the last report.
    settle: Option<Instant>,
    /// When duplicating the desktop may be tried again, after it failed.
    retry: Option<Instant>,
}

impl WhenToWatch {
    /// Whether there is a game to watch.
    pub fn given(&mut self, game: bool) {
        self.game = game;
    }

    /// A report of a new foreground window: whether it's the game, not minimised. Windows is asked
    /// again once the reports have settled ([`WhenToWatch::settle_due`]).
    pub fn reported(&mut self, front: bool, now: Instant) {
        self.asked(front, now);
        self.settle = Some(now + SETTLE);
    }

    /// What Windows says: whether the game is in front, not minimised.
    pub fn asked(&mut self, front: bool, now: Instant) {
        if self.front && !front {
            self.left = Some(now);
        }
        self.front = front;
    }

    /// Whether Windows is due to be asked which window is in front: once a burst of reports is
    /// `SETTLE` old, once.
    pub fn settle_due(&mut self, now: Instant) -> bool {
        let due = self.settle.is_some_and(|at| at <= now);
        if due {
            self.settle = None;
        }
        due
    }

    /// The desktop couldn't be duplicated: not tried again for `RETRY_AFTER`.
    pub fn failed(&mut self, now: Instant) {
        self.retry = Some(now + RETRY_AFTER);
    }

    /// Whether to watch now.
    pub fn watching(&self, now: Instant) -> bool {
        self.game && self.attended(now) && self.retry.is_none_or(|at| at <= now)
    }

    /// How long the watcher, not watching, sleeps if no order comes before it decides again:
    /// until Windows is to be asked which window is in front, or until duplicating the desktop may
    /// be tried again with the game still in front; `None`, till the next order. Nothing else
    /// starts the watching by itself: a linger only ends it.
    pub fn idle_wait(&self, now: Instant) -> Option<Duration> {
        let retry = self.retry.filter(|&at| self.game && self.attended(at));
        [self.settle, retry]
            .into_iter()
            .flatten()
            .filter(|&at| at > now)
            .min()
            .map(|at| at - now)
    }

    /// Whether the player is at the game at `at`: it's in front, or left it less than `LINGER`
    /// before.
    fn attended(&self, at: Instant) -> bool {
        self.front || self.left.is_some_and(|left| at < left + LINGER)
    }
}

/// How long the watcher sleeps before it looks at the desktop again, `since_look` after its last
/// look with the player still for `still_for`: the rest of `LOOK_INTERVAL` while they move, and of
/// `STILL_LOOK_INTERVAL` once they've kept still -- but then at most `INPUT_POLL`, after which it
/// asks about them again. Zero: look now.
pub fn look_wait(since_look: Duration, still_for: Duration) -> Duration {
    if still_for < STILL_AFTER {
        LOOK_INTERVAL.saturating_sub(since_look)
    } else {
        STILL_LOOK_INTERVAL
            .saturating_sub(since_look)
            .min(INPUT_POLL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// A game to watch, in front from `at`.
    fn in_front(at: Instant) -> WhenToWatch {
        let mut when = WhenToWatch::default();
        when.given(true);
        when.reported(true, at);
        when
    }

    #[test]
    fn watches_while_the_game_is_in_front_and_a_linger_after_it_leaves() {
        let start = Instant::now();
        let mut when = in_front(start);
        assert!(when.watching(start));
        assert!(when.watching(start + Duration::from_secs(3600)));
        let left = start + Duration::from_secs(10);
        when.reported(false, left);
        assert!(when.watching(left));
        assert!(when.watching(left + LINGER - MS));
        assert!(!when.watching(left + LINGER));
    }

    #[test]
    fn nothing_is_watched_without_a_game_or_before_it_comes_to_the_front() {
        let now = Instant::now();
        let mut when = WhenToWatch::default();
        assert!(!when.watching(now));
        // Given, but another window in front.
        when.given(true);
        when.asked(false, now);
        assert!(!when.watching(now));
        // In front, but no game given any more: minimised, or the overlay turned off.
        when.asked(true, now);
        when.given(false);
        assert!(!when.watching(now));
        when.given(true);
        assert!(when.watching(now));
    }

    #[test]
    fn a_game_that_was_never_in_front_does_not_linger() {
        let now = Instant::now();
        let mut when = WhenToWatch::default();
        when.given(true);
        when.reported(false, now);
        when.asked(false, now + SETTLE);
        assert!(!when.watching(now));
        assert!(!when.watching(now + SETTLE));
    }

    #[test]
    fn a_quick_return_watches_on_and_the_next_leave_lingers_from_itself() {
        let start = Instant::now();
        let mut when = in_front(start);
        let away = start + Duration::from_secs(1);
        when.reported(false, away);
        assert!(when.watching(away + LINGER / 4));
        when.reported(true, away + LINGER / 2);
        assert!(when.watching(away + LINGER));
        assert!(when.watching(start + Duration::from_secs(60)));
        let left = start + Duration::from_secs(61);
        when.reported(false, left);
        assert!(when.watching(left + LINGER - MS));
        assert!(!when.watching(left + LINGER));
    }

    #[test]
    fn windows_is_asked_once_a_burst_of_reports_has_settled() {
        let start = Instant::now();
        let mut when = in_front(start);
        // Alt+Tab through two windows: three reports 50 ms apart.
        when.reported(false, start + 50 * MS);
        when.reported(false, start + 100 * MS);
        let last = start + 150 * MS;
        when.reported(true, last);
        assert!(!when.settle_due(last + SETTLE - MS));
        assert!(when.settle_due(last + SETTLE));
        assert!(!when.settle_due(last + SETTLE + MS));
    }

    #[test]
    fn what_windows_says_once_settled_overrides_a_report_out_of_step() {
        let start = Instant::now();
        // A console window's activation reported after the game took the foreground back.
        let mut when = in_front(start);
        when.reported(false, start + MS);
        assert!(when.settle_due(start + MS + SETTLE));
        when.asked(true, start + MS + SETTLE);
        assert!(when.watching(start + Duration::from_secs(60)));
        // The game reported in front, yet Windows says another window is: the linger runs from
        // the answer.
        let mut when = WhenToWatch::default();
        when.given(true);
        when.reported(true, start);
        let asked = start + SETTLE;
        when.asked(false, asked);
        assert!(when.watching(asked + LINGER - MS));
        assert!(!when.watching(asked + LINGER));
    }

    #[test]
    fn a_failed_duplication_is_tried_again_after_a_pause_while_the_game_stays_in_front() {
        let start = Instant::now();
        let mut when = in_front(start);
        assert!(when.settle_due(start + SETTLE));
        let failed = start + Duration::from_secs(1);
        when.failed(failed);
        assert!(!when.watching(failed));
        assert!(!when.watching(failed + RETRY_AFTER - MS));
        assert_eq!(when.idle_wait(failed), Some(RETRY_AFTER));
        assert!(when.watching(failed + RETRY_AFTER));
        // Not while the game has left the front for longer than the linger by then.
        let mut when = in_front(start);
        assert!(when.settle_due(start + SETTLE));
        when.failed(failed);
        when.asked(false, failed);
        assert_eq!(when.idle_wait(failed), None);
        // Nor without a game.
        let mut when = in_front(start);
        assert!(when.settle_due(start + SETTLE));
        when.failed(failed);
        when.given(false);
        assert_eq!(when.idle_wait(failed), None);
    }

    #[test]
    fn a_watcher_with_nothing_to_watch_sleeps_till_an_order_or_the_settle() {
        let start = Instant::now();
        let mut when = WhenToWatch::default();
        assert_eq!(when.idle_wait(start), None);
        when.given(true);
        when.reported(false, start);
        assert_eq!(
            when.idle_wait(start + SETTLE / 2),
            Some(SETTLE - SETTLE / 2)
        );
        assert!(when.settle_due(start + SETTLE));
        when.asked(false, start + SETTLE);
        assert_eq!(when.idle_wait(start + SETTLE), None);
        // A retry time gone by never makes it spin.
        when.failed(start);
        when.asked(true, start + Duration::from_secs(10));
        assert!(when.watching(start + Duration::from_secs(10)));
        when.asked(false, start + Duration::from_secs(10));
        assert_eq!(when.idle_wait(start + Duration::from_secs(20)), None);
    }

    #[test]
    fn a_moving_player_is_looked_at_every_look_interval() {
        let moving = Duration::ZERO;
        assert_eq!(look_wait(Duration::ZERO, moving), LOOK_INTERVAL);
        let part = LOOK_INTERVAL / 3;
        assert_eq!(look_wait(part, moving), LOOK_INTERVAL - part);
        assert_eq!(look_wait(LOOK_INTERVAL, moving), Duration::ZERO);
        assert_eq!(look_wait(LOOK_INTERVAL + part, moving), Duration::ZERO);
        // Still for just under `STILL_AFTER` is moving yet.
        assert_eq!(look_wait(Duration::ZERO, STILL_AFTER - MS), LOOK_INTERVAL);
    }

    #[test]
    fn a_still_player_is_looked_at_less_often_and_asked_after_in_between() {
        // From one look to the next: steps of `INPUT_POLL` at most, which add up to
        // `STILL_LOOK_INTERVAL` -- as few wakes as that allows.
        let mut since = Duration::ZERO;
        let mut waits = Vec::new();
        loop {
            let wait = look_wait(since, STILL_AFTER);
            if wait.is_zero() {
                break;
            }
            waits.push(wait);
            since += wait;
        }
        assert_eq!(since, STILL_LOOK_INTERVAL);
        assert!(waits.iter().all(|&wait| wait <= INPUT_POLL));
        assert_eq!(
            waits.len() as u128,
            STILL_LOOK_INTERVAL
                .as_millis()
                .div_ceil(INPUT_POLL.as_millis())
        );
    }

    #[test]
    fn the_first_move_after_keeping_still_ends_the_wait() {
        // Still, past `LOOK_INTERVAL` since the last look and short of the next: another step.
        let since = LOOK_INTERVAL * 2;
        let wait = look_wait(since, STILL_AFTER);
        assert!(!wait.is_zero() && wait <= INPUT_POLL);
        // They moved during that step: the look is due at once.
        assert_eq!(look_wait(since, Duration::ZERO), Duration::ZERO);
        // Moved right after a look: the look waits out the rest of `LOOK_INTERVAL`.
        let part = LOOK_INTERVAL / 3;
        assert_eq!(look_wait(part, Duration::ZERO), LOOK_INTERVAL - part);
    }
}
