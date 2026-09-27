//! The trade site's own buttons on a listing, pressed for a signed-in player: "Travel to Hideout"
//! on an instant-buyout listing and "Direct Whisper" on one sold in person. Either is one
//! `POST /api/trade2/whisper` with the listing's token -- `hideout_token` or `whisper_token`, which
//! only a fetch made with the account's session carries ([`FetchedItem`]) -- and the session's
//! cookie, which the caller's client adds. The site acts through the player's game session: it
//! takes the character to the seller's hideout, or sends the seller the listing's whisper from the
//! character. Nothing is typed into the game.
//!
//! The request is the site's own, as two tools that press these buttons send it: PoE Overlay II
//! 1.67.0 (`main.js`, its `whisper` and `secure` calls: `{"token": ...}` with
//! `X-Requested-With: XMLHttpRequest`, `Origin` and the session cookie; success is the answer's
//! `success`) and Exiled Exchange 2's travel button (commit `5dfba96c`, March 2026, removed in
//! May: `{"token": ..., "continue": true}` on a second try, `Accept: application/json`, the
//! endpoint's own rate-limit policy `trade-whisper-request-limit`). Without a session both sites
//! answer `401` with `{"error":{"code":8,"message":"Unauthorized"}}` (checked 2026-09-27).

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http_client::http::header::{ACCEPT, CONTENT_TYPE, ORIGIN};
use http_client::http::{Method, Request};
use http_client::{AsyncBody, HttpClient};
use serde::{Deserialize, Serialize};

use crate::rate_limit::RateLimiter;
use crate::{AccountStatus, FetchedItem, TradeApiError, TradeSite, capped, checked_body};

/// One of the trade site's buttons on a listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// "Travel to Hideout", on an instant-buyout listing: the site takes the player's character to
    /// the seller's hideout, where the item is bought from the seller's merchant. The seller needn't
    /// be online.
    Travel,
    /// "Direct Whisper", on a listing sold in person: the site sends the seller the listing's
    /// whisper from the player's character.
    Whisper,
}

impl Action {
    /// The button the trade site offers on `listing`, with the token it takes: travel for an
    /// instant buyout, a whisper for a listing sold in person while its seller is online or away --
    /// the site offers none to an offline seller. `None` without that token: a fetch made without
    /// a session carries none.
    pub fn offered(listing: &FetchedItem) -> Option<(Action, &str)> {
        let (action, token) = if listing.instant_buyout {
            (Action::Travel, &listing.hideout_token)
        } else if listing.account_status != AccountStatus::Offline {
            (Action::Whisper, &listing.whisper_token)
        } else {
            return None;
        };
        token.as_deref().map(|token| (action, token))
    }

    /// The request, as the log's rate-limit lines name it.
    fn what(self) -> &'static str {
        match self {
            Action::Travel => "travel to hideout",
            Action::Whisper => "whisper",
        }
    }
}

/// How long after the fetch the site takes `token`: its `exp` less its `iat`. The tokens are JSON
/// Web Tokens whose claims say when they were issued and when they lapse -- a live `hideout_token`
/// (June 2026) holds for 300 s. `None` for a token that doesn't say.
pub fn token_lifetime(token: &str) -> Option<Duration> {
    #[derive(Deserialize)]
    struct Claims {
        iat: u64,
        exp: u64,
    }
    let payload = token.split('.').nth(1)?;
    let claims: Claims =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?)
            .ok()?;
    let secs = claims.exp.checked_sub(claims.iat)?;
    (secs > 0).then(|| Duration::from_secs(secs))
}

/// What the site made of a press it took (a 2xx).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// `"success": true`.
    Done,
    /// Anything else: the site took the request but didn't act. Its body, trimmed and capped as a
    /// refusal's message is, for the log.
    NotDone(String),
}

/// What a press came to, for the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The site did it: the character is on its way to the seller's hideout, or the whisper went
    /// out.
    Done,
    /// The site didn't make a travel at its first press: the listing is in demand. The site's own
    /// page asks "Teleport anyway?" then, and a second press says yes (`continue`).
    InDemand,
    /// The site won't do it for this listing any more -- the seller left, the item sold, the token
    /// lapsed: a new search shows what's there now. With the site's own words when it gave any.
    Gone(Option<String>),
    /// The site doesn't take the session: it expired, or the player signed out on the site.
    SignedOut,
    /// The trade site's rate limit, with the seconds it said to wait.
    RateLimited(Option<u64>),
    /// No answer to go by: no connection, or the site failing (a 5xx, a Cloudflare page).
    /// Pressing again may work.
    NoAnswer,
}

impl Outcome {
    /// What `result` -- the answer to `action` pressed with `anyway` -- comes to. A refusal is read
    /// by GGG's error envelope where there is one: `401`, or code 8 (`Unauthorized`), means the
    /// session; a `403` in the envelope (code 6, `Forbidden`) too, while a `403` without one is
    /// Cloudflare's page, not the site's word on the session.
    pub fn of(action: Action, anyway: bool, result: &Result<Answer>) -> Outcome {
        let err = match result {
            Ok(Answer::Done) => return Outcome::Done,
            Ok(Answer::NotDone(_)) if action == Action::Travel && !anyway => {
                return Outcome::InDemand;
            }
            Ok(Answer::NotDone(_)) => return Outcome::Gone(None),
            Err(err) => err,
        };
        let Some(refusal) = err.downcast_ref::<TradeApiError>() else {
            return Outcome::NoAnswer;
        };
        if refusal.is_rate_limited() {
            Outcome::RateLimited(refusal.retry_after_secs)
        } else if refusal.status == 401
            || refusal.code == Some(8)
            || (refusal.status == 403 && refusal.code.is_some())
        {
            Outcome::SignedOut
        } else if refusal.status == 403 || refusal.status >= 500 {
            Outcome::NoAnswer
        } else {
            Outcome::Gone(Some(refusal.message.clone()))
        }
    }
}

/// The request body. `continue` goes only with a second press of a travel the site answered in
/// demand: the site's own "Teleport anyway?".
#[derive(Serialize)]
struct Body<'a> {
    token: &'a str,
    #[serde(rename = "continue", skip_serializing_if = "is_false")]
    anyway: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Deserialize)]
struct AnswerBody {
    #[serde(default)]
    success: bool,
}

fn url(site: TradeSite) -> String {
    format!("{}/whisper", site.api_base())
}

/// `POST {site}/api/trade2/whisper` for `token`, with the headers the site's own page sends.
fn request(site: TradeSite, token: &str, anyway: bool) -> Result<Request<AsyncBody>> {
    let body = serde_json::to_string(&Body { token, anyway })?;
    Ok(Request::builder()
        .method(Method::POST)
        .uri(url(site))
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .header("X-Requested-With", "XMLHttpRequest")
        .header(ORIGIN, site.origin())
        .body(AsyncBody::from(body))?)
}

/// A 2xx body's answer.
fn read_answer(body: &str) -> Answer {
    match serde_json::from_str(body) {
        Ok(AnswerBody { success: true }) => Answer::Done,
        _ => Answer::NotDone(capped(body)),
    }
}

/// Presses `action`'s button with `token` on `site` -- `anyway`: the second press of a travel the
/// site answered in demand -- through `client`, whose session the site acts for. The response is
/// recorded on `limiter`, the whisper endpoint's own family, apart from searches and fetches; a
/// refusal comes back as a [`TradeApiError`]. Nothing is retried here: each request is one press
/// of the player's.
pub async fn send(
    client: &Arc<dyn HttpClient>,
    site: TradeSite,
    action: Action,
    token: &str,
    anyway: bool,
    limiter: &mut RateLimiter,
) -> Result<Answer> {
    let url = url(site);
    let response = client.send(request(site, token, anyway)?);
    let body = checked_body(response, "POST", &url, Some(limiter), action.what()).await?;
    Ok(read_answer(&body))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver, Sender};

    use futures::future::BoxFuture;
    use http_client::http::HeaderValue;
    use http_client::{Inner, Response, Url};

    use super::*;
    use crate::parse_fetch_response;

    /// A live-shaped fetch page as a signed-in account gets it (made from a live instant-buyout
    /// page of June 2026 and the site's whisper fields; accounts, ids and tokens made up, the
    /// tokens with the live claims): an offline seller's instant buyout with its `hideout_token`,
    /// and three listings sold in person, each with its whisper and `whisper_token`, whose sellers
    /// are online, away and offline.
    const SIGNED_IN_PAGE: &str = include_str!("../tests/fixtures/fetch-signed-in.json");

    #[test]
    fn a_signed_in_page_offers_travel_on_the_buyout_and_whispers_to_sellers_about() {
        let listings = parse_fetch_response(SIGNED_IN_PAGE).expect("the page parses");
        let offered: Vec<_> = listings
            .iter()
            .map(|listing| Action::offered(listing).map(|(action, _)| action))
            .collect();
        assert_eq!(
            offered,
            [
                Some(Action::Travel),
                Some(Action::Whisper),
                Some(Action::Whisper),
                // The site offers no whisper to an offline seller, token or not.
                None,
            ]
        );
        let (buyout, online) = (&listings[0], &listings[1]);
        assert_eq!(buyout.whisper_token, None);
        assert_eq!(online.hideout_token, None);
        assert_eq!(
            Action::offered(buyout).map(|(_, token)| token),
            buyout.hideout_token.as_deref()
        );
        // Every token holds for five minutes from the fetch, as the live one does.
        for token in listings
            .iter()
            .flat_map(|listing| [&listing.hideout_token, &listing.whisper_token])
            .flatten()
        {
            assert_eq!(token_lifetime(token), Some(Duration::from_secs(300)));
        }
    }

    #[test]
    fn an_anonymous_page_offers_nothing() {
        // As a fetch without a session answers: no tokens, whatever the listing.
        let page = r#"{"result": [
            {"id": "a", "item": {"name": "", "typeLine": "Divine Orb"},
             "listing": {"indexed": "2026-09-27T06:00:00Z", "fee": 12,
                         "account": {"name": "a#1", "online": null},
                         "price": {"type": "~b/o", "amount": 1, "currency": "exalted"}}},
            {"id": "b", "item": {"name": "", "typeLine": "Divine Orb"},
             "listing": {"indexed": "2026-09-27T06:00:00Z", "whisper": "@B Hi",
                         "account": {"name": "b#2", "online": {"league": "Standard"}},
                         "price": {"type": "~price", "amount": 1, "currency": "exalted"}}},
            {"id": "c", "item": {"name": "", "typeLine": "Divine Orb"},
             "listing": {"indexed": "2026-09-27T06:00:00Z", "whisper": "@C Hi",
                         "whisper_token": "", "hideout_token": "",
                         "account": {"name": "c#3", "online": {"league": "Standard"}},
                         "price": {"type": "~price", "amount": 1, "currency": "exalted"}}}
        ]}"#;
        let listings = parse_fetch_response(page).expect("the page parses");
        assert!(
            listings
                .iter()
                .all(|listing| Action::offered(listing).is_none())
        );
    }

    #[test]
    fn a_token_that_isnt_a_timed_web_token_has_no_lifetime() {
        assert_eq!(token_lifetime("opaque"), None);
        assert_eq!(token_lifetime("a.bm90IGpzb24.c"), None);
        // `{"iat":10}`: issued, but no lapse.
        assert_eq!(token_lifetime("a.eyJpYXQiOjEwfQ.c"), None);
        // `{"iat":10,"exp":10}`: lapsed as it was issued.
        assert_eq!(token_lifetime("a.eyJpYXQiOjEwLCJleHAiOjEwfQ.c"), None);
    }

    /// The trade site as `send` meets it: every request answered with one canned response -- or,
    /// with no status, never answered, as when the connection fails -- and, when it's `recording`,
    /// kept.
    struct Site {
        status: Option<u16>,
        headers: &'static [(&'static str, &'static str)],
        body: &'static str,
        sent: Option<Sender<Sent>>,
    }

    /// A request as the site got it.
    #[derive(Debug, PartialEq)]
    struct Sent {
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl Site {
        fn answering(
            status: u16,
            headers: &'static [(&'static str, &'static str)],
            body: &'static str,
        ) -> Arc<Site> {
            Arc::new(Site {
                status: Some(status),
                headers,
                body,
                sent: None,
            })
        }

        /// A site answering every request `{"success":true}`, and the requests it got.
        fn recording() -> (Arc<Site>, Receiver<Sent>) {
            let (sent, received) = mpsc::channel();
            let site = Arc::new(Site {
                status: Some(200),
                headers: &[],
                body: r#"{"success":true}"#,
                sent: Some(sent),
            });
            (site, received)
        }

        fn unreachable() -> Arc<Site> {
            Arc::new(Site {
                status: None,
                headers: &[],
                body: "",
                sent: None,
            })
        }
    }

    impl HttpClient for Site {
        fn user_agent(&self) -> Option<&HeaderValue> {
            None
        }

        fn proxy(&self) -> Option<&Url> {
            None
        }

        fn send(
            &self,
            request: Request<AsyncBody>,
        ) -> BoxFuture<'static, Result<Response<AsyncBody>>> {
            let (parts, body) = request.into_parts();
            let Inner::Bytes(body) = body.0 else {
                panic!("a request body in memory");
            };
            let text = String::from_utf8(body.into_inner().to_vec()).expect("a text body");
            let mut headers: Vec<_> = parts
                .headers
                .iter()
                .map(|(name, value)| {
                    let value = value.to_str().expect("a text header").to_owned();
                    (name.as_str().to_owned(), value)
                })
                .collect();
            headers.sort();
            if let Some(sent) = &self.sent {
                sent.send(Sent {
                    method: parts.method.to_string(),
                    url: parts.uri.to_string(),
                    headers,
                    body: text,
                })
                .expect("the test holds the receiver");
            }
            let Some(status) = self.status else {
                let refused = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
                return Box::pin(async move { Err(anyhow::Error::new(refused)) });
            };
            let mut response = Response::builder().status(status);
            for &(name, value) in self.headers {
                response = response.header(name, value);
            }
            let response = response
                .body(AsyncBody::from(self.body))
                .map_err(anyhow::Error::from);
            Box::pin(async move { response })
        }
    }

    /// `action` pressed with `anyway` on `site`'s `trade` site: the site's answer, and what it
    /// comes to.
    fn press(
        site: &Arc<Site>,
        trade: TradeSite,
        action: Action,
        anyway: bool,
        limiter: &mut RateLimiter,
    ) -> Outcome {
        let client: Arc<dyn HttpClient> = site.clone();
        let result =
            futures::executor::block_on(send(&client, trade, action, "tok.en", anyway, limiter));
        Outcome::of(action, anyway, &result)
    }

    #[test]
    fn a_press_posts_the_token_the_way_the_sites_own_page_does() {
        let (site, received) = Site::recording();
        let mut limiter = RateLimiter::new();
        press(
            &site,
            TradeSite::International,
            Action::Whisper,
            false,
            &mut limiter,
        );
        press(
            &site,
            TradeSite::Russian,
            Action::Travel,
            true,
            &mut limiter,
        );
        let headers = |origin: &str| {
            [
                ("accept", "application/json"),
                ("content-type", "application/json"),
                ("origin", origin),
                ("x-requested-with", "XMLHttpRequest"),
            ]
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .to_vec()
        };
        assert_eq!(
            received.try_iter().collect::<Vec<_>>(),
            [
                Sent {
                    method: "POST".to_owned(),
                    url: "https://www.pathofexile.com/api/trade2/whisper".to_owned(),
                    headers: headers("https://www.pathofexile.com"),
                    body: r#"{"token":"tok.en"}"#.to_owned(),
                },
                // A travel pressed again after "in demand" says to go anyway.
                Sent {
                    method: "POST".to_owned(),
                    url: "https://ru.pathofexile.com/api/trade2/whisper".to_owned(),
                    headers: headers("https://ru.pathofexile.com"),
                    body: r#"{"token":"tok.en","continue":true}"#.to_owned(),
                },
            ]
        );
    }

    #[test]
    fn the_sites_answers_come_to_what_the_player_is_told() {
        let mut limiter = RateLimiter::new();
        let mut outcome = |site: Arc<Site>, action, anyway| {
            press(
                &site,
                TradeSite::International,
                action,
                anyway,
                &mut limiter,
            )
        };
        let done = || Site::answering(200, &[], r#"{"success":true}"#);
        let not_done = || Site::answering(200, &[], r#"{"success":false}"#);
        assert_eq!(outcome(done(), Action::Travel, false), Outcome::Done);
        assert_eq!(outcome(done(), Action::Whisper, false), Outcome::Done);
        // A travel the site didn't make is in demand at the first press; after "anyway", and for
        // a whisper, the listing is past acting on.
        assert_eq!(
            outcome(not_done(), Action::Travel, false),
            Outcome::InDemand
        );
        assert_eq!(
            outcome(not_done(), Action::Travel, true),
            Outcome::Gone(None)
        );
        assert_eq!(
            outcome(not_done(), Action::Whisper, false),
            Outcome::Gone(None)
        );
        // A 2xx that isn't the answer's shape didn't act either.
        assert_eq!(
            outcome(Site::answering(200, &[], "<html>"), Action::Whisper, false),
            Outcome::Gone(None)
        );
        // The answer the site gives without a session (live 2026-09-27), and a forbidding one.
        let unauthorized = r#"{"error":{"code":8,"message":"Unauthorized"}}"#;
        assert_eq!(
            outcome(
                Site::answering(401, &[], unauthorized),
                Action::Travel,
                false
            ),
            Outcome::SignedOut
        );
        let forbidden = r#"{"error":{"code":6,"message":"Forbidden"}}"#;
        assert_eq!(
            outcome(Site::answering(403, &[], forbidden), Action::Whisper, false),
            Outcome::SignedOut
        );
        // Cloudflare's page isn't the site's word on the session.
        let cloudflare = "<!DOCTYPE html><title>Just a moment...</title>";
        assert_eq!(
            outcome(Site::answering(403, &[], cloudflare), Action::Travel, false),
            Outcome::NoAnswer
        );
        let not_found = r#"{"error":{"code":1,"message":"Resource not found"}}"#;
        assert_eq!(
            outcome(Site::answering(404, &[], not_found), Action::Whisper, false),
            Outcome::Gone(Some("Resource not found".to_owned()))
        );
        assert_eq!(
            outcome(
                Site::answering(502, &[], "Bad Gateway"),
                Action::Travel,
                false
            ),
            Outcome::NoAnswer
        );
        assert_eq!(
            outcome(Site::unreachable(), Action::Whisper, false),
            Outcome::NoAnswer
        );
    }

    #[test]
    fn a_refusal_for_the_rate_limit_says_how_long_and_holds_the_whisper_family() {
        let site = Site::answering(
            429,
            &[("retry-after", "60")],
            r#"{"error":{"code":3,"message":"Rate limit exceeded"}}"#,
        );
        let mut limiter = RateLimiter::new();
        let outcome = press(
            &site,
            TradeSite::International,
            Action::Travel,
            false,
            &mut limiter,
        );
        assert_eq!(outcome, Outcome::RateLimited(Some(60)));
        let wait = limiter
            .required_wait()
            .expect("the refusal's Retry-After arms the limiter");
        assert!((Duration::from_secs(55)..=Duration::from_secs(60)).contains(&wait));
    }
}
