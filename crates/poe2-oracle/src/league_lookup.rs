//! The signed-in account's PoE 2 private leagues as the app knows them -- the list
//! pathofexile.com last gave (`trade_client::private_leagues::mine`) -- and how looking them up
//! goes, as the settings window's Account section says it.
//!
//! One lookup at a time: another reason to look while one is under way joins it, and a press of
//! «Обновить» makes it the player's, whose outcome the section then says. An answer counts only
//! while it speaks for the session it was asked with: a sign-out forgets the list and whatever was
//! under way. No answer keeps the list as it was. The next lookup by itself is due an interval
//! after the last one ended (`Settings::private_leagues_refresh`).
//!
//! Pure, so the native test pass covers it; `price_check` runs the lookups and their timer.

use std::time::{Duration, Instant};

use trade_client::private_leagues::PrivateLeague;

/// What the Account section says of the lookups, under the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupLine {
    /// One is under way that the player asked for, or that would give the first list.
    Looking,
    /// The one the player asked for found this many leagues -- some: an empty list says none
    /// itself.
    Found(usize),
    /// The last one got no answer, and this is why: said when the player asked for it, or while
    /// there's no list to show.
    Failed(String),
}

/// A lookup started: its answer is taken back with it ([`LeagueLookup::finish`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket(u64);

#[derive(Debug, Default)]
pub struct LeagueLookup {
    leagues: Vec<PrivateLeague>,
    /// The session the lookups speak for, bumped by a sign-out: an answer asked for before it is
    /// dropped.
    session: u64,
    /// The lookup under way, if any: whether the player asked for it.
    running: Option<bool>,
    /// A lookup has answered since the sign-in: `leagues` is the account's list, not empty for want
    /// of an answer.
    answered: bool,
    /// How the last one ended -- how many leagues it found, or why it got no answer -- and whether
    /// the player asked for it.
    last: Option<(bool, Result<usize, String>)>,
    /// When the last one ended.
    ended: Option<Instant>,
}

impl LeagueLookup {
    /// The account's leagues as the last answer listed them; empty while signed out.
    pub fn leagues(&self) -> &[PrivateLeague] {
        &self.leagues
    }

    /// Whether a lookup has answered since the sign-in: an empty list then means the account has
    /// none.
    pub fn answered(&self) -> bool {
        self.answered
    }

    /// Whether a lookup the player asked for is under way.
    pub fn busy(&self) -> bool {
        self.running == Some(true)
    }

    /// A lookup to start -- `asked`: the player pressed «Обновить» -- or `None` while one is under
    /// way, which the press makes the player's.
    pub fn start(&mut self, asked: bool) -> Option<Ticket> {
        match &mut self.running {
            Some(running) => {
                *running |= asked;
                None
            }
            None => {
                self.running = Some(asked);
                Some(Ticket(self.session))
            }
        }
    }

    /// Takes a lookup's answer, which came at `now`: the account's leagues, or why there were
    /// none. Whether it was taken: not after a sign-out.
    pub fn finish(
        &mut self,
        ticket: Ticket,
        answer: Result<Vec<PrivateLeague>, String>,
        now: Instant,
    ) -> bool {
        if ticket.0 != self.session {
            return false;
        }
        let asked = self.running.take().unwrap_or(false);
        let outcome = answer.map(|leagues| {
            self.answered = true;
            self.leagues = leagues;
            self.leagues.len()
        });
        self.last = Some((asked, outcome));
        self.ended = Some(now);
        true
    }

    /// Signed out: the list, and whatever lookup was under way, are forgotten. Whether there was
    /// anything to forget.
    pub fn forget(&mut self) -> bool {
        let had = !self.leagues.is_empty() || self.running.is_some() || self.last.is_some();
        *self = LeagueLookup {
            session: self.session + 1,
            ..LeagueLookup::default()
        };
        had
    }

    /// How long from `now` until the next lookup by itself, one `every` after the last one ended:
    /// none if that's past, or if none has ended since the sign-in.
    pub fn due_in(&self, every: Duration, now: Instant) -> Duration {
        self.ended.map_or(Duration::ZERO, |ended| {
            (ended + every).saturating_duration_since(now)
        })
    }

    /// What the Account section says under the list, if anything.
    pub fn line(&self) -> Option<LookupLine> {
        match (self.running, &self.last) {
            (Some(asked), _) if asked || !self.answered => Some(LookupLine::Looking),
            (_, Some((asked, Err(reason)))) if *asked || !self.answered => {
                Some(LookupLine::Failed(reason.clone()))
            }
            (_, Some((true, Ok(found)))) if *found > 0 => Some(LookupLine::Found(*found)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cardiff() -> PrivateLeague {
        PrivateLeague {
            id: "HC FRites League by Cardiff (PL86503)".to_owned(),
            parent: "HC Forbidden Rites".to_owned(),
        }
    }

    #[test]
    fn a_press_during_a_lookup_joins_it_and_its_outcome_is_said() {
        let mut lookup = LeagueLookup::default();
        let ticket = lookup.start(false).unwrap();
        assert!(!lookup.busy(), "a lookup by itself isn't the player's");
        assert_eq!(lookup.start(true), None, "one at a time");
        assert!(lookup.busy());
        assert_eq!(lookup.line(), Some(LookupLine::Looking));

        assert!(lookup.finish(ticket, Ok(vec![cardiff()]), Instant::now()));
        assert!(!lookup.busy());
        assert_eq!(lookup.leagues(), [cardiff()]);
        assert_eq!(lookup.line(), Some(LookupLine::Found(1)));

        // One by itself after it says nothing of its own: the list is there to see.
        let ticket = lookup.start(false).unwrap();
        assert_eq!(lookup.line(), Some(LookupLine::Found(1)), "until it ends");
        assert!(lookup.finish(ticket, Ok(vec![cardiff()]), Instant::now()));
        assert_eq!(lookup.line(), None);
    }

    #[test]
    fn no_answer_keeps_the_list_and_says_why_only_when_asked_or_nothing_is_known() {
        let mut lookup = LeagueLookup::default();
        let ticket = lookup.start(false).unwrap();
        assert_eq!(lookup.line(), Some(LookupLine::Looking), "the first list");
        lookup.finish(ticket, Err("HTTP 503".to_owned()), Instant::now());
        assert!(!lookup.answered());
        assert_eq!(
            lookup.line(),
            Some(LookupLine::Failed("HTTP 503".to_owned()))
        );

        let ticket = lookup.start(false).unwrap();
        lookup.finish(ticket, Ok(vec![cardiff()]), Instant::now());
        let ticket = lookup.start(false).unwrap();
        lookup.finish(ticket, Err("timed out".to_owned()), Instant::now());
        assert_eq!(lookup.leagues(), [cardiff()]);
        assert_eq!(lookup.line(), None, "a lookup by itself stays quiet");

        let ticket = lookup.start(true).unwrap();
        lookup.finish(ticket, Err("timed out".to_owned()), Instant::now());
        assert_eq!(lookup.leagues(), [cardiff()]);
        assert_eq!(
            lookup.line(),
            Some(LookupLine::Failed("timed out".to_owned()))
        );
    }

    #[test]
    fn an_answer_asked_for_before_a_sign_out_is_dropped() {
        let mut lookup = LeagueLookup::default();
        let ticket = lookup.start(true).unwrap();
        assert!(lookup.forget());
        assert!(!lookup.busy());
        assert!(!lookup.finish(ticket, Ok(vec![cardiff()]), Instant::now()));
        assert!(lookup.leagues().is_empty());
        assert_eq!(lookup.line(), None);

        // Signed in again, a lookup starts at once and its answer counts.
        let ticket = lookup.start(false).unwrap();
        assert!(lookup.finish(ticket, Ok(Vec::new()), Instant::now()));
        assert!(lookup.answered());
    }

    #[test]
    fn the_next_lookup_by_itself_is_due_an_interval_after_the_last_ended() {
        const HOUR: Duration = Duration::from_secs(3600);
        let mut lookup = LeagueLookup::default();
        let start = Instant::now();
        assert_eq!(lookup.due_in(HOUR, start), Duration::ZERO, "none yet");

        let ticket = lookup.start(false).unwrap();
        lookup.finish(ticket, Ok(Vec::new()), start);
        let later = start + Duration::from_secs(600);
        assert_eq!(lookup.due_in(HOUR, later), HOUR - Duration::from_secs(600));
        // A shorter interval picked since: past due, so at once.
        assert_eq!(
            lookup.due_in(Duration::from_secs(300), later),
            Duration::ZERO
        );
    }
}
