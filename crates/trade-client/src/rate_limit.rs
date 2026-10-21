//! Per-endpoint-family request-rate limiter for PoE2 trade API responses. Header format verified
//! live (2026-09-22) against the real search endpoint: `X-Rate-Limit-<Rules>` (e.g.
//! `x-rate-limit-ip: 5:10:60,15:60:300,30:300:1800`, comma-separated
//! `<hits>:<period_secs>:<restrict_secs>` triples -- the POLICY) paired with
//! `X-Rate-Limit-<Rules>-State` (e.g. `x-rate-limit-ip-state: 1:10:0,1:60:0,1:300:0`,
//! comma-separated `<current_hits>:<period_secs>:<restricted_secs_remaining>` triples -- the
//! CURRENT STATE, whose trailing field is nonzero only while actively restricted). A request made
//! during a restriction is refused with `429` and `retry-after: <secs>` -- and, verified live the
//! same day, no `X-Rate-Limit-*` headers at all, so that `Retry-After` is the refusal's only
//! timing signal.
//!
//! UI/runtime-agnostic per this crate's module doc comment: [`RateLimiter::required_wait`] only
//! says how long to hold off. Sleeping -- and telling the user why -- is the caller's job, on
//! whatever executor it owns.

use std::time::{Duration, Instant};

use http_client::http::{HeaderMap, header::RETRY_AFTER};

/// One `<hits>:<period_secs>:<restrict_secs>` triple, the format both headers share. In a policy
/// header: at most `hits` requests per rolling `period_secs`-second window, exceeding which
/// restricts the caller for `restrict_secs` seconds. In a `-State` header: `hits` requests made
/// in the current `period_secs` window, and `restrict_secs` of an active restriction still to
/// serve (`0` when unrestricted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RateWindow {
    hits: u32,
    period_secs: u32,
    restrict_secs: u32,
}

/// Tracks when one PoE2 trade API endpoint family may next be called. Search/fetch/exchange each
/// have their own independent policy (verified live: 5 req/10s tightest for search, 12 req/4s
/// tightest for fetch, 5 req/15s tightest for exchange), so callers own one `RateLimiter` per
/// family, never a shared instance -- but a refusal restricts the whole IP, not its family alone
/// (see [`Self::refused_until`]).
///
/// Keeps the resulting deadline rather than the raw policy/state: every constraint one response
/// carries counts down from the same moment, so together they pin a single instant -- and a later
/// response with no rate-limit information at all (a header-less 5xx) leaves that instant as it
/// is, instead of re-anchoring stale state to a newer timestamp.
#[derive(Debug, Clone, Default)]
pub struct RateLimiter {
    /// When the next request becomes safe, per the latest response that carried rate-limit
    /// information; `None` until one has.
    ready_at: Option<Instant>,
    /// When the latest refusal (`429`) this limiter learned of ends; `None` if it never saw one.
    refused_until: Option<Instant>,
}

impl RateLimiter {
    /// An empty limiter: nothing has been learned yet, so [`Self::required_wait`] is `None` until
    /// the first response reaches [`Self::record_response`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Learns from one response of this limiter's endpoint family. Call it for EVERY response,
    /// error statuses included, before judging the status: the `429` refusing a request carries
    /// exactly the `Retry-After` the next one has to honour. The wait a response implies (see
    /// [`Self::required_wait`]) replaces whatever an earlier one implied -- the server's latest
    /// word is authoritative, all-clears included -- while a response carrying no rate-limit
    /// information at all leaves the known wait untouched.
    pub fn record_response(&mut self, status: u16, headers: &HeaderMap) {
        self.record_response_at(status, headers, Instant::now());
    }

    fn record_response_at(&mut self, status: u16, headers: &HeaderMap, now: Instant) {
        if let Some(wait_secs) = response_wait_secs(status, headers) {
            // `checked_add`: the seconds are server-controlled, and an absurd value must not
            // panic -- it just fails to arm the limiter.
            self.ready_at = now.checked_add(Duration::from_secs(wait_secs));
            if status == 429 {
                self.refused_until = self.ready_at;
            }
        }
    }

    /// When the latest refusal this limiter learned of ends. A refusal restricts the player's
    /// whole IP, not this family alone -- live 2026-09-23 a refused fetch was followed nine seconds
    /// later by a refused search -- so the caller holds every family until then
    /// ([`Self::hold_until`]) instead of spending the other families' requests on more refusals.
    pub fn refused_until(&self) -> Option<Instant> {
        self.refused_until
    }

    /// Holds this family's next request until `deadline` at the earliest: another family's
    /// refusal (see [`Self::refused_until`]).
    pub fn hold_until(&mut self, deadline: Instant) {
        self.ready_at = self.ready_at.max(Some(deadline));
    }

    /// How long from now until the next request to this endpoint family is safe; `None` if it
    /// already is, or nothing has been learned yet. The latest response's wait is the largest of:
    /// an active restriction's remaining seconds (a `-State` triple's trailing field, or a `429`'s
    /// `Retry-After`), and, for every window its state shows at the policy's cap
    /// (`current_hits >= hits`), that window's full `period_secs` -- one more request would exceed
    /// the cap (and earn a far longer restriction) until the window's hits age out, and with no
    /// per-hit timestamps the whole period is the only safe bound. Time elapsed since that
    /// response is subtracted.
    pub fn required_wait(&self) -> Option<Duration> {
        self.required_wait_at(Instant::now())
    }

    fn required_wait_at(&self, now: Instant) -> Option<Duration> {
        let wait = self.ready_at?.checked_duration_since(now)?;
        (!wait.is_zero()).then_some(wait)
    }

    /// Folds in what another copy of this limiter learned, keeping the later of the two
    /// deadlines: two requests of one family can run at once, each on its own copy, and the one
    /// that finishes last must not erase a restriction the other just learned.
    pub fn merge(&mut self, other: &RateLimiter) {
        self.ready_at = self.ready_at.max(other.ready_at);
        self.refused_until = self.refused_until.max(other.refused_until);
    }
}

/// GGG's limit on how often one client may call the trade API, which no response header shows: a
/// bucket of about 22 requests, searches and fetches together, refilling about 1.8 a minute, past
/// which the site refuses the next request with `429` and `Retry-After: 600`. The market
/// collector measured it on 2026-09-29; player report #18 (2026-10-20) met it: the 33rd search or
/// fetch in 5 min 50 s was refused for 600 s while every published window stood at a third of its
/// cap or less. `Pace` keeps under it with a bucket of 20 refilling 1.8 a minute; the caller takes
/// one request from it as each one goes out.
#[derive(Debug, Clone)]
pub struct Pace {
    /// The requests in hand at `at`.
    tokens: f64,
    /// When `tokens` was counted; `None` while the bucket is full and untouched.
    at: Option<Instant>,
}

impl Default for Pace {
    fn default() -> Self {
        Self {
            tokens: Self::CAPACITY,
            at: None,
        }
    }
}

impl Pace {
    const CAPACITY: f64 = 20.0;
    const PER_SEC: f64 = 1.8 / 60.0;

    /// How long from now until `requests` more may go out; `None` if they may now.
    pub fn wait_for(&self, requests: u32) -> Option<Duration> {
        self.wait_for_at(requests, Instant::now())
    }

    fn wait_for_at(&self, requests: u32, now: Instant) -> Option<Duration> {
        let short = f64::from(requests) - self.tokens_at(now);
        (short > 0.0).then(|| Duration::from_secs_f64(short / Self::PER_SEC))
    }

    /// Counts one request going out now.
    pub fn take(&mut self) {
        self.take_at(Instant::now());
    }

    fn take_at(&mut self, now: Instant) {
        self.tokens = self.tokens_at(now) - 1.0;
        self.at = Some(now);
    }

    fn tokens_at(&self, now: Instant) -> f64 {
        match self.at {
            None => self.tokens,
            Some(at) => {
                let refilled = now.saturating_duration_since(at).as_secs_f64() * Self::PER_SEC;
                (self.tokens + refilled).min(Self::CAPACITY)
            }
        }
    }
}

/// The wait, in seconds from receipt, that one response implies (see
/// [`RateLimiter::required_wait`]), or `None` if it carries no rate-limit information at all.
/// Every rule family counts -- each restricts independently (e.g. `ip` and `account`). A
/// policy/state pair this crate fails to parse is skipped, not treated as an error: rate limiting
/// is a best-effort courtesy to the trade API, not a correctness requirement.
fn response_wait_secs(status: u16, headers: &HeaderMap) -> Option<u64> {
    let mut wait_secs = if status == 429 {
        retry_after_secs(headers)
    } else {
        None
    };
    // Header names arrive already lowercased -- `http`'s `HeaderName` normalizes on construction
    // -- so matching needs no case-folding even though HTTP header names are case-insensitive.
    for (name, value) in headers {
        let Some(family) = name.as_str().strip_suffix("-state") else {
            continue;
        };
        if !family.starts_with("x-rate-limit-") {
            continue;
        }
        let Some(policy) = headers
            .get(family)
            .and_then(|policy| policy.to_str().ok())
            .and_then(parse_windows)
        else {
            continue;
        };
        let Some(state) = value.to_str().ok().and_then(parse_windows) else {
            continue;
        };
        for window in state {
            // One short of a cap already waits. A steady run of checks drew a 429 and a
            // ten-minute lockout on 2026-09-23 while stopping only AT the cap: that leaves no room
            // for the requests the state doesn't show yet -- another tool on the same IP, or a
            // request of this app's still in flight -- and a window's overrun costs minutes.
            let at_cap = policy
                .iter()
                .any(|cap| cap.period_secs == window.period_secs && window.hits + 1 >= cap.hits);
            let window_wait = if at_cap {
                window.restrict_secs.max(window.period_secs)
            } else {
                window.restrict_secs
            };
            wait_secs = Some(wait_secs.unwrap_or(0).max(u64::from(window_wait)));
        }
    }
    wait_secs
}

/// A response's `Retry-After` header as whole seconds -- the only form the trade API was seen to
/// send (`retry-after: 259`, verified live 2026-09-22); the HTTP-date form is not parsed. Shared
/// with `TradeApiError`, which reports it for any refusal.
pub(crate) fn retry_after_secs(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
}

/// The rate-limit headers of one response, compactly, for the log: each rule family's windows as
/// `hits/cap per period` -- `ip 2/5 per 10s, 6/15 per 60s, 11/30 per 300s, 80/600 per 21600s` --
/// with any restriction still to serve, and a refusal's `Retry-After`. The trade site counts every
/// request from the player's IP, their browser's included, and only these headers show how close
/// a lockout was. `None` for a response carrying none of them.
pub fn describe_limits(headers: &HeaderMap) -> Option<String> {
    let mut parts = Vec::new();
    for (name, value) in headers {
        let Some(family) = name.as_str().strip_suffix("-state") else {
            continue;
        };
        let Some(rule) = family.strip_prefix("x-rate-limit-") else {
            continue;
        };
        let policy = headers
            .get(family)
            .and_then(|policy| policy.to_str().ok())
            .and_then(parse_windows)
            .unwrap_or_default();
        let Some(state) = value.to_str().ok().and_then(parse_windows) else {
            continue;
        };
        let windows: Vec<String> = state
            .iter()
            .map(|window| {
                let cap = policy
                    .iter()
                    .find(|cap| cap.period_secs == window.period_secs)
                    .map_or_else(|| "?".to_owned(), |cap| cap.hits.to_string());
                let restricted = match window.restrict_secs {
                    0 => String::new(),
                    secs => format!(" restricted {secs}s"),
                };
                format!(
                    "{}/{cap} per {}s{restricted}",
                    window.hits, window.period_secs
                )
            })
            .collect();
        parts.push(format!("{rule} {}", windows.join(", ")));
    }
    parts.extend(retry_after_secs(headers).map(|secs| format!("retry after {secs}s")));
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// Parses a policy or state header value's comma-separated triples (see [`RateWindow`]). `None`
/// if any triple is malformed.
fn parse_windows(value: &str) -> Option<Vec<RateWindow>> {
    value.split(',').map(parse_window).collect()
}

fn parse_window(triple: &str) -> Option<RateWindow> {
    let mut parts = triple.trim().split(':');
    let hits = parts.next()?.parse().ok()?;
    let period_secs = parts.next()?.parse().ok()?;
    let restrict_secs = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(RateWindow {
        hits,
        period_secs,
        restrict_secs,
    })
}

#[cfg(test)]
mod tests {
    use http_client::http::HeaderValue;

    use super::*;

    /// The `x-rate-limit-ip` policy captured live (2026-09-22) against the real search endpoint,
    /// paired with `state` as its `-state` header.
    fn search_headers(state: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-rate-limit-ip",
            HeaderValue::from_static("5:10:60,15:60:300,30:300:1800"),
        );
        headers.insert("x-rate-limit-ip-state", HeaderValue::from_static(state));
        headers
    }

    /// The live 429 refusal (2026-09-22): `retry-after` plus generic Cloudflare headers, and no
    /// `x-rate-limit-*` headers at all.
    fn refusal_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("259"));
        headers.insert("server", HeaderValue::from_static("cloudflare"));
        headers
    }

    fn secs(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    #[test]
    fn nothing_learned_means_no_wait() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new();
        assert_eq!(limiter.required_wait_at(t0), None);

        limiter.record_response_at(200, &HeaderMap::new(), t0);
        assert_eq!(limiter.required_wait_at(t0), None);
    }

    #[test]
    fn comfortably_under_every_cap_means_no_wait() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new();
        limiter.record_response_at(200, &search_headers("1:10:0,1:60:0,1:300:0"), t0);
        assert_eq!(limiter.required_wait_at(t0), None);
    }

    #[test]
    fn window_at_its_cap_waits_out_the_rest_of_its_period() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new();
        // Only the tightest window (5 hits / 10s) is at its cap.
        limiter.record_response_at(200, &search_headers("5:10:0,5:60:0,5:300:0"), t0);
        assert_eq!(limiter.required_wait_at(t0), Some(secs(10)));
        assert_eq!(limiter.required_wait_at(t0 + secs(4)), Some(secs(6)));
        assert_eq!(limiter.required_wait_at(t0 + secs(10)), None);
    }

    #[test]
    fn one_short_of_a_cap_already_waits() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new();
        // The long window (30 hits / 300s) is one hit short: the next request could be the one
        // that trips its half-hour restriction.
        limiter.record_response_at(200, &search_headers("1:10:0,3:60:0,29:300:0"), t0);
        assert_eq!(limiter.required_wait_at(t0), Some(secs(300)));
    }

    #[test]
    fn active_restriction_in_state_is_served_out() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new();
        // Under every cap again, but 37s of a restriction still to serve.
        limiter.record_response_at(200, &search_headers("2:10:37,2:60:0,2:300:0"), t0);
        assert_eq!(limiter.required_wait_at(t0), Some(secs(37)));
        assert_eq!(limiter.required_wait_at(t0 + secs(30)), Some(secs(7)));
    }

    #[test]
    fn refusal_retry_after_counts_down_from_receipt() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new();
        limiter.record_response_at(429, &refusal_headers(), t0);
        assert_eq!(limiter.required_wait_at(t0), Some(secs(259)));
        assert_eq!(limiter.required_wait_at(t0 + secs(200)), Some(secs(59)));
        assert_eq!(limiter.required_wait_at(t0 + secs(259)), None);
    }

    #[test]
    fn every_rule_family_counts() {
        let t0 = Instant::now();
        // The capped family comes first: whichever family is read last must not decide alone.
        let mut headers = HeaderMap::new();
        headers.insert("x-rate-limit-account", HeaderValue::from_static("3:5:60"));
        headers.insert(
            "x-rate-limit-account-state",
            HeaderValue::from_static("3:5:0"),
        );
        headers.extend(search_headers("1:10:0,1:60:0,1:300:0"));
        let mut limiter = RateLimiter::new();
        limiter.record_response_at(200, &headers, t0);
        assert_eq!(limiter.required_wait_at(t0), Some(secs(5)));
    }

    #[test]
    fn latest_rate_limit_information_wins() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new();
        limiter.record_response_at(429, &refusal_headers(), t0);

        // A header-less failure says nothing about rate limiting: the restriction stands.
        limiter.record_response_at(502, &HeaderMap::new(), t0 + secs(9));
        assert_eq!(limiter.required_wait_at(t0 + secs(9)), Some(secs(250)));

        // A fresh all-clear is the server's latest word, even ahead of the old deadline.
        let all_clear = search_headers("1:10:0,1:60:0,1:300:0");
        limiter.record_response_at(200, &all_clear, t0 + secs(10));
        assert_eq!(limiter.required_wait_at(t0 + secs(10)), None);
    }

    #[test]
    fn a_refusal_is_the_deadline_every_family_holds_to() {
        let t0 = Instant::now();
        let mut fetch = RateLimiter::new();
        fetch.record_response_at(429, &refusal_headers(), t0);
        let mut search = RateLimiter::new();
        search.record_response_at(200, &search_headers("1:10:0,1:60:0,1:300:0"), t0);
        assert_eq!(search.refused_until(), None, "an all-clear is no refusal");

        search.hold_until(fetch.refused_until().expect("the 429 is remembered"));
        assert_eq!(search.required_wait_at(t0 + secs(9)), Some(secs(250)));
        // A copy merged back keeps the refusal, so it reaches the other families from there too.
        let mut merged = RateLimiter::new();
        merged.merge(&fetch);
        assert_eq!(merged.refused_until(), fetch.refused_until());
    }

    #[test]
    fn limits_read_as_hits_of_cap_per_window() {
        let mut headers = search_headers("2:10:0,6:60:0,30:300:1800");
        headers.insert("x-rate-limit-rules", HeaderValue::from_static("Ip"));
        assert_eq!(
            describe_limits(&headers).as_deref(),
            Some("ip 2/5 per 10s, 6/15 per 60s, 30/30 per 300s restricted 1800s")
        );
        assert_eq!(
            describe_limits(&refusal_headers()).as_deref(),
            Some("retry after 259s")
        );
        assert_eq!(describe_limits(&HeaderMap::new()), None);
    }

    #[test]
    fn malformed_triples_fail_to_parse() {
        assert_eq!(parse_windows("5:10:60,not-a-triple"), None);
        assert_eq!(parse_windows("1:10:0,1:60"), None);
    }

    #[test]
    fn the_pace_lets_twenty_requests_through_then_one_every_33_seconds() {
        let start = Instant::now();
        let mut pace = Pace::default();
        assert_eq!(pace.wait_for_at(20, start), None);
        for _ in 0..19 {
            pace.take_at(start);
        }
        // One left: a check's search and fetch need two, a lone fetch one.
        assert_eq!(pace.wait_for_at(1, start), None);
        let two = pace.wait_for_at(2, start).expect("a check waits");
        assert_eq!(two.as_secs(), 33);
        pace.take_at(start);
        // Empty, it refills 1.8 a minute: one request per 33⅓ s.
        assert!(pace.wait_for_at(1, start + secs(33)).is_some());
        assert_eq!(pace.wait_for_at(1, start + secs(34)), None);
        // An hour idle fills it again, and no more than full.
        assert_eq!(pace.wait_for_at(20, start + secs(3600)), None);
        assert!(pace.wait_for_at(21, start + secs(3600)).is_some());
    }

    #[test]
    fn the_pace_waits_before_the_request_the_site_refused_in_report_18() {
        // Player report #18 (2026-10-20): 33 searches and fetches from 18:56:38, in ms from the
        // first. The site refused the last one, a search, for 600 s, while every published
        // window stood at a third of its cap or less.
        const RUN: [u64; 33] = [
            0, 314, 2632, 2948, 27164, 27481, 34805, 35116, 48468, 48775, 55491, 55805, 94058,
            94384, 96829, 97143, 100208, 100533, 270002, 270331, 274295, 274608, 289760, 290092,
            293678, 293998, 318902, 319219, 326742, 327079, 342607, 342935, 350160,
        ];
        let start = Instant::now();
        let mut pace = Pace::default();
        let first_wait = RUN.iter().enumerate().position(|(i, &ms)| {
            let at = start + Duration::from_millis(ms);
            // Searches and fetches alternate, a search first; a search waits for its fetch too.
            let wait = pace.wait_for_at(if i % 2 == 0 { 2 } else { 1 }, at);
            pace.take_at(at);
            wait.is_some()
        });
        assert!(
            first_wait.is_some_and(|at| at < RUN.len() - 1),
            "the pace let the refused request out: first wait at {first_wait:?}"
        );
    }
}
