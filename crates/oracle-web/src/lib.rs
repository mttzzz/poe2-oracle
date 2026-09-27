//! oracle.pushka.biz: the web service of PoE2 Oracle.
//!
//! One small server does five jobs:
//! - it serves the site the way GitHub Pages served it: the landing pages from `site/` (English
//!   at the root, Russian under `/ru/`), the player guide's two books built from `docs/guide`
//!   under `/guide/en/` and `/guide/ru/`, and the guide's pictures under `/images/` and
//!   `/guide/images/`; `/` and `/guide/` lead to the reader's language ([`site`]);
//! - it takes the reports the app's report window and the site's form send
//!   ([`oracle_protocol::Report`]) and passes each on twice: as an issue in the private GitHub
//!   repository for reports (`GITHUB_REPORTS_REPO`) and as a Telegram message to the owner
//!   ([`reports`]);
//! - it lists the app repository's releases (`GITHUB_REPO`) every two minutes and answers the
//!   app's updater with the latest of each kind, the app's release and the data pack, and serves
//!   their files, which GitHub itself hands out only with a token while the repository is private
//!   ([`releases`]);
//! - it keeps the running apps connected to an event stream that tells them the latest versions
//!   as soon as it lists them, so a new release reaches them within minutes ([`events`]);
//! - it counts downloads, stream connections, update checks and reports per Moscow day, and every
//!   morning posts the day before to the owner's Telegram ([`stats`]).
//!
//! Everything is configured from the environment ([`Config::from_env`]). Without a GitHub or a
//! Telegram token the service runs dry on that side: it logs what it would have sent and sends
//! nothing, which is how the dev lanes run it. A lane can instead point it at a stand-in for
//! GitHub, which serves test releases and takes the issues (`GITHUB_API`, with a dummy token), and
//! have it list the releases every few seconds (`LIST_RELEASES_EVERY`): `lanes/dev.sh` does both
//! when the lane has test releases to offer.

mod events;
mod github;
mod issue;
mod limits;
mod moscow;
mod releases;
mod reports;
mod site;
mod stats;
mod store;
mod telegram;
mod upstream;

use std::error::Error;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::response::Response;
use axum::routing::{get, post};
use oracle_protocol::{EVENTS_PATH, LATEST_DATA_PATH, LATEST_RELEASE_PATH, REPORTS_PATH};
use tokio::net::TcpListener;
use tokio::sync::{Notify, Semaphore};
use tokio_util::task::TaskTracker;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use tracing::{Span, info, info_span, warn};

pub use events::PER_CLIENT as STREAMS_PER_CLIENT;

/// How the service runs. [`Config::from_env`] reads it from the environment; tests fill it in
/// directly, pointing the API addresses at stand-ins. No `Debug`: it holds the tokens.
#[derive(Clone)]
pub struct Config {
    /// `PORT`: the port it listens on, on every address.
    pub port: u16,
    /// `PUBLIC_URL`: where the service is reachable, without a trailing slash. The release answer
    /// names its downloads under it, and the updater accepts only downloads from its own service.
    pub public_url: String,
    /// `SITE_DIR`: the landing pages, `site/` of the repository.
    pub site_dir: PathBuf,
    /// `GUIDE_DIR`: the guide's two books as `docs/guide/build.sh` writes them, in `en/` and `ru/`.
    pub guide_dir: PathBuf,
    /// `IMAGES_DIR`: the guide's pictures, `docs/guide/src/images`, which the landing pages show
    /// from `/images/` and the books from `/guide/images/`.
    pub images_dir: PathBuf,
    /// `GITHUB_TOKEN`: the owner's fine-grained token for [`Config::github_repo`] and
    /// [`Config::github_reports_repo`], with Contents read (the releases) and Issues read/write
    /// (the reports).
    pub github_token: Option<String>,
    /// `GITHUB_REPO`: `owner/name` of the repository the releases come from, and the reports go to
    /// when [`Config::github_reports_repo`] names none.
    pub github_repo: String,
    /// `GITHUB_REPORTS_REPO`: `owner/name` of the repository the reports go to as issues. A report
    /// holds what the player wrote, a contact, the item's or crash's text, so this is a private
    /// repository of their own, and [`Config::github_repo`] can be public. Unset, the reports go
    /// to [`Config::github_repo`].
    pub github_reports_repo: Option<String>,
    /// `TELEGRAM_TOKEN`: the owner's tg-inbox bot.
    pub telegram_token: Option<String>,
    /// `TELEGRAM_CHAT_ID`: the owner's chat with that bot.
    pub telegram_chat_id: Option<String>,
    /// `REDIS_URL`: where counters and rate limits are shared between replicas; in memory without.
    pub redis_url: Option<String>,
    /// `GITHUB_API`: GitHub's REST API, without a trailing slash. Unset in production; the dev
    /// lane points it at its stand-in, `lanes/fake-github.py`, and tests at theirs. Every call
    /// goes there with [`Config::github_token`]: issues as well as releases.
    pub github_api: String,
    /// Telegram's Bot API. Not in the environment: only tests point it elsewhere.
    pub telegram_api: String,
    /// `LIST_RELEASES_EVERY`: how often the releases are listed, in whole seconds, never under
    /// [`LIST_RELEASES_AT_LEAST`]. Every two minutes unless set; the dev lane lists its stand-in's
    /// every few seconds, and tests shorten it further.
    pub list_releases_every: Duration,
    /// The most event streams at once ([`events::AT_ONCE`]), or fewer when the open-file limit
    /// leaves room for fewer. Not in the environment: only tests lower it.
    pub event_streams: usize,
}

impl Default for Config {
    /// Production's layout (the Docker image's `/app`) with no tokens: a dry run.
    fn default() -> Config {
        Config {
            port: 8080,
            public_url: oracle_protocol::PRODUCTION_BASE.to_owned(),
            site_dir: PathBuf::from("/app/site"),
            guide_dir: PathBuf::from("/app/guide"),
            images_dir: PathBuf::from("/app/images"),
            github_token: None,
            github_repo: "mttzzz/poe2-oracle".to_owned(),
            github_reports_repo: None,
            telegram_token: None,
            telegram_chat_id: None,
            redis_url: None,
            github_api: "https://api.github.com".to_owned(),
            telegram_api: "https://api.telegram.org".to_owned(),
            list_releases_every: releases::LIST_EVERY,
            event_streams: events::AT_ONCE,
        }
    }
}

/// The shortest `LIST_RELEASES_EVERY` the environment may set: a test release shows within
/// seconds, and GitHub is never asked for the list every second.
pub const LIST_RELEASES_AT_LEAST: Duration = Duration::from_secs(5);

impl Config {
    /// The configuration the environment gives, over [`Config::default`]. An empty variable
    /// counts as unset, so a deploy template can leave a secret blank.
    pub fn from_env() -> Result<Config, String> {
        Config::from_vars(|name| std::env::var(name).ok())
    }

    /// [`Config::from_env`] with each variable looked up by `lookup`, so that tests can give theirs
    /// without touching the process's environment.
    fn from_vars(lookup: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
        let var = |name: &str| lookup(name).filter(|value| !value.trim().is_empty());
        let mut config = Config::default();
        if let Some(port) = var("PORT") {
            config.port = port
                .trim()
                .parse()
                .map_err(|_| format!("PORT is not a port number: {port}"))?;
        }
        if let Some(url) = var("PUBLIC_URL") {
            config.public_url = url.trim().trim_end_matches('/').to_owned();
        }
        if let Some(dir) = var("SITE_DIR") {
            config.site_dir = dir.into();
        }
        if let Some(dir) = var("GUIDE_DIR") {
            config.guide_dir = dir.into();
        }
        if let Some(dir) = var("IMAGES_DIR") {
            config.images_dir = dir.into();
        }
        if let Some(repo) = var("GITHUB_REPO") {
            config.github_repo = repository("GITHUB_REPO", &repo)?;
        }
        if let Some(repo) = var("GITHUB_REPORTS_REPO") {
            config.github_reports_repo = Some(repository("GITHUB_REPORTS_REPO", &repo)?);
        }
        config.github_token = var("GITHUB_TOKEN").map(|token| token.trim().to_owned());
        if let Some(api) = var("GITHUB_API") {
            config.github_api = api.trim().trim_end_matches('/').to_owned();
        }
        if let Some(value) = var("LIST_RELEASES_EVERY") {
            let every = value.trim().parse().map(Duration::from_secs).map_err(|_| {
                format!("LIST_RELEASES_EVERY is not a whole number of seconds: {value}")
            })?;
            if every < LIST_RELEASES_AT_LEAST {
                return Err(format!(
                    "LIST_RELEASES_EVERY is {} s; it may be no shorter than {} s",
                    every.as_secs(),
                    LIST_RELEASES_AT_LEAST.as_secs()
                ));
            }
            config.list_releases_every = every;
        }
        config.telegram_token = var("TELEGRAM_TOKEN").map(|token| token.trim().to_owned());
        config.telegram_chat_id = var("TELEGRAM_CHAT_ID").map(|chat| chat.trim().to_owned());
        config.redis_url = var("REDIS_URL");
        Ok(config)
    }

    /// The repository the reports go to as issues: [`Config::github_reports_repo`], or else
    /// [`Config::github_repo`].
    fn reports_repo(&self) -> &str {
        self.github_reports_repo
            .as_deref()
            .unwrap_or(&self.github_repo)
    }
}

/// `value`, the variable `name`, trimmed, when it's a GitHub repository's `owner/name`: two parts
/// of ASCII letters, digits, `-`, `_` and `.`, neither of them `.` or `..`. It goes into the path of
/// every call to GitHub, so a URL, a missing or extra `/` or a stray character stops the service at
/// start instead of sending the calls astray.
fn repository(name: &str, value: &str) -> Result<String, String> {
    let repo = value.trim();
    let part = |part: &str| {
        !matches!(part, "" | "." | "..")
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    match repo.split_once('/') {
        Some((owner, repo_name)) if part(owner) && part(repo_name) => Ok(repo.to_owned()),
        _ => Err(format!(
            "{name} is not a GitHub repository's owner/name: {repo}"
        )),
    }
}

/// How many reports the service holds in memory at once. Each can take up to ~20 MiB while it's
/// read and parsed (its raw 12 MiB and the diagnostics zip decoded from it), and a report with a
/// zip keeps up to 8 MiB after its answer, until the zip has gone to Telegram; the pod has
/// 256 MiB. A report arriving while every place is taken is refused with 503 and `Retry-After`.
pub const REPORTS_AT_ONCE: usize = 3;

/// Everything the handlers share.
struct App {
    site: site::Site,
    store: store::Store,
    github: github::GitHub,
    telegram: telegram::Telegram,
    releases: releases::Releases,
    /// The event streams open, which shutting down ends.
    streams: Arc<events::Streams>,
    /// The [`REPORTS_AT_ONCE`] places for reports in memory: one is taken before a body is read
    /// and given back once it's parsed -- or, for a report with a diagnostics zip, once Telegram
    /// has the zip or the tries at sending it are over.
    reports_in_memory: Arc<Semaphore>,
    /// Work that outlives its request: a report's files going to Telegram after the sender has
    /// its answer. Shutting down waits for it.
    background: TaskTracker,
}

impl App {
    /// The service for `config`. Redis, when configured, is connected on first use, so a Redis
    /// that is down at start delays nothing.
    fn new(config: Config) -> Result<Arc<App>, Box<dyn Error + Send + Sync>> {
        let http = upstream::client()?;
        let store = match &config.redis_url {
            Some(url) => store::Store::redis(url)?,
            None => store::Store::memory(),
        };
        let reports_repo = config.reports_repo().to_owned();
        let github = github::GitHub::new(
            http.clone(),
            config.github_api,
            config.github_repo,
            reports_repo,
            config.github_token,
        );
        Ok(Arc::new(App {
            site: site::Site::new(config.site_dir, config.guide_dir, config.images_dir),
            store,
            releases: releases::Releases::new(config.public_url, github.configured()),
            github,
            telegram: telegram::Telegram::new(
                http,
                config.telegram_api,
                config.telegram_token,
                config.telegram_chat_id,
            ),
            streams: Arc::new(events::Streams::new(config.event_streams)),
            reports_in_memory: Arc::new(Semaphore::new(REPORTS_AT_ONCE)),
            background: TaskTracker::new(),
        }))
    }
}

/// The whole service's routes: the API, the release downloads, and the site behind them.
fn router(app: Arc<App>) -> Router {
    // Only the site's files are compressed: the release answers are a few hundred bytes with a
    // strong ETag, the downloads are already-compressed files sent with their exact
    // Content-Length, and each event must reach the app as soon as it's written.
    let site = Router::new()
        .fallback(site::serve)
        .layer(CompressionLayer::new())
        .layer(axum::middleware::map_response(site::weaken_encoded_etag));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(REPORTS_PATH, post(reports::submit))
        .route(LATEST_RELEASE_PATH, get(releases::latest))
        .route(LATEST_DATA_PATH, get(releases::latest_data))
        .route(EVENTS_PATH, get(events::follow))
        .route("/download/latest", get(releases::latest_installer))
        .route("/download/{tag}/{asset}", get(releases::asset))
        .merge(site)
        .layer(axum::middleware::map_response(security_headers))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::extract::Request| {
                    // The probes ask every few seconds; logging them would bury everything else.
                    if request.uri().path() == "/healthz" {
                        return Span::none();
                    }
                    // The path only: a query string is the visitor's business. Cut, as whatever
                    // a request brings: a path can be tens of kilobytes.
                    info_span!("request", method = %request.method(), path = %short(request.uri().path(), 200))
                })
                .on_request(())
                .on_response(|response: &Response, latency: Duration, span: &Span| {
                    if !span.is_none() {
                        info!(
                            status = response.status().as_u16(),
                            ms = latency.as_millis() as u64,
                            "answered"
                        );
                    }
                })
                .on_failure(()),
        )
        .with_state(app)
}

/// Headers every answer carries: no content-type guessing, no full URLs in the Referer sent to
/// other sites, and no framing by other sites (the report form must not be overlaid).
async fn security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    for (name, value) in [
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "strict-origin-when-cross-origin"),
        (header::CONTENT_SECURITY_POLICY, "frame-ancestors 'none'"),
        (header::X_FRAME_OPTIONS, "DENY"),
    ] {
        headers
            .entry(name)
            .or_insert(HeaderValue::from_static(value));
    }
    response
}

/// `text` cut to `max` characters, with an ellipsis when it was longer.
fn short(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", text[..end].trim_end()),
        None => text.to_owned(),
    }
}

/// How long the service keeps serving after SIGTERM before it stops taking connections: the pod's
/// endpoint takes a moment to leave the gateway, and requests sent meanwhile should still land.
const WITHDRAW_DELAY: Duration = Duration::from_secs(5);
/// How long requests in flight, and reports still passing on, get to finish once the service stops
/// taking connections. Kubernetes kills the pod 30 s after SIGTERM.
const DRAIN_LIMIT: Duration = Duration::from_secs(20);

/// Serves `config` on every address until SIGTERM or Ctrl+C ([`serve`]).
pub async fn run(config: Config) -> Result<(), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind(("0.0.0.0", config.port)).await?;
    serve(listener, config, shutdown_signal()).await
}

/// Serves `config` on `listener`, with the release listing and the morning digest running
/// alongside, until `stop` resolves -- for [`run`], [`WITHDRAW_DELAY`] after SIGTERM, or at once
/// on Ctrl+C. Then it stops taking connections: the event streams end at once, and the requests
/// in flight and the reports still passing on get [`DRAIN_LIMIT`] to finish.
pub async fn serve(
    listener: TcpListener,
    config: Config,
    stop: impl Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let public_url = config.public_url.clone();
    let list_releases_every = config.list_releases_every;
    let counters = if config.redis_url.is_some() {
        "redis"
    } else {
        "memory"
    };
    for (name, dir) in [
        ("SITE_DIR", &config.site_dir),
        ("GUIDE_DIR", &config.guide_dir),
        ("IMAGES_DIR", &config.images_dir),
    ] {
        if !dir.is_dir() {
            warn!(name, dir = %dir.display(), "not a directory: everything under it answers 404");
        }
    }
    let app = App::new(config)?;
    let mode = |on: bool| if on { "on" } else { "dry run" };
    info!(
        address = %listener.local_addr()?,
        %public_url,
        github = mode(app.github.configured()),
        releases_repo = %app.github.releases_repo(),
        reports_repo = %app.github.reports_repo(),
        telegram = mode(app.telegram.configured()),
        counters,
        event_streams = app.streams.at_once(),
        "serving"
    );
    let digest = tokio::spawn(stats::post_digests(app.clone()));
    let listing = tokio::spawn(releases::refresh(app.clone(), list_releases_every));
    let draining = Arc::new(Notify::new());
    let stopping = {
        let (app, draining) = (app.clone(), draining.clone());
        async move {
            stop.await;
            info!("no longer taking connections; ending the event streams, finishing the rest");
            app.streams.stop();
            draining.notify_one();
        }
    };
    let mut server = tokio::spawn(
        axum::serve(
            listener,
            router(app.clone()).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(stopping)
        .into_future(),
    );
    tokio::select! {
        served = &mut server => return Ok(served??),
        () = draining.notified() => {}
    }
    let deadline = tokio::time::Instant::now() + DRAIN_LIMIT;
    match tokio::time::timeout_at(deadline, &mut server).await {
        Ok(served) => served??,
        Err(_) => warn!(
            "requests still running {} s after shutdown began; leaving them",
            DRAIN_LIMIT.as_secs()
        ),
    }
    app.background.close();
    if tokio::time::timeout_at(deadline, app.background.wait())
        .await
        .is_err()
    {
        warn!(
            "reports still passing on {} s after shutdown began; leaving them",
            DRAIN_LIMIT.as_secs()
        );
    }
    digest.abort();
    listing.abort();
    info!("stopped");
    Ok(())
}

/// Resolves when the service should stop taking connections: [`WITHDRAW_DELAY`] after SIGTERM,
/// at once on Ctrl+C.
async fn shutdown_signal() {
    let mut terminate =
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(signal) => Some(signal),
            Err(error) => {
                warn!(%error, "can't listen for SIGTERM; only Ctrl+C stops the service");
                None
            }
        };
    let sigterm = async {
        match &mut terminate {
            Some(signal) => {
                signal.recv().await;
            }
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        () = sigterm => {
            info!("SIGTERM: still serving for {} s while the pod leaves the gateway", WITHDRAW_DELAY.as_secs());
            tokio::time::sleep(WITHDRAW_DELAY).await;
        }
        _ = tokio::signal::ctrl_c() => info!("interrupted"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The configuration of an environment that holds `vars` and nothing else.
    fn config(vars: &[(&str, &str)]) -> Result<Config, String> {
        Config::from_vars(|name| {
            vars.iter()
                .find(|(var, _)| *var == name)
                .map(|(_, value)| (*value).to_owned())
        })
    }

    #[test]
    fn reports_go_to_their_own_repository_or_else_to_github_repo() {
        let apart = config(&[
            ("GITHUB_REPO", "mttzzz/poe2-oracle"),
            ("GITHUB_REPORTS_REPO", " mttzzz/poe2-oracle-reports\n"),
        ])
        .unwrap();
        assert_eq!(apart.reports_repo(), "mttzzz/poe2-oracle-reports");
        assert_eq!(apart.github_repo, "mttzzz/poe2-oracle", "the releases stay");

        // Unset, or left blank by a deploy template: the reports go where the releases come from.
        for reports in [None, Some(""), Some("  ")] {
            let mut vars = vec![("GITHUB_REPO", "someone/fork")];
            vars.extend(reports.map(|value| ("GITHUB_REPORTS_REPO", value)));
            assert_eq!(
                config(&vars).unwrap().reports_repo(),
                "someone/fork",
                "{reports:?}"
            );
        }
        assert_eq!(config(&[]).unwrap().reports_repo(), "mttzzz/poe2-oracle");

        // Whatever GitHub allows in a name.
        let allowed = config(&[("GITHUB_REPORTS_REPO", "Some-Org_1/poe2_oracle.reports-2")]);
        assert_eq!(
            allowed.unwrap().reports_repo(),
            "Some-Org_1/poe2_oracle.reports-2"
        );
    }

    #[test]
    fn a_malformed_repository_stops_the_service_at_start() {
        for name in ["GITHUB_REPO", "GITHUB_REPORTS_REPO"] {
            for value in [
                "poe2-oracle-reports",
                "https://github.com/mttzzz/poe2-oracle-reports",
                "github.com/mttzzz/poe2-oracle-reports",
                "mttzzz/poe2-oracle-reports/issues",
                "mttzzz/",
                "/poe2-oracle-reports",
                "mttzzz/..",
                "../poe2-oracle",
                "mttzzz/poe2 oracle",
                "mttzzz/poe2-oracle?per_page=1",
                "mttzzz/poe2%2Foracle",
            ] {
                let Err(problem) = config(&[(name, value)]) else {
                    panic!("{name}={value} passed");
                };
                // Names the variable and shows the value, so the log says what to fix.
                assert!(
                    problem.starts_with(&format!("{name} ")) && problem.ends_with(value),
                    "{problem}"
                );
            }
        }
    }
}
