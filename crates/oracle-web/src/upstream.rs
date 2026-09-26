//! What the service's own calls to GitHub and Telegram share: one HTTP client, and failures worded
//! for the log.

use std::error::Error;
use std::time::Duration;

/// GitHub rejects API calls without a User-Agent and asks for the caller's name in it.
const USER_AGENT: &str = concat!(
    "oracle-web/",
    env!("CARGO_PKG_VERSION"),
    " (+https://oracle.pushka.biz)"
);

/// The client every call goes through. Each call sets its own overall timeout; the read timeout
/// only ends a transfer that has stalled, so a long download to a slow player isn't cut short.
pub fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .build()
}

/// A call that got no answer, for the log: the error and its causes, never the URL -- Telegram's
/// carries the bot token in its path.
pub fn describe(error: reqwest::Error) -> String {
    let error = error.without_url();
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(inner) = cause {
        text.push_str(": ");
        text.push_str(&inner.to_string());
        cause = inner.source();
    }
    text
}
