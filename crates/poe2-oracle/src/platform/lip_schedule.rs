//! The lip watcher's schedule (`platform::lip_watch`, on Windows): when it watches the HUD's
//! rails, when it next looks at them, and when it reads a look back. Kept apart from the Windows
//! calls that feed it, so it builds and is tested on every target.
//!
//! It watches while the game is in front and not minimised, and for [`LINGER`] after it leaves
//! the front. Nothing is polled for that: Windows reports each change of the foreground window
//! (`game_window::watch_foreground`), and is asked again once a burst of reports has settled
//! ([`SETTLE`]), since a report can land out of step with where the foreground ends up. Behind
//! another window the game still shows a tooltip under the cursor (seen live 2026-09-24), but the
//! plates are then left to the XP sampler's look every two seconds: watching holds a Direct3D
//! device and a duplication of the game's monitor -- video memory for the monitor's frame, 32 MiB
//! at 4K -- besides what its looks cost.
//!
//! While it watches, its looks follow the player's input -- a tooltip comes or goes when they move
//! the mouse or press a key -- and what the latest look read ([`next_look`]):
//!
//! - every [`LOOK_INTERVAL`] while they move, and until they have kept still for [`STILL_AFTER`];
//! - then each look waits as long as they had kept still at it, up to [`SAFETY_NET`]: a follow-up
//!   about twice `STILL_AFTER` after their last input, for a tooltip the game shows after a
//!   hover's delay, the next about twice as late, and so on. What the game puts over a rail with
//!   no input at all -- a loading screen a while after a click -- takes its plate down within a
//!   second or two;
//! - but a covered rail every [`STILL_LOOK_INTERVAL`] for [`UNCOVER_WATCH`] after their last
//!   input: a cover that goes by itself, a loading screen that ends, gives its plate back as soon.
//!
//! Measured 2026-09-27 on the test machine (the build of 93296e4, the game in front, idle), the
//! watcher's thread took 5.8 ms of CPU and 16 context switches a second -- 1.45 ms and four
//! switches a look -- while it looked every 250 ms from a second after the last input on, and
//! asked Windows every 100 ms when that input was. Now a still player's input ends the watcher's
//! wait itself (raw input, `lip_watch`); only where it can't does the wait ask Windows every
//! [`INPUT_POLL`] ([`wait_until`]). And the watcher never waits for the GPU: a look's copy is read
//! back [`READ_AFTER`] after it, and later and later again while the GPU hasn't made it
//! ([`read_retry`]).

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
/// How long a new duplication's first look waits for its first frame -- the desktop as it is --
/// where every later look takes a frame only if one is there.
pub const FIRST_FRAME_WAIT: Duration = Duration::from_millis(100);
/// The least time between two looks at the desktop while the player moves the mouse or presses
/// keys. The game presents far more often: a look at each of its frames cost 4 % of a core,
/// measured 2026-09-24 on the test machine at 77 frames a second. 50 ms, not the 25 of before: a
/// look waits for nothing now and is read back `READ_AFTER` after it, so the watcher reads a
/// tooltip over a rail within about 55 ms of the frame that shows it, 30 on average, and the
/// plate is gone at the next composition -- under the tenth of a second within which a response
/// still reads as instant -- at half the looks' cost; 25 ms would read it within 30 and 17.
pub const LOOK_INTERVAL: Duration = Duration::from_millis(50);
/// How long after their last input the player still counts as moving: the looks keep to
/// `LOOK_INTERVAL` through the game's next frames, the first of which shows a tooltip for the item
/// the pointer came to rest on -- 33 ms after at 30 frames a second.
pub const STILL_AFTER: Duration = Duration::from_millis(250);
/// How often a still player's covered rail is looked at, for `UNCOVER_WATCH` after their last
/// input.
pub const STILL_LOOK_INTERVAL: Duration = Duration::from_millis(250);
/// How long after the player's last input a covered rail is looked at every
/// `STILL_LOOK_INTERVAL`: longer than a loading screen mostly takes. A rail covered for longer --
/// the passive tree left open, or a HUD laid out otherwise, whose rails never show -- is looked at
/// on the `SAFETY_NET` then.
pub const UNCOVER_WATCH: Duration = Duration::from_secs(10);
/// The longest time between two looks while it watches.
pub const SAFETY_NET: Duration = Duration::from_secs(2);
/// How long after a look its copy is first tried to be read back: the GPU makes it behind the
/// game's own work.
pub const READ_AFTER: Duration = Duration::from_millis(4);
/// How often a look reads the experience bar too: the sampler takes a reading every two seconds,
/// and a reading this old is as good as its own.
pub const BAR_EVERY: Duration = Duration::from_millis(500);
/// How often a still player is asked after -- when their last input was (`GetLastInputInfo`) --
/// where their input can't wake the watcher.
pub const INPUT_POLL: Duration = Duration::from_millis(100);
/// How much later than raw input was asked for, and how long ago, input Windows saw has to be to
/// tell that its raw input never came ([`input_missed`]).
pub const INPUT_GRACE: Duration = Duration::from_millis(100);

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

    /// When whether to watch may next change by itself, after `now`: when Windows is to be asked
    /// which window is in front, when a linger ends, or when duplicating the desktop may be tried
    /// again; `None` if nothing is to come.
    pub fn next_change(&self, now: Instant) -> Option<Instant> {
        let linger = self.left.filter(|_| !self.front).map(|left| left + LINGER);
        [self.settle, linger, self.retry]
            .into_iter()
            .flatten()
            .filter(|&at| at > now)
            .min()
    }

    /// Whether the player is at the game at `at`: it's in front, or left it less than `LINGER`
    /// before.
    fn attended(&self, at: Instant) -> bool {
        self.front || self.left.is_some_and(|left| at < left + LINGER)
    }
}

/// When the look after the one at `last_look` is due, with the player's last input at
/// `last_input` and `clear` whether the latest look read showed every rail.
pub fn next_look(last_look: Instant, last_input: Instant, clear: bool) -> Instant {
    // How long they had kept still at the look; zero if they have moved since.
    let still = last_look.saturating_duration_since(last_input);
    let wait = if still < STILL_AFTER {
        LOOK_INTERVAL
    } else if !clear && still < UNCOVER_WATCH {
        STILL_LOOK_INTERVAL
    } else {
        still.min(SAFETY_NET)
    };
    last_look + wait
}

/// Whether the player, their last input at `last_input`, keeps still at `now`: the looks thin out,
/// and the watcher's waits end on their input.
pub fn still(now: Instant, last_input: Instant) -> bool {
    now.saturating_duration_since(last_input) >= STILL_AFTER
}

/// Till when the watcher, watching, sleeps at `now` before it decides again: till `due` -- the
/// earliest of its next look, its next read-back and the next change of [`WhenToWatch`] -- but
/// while the player keeps still (their last input at `last_input`) and their input can't end the
/// wait (`input_wakes`), for `INPUT_POLL` at most, after which it asks Windows when that input was.
pub fn wait_until(
    due: Option<Instant>,
    now: Instant,
    last_input: Instant,
    input_wakes: bool,
) -> Option<Instant> {
    if still(now, last_input) && !input_wakes {
        let poll = now + INPUT_POLL;
        Some(due.map_or(poll, |due| due.min(poll)))
    } else {
        due
    }
}

/// How long after a try at reading a look's copy back the next is due, `misses` tries having
/// found the GPU not done with it: `READ_AFTER` -- the first try's, after the copy -- then twice
/// that, four times, and so on, `LOOK_INTERVAL` at most. A GPU the game keeps busy is waited for
/// with a few wakes, not one every `READ_AFTER`.
pub fn read_retry(misses: u32) -> Duration {
    READ_AFTER
        .saturating_mul(1 << misses.min(16))
        .min(LOOK_INTERVAL)
}

/// Whether the latest input Windows saw, at `last_input`, never woke the watcher by `now`, raw
/// input having been asked for at `armed`: it came more than `INPUT_GRACE` after the ask -- not
/// the move that led up to it, which Windows dates by its 16 ms ticks -- and more than
/// `INPUT_GRACE` ago, time enough for its raw input to have come. A game run as administrator
/// may send none to a program that isn't.
pub fn input_missed(armed: Instant, last_input: Instant, now: Instant) -> bool {
    last_input > armed + INPUT_GRACE && now.saturating_duration_since(last_input) > INPUT_GRACE
}

/// Whether a look at `now` reads the bar too, last read at `last_read`.
pub fn bar_due(last_read: Option<Instant>, now: Instant) -> bool {
    last_read.is_none_or(|at| now.saturating_duration_since(at) >= BAR_EVERY)
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
    fn the_watching_decides_again_at_the_settle_and_where_a_linger_ends() {
        let start = Instant::now();
        let mut when = in_front(start);
        assert_eq!(when.next_change(start), Some(start + SETTLE));
        assert!(when.settle_due(start + SETTLE));
        when.asked(true, start + SETTLE);
        // In front and settled: nothing changes by itself.
        assert_eq!(when.next_change(start + SETTLE), None);
        let left = start + Duration::from_secs(10);
        when.asked(false, left);
        assert_eq!(when.next_change(left), Some(left + LINGER));
        assert_eq!(when.next_change(left + LINGER), None);
        // Back in front before the linger ended: no end to wake for.
        when.asked(true, left + LINGER / 2);
        assert_eq!(when.next_change(left + LINGER / 2), None);
    }

    /// How long after the player's last input at `input` each look came, from a look then till
    /// `end`, each reading the rails as `clear` says.
    fn looks(input: Instant, end: Instant, clear: bool) -> Vec<Duration> {
        let mut looks = Vec::new();
        let mut at = input;
        while at < end {
            looks.push(at - input);
            at = next_look(at, input, clear);
        }
        looks
    }

    /// The time from each look to the next.
    fn gaps(looks: &[Duration]) -> Vec<Duration> {
        looks.windows(2).map(|pair| pair[1] - pair[0]).collect()
    }

    #[test]
    fn a_moving_player_is_looked_at_every_look_interval() {
        let input = Instant::now();
        // Moved at the look, or since.
        assert_eq!(next_look(input, input, true), input + LOOK_INTERVAL);
        let look = input + Duration::from_secs(5);
        assert_eq!(next_look(look, look + MS, true), look + LOOK_INTERVAL);
        // Still for just under `STILL_AFTER` at the look is moving yet, a rail covered or not.
        let look = input + STILL_AFTER - MS;
        assert_eq!(next_look(look, input, true), look + LOOK_INTERVAL);
        assert_eq!(next_look(look, input, false), look + LOOK_INTERVAL);
    }

    #[test]
    fn a_still_players_looks_thin_out_to_the_safety_net() {
        let input = Instant::now();
        let minute = Duration::from_secs(60);
        let looks = looks(input, input + minute, true);
        let gaps = gaps(&looks);
        // Further and further apart, never more than the safety net.
        assert!(gaps.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(gaps.iter().all(|&gap| gap <= SAFETY_NET));
        // `LOOK_INTERVAL` apart until they've kept still for `STILL_AFTER`.
        let moving = looks.iter().filter(|&&at| at < STILL_AFTER).count();
        assert!(gaps[..moving].iter().all(|&gap| gap == LOOK_INTERVAL));
        // A follow-up for a tooltip the game shows after a hover's delay.
        let follow_up = Duration::from_millis(300)..=Duration::from_millis(600);
        assert!(looks.iter().any(|at| follow_up.contains(at)));
        // Then one look every `SAFETY_NET`, and only a few more in the first seconds.
        assert_eq!(gaps.last(), Some(&SAFETY_NET));
        assert!(looks.len() as u128 <= minute.as_millis() / SAFETY_NET.as_millis() + 10);
    }

    #[test]
    fn a_covered_rail_is_looked_at_often_while_still_then_on_the_safety_net() {
        let input = Instant::now();
        let looks = looks(input, input + Duration::from_secs(30), false);
        for (&at, gap) in looks.iter().zip(gaps(&looks)) {
            if at < UNCOVER_WATCH {
                assert!(gap <= STILL_LOOK_INTERVAL, "{at:?}: {gap:?}");
            } else {
                assert_eq!(gap, SAFETY_NET, "{at:?}");
            }
        }
    }

    #[test]
    fn input_brings_the_next_look_forward() {
        let input = Instant::now();
        let look = input + Duration::from_secs(10);
        assert_eq!(next_look(look, input, true), look + SAFETY_NET);
        // They moved 300 ms after that look: the next is due at once.
        let moved = look + Duration::from_millis(300);
        assert!(next_look(look, moved, true) <= moved);
    }

    #[test]
    fn a_still_player_is_asked_after_only_where_their_input_cannot_end_the_wait() {
        let input = Instant::now();
        let now = input + Duration::from_secs(5);
        let due = Some(now + SAFETY_NET);
        assert_eq!(wait_until(due, now, input, true), due);
        assert_eq!(wait_until(due, now, input, false), Some(now + INPUT_POLL));
        assert_eq!(wait_until(None, now, input, false), Some(now + INPUT_POLL));
        // Anything sooner stands.
        let soon = Some(now + READ_AFTER);
        assert_eq!(wait_until(soon, now, input, false), soon);
        // Nor is a moving player asked after: their looks come at their own pace.
        assert_eq!(wait_until(due, now, now, false), due);
    }

    #[test]
    fn a_read_back_the_gpu_has_not_made_is_tried_later_and_later() {
        assert_eq!(read_retry(0), READ_AFTER);
        let retries: Vec<Duration> = (0..8).map(read_retry).collect();
        assert!(
            retries
                .windows(2)
                .all(|pair| pair[0] < pair[1] || pair[1] == LOOK_INTERVAL)
        );
        assert_eq!(read_retry(u32::MAX), LOOK_INTERVAL);
        // A GPU the game keeps busy for a tenth of a second is tried a few times, not 25.
        let mut waited = Duration::ZERO;
        let mut tries = 0;
        while waited < Duration::from_millis(100) {
            waited += read_retry(tries);
            tries += 1;
        }
        assert!(tries <= 5, "{tries}");
    }

    #[test]
    fn input_that_never_woke_the_watcher_is_told_from_input_on_its_way() {
        let armed = Instant::now();
        // The move that led up to the ask, dated by Windows' coarse ticks.
        let late = armed + Duration::from_secs(2);
        assert!(!input_missed(armed, armed + INPUT_GRACE / 2, late));
        // Input after the ask, its raw input maybe on its way yet...
        let input = armed + Duration::from_secs(1);
        assert!(!input_missed(armed, input, input + INPUT_GRACE / 2));
        // ...but not this long after.
        assert!(input_missed(armed, input, input + INPUT_GRACE + MS));
    }
}
