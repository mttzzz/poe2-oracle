//! "N мин. назад"-style listing ages for the results table, from the trade API's `indexed`
//! timestamps (`2026-09-20T04:31:02Z` -- UTC, whole seconds, verified live). Pure and not
//! Windows-gated, so the native CI test pass covers it.

/// The age of a listing `indexed` at the given ISO-8601 UTC timestamp, as of `now_unix`, in the
/// short Russian form `Intl.RelativeTimeFormat("ru", { style: "short" })` produces (the form
/// EE2's Russian UI shows): `5 мин. назад`, `2 ч. назад`, `3 дн. назад`, `1 мес. назад`.
/// `None` for a timestamp that isn't in the trade API's shape.
pub fn listed_ago(indexed: &str, now_unix: i64) -> Option<String> {
    // A clock slightly behind the server's must not produce "in the future" ages.
    let elapsed = (now_unix - parse_utc(indexed)?).max(0);
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    Some(match elapsed {
        s if s < MINUTE => "только что".to_owned(),
        s if s < HOUR => format!("{} мин. назад", s / MINUTE),
        s if s < DAY => format!("{} ч. назад", s / HOUR),
        s if s < 30 * DAY => format!("{} дн. назад", s / DAY),
        s if s < 365 * DAY => format!("{} мес. назад", s / (30 * DAY)),
        s => format!("{} г. назад", s / (365 * DAY)),
    })
}

/// Seconds since the Unix epoch for `YYYY-MM-DDTHH:MM:SSZ`.
fn parse_utc(timestamp: &str) -> Option<i64> {
    let (date, time) = timestamp.strip_suffix('Z')?.split_once('T')?;
    let [year, month, day] = fields(date, '-')?;
    let [hour, minute, second] = fields(time, ':')?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn fields(text: &str, separator: char) -> Option<[i64; 3]> {
    let mut parts = text.split(separator).map(|part| part.parse::<i64>().ok());
    let parsed = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(parsed)
}

/// Days since 1970-01-01 for a proleptic Gregorian date -- Howard Hinnant's `days_from_civil`,
/// which counts years from March so the leap day is the last day of its year.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_trade_timestamps_across_century_and_leap_days() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc("1999-12-31T23:59:59Z"), Some(946_684_799));
        assert_eq!(parse_utc("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(parse_utc("2024-02-29T12:00:00Z"), Some(1_709_208_000));
        assert_eq!(parse_utc("2026-09-20T04:31:02Z"), Some(1_789_878_662));
    }

    #[test]
    fn picks_the_largest_whole_unit() {
        let indexed = "2026-09-20T04:31:02Z";
        let at = |elapsed: i64| listed_ago(indexed, 1_789_878_662 + elapsed).unwrap();
        assert_eq!(at(59), "только что");
        assert_eq!(at(60), "1 мин. назад");
        assert_eq!(at(3_599), "59 мин. назад");
        assert_eq!(at(3_600), "1 ч. назад");
        assert_eq!(at(2 * 86_400 + 5), "2 дн. назад");
        assert_eq!(at(40 * 86_400), "1 мес. назад");
        assert_eq!(at(400 * 86_400), "1 г. назад");
        // Local clock behind the server's.
        assert_eq!(at(-30), "только что");
    }

    #[test]
    fn rejects_other_shapes() {
        assert_eq!(listed_ago("2026-09-20 04:31:02", 0), None);
        assert_eq!(listed_ago("2026-13-20T04:31:02Z", 0), None);
        assert_eq!(listed_ago("2026-09-20T04:31Z", 0), None);
        assert_eq!(listed_ago("", 0), None);
    }
}
