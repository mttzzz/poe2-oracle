//! The player's pathofexile.com web session -- the site's `POESESSID` cookie, taken from the
//! sign-in window (`crate::login`) -- and what carries it. Signed in, searches reach the player's
//! private leagues and live search can watch a search (`crate::live_search`).
//!
//! The session is a key to the player's web account, so it goes nowhere it isn't needed:
//! - it is kept in the Windows Credential Manager (`platform::credentials`), never in the settings
//!   file, the log or the diagnostics report, and a `Debug` print of anything holding it masks it;
//!   the sign-in window's browser keeps no copy (`platform::login_window`);
//! - it is sent only as `Cookie: POESESSID=<value>`, and only to `https://www.pathofexile.com` and
//!   `https://ru.pathofexile.com` -- by [`SessionHttpClient`] for the app's HTTP requests and by
//!   live search's socket handshake; never to poe2scout, GGG's CDN (item images, exchange prices)
//!   or GitHub.
//!   A redirect to another host drops it (reqwest strips `Cookie` on a cross-host redirect), and
//!   the session check follows no redirect at all.
//!
//! [`TradeSession`] holds the value, shared by the HTTP client and the live search threads;
//! [`SessionStatus`] is what the app knows of it -- checked against the account page at startup,
//! and before a sign-in keeps it ([`check_candidate`]) -- for the settings window and the panel.
//! Both are GPUI globals.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{App, Global};
use http_client::http::HeaderValue;
use http_client::http::header::COOKIE;
use http_client::{AsyncBody, HttpClient, Request, Response, Uri, Url};
use parking_lot::RwLock;
use trade_client::account::{self, AccountCheck};

/// The Credential Manager entry the session is saved under.
pub const CREDENTIAL_TARGET: &str = "PoE2 Oracle/pathofexile.com";
/// The entry's user name: what the secret is.
#[cfg(target_os = "windows")]
const CREDENTIAL_USER: &str = "POESESSID";

/// The hosts the session belongs to: the trade sites, the only ones it is sent to.
const SESSION_HOSTS: [&str; 2] = ["www.pathofexile.com", "ru.pathofexile.com"];

/// The session itself, or none. Cheap to clone: every clone shares the one value, so the HTTP
/// client and the live search threads see a sign-in or sign-out at once.
#[derive(Clone, Default)]
pub struct TradeSession(Arc<RwLock<Option<String>>>);

impl Global for TradeSession {}

impl TradeSession {
    pub fn new(session: Option<String>) -> TradeSession {
        TradeSession(Arc::new(RwLock::new(session)))
    }

    pub fn set(&self, session: String) {
        *self.0.write() = Some(session);
    }

    pub fn clear(&self) {
        *self.0.write() = None;
    }

    pub fn is_signed_in(&self) -> bool {
        self.0.read().is_some()
    }

    /// `POESESSID=<session>` for a `Cookie` header to the trade site, marked sensitive so that no
    /// `Debug` print of a request shows it.
    pub fn cookie(&self) -> Option<HeaderValue> {
        let session = self.0.read();
        let mut cookie =
            HeaderValue::from_str(&format!("POESESSID={}", session.as_deref()?)).ok()?;
        cookie.set_sensitive(true);
        Some(cookie)
    }
}

impl fmt::Debug for TradeSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown = if self.is_signed_in() {
            "<hidden>"
        } else {
            "<none>"
        };
        f.debug_tuple("TradeSession").field(&shown).finish()
    }
}

/// Whether a request to `uri` may carry the session: https to one of the trade sites' hosts, on
/// the default port.
fn is_session_host(uri: &Uri) -> bool {
    uri.scheme_str() == Some("https")
        && uri.port_u16().is_none_or(|port| port == 443)
        && uri.host().is_some_and(|host| {
            SESSION_HOSTS
                .iter()
                .any(|session_host| host.eq_ignore_ascii_case(session_host))
        })
}

/// The app's HTTP client: the one it builds, plus the session's cookie on every request to a
/// trade site host (`is_session_host`) while the player is signed in. Everything else passes
/// through untouched. The app's own requests carry no other cookie, so the header is set, not
/// merged.
pub struct SessionHttpClient {
    inner: Arc<dyn HttpClient>,
    session: TradeSession,
}

impl SessionHttpClient {
    pub fn new(inner: Arc<dyn HttpClient>, session: TradeSession) -> SessionHttpClient {
        SessionHttpClient { inner, session }
    }
}

impl HttpClient for SessionHttpClient {
    fn user_agent(&self) -> Option<&HeaderValue> {
        self.inner.user_agent()
    }

    fn proxy(&self) -> Option<&Url> {
        self.inner.proxy()
    }

    fn send(
        &self,
        mut request: Request<AsyncBody>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<AsyncBody>>> + Send + 'static>> {
        if is_session_host(request.uri())
            && let Some(cookie) = self.session.cookie()
        {
            request.headers_mut().insert(COOKIE, cookie);
        }
        self.inner.send(request)
    }
}

/// `value` if it can be the session id, `None` if not: a POESESSID is 32 hex digits, and letting
/// through only ASCII letters and digits means nothing in it can change what the `Cookie` header
/// says.
pub fn parse(value: &str) -> Option<String> {
    ((16..=128).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .then(|| value.to_owned())
}

/// What the app knows about the session, as the settings window shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SessionStatus {
    #[default]
    SignedOut,
    /// The account page is being asked.
    Checking,
    /// The site accepts the session -- for this account, when its page names it.
    SignedIn { account: Option<String> },
    /// The site doesn't accept the session (any more): it expired, or the player signed out on
    /// the site.
    Invalid,
    /// The check got no answer (no network, an outage); the session is used as it is.
    Unchecked(String),
}

impl Global for SessionStatus {}

impl SessionStatus {
    /// Whether what a session enables is offered: one is stored, and the site hasn't refused it.
    pub fn signed_in(&self) -> bool {
        matches!(
            self,
            SessionStatus::Checking | SessionStatus::SignedIn { .. } | SessionStatus::Unchecked(_)
        )
    }
}

/// Bumped by every sign-in, sign-out and check: a check's answer is taken only while it still
/// speaks for the stored session.
static CHECKS: AtomicU64 = AtomicU64::new(0);

/// The session saved by the last sign-in, if any; one that can't be read counts as signed out.
#[cfg(target_os = "windows")]
pub fn load() -> TradeSession {
    match crate::platform::credentials::read(CREDENTIAL_TARGET) {
        Ok(saved) => TradeSession::new(saved.as_deref().and_then(parse)),
        Err(err) => {
            log::warn!("reading the saved pathofexile.com session failed: {err:#}");
            TradeSession::default()
        }
    }
}

/// The HTTP client [`SessionHttpClient`] wraps, without the stored session: a session the sign-in
/// window found is checked through it, carrying that one instead ([`check_candidate`]).
struct InnerClient(Arc<dyn HttpClient>);

impl Global for InnerClient {}

/// Makes `session` -- the one the app's HTTP client was built with, around `inner` -- the app's,
/// and checks it if there is one. A sign-in the app didn't live to finish may have left its
/// browser's profile behind: that goes first.
pub fn init(session: TradeSession, inner: Arc<dyn HttpClient>, cx: &mut App) {
    #[cfg(target_os = "windows")]
    crate::platform::login_window::wipe_profiles();
    cx.set_global(session);
    cx.set_global(InnerClient(inner));
    cx.set_global(SessionStatus::SignedOut);
    check(cx);
}

/// Asks the account page about `session` -- one the sign-in window found, not the app's -- through
/// a client that carries it instead of the stored one, to the same hosts only.
pub fn check_candidate(
    session: String,
    cx: &App,
) -> impl Future<Output = anyhow::Result<AccountCheck>> + use<> {
    let client: Arc<dyn HttpClient> = Arc::new(SessionHttpClient::new(
        cx.global::<InnerClient>().0.clone(),
        TradeSession::new(Some(session)),
    ));
    async move { account::check_session(&client).await }
}

/// Signs in with `session`, which the account page just accepted (for `account`, when the page
/// names it): saved in the Credential Manager, and used from the next request on.
#[cfg(target_os = "windows")]
pub fn sign_in(session: String, account: Option<String>, cx: &mut App) -> anyhow::Result<()> {
    crate::platform::credentials::write(CREDENTIAL_TARGET, CREDENTIAL_USER, &session)?;
    cx.global::<TradeSession>().set(session);
    CHECKS.fetch_add(1, Ordering::Relaxed);
    cx.set_global(SessionStatus::SignedIn { account });
    log::info!("pathofexile.com session saved");
    Ok(())
}

/// Forgets the session: deleted from the Credential Manager, and no longer sent.
#[cfg(target_os = "windows")]
pub fn sign_out(cx: &mut App) {
    if let Err(err) = crate::platform::credentials::delete(CREDENTIAL_TARGET) {
        log::warn!("deleting the saved pathofexile.com session failed: {err:#}");
    }
    cx.global::<TradeSession>().clear();
    CHECKS.fetch_add(1, Ordering::Relaxed);
    cx.set_global(SessionStatus::SignedOut);
    log::info!("pathofexile.com session forgotten");
}

/// The site refused the session elsewhere (live search's socket): it is invalid from now on, as a
/// check would find.
pub fn refused(cx: &mut App) {
    CHECKS.fetch_add(1, Ordering::Relaxed);
    if cx.global::<TradeSession>().is_signed_in() {
        cx.set_global(SessionStatus::Invalid);
    }
}

/// Asks the account page about the stored session and shows the answer: at start, and whenever
/// the trade site refuses what only a signed-in account may search (a weighted sum).
pub fn check(cx: &mut App) {
    let check = CHECKS.fetch_add(1, Ordering::Relaxed) + 1;
    if !cx.global::<TradeSession>().is_signed_in() {
        cx.set_global(SessionStatus::SignedOut);
        return;
    }
    cx.set_global(SessionStatus::Checking);
    let client = cx.http_client();
    cx.spawn(async move |cx| {
        let status = match account::check_session(&client).await {
            Ok(AccountCheck::SignedIn { account }) => SessionStatus::SignedIn { account },
            Ok(AccountCheck::SignedOut) => SessionStatus::Invalid,
            Err(err) => SessionStatus::Unchecked(format!("{err:#}")),
        };
        // No account name in the log: the diagnostics report sends the log along.
        match &status {
            SessionStatus::SignedIn { .. } => log::info!("pathofexile.com session accepted"),
            SessionStatus::Invalid => log::warn!("pathofexile.com session refused"),
            SessionStatus::Unchecked(err) => {
                log::warn!("checking the pathofexile.com session failed: {err}")
            }
            _ => {}
        }
        cx.update(|cx| {
            if CHECKS.load(Ordering::Relaxed) == check {
                cx.set_global(status);
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;

    use super::*;

    const SESSION: &str = "0123456789abcdef0123456789abcdef";

    /// The app's real client stand-in: records each request's URL and `Cookie` header.
    #[derive(Default)]
    struct Recorder {
        requests: Mutex<Vec<(String, Option<String>)>>,
    }

    impl HttpClient for Recorder {
        fn user_agent(&self) -> Option<&HeaderValue> {
            None
        }

        fn proxy(&self) -> Option<&Url> {
            None
        }

        fn send(
            &self,
            request: Request<AsyncBody>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<AsyncBody>>> + Send + 'static>>
        {
            let cookie = request
                .headers()
                .get(COOKIE)
                .map(|cookie| cookie.to_str().unwrap().to_owned());
            self.requests
                .lock()
                .push((request.uri().to_string(), cookie));
            Box::pin(async { Ok(Response::new(AsyncBody::default())) })
        }
    }

    /// Each URL's `Cookie` header as `SessionHttpClient` sends it with `session`.
    fn cookies_sent(session: &TradeSession, urls: &[&str]) -> Vec<Option<String>> {
        let recorder = Arc::new(Recorder::default());
        let client = SessionHttpClient::new(recorder.clone(), session.clone());
        for url in urls {
            // The request is handed on synchronously; the response isn't needed.
            drop(client.get(url, AsyncBody::default(), true));
        }
        let requests = recorder.requests.lock();
        assert_eq!(requests.len(), urls.len());
        requests.iter().map(|(_, cookie)| cookie.clone()).collect()
    }

    #[test]
    fn the_cookie_goes_to_both_trade_sites_only() {
        let session = TradeSession::new(Some(SESSION.to_owned()));
        let cookie = Some(format!("POESESSID={SESSION}"));
        assert_eq!(
            cookies_sent(
                &session,
                &[
                    "https://www.pathofexile.com/api/trade2/search/Standard",
                    "https://ru.pathofexile.com/api/trade2/fetch/a1?query=Q",
                    "https://www.pathofexile.com:443/my-account",
                ],
            ),
            [cookie.clone(), cookie.clone(), cookie]
        );
        assert_eq!(
            cookies_sent(
                &session,
                &[
                    "http://www.pathofexile.com/api/trade2/data/leagues",
                    "https://www.pathofexile.com:8443/api/trade2/data/leagues",
                    "https://pathofexile.com/my-account",
                    "https://www.pathofexile.com.example.org/my-account",
                    "https://web.poecdn.com/image/Art/2DItems/Currency/CurrencyRerollRare.png",
                    "https://web.poecdn.com/api/currency-exchange/poe2/1790157600",
                    "https://api.poe2scout.com/poe2/Leagues/Standard/Items",
                    "https://api.github.com/repos/mttzzz/poe2-oracle/releases/latest",
                ],
            ),
            [None, None, None, None, None, None, None, None]
        );
    }

    #[test]
    fn nothing_is_sent_while_signed_out() {
        let session = TradeSession::new(Some(SESSION.to_owned()));
        session.clear();
        assert_eq!(
            cookies_sent(
                &session,
                &[
                    "https://www.pathofexile.com/my-account",
                    "https://ru.pathofexile.com/api/trade2/fetch/a1?query=Q",
                ],
            ),
            [None, None]
        );
        // A sign-in reaches the client built before it: the clones share the value.
        let client_side = session.clone();
        session.set(SESSION.to_owned());
        assert_eq!(
            cookies_sent(&client_side, &["https://www.pathofexile.com/my-account"]),
            [Some(format!("POESESSID={SESSION}"))]
        );
    }

    #[test]
    fn debug_prints_never_show_the_session() {
        let session = TradeSession::new(Some(SESSION.to_owned()));
        assert!(!format!("{session:?}").contains(SESSION));
        assert!(!format!("{:?}", session.cookie().unwrap()).contains(SESSION));
    }

    #[test]
    fn only_a_bare_session_id_is_taken() {
        assert_eq!(parse(SESSION), Some(SESSION.to_owned()));
        // Nothing that could say more than one cookie, or isn't a session at all.
        assert_eq!(parse(&format!("{SESSION}; other=1")), None);
        assert_eq!(parse(&format!("{SESSION}\r\nX-Other: 1")), None);
        assert_eq!(parse(&format!("POESESSID={SESSION}")), None);
        assert_eq!(parse(&format!(" {SESSION}")), None);
        assert_eq!(parse("короткий"), None);
        assert_eq!(parse("abc"), None);
        assert_eq!(parse(""), None);
    }
}
