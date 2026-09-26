//! Where the running app shows itself -- its icon in the notification area, its taskbar button or
//! both, as the player chose (`Settings::app_icon`) -- apart from Windows and GPUI: the steps `app`
//! takes toward the choice ([`settle`]), and when it tries again after they fell short ([`Tries`]).
//!
//! The two are the app's way in: its panel is a `PopUp` window, which has no taskbar button, so
//! with neither of them only the panel's ⚙ or a second launch would reach the settings. Whatever
//! fails, one is never taken away before the other shows: a choice that can't show keeps what
//! showed before, and at the start, with nothing showing yet, the other one stands in for it.

use std::fmt;
use std::time::Duration;

use crate::settings::AppIcon;

/// What of the app shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shows {
    /// Its icon in the notification area.
    pub tray: bool,
    /// Its taskbar button.
    pub button: bool,
}

impl Shows {
    /// What the player's choice shows.
    pub fn wanted(app_icon: AppIcon) -> Shows {
        Shows {
            tray: app_icon.tray(),
            button: app_icon.taskbar(),
        }
    }
}

/// A step toward the player's choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The tray icon shows: made the first time, shown again once it was hidden.
    ShowTray,
    /// The taskbar button is made.
    ShowButton,
    /// The tray icon is hidden.
    HideTray,
    /// The taskbar button is removed.
    RemoveButton,
}

/// What the step does, for the log: "showing the tray icon failed: …".
impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Step::ShowTray => "showing the tray icon",
            Step::ShowButton => "showing the taskbar button",
            Step::HideTray => "hiding the tray icon",
            Step::RemoveButton => "removing the taskbar button",
        })
    }
}

/// Takes the steps from what `shows` toward what `wanted` shows, each by `take`, which says whether
/// it took, and gives what shows after them. What is wanted shows first; what isn't goes only while
/// the other one shows -- the wanted one, then. When nothing shows at all (the app's start) and the
/// wanted one can't, the other one is made instead. No step is taken twice: one that failed waits
/// for the next pass ([`Tries`]).
pub fn settle(wanted: AppIcon, mut shows: Shows, mut take: impl FnMut(Step) -> bool) -> Shows {
    let want = Shows::wanted(wanted);
    if want.tray && !shows.tray {
        shows.tray = take(Step::ShowTray);
    }
    if want.button && !shows.button {
        shows.button = take(Step::ShowButton);
    }
    if !shows.tray && !shows.button {
        match wanted {
            AppIcon::Tray => shows.button = take(Step::ShowButton),
            AppIcon::Taskbar => shows.tray = take(Step::ShowTray),
            // Both were tried just now.
            AppIcon::Both => {}
        }
    }
    if !want.tray && shows.tray && shows.button {
        shows.tray = !take(Step::HideTray);
    }
    if !want.button && shows.button && shows.tray {
        shows.button = !take(Step::RemoveButton);
    }
    shows
}

/// How the passes toward the player's choice went: whether the next one waits after one fell
/// short, and which failures the log already has.
#[derive(Debug, Default)]
pub struct Tries {
    short: Option<Short>,
    /// The steps whose failure was logged and that haven't taken since, by `Step as usize`.
    logged: [bool; 4],
}

/// Passes in a row that fell short of the same choice.
#[derive(Debug)]
struct Short {
    wanted: AppIcon,
    times: u32,
    /// The next pass waits ([`Tries::waited`]).
    waiting: bool,
}

impl Tries {
    /// Whether a pass toward `wanted` may run: not while the wait after passes that fell short of
    /// the same choice lasts. Another choice goes at once.
    pub fn due(&self, wanted: AppIcon) -> bool {
        !matches!(
            self.short,
            Some(Short { wanted: short_of, waiting: true, .. }) if short_of == wanted
        )
    }

    /// Notes what shows after a pass toward `wanted`. Short of it, the next pass waits -- 5 s,
    /// doubling up to 5 min over the passes in a row that fall short of the same choice -- and this
    /// says how long, until [`Tries::waited`].
    pub fn ended(&mut self, wanted: AppIcon, shows: Shows) -> Option<Duration> {
        if shows == Shows::wanted(wanted) {
            self.short = None;
            return None;
        }
        let times = match &self.short {
            Some(short) if short.wanted == wanted => short.times.saturating_add(1),
            _ => 1,
        };
        self.short = Some(Short {
            wanted,
            times,
            waiting: true,
        });
        Some(retry_delay(times))
    }

    /// The wait [`Tries::ended`] asked for is over.
    pub fn waited(&mut self) {
        if let Some(short) = &mut self.short {
            short.waiting = false;
        }
    }

    /// Notes whether `step` took, and says whether that's news for the log: its failure the first
    /// time since it last took, and its taking once a failure of it was logged -- not every retry.
    pub fn news(&mut self, step: Step, took: bool) -> bool {
        let logged = &mut self.logged[step as usize];
        let news = *logged == took;
        *logged = !took;
        news
    }
}

/// How long after the `times`-th pass in a row that fell short the next one runs: 5 s, doubling up
/// to 5 min.
fn retry_delay(times: u32) -> Duration {
    const FIRST: Duration = Duration::from_secs(5);
    const LONGEST: Duration = Duration::from_secs(5 * 60);
    let doublings = times.saturating_sub(1);
    FIRST
        .saturating_mul(1u32.checked_shl(doublings).unwrap_or(u32::MAX))
        .min(LONGEST)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Step::{HideTray, RemoveButton, ShowButton, ShowTray};

    const NOTHING: Shows = Shows {
        tray: false,
        button: false,
    };
    const TRAY: Shows = Shows {
        tray: true,
        button: false,
    };
    const BUTTON: Shows = Shows {
        tray: false,
        button: true,
    };
    const BOTH: Shows = Shows {
        tray: true,
        button: true,
    };
    const CHOICES: [AppIcon; 3] = [AppIcon::Tray, AppIcon::Taskbar, AppIcon::Both];
    const STEPS: [Step; 4] = [ShowTray, ShowButton, HideTray, RemoveButton];

    /// The steps [`settle`] takes, in order, with those in `failing` failing, and what shows after.
    fn settled(wanted: AppIcon, shows: Shows, failing: &[Step]) -> (Vec<Step>, Shows) {
        let mut taken = Vec::new();
        let after = settle(wanted, shows, |step| {
            taken.push(step);
            !failing.contains(&step)
        });
        (taken, after)
    }

    #[test]
    fn the_new_one_shows_before_the_old_one_goes() {
        assert_eq!(
            settled(AppIcon::Taskbar, TRAY, &[]),
            (vec![ShowButton, HideTray], BUTTON)
        );
        assert_eq!(
            settled(AppIcon::Tray, BUTTON, &[]),
            (vec![ShowTray, RemoveButton], TRAY)
        );
        assert_eq!(settled(AppIcon::Both, TRAY, &[]), (vec![ShowButton], BOTH));
        assert_eq!(
            settled(AppIcon::Tray, BOTH, &[]),
            (vec![RemoveButton], TRAY)
        );
    }

    #[test]
    fn the_old_one_stays_when_the_new_one_cant_show() {
        assert_eq!(
            settled(AppIcon::Taskbar, TRAY, &[ShowButton]),
            (vec![ShowButton], TRAY)
        );
        assert_eq!(
            settled(AppIcon::Tray, BUTTON, &[ShowTray]),
            (vec![ShowTray], BUTTON)
        );
        assert_eq!(
            settled(AppIcon::Both, BUTTON, &[ShowTray]),
            (vec![ShowTray], BUTTON)
        );
    }

    #[test]
    fn at_the_start_the_other_one_stands_in_for_one_that_cant_show() {
        assert_eq!(
            settled(AppIcon::Tray, NOTHING, &[ShowTray]),
            (vec![ShowTray, ShowButton], BUTTON)
        );
        assert_eq!(
            settled(AppIcon::Taskbar, NOTHING, &[ShowButton]),
            (vec![ShowButton, ShowTray], TRAY)
        );
        // Either of both is enough.
        assert_eq!(
            settled(AppIcon::Both, NOTHING, &[ShowTray]),
            (vec![ShowTray, ShowButton], BUTTON)
        );
        // With neither possible, the next pass tries again.
        assert_eq!(
            settled(AppIcon::Tray, NOTHING, &[ShowTray, ShowButton]),
            (vec![ShowTray, ShowButton], NOTHING)
        );
    }

    #[test]
    fn a_stand_in_goes_once_the_chosen_one_shows() {
        let (_, stand_in) = settled(AppIcon::Tray, NOTHING, &[ShowTray]);
        assert_eq!(
            settled(AppIcon::Tray, stand_in, &[]),
            (vec![ShowTray, RemoveButton], TRAY)
        );
    }

    #[test]
    fn whatever_fails_the_app_never_disappears() {
        for wanted in CHOICES {
            let want = Shows::wanted(wanted);
            for shows in [NOTHING, TRAY, BUTTON, BOTH] {
                for failures in 0..(1u8 << STEPS.len()) {
                    let failing: Vec<Step> = STEPS
                        .into_iter()
                        .enumerate()
                        .filter(|&(bit, _)| failures & (1 << bit) != 0)
                        .map(|(_, step)| step)
                        .collect();
                    let case = format!("{wanted:?} from {shows:?}, {failing:?} failing");
                    let (taken, after) = settled(wanted, shows, &failing);
                    if shows != NOTHING {
                        assert_ne!(after, NOTHING, "{case}");
                    }
                    if failing.is_empty() {
                        assert_eq!(after, want, "{case}");
                    }
                    if shows == want {
                        assert!(taken.is_empty(), "{case}: {taken:?}");
                    }
                    for (i, step) in taken.iter().enumerate() {
                        assert!(!taken[..i].contains(step), "{case}: {taken:?}");
                    }
                    // A stand-in only while nothing shows.
                    let stand_in = (!want.button && taken.contains(&ShowButton))
                        || (!want.tray && taken.contains(&ShowTray));
                    assert!(!stand_in || shows == NOTHING, "{case}: {taken:?}");
                }
            }
        }
    }

    #[test]
    fn a_pass_that_falls_short_waits_5_s_doubling_to_5_min() {
        let mut tries = Tries::default();
        let mut waits = Vec::new();
        for _ in 0..8 {
            let wait = tries.ended(AppIcon::Tray, BUTTON).unwrap();
            assert!(!tries.due(AppIcon::Tray));
            tries.waited();
            assert!(tries.due(AppIcon::Tray));
            waits.push(wait.as_secs());
        }
        assert_eq!(waits, [5, 10, 20, 40, 80, 160, 300, 300]);
    }

    #[test]
    fn another_choice_doesnt_wait_for_one_that_fell_short() {
        let mut tries = Tries::default();
        tries.ended(AppIcon::Taskbar, TRAY);
        tries.waited();
        tries.ended(AppIcon::Taskbar, TRAY);
        assert!(!tries.due(AppIcon::Taskbar));
        assert!(tries.due(AppIcon::Tray));
        assert!(tries.due(AppIcon::Both));
        // Its own waits start at the first.
        assert_eq!(
            tries.ended(AppIcon::Both, TRAY),
            Some(Duration::from_secs(5))
        );
        assert!(tries.due(AppIcon::Taskbar));
    }

    #[test]
    fn a_pass_that_gets_there_ends_the_waits() {
        let mut tries = Tries::default();
        tries.ended(AppIcon::Taskbar, TRAY);
        tries.waited();
        tries.ended(AppIcon::Taskbar, BOTH);
        assert_eq!(tries.ended(AppIcon::Taskbar, BUTTON), None);
        assert!(tries.due(AppIcon::Taskbar));
        assert_eq!(
            tries.ended(AppIcon::Taskbar, TRAY),
            Some(Duration::from_secs(5))
        );
    }

    #[test]
    fn a_failure_is_logged_once_until_its_step_takes() {
        let mut tries = Tries::default();
        assert!(tries.news(ShowTray, false));
        assert!(!tries.news(ShowTray, false));
        assert!(tries.news(ShowButton, false));
        assert!(!tries.news(HideTray, true));
        // Taking after all is news once.
        assert!(tries.news(ShowTray, true));
        assert!(!tries.news(ShowTray, true));
        assert!(tries.news(ShowTray, false));
        assert!(!tries.news(ShowButton, false));
    }
}
