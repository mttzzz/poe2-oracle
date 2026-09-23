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
}
