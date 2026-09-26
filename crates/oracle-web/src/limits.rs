//! How many reports the service takes: per client address, and all together per day. The limits
//! keep a stuck "Send" button or a script from flooding the owner's issues and Telegram; a player
//! writing in earnest never meets them.

use std::net::{IpAddr, Ipv6Addr};

use axum::http::HeaderMap;
use oracle_protocol::ReportSource;

use crate::moscow::Day;
use crate::store::{Counter, Store, full};

/// A span of time a limit counts in. Fixed windows: the counters are plain Redis counters that
/// expire with their window.
#[derive(Clone, Copy, Debug)]
enum Window {
    /// Clock-aligned ten minutes: :00 to :10, :10 to :20…
    TenMinutes,
    /// A Moscow calendar day.
    Day,
}

struct Limit {
    name: &'static str,
    max: u64,
    window: Window,
    /// Counted per client address, or for everyone together.
    per_client: bool,
}

/// Reports from the app: a player may write a few in a row, crash reports included.
const APP: [Limit; 3] = [
    Limit {
        name: "app-10m",
        max: 5,
        window: Window::TenMinutes,
        per_client: true,
    },
    Limit {
        name: "app-day",
        max: 20,
        window: Window::Day,
        per_client: true,
    },
    EVERYONE,
];
/// Reports from the site's form, which anyone can post without the app.
const SITE: [Limit; 2] = [
    Limit {
        name: "site-10m",
        max: 3,
        window: Window::TenMinutes,
        per_client: true,
    },
    EVERYONE,
];
/// All reports together, from anywhere: the owner's inbox stays readable whatever happens.
const EVERYONE: Limit = Limit {
    name: "all-day",
    max: 300,
    window: Window::Day,
    per_client: false,
};

/// The counters a report from `source`, sent by `client` (a [`client_key`]) at `now`, counts
/// against.
pub fn counters(source: ReportSource, client: &str, now: i64) -> Vec<Counter> {
    let limits: &[Limit] = match source {
        ReportSource::App => &APP,
        ReportSource::Site => &SITE,
    };
    limits
        .iter()
        .map(|limit| {
            let (window, expires_at) = match limit.window {
                Window::TenMinutes => {
                    let index = now.div_euclid(600);
                    (index.to_string(), (index + 1) * 600)
                }
                Window::Day => {
                    let day = Day::of(now);
                    (day.to_string(), day.end())
                }
            };
            let key = if limit.per_client {
                format!("oracle:rl:{}:{client}:{window}", limit.name)
            } else {
                format!("oracle:rl:{}:{window}", limit.name)
            };
            Counter {
                key,
                max: limit.max,
                expires_at,
            }
        })
        .collect()
}

/// Whether `client` is turned away at `now` whatever its report turns out to be -- the limits of
/// every source are full for it -- and for how many seconds: the service then refuses the report
/// before reading its body. It only reads the counters. A report it lets on is counted, or turned
/// away, by [`Store::admit`]; a Redis failure lets it on.
pub async fn closed_to(store: &Store, client: &str, now: i64) -> Option<i64> {
    let sources =
        [ReportSource::App, ReportSource::Site].map(|source| counters(source, client, now));
    let keys: Vec<String> = sources
        .iter()
        .flatten()
        .map(|counter| counter.key.clone())
        .collect();
    let values = store.values(&keys, now).await?;
    let mut values = values.get(..keys.len())?;
    let mut soonest: Option<i64> = None;
    for counters in &sources {
        let (these, rest) = values.split_at(counters.len());
        values = rest;
        // A source open to the client: its report may be one.
        let wait = full(counters, these, now)?;
        soonest = Some(soonest.map_or(wait, |soonest| soonest.min(wait)));
    }
    soonest
}

/// Who sent a request, as the rate limits count them: the right-most `X-Forwarded-For` entry,
/// else `peer`, the address the connection came from.
///
/// In production the pod sits behind Envoy, which appends the address it got the connection from
/// (the gateway's PROXY protocol gives it the player's) to whatever `X-Forwarded-For` the client
/// sent. Only that last entry is Envoy's; everything before it is the client's to invent.
///
/// An IPv6 address counts by its /64: one household or phone gets a whole /64 and can take a new
/// address from it at will.
pub fn client_key(headers: &HeaderMap, peer: Option<IpAddr>) -> String {
    let forwarded = headers
        .get_all("x-forwarded-for")
        .iter()
        .next_back()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit(',').next())
        .and_then(|entry| entry.trim().parse::<IpAddr>().ok());
    match forwarded.or(peer).map(|address| address.to_canonical()) {
        Some(IpAddr::V4(address)) => address.to_string(),
        Some(IpAddr::V6(address)) => {
            let [a, b, c, d, ..] = address.segments();
            format!("{}/64", Ipv6Addr::new(a, b, c, d, 0, 0, 0, 0))
        }
        None => "unknown".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use axum::http::HeaderValue;

    use super::*;

    /// 2026-09-26 12:00:00 in Moscow: 09:00 UTC, the start of a ten-minute window.
    const NOON: i64 = 1_790_413_200;

    fn headers(forwarded: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in forwarded {
            headers.append("x-forwarded-for", HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    const PEER: Option<IpAddr> = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)));

    #[test]
    fn the_client_is_the_entry_envoy_appended() {
        assert_eq!(client_key(&headers(&["203.0.113.9"]), PEER), "203.0.113.9");
        // What the client sent itself comes first and is ignored.
        assert_eq!(
            client_key(&headers(&["1.1.1.1, 203.0.113.9"]), PEER),
            "203.0.113.9"
        );
        assert_eq!(
            client_key(&headers(&["1.1.1.1", "8.8.8.8,203.0.113.9 "]), PEER),
            "203.0.113.9"
        );
        // No usable entry: the connection's own address.
        assert_eq!(client_key(&headers(&[]), PEER), "10.0.0.7");
        assert_eq!(
            client_key(&headers(&["1.1.1.1, garbage"]), PEER),
            "10.0.0.7"
        );
        assert_eq!(client_key(&headers(&[]), None), "unknown");
    }

    #[test]
    fn ipv6_clients_count_by_their_64() {
        let one = client_key(&headers(&["2001:db8:1:2:aaaa::1"]), PEER);
        let other = client_key(&headers(&["2001:db8:1:2:bbbb::2"]), PEER);
        assert_eq!(one, "2001:db8:1:2::/64");
        assert_eq!(one, other);
        assert_ne!(one, client_key(&headers(&["2001:db8:1:3::1"]), PEER));
        // An IPv4 address written as IPv6 is the IPv4 client.
        assert_eq!(
            client_key(&headers(&["::ffff:203.0.113.9"]), PEER),
            "203.0.113.9"
        );
    }

    async fn send(store: &Store, source: ReportSource, client: &str, now: i64) -> Result<(), i64> {
        store.admit(&counters(source, client, now), now).await
    }

    #[tokio::test]
    async fn app_reports_five_per_ten_minutes() {
        let store = Store::memory();
        for minute in 0..5 {
            assert_eq!(
                send(&store, ReportSource::App, "a", NOON + minute * 60).await,
                Ok(())
            );
        }
        // The sixth waits for the window's end, at 12:10.
        assert_eq!(
            send(&store, ReportSource::App, "a", NOON + 300).await,
            Err(300)
        );
        assert_eq!(
            send(&store, ReportSource::App, "a", NOON + 599).await,
            Err(1)
        );
        assert_eq!(
            send(&store, ReportSource::App, "b", NOON + 300).await,
            Ok(())
        );
        assert_eq!(
            send(&store, ReportSource::App, "a", NOON + 600).await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn app_reports_twenty_per_moscow_day() {
        let store = Store::memory();
        for window in 0..4 {
            for report in 0..5 {
                let now = NOON + window * 600 + report;
                assert_eq!(send(&store, ReportSource::App, "a", now).await, Ok(()));
            }
        }
        // The next window would take five more, but the day is full until Moscow midnight: 12 h.
        let later = NOON + 3600;
        assert_eq!(
            send(&store, ReportSource::App, "a", later).await,
            Err(12 * 3600 - 3600)
        );
        let midnight = Day::of(NOON).end();
        assert_eq!(send(&store, ReportSource::App, "a", midnight).await, Ok(()));
    }

    #[tokio::test]
    async fn site_reports_three_per_ten_minutes() {
        let store = Store::memory();
        for second in 0..3 {
            assert_eq!(
                send(&store, ReportSource::Site, "a", NOON + 590 + second).await,
                Ok(())
            );
        }
        assert_eq!(
            send(&store, ReportSource::Site, "a", NOON + 595).await,
            Err(5)
        );
        // The app's window is its own.
        assert_eq!(
            send(&store, ReportSource::App, "a", NOON + 595).await,
            Ok(())
        );
        assert_eq!(
            send(&store, ReportSource::Site, "a", NOON + 600).await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn three_hundred_a_day_from_everyone_and_refusals_use_none_of_it() {
        let store = Store::memory();
        // A flood from one address: three go through, the rest are refused by its own limit and
        // leave the day's budget alone.
        for _ in 0..50 {
            let _ = send(&store, ReportSource::Site, "flood", NOON).await;
        }
        for client in 0..297 {
            let now = NOON + client;
            assert_eq!(
                send(&store, ReportSource::App, &client.to_string(), now).await,
                Ok(())
            );
        }
        let wait = send(&store, ReportSource::App, "new", NOON + 400).await;
        assert_eq!(wait, Err(Day::of(NOON).end() - NOON - 400));
    }

    #[tokio::test]
    async fn a_client_is_refused_unread_only_when_no_report_of_its_could_pass() {
        let store = Store::memory();
        for window in 0..4 {
            for report in 0..5 {
                let now = NOON + window * 600 + report;
                assert_eq!(send(&store, ReportSource::App, "a", now).await, Ok(()));
            }
        }
        // The app's twenty for the day leave the site's form open to the same address.
        let later = NOON + 3600;
        assert_eq!(closed_to(&store, "a", later).await, None);
        for second in 0..3 {
            assert_eq!(
                send(&store, ReportSource::Site, "a", later + second).await,
                Ok(())
            );
        }
        // Both full: refused until the sooner of the two opens, the site's window at 13:10.
        assert_eq!(closed_to(&store, "a", later + 60).await, Some(540));
        assert_eq!(closed_to(&store, "b", later + 60).await, None);
        assert_eq!(closed_to(&store, "a", later + 600).await, None);

        // The day's three hundred close it to everyone until Moscow midnight.
        let store = Store::memory();
        for client in 0..300 {
            assert_eq!(
                send(&store, ReportSource::App, &client.to_string(), NOON).await,
                Ok(())
            );
        }
        assert_eq!(
            closed_to(&store, "new", NOON).await,
            Some(Day::of(NOON).end() - NOON)
        );
    }
}
