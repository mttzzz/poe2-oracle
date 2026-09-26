//! The player's pathofexile.com account as the site's own pages show it to the session the
//! caller's client sends -- the app adds its `POESESSID` cookie to every request for the site
//! (`poe2-oracle`'s `session`). Only the account page is read, and only to tell whether that
//! session is signed in; nothing here changes anything on the account.

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use futures::AsyncReadExt as _;
use http_client::http::header::LOCATION;
use http_client::{AsyncBody, HttpClient};

/// The account page: a signed-in session gets it, anyone else a 401 (verified anonymously
/// 2026-09-23) or the login page.
pub const ACCOUNT_PAGE: &str = "https://www.pathofexile.com/my-account";

/// What the account page said about the session a request carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountCheck {
    /// The page answered: the session is signed in -- to this account, when the page names it.
    SignedIn { account: Option<String> },
    /// The page refused, or sent the request to the login page: no valid session.
    SignedOut,
}

/// Asks the account page whether the session the client sends is signed in. Redirects aren't
/// followed: a signed-out session is sent to the login page, and the session cookie never follows
/// a redirect anywhere.
pub async fn check_session(client: &Arc<dyn HttpClient>) -> Result<AccountCheck> {
    let mut response = client
        .get(ACCOUNT_PAGE, AsyncBody::default(), false)
        .await
        .with_context(|| format!("GET {ACCOUNT_PAGE}"))?;
    let status = response.status().as_u16();
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut page = String::new();
    if status == 200 {
        response
            .body_mut()
            .read_to_string(&mut page)
            .await
            .context("reading the account page")?;
    }
    read_answer(status, location.as_deref(), &page)
}

/// The account page's status, redirect target and (for a 200) page, read.
fn read_answer(status: u16, location: Option<&str>, page: &str) -> Result<AccountCheck> {
    match status {
        200 => Ok(AccountCheck::SignedIn {
            account: account_name(page),
        }),
        401 => Ok(AccountCheck::SignedOut),
        300..=399 if location.is_some_and(|to| to.contains("/login")) => {
            Ok(AccountCheck::SignedOut)
        }
        _ => bail!("the account page answered HTTP {status}"),
    }
}

/// The account's name, best effort: the text of the page's first link to a profile
/// (`/account/view-profile/<name>`, the header's own link to the signed-in account), else the
/// name that link's path carries. `None` when the page has no such link.
fn account_name(page: &str) -> Option<String> {
    const PROFILE_PATH: &str = "/account/view-profile/";
    let start = page.find(PROFILE_PATH)? + PROFILE_PATH.len();
    let rest = &page[start..];
    let path_end = rest
        .find(['"', '\'', '/', '?', '#', '>', ' '])
        .unwrap_or(rest.len());
    let from_path = percent_decode(&rest[..path_end]);
    // The link's own text, when it is plain text: `...">Name#1234</a>`.
    let from_text = rest
        .find('>')
        .map(|open| &rest[open + 1..])
        .and_then(|text| text.find("</a>").map(|close| &text[..close]))
        .map(|text| unescape_html(text.trim()))
        .filter(|text| !text.is_empty() && !text.contains('<'));
    from_text
        .or((!from_path.is_empty()).then_some(from_path))
        .filter(|name| name.chars().count() <= 64)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes
            .get(at + 1..at + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[at], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                at += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// `text` with the escapes the site's pages write decoded: `&amp;`, `&lt;`, `&gt;`, `&quot;` and
/// an apostrophe's `&#039;` (as in "Kirac&#039;s Vault Pass").
pub(crate) fn unescape_html(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_signed_in_a_refusal_or_the_login_page_is_not() {
        assert_eq!(
            read_answer(200, None, "<html></html>").unwrap(),
            AccountCheck::SignedIn { account: None }
        );
        assert_eq!(read_answer(401, None, "").unwrap(), AccountCheck::SignedOut);
        assert_eq!(
            read_answer(302, Some("https://www.pathofexile.com/login"), "").unwrap(),
            AccountCheck::SignedOut
        );
        // Neither a verdict: a redirect elsewhere, a Cloudflare challenge, an outage.
        assert!(read_answer(302, Some("https://www.pathofexile.com/maintenance"), "").is_err());
        assert!(read_answer(403, None, "").is_err());
        assert!(read_answer(503, None, "").is_err());
    }

    #[test]
    fn the_account_name_comes_from_the_profile_link() {
        let header = r#"<div class="loggedInStatus"><span class="profile-link">
            <a href="/account/view-profile/Seller-1234">Seller#1234</a></span></div>"#;
        assert_eq!(account_name(header).as_deref(), Some("Seller#1234"));
        // Without plain link text, the name in the link's path.
        let icon_only = r#"<a href="/account/view-profile/Name%C3%A9-7/characters"><img></a>"#;
        assert_eq!(account_name(icon_only).as_deref(), Some("Nameé-7"));
        assert_eq!(account_name("<html>no profile here</html>"), None);
    }
}
