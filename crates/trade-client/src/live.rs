//! Live search: the trade site's websocket that reports each new listing of a saved search the
//! moment it is listed -- the site's own "Activate Live Search". GGG documents none of it; the
//! protocol here is the one open-source live-search clients speak (5k-mirrors/
//! poe-live-search-manager, `app/main/web-sockets/actions.js` and `app/main/api/api.js`), on
//! trade2's path:
//! - the socket is `wss://{www|ru}.pathofexile.com/api/trade2/live/poe2/{league}/{search id}`
//!   ([`live_url`]); its handshake carries the player's session cookie (`Cookie: POESESSID=...`),
//!   `Origin` set to the same site and a browser `User-Agent` -- it is a signed-in feature;
//! - the server sends text messages `{"new": ["<listing id>", ...]}` ([`parse_message`]); other
//!   keys are tolerated, and the client sends nothing back;
//! - it pings (WebSocket ping frames) at least every 30 s; a client that hears nothing for longer
//!   has lost the connection;
//! - a refused handshake's HTTP status says why ([`LiveEnd::from_status`]): 401 no valid session,
//!   404 no such search, 429 too many requests. About 20 sockets per account are allowed at once
//!   ([`MAX_LIVE_SEARCHES`]).
//!
//! New listings are fetched by id like a search's own (`crate::fetch`, 10 at a time, the search id
//! as `query`). The socket itself belongs to the caller: this crate has no executor or threads.

use anyhow::{Context as _, Result};
use serde::Deserialize;

use crate::{TradeSite, encode_league};

/// The most live searches one account may have open at once.
pub const MAX_LIVE_SEARCHES: usize = 20;

/// The live search socket of the search `search_id` (the id a search returns) in `league`, on
/// `site`: the site the search was made on, whose listings and whispers are in its language.
pub fn live_url(site: TradeSite, league: &str, search_id: &str) -> String {
    format!(
        "wss://{}/api/trade2/live/poe2/{}/{search_id}",
        site.host(),
        encode_league(league)
    )
}

/// What one text message of the socket says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveMessage {
    /// Listings the search just got, by id.
    New(Vec<String>),
    /// Anything else -- an `{"auth": true}`-style greeting, an empty list: nothing to do.
    Other,
}

#[derive(Deserialize)]
struct RawMessage {
    #[serde(default)]
    new: Option<Vec<String>>,
}

/// Reads one text message; keys other than `new` are ignored.
pub fn parse_message(text: &str) -> Result<LiveMessage> {
    let raw: RawMessage = serde_json::from_str(text).context("parsing a live search message")?;
    Ok(match raw.new {
        Some(ids) if !ids.is_empty() => LiveMessage::New(ids),
        _ => LiveMessage::Other,
    })
}

/// Why the site refused or closed a live search socket, which decides what its client does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveEnd {
    /// 401: no session, or one the site no longer accepts. Final: signing in again is the
    /// player's move.
    Unauthorized,
    /// 404: the search is gone -- expired, or never existed. Final.
    SearchGone,
    /// 429: too many requests or sockets. Worth another try, later.
    RateLimited,
    /// Anything else -- a dropped connection, a restarting server, a 5xx: worth another try.
    Other,
}

impl LiveEnd {
    /// The end a refused handshake's HTTP status means.
    pub fn from_status(status: u16) -> LiveEnd {
        match status {
            401 => LiveEnd::Unauthorized,
            404 => LiveEnd::SearchGone,
            429 => LiveEnd::RateLimited,
            _ => LiveEnd::Other,
        }
    }

    /// The end a close frame's code means: the same three statuses, as the code itself or in the
    /// private-use range as 4000 + status, the usual way servers carry an HTTP status in one.
    /// Unverified against GGG's server -- the clients this module follows only ever saw refused
    /// handshakes -- so any other code is [`LiveEnd::Other`].
    pub fn from_close_code(code: u16) -> LiveEnd {
        let status = if (4000..5000).contains(&code) {
            code - 4000
        } else {
            code
        };
        match status {
            401 | 404 | 429 => LiveEnd::from_status(status),
            _ => LiveEnd::Other,
        }
    }

    /// Whether the socket must not be opened again: nothing a retry could change.
    pub fn is_final(self) -> bool {
        matches!(self, LiveEnd::Unauthorized | LiveEnd::SearchGone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_url_encodes_the_league_as_one_path_segment() {
        assert_eq!(
            live_url(TradeSite::International, "Forbidden Rites", "Ab12Cd34"),
            "wss://www.pathofexile.com/api/trade2/live/poe2/Forbidden%20Rites/Ab12Cd34"
        );
        // A private league keeps its parentheses, as the site's own links do.
        assert_eq!(
            live_url(TradeSite::International, "My League (PL12345)", "Ab12Cd34"),
            "wss://www.pathofexile.com/api/trade2/live/poe2/My%20League%20(PL12345)/Ab12Cd34"
        );
        // Anything that would end the segment early is encoded.
        assert_eq!(
            live_url(TradeSite::International, "a/b?c#d%e", "x"),
            "wss://www.pathofexile.com/api/trade2/live/poe2/a%2Fb%3Fc%23d%25e/x"
        );
    }

    #[test]
    fn live_url_goes_to_the_site_the_search_was_made_on() {
        assert_eq!(
            live_url(TradeSite::Russian, "HC Forbidden Rites", "Ab12Cd34"),
            "wss://ru.pathofexile.com/api/trade2/live/poe2/HC%20Forbidden%20Rites/Ab12Cd34"
        );
        // A league name typed in Russian is sent as its UTF-8 bytes.
        assert_eq!(
            live_url(TradeSite::Russian, "Лига (PL7)", "Q"),
            "wss://ru.pathofexile.com/api/trade2/live/poe2/%D0%9B%D0%B8%D0%B3%D0%B0%20(PL7)/Q"
        );
    }

    #[test]
    fn new_listings_are_read_past_unknown_keys() {
        assert_eq!(
            parse_message(r#"{"auth": true, "new": ["a1b2", "c3d4"], "extra": {"x": [1, 2]}}"#)
                .unwrap(),
            LiveMessage::New(vec!["a1b2".to_owned(), "c3d4".to_owned()])
        );
        assert_eq!(
            parse_message(r#"{"auth": true}"#).unwrap(),
            LiveMessage::Other
        );
        assert_eq!(parse_message(r#"{"new": []}"#).unwrap(), LiveMessage::Other);
        assert!(parse_message("not json").is_err());
        assert!(parse_message(r#"{"new": "a1b2"}"#).is_err());
    }

    #[test]
    fn refusals_map_to_what_the_client_does_next() {
        assert_eq!(LiveEnd::from_status(401), LiveEnd::Unauthorized);
        assert_eq!(LiveEnd::from_status(404), LiveEnd::SearchGone);
        assert_eq!(LiveEnd::from_status(429), LiveEnd::RateLimited);
        assert_eq!(LiveEnd::from_status(502), LiveEnd::Other);
        assert!(LiveEnd::Unauthorized.is_final());
        assert!(LiveEnd::SearchGone.is_final());
        assert!(!LiveEnd::RateLimited.is_final());
        assert!(!LiveEnd::Other.is_final());
    }

    #[test]
    fn close_codes_carry_the_same_statuses() {
        assert_eq!(LiveEnd::from_close_code(4401), LiveEnd::Unauthorized);
        assert_eq!(LiveEnd::from_close_code(4404), LiveEnd::SearchGone);
        assert_eq!(LiveEnd::from_close_code(4429), LiveEnd::RateLimited);
        assert_eq!(LiveEnd::from_close_code(401), LiveEnd::Unauthorized);
        // A normal close or an abnormal drop is just a lost connection.
        assert_eq!(LiveEnd::from_close_code(1000), LiveEnd::Other);
        assert_eq!(LiveEnd::from_close_code(1006), LiveEnd::Other);
        assert_eq!(LiveEnd::from_close_code(4000), LiveEnd::Other);
    }
}
