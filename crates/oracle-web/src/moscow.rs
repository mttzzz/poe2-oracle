//! Moscow calendar days. The counters, the daily report limits and the morning digest all follow
//! the owner's clock, and Moscow has kept UTC+3 all year since 2014: a fixed offset is exact, and
//! no time-zone database is needed.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

const OFFSET_SECS: i64 = 3 * 3600;
pub const DAY_SECS: i64 = 86_400;

/// Unix seconds, now.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

/// One calendar day in Moscow, as days since 1970-01-01 there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Day(i64);

impl Day {
    /// The Moscow day `unix` falls on.
    pub fn of(unix: i64) -> Day {
        Day((unix + OFFSET_SECS).div_euclid(DAY_SECS))
    }

    /// `days` days earlier.
    pub fn minus(self, days: i64) -> Day {
        Day(self.0 - days)
    }

    /// Midnight in Moscow that starts the day, in Unix seconds.
    pub fn start(self) -> i64 {
        self.0 * DAY_SECS - OFFSET_SECS
    }

    /// Midnight in Moscow that ends the day.
    pub fn end(self) -> i64 {
        self.start() + DAY_SECS
    }

    /// Year, month (1-12) and day of the month.
    pub fn date(self) -> (i64, u32, u32) {
        civil_from_days(self.0)
    }

    /// Monday 0 to Sunday 6. 1970-01-01 was a Thursday.
    pub fn weekday(self) -> usize {
        (self.0 + 3).rem_euclid(7) as usize
    }
}

/// `YYYY-MM-DD`, as the Redis keys carry it.
impl fmt::Display for Day {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = self.date();
        write!(f, "{year:04}-{month:02}-{day:02}")
    }
}

/// `hour`:00 in Moscow next after `unix`.
pub fn next_hour_of_day(unix: i64, hour: i64) -> i64 {
    let today = Day::of(unix).start() + hour * 3600;
    if today > unix {
        today
    } else {
        today + DAY_SECS
    }
}

/// `YYYY-MM-DD-HHMMSS` in Moscow: names a report's files when it has no issue number.
pub fn stamp(unix: i64) -> String {
    let secs = (unix + OFFSET_SECS).rem_euclid(DAY_SECS);
    format!(
        "{}-{:02}{:02}{:02}",
        Day::of(unix),
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// The proleptic Gregorian date of a day count since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_from_march + 2) / 5 + 1) as u32;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-09-25 21:00:00 UTC: midnight of 2026-09-26 in Moscow.
    const MOSCOW_MIDNIGHT: i64 = 1_790_370_000;

    #[test]
    fn the_day_turns_at_moscow_midnight() {
        assert_eq!(Day::of(MOSCOW_MIDNIGHT - 1).to_string(), "2026-09-25");
        assert_eq!(Day::of(MOSCOW_MIDNIGHT).to_string(), "2026-09-26");
        assert_eq!(Day::of(MOSCOW_MIDNIGHT).start(), MOSCOW_MIDNIGHT);
        assert_eq!(Day::of(MOSCOW_MIDNIGHT - 1).end(), MOSCOW_MIDNIGHT);
        // 23:59 UTC on the 25th is already the 26th in Moscow.
        assert_eq!(
            Day::of(MOSCOW_MIDNIGHT + 3 * 3600 - 60).to_string(),
            "2026-09-26"
        );
    }

    #[test]
    fn dates_and_weekdays_across_months_and_leap_years() {
        assert_eq!(Day::of(0).to_string(), "1970-01-01");
        assert_eq!(Day::of(0).weekday(), 3, "a Thursday");
        let saturday = Day::of(MOSCOW_MIDNIGHT);
        assert_eq!(saturday.weekday(), 5);
        assert_eq!(saturday.minus(7).to_string(), "2026-09-19");
        assert_eq!(saturday.minus(7).weekday(), 5);
        assert_eq!(saturday.minus(26).to_string(), "2026-08-31");
        // 2024-03-01 00:00 MSK, the day after a leap day.
        let march = Day::of(1_709_240_400);
        assert_eq!(march.to_string(), "2024-03-01");
        assert_eq!(march.minus(1).to_string(), "2024-02-29");
        assert_eq!(march.minus(1).weekday(), 3, "2024-02-29 was a Thursday");
        assert_eq!(saturday.minus(268).to_string(), "2026-01-01");
    }

    #[test]
    fn the_next_nine_oclock_in_moscow() {
        let nine = MOSCOW_MIDNIGHT + 9 * 3600;
        assert_eq!(next_hour_of_day(MOSCOW_MIDNIGHT, 9), nine);
        assert_eq!(next_hour_of_day(nine - 1, 9), nine);
        // At nine sharp the next one is tomorrow's: a digest never goes twice for one moment.
        assert_eq!(next_hour_of_day(nine, 9), nine + DAY_SECS);
        // 23:30 in Moscow is 20:30 UTC, still the previous UTC evening.
        assert_eq!(next_hour_of_day(MOSCOW_MIDNIGHT - 1800, 9), nine);
    }

    #[test]
    fn stamps_are_moscow_time() {
        assert_eq!(
            stamp(MOSCOW_MIDNIGHT + 9 * 3600 + 5 * 60 + 7),
            "2026-09-26-090507"
        );
        assert_eq!(stamp(MOSCOW_MIDNIGHT - 1), "2026-09-25-235959");
    }
}
