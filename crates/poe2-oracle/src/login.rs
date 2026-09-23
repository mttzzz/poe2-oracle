//! «Войти»: signing in to pathofexile.com in the app's sign-in window (`platform::login_window`),
//! the site's own login page in Edge WebView2 and in the interface language (`login_page`), so the
//! player copies nothing by hand. After every page of the site the window reports the browser's
//! `POESESSID` cookies. The site hands every visitor one, signed in or not, so each is asked about
//! on the account page first (`session::check_candidate`); the first one the site accepts becomes
//! the app's session (`session::sign_in`, into the Credential Manager), and the window closes.
//! Both sites set the cookie for all of `.pathofexile.com` and share their sessions (checked
//! 2026-09-23): one signed in on `ru.pathofexile.com` is the one `www`'s account page accepts.

use std::iter;

use async_channel::Receiver;
use gpui::{App, AsyncApp, Global};
use trade_client::account::AccountCheck;

use crate::i18n;
use crate::platform::login_window::{self, LoginEvent};
use crate::session;

/// The sign-in window as the settings window shows it.
pub struct Login {
    /// From «Войти» until the window closes: another «Войти» brings it forward.
    open: bool,
    /// Why the last sign-in didn't finish, until the next one.
    problem: Option<LoginProblem>,
}

impl Global for Login {}

impl Login {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn problem(&self) -> Option<&LoginProblem> {
        self.problem.as_ref()
    }
}

/// Why a sign-in didn't finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginProblem {
    /// No WebView2 runtime to show the site with.
    NoRuntime,
    /// The window's browser failed to start or to open the site.
    Failed(String),
    /// The site accepted the session, but the Credential Manager didn't take it.
    NotSaved(String),
}

/// Sets up «Войти».
pub fn init(cx: &mut App) {
    cx.set_global(Login {
        open: false,
        problem: None,
    });
}

/// «Войти»: opens the sign-in window, or brings the open one forward.
pub fn open(cx: &mut App) {
    // Showing a window sends GPUI's windows messages at once: done from tasks, not this update.
    if cx.global::<Login>().open {
        cx.spawn(async |_| login_window::bring_forward()).detach();
        return;
    }
    let page = login_page();
    let login = cx.global_mut::<Login>();
    login.open = true;
    login.problem = None;
    cx.spawn(async move |cx| {
        let problem = sign_in(&page, cx).await;
        cx.update(|cx| {
            let login = cx.global_mut::<Login>();
            login.open = false;
            login.problem = problem;
        });
    })
    .detach();
}

/// The login page in the interface language: the Russian site's for a Russian interface, the
/// international one's otherwise.
fn login_page() -> String {
    format!("{}/login", i18n::lang().trade_site().origin())
}

/// One sign-in on `page`, from opening the window until it closes: what kept it from finishing, if
/// anything.
async fn sign_in(page: &str, cx: &mut AsyncApp) -> Option<LoginProblem> {
    let Some(version) = login_window::runtime_version() else {
        log::warn!("no WebView2 runtime: the sign-in window can't open");
        return Some(LoginProblem::NoRuntime);
    };
    let (events_tx, events) = async_channel::unbounded();
    if let Err(err) = login_window::open(page, events_tx) {
        log::warn!("opening the sign-in window failed: {err:#}");
        return Some(LoginProblem::Failed(format!("{err:#}")));
    }
    log::info!("sign-in window opened on {page} (WebView2 {version})");
    let problem = watch(&events, cx).await;
    log::info!("sign-in window closed");
    problem
}

/// Follows the open window until it closes: asks the account page about the sessions it reports,
/// signs in with the first one the site accepts and closes the window.
async fn watch(events: &Receiver<LoginEvent>, cx: &mut AsyncApp) -> Option<LoginProblem> {
    let mut problem = None;
    let mut signed_in = false;
    while let Ok(first) = events.recv().await {
        // Whatever came during the last check at once: only the newest page's sessions are asked.
        let mut sessions = Vec::new();
        let mut closed = false;
        for event in iter::once(first).chain(iter::from_fn(|| events.try_recv().ok())) {
            match event {
                LoginEvent::Sessions(found) => sessions = found,
                LoginEvent::Failed(err) => problem = Some(LoginProblem::Failed(err)),
                LoginEvent::Closed => closed = true,
            }
        }
        if !signed_in {
            for candidate in sessions.iter().filter_map(|value| session::parse(value)) {
                let check = cx.update(|cx| session::check_candidate(candidate.clone(), cx));
                match check.await {
                    Ok(AccountCheck::SignedIn { account }) => {
                        signed_in = true;
                        if let Err(err) = cx.update(|cx| session::sign_in(candidate, account, cx)) {
                            log::warn!("saving the pathofexile.com session failed: {err:#}");
                            problem = Some(LoginProblem::NotSaved(format!("{err:#}")));
                        }
                        login_window::close();
                        break;
                    }
                    // The visitor's session: the player hasn't signed in yet.
                    Ok(AccountCheck::SignedOut) => {}
                    Err(err) => log::warn!("checking the sign-in window's session failed: {err:#}"),
                }
            }
        }
        if closed {
            break;
        }
    }
    problem
}
