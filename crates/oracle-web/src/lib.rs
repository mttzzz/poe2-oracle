//! oracle.pushka.biz: the web service of PoE2 Oracle.
//!
//! One small server does four jobs:
//! - it serves the site the way GitHub Pages served it: the landing pages from `site/` (English
//!   at the root, Russian under `/ru/`), the player guide mdBook builds from `docs/guide` under
//!   `/guide/`, and the guide's pictures under `/images/` ([`site`]);
//! - it takes the reports the app's report window and the site's form send
//!   ([`oracle_protocol::Report`]) and passes each on twice: as an issue in the private GitHub
//!   repository and as a Telegram message to the owner ([`reports`]);
//! - it answers the app's update check with the repository's latest release and serves that
//!   release's files, which GitHub itself hands out only with a token while the repository is
//!   private ([`releases`]);
//! - it counts downloads, update checks and reports per Moscow day, and every morning posts the
//!   day before to the owner's Telegram ([`stats`]).
//!
//! Everything is configured from the environment ([`Config::from_env`]). Without a GitHub or a
//! Telegram token the service runs dry on that side: it logs what it would have sent and sends
//! nothing, which is how the dev lanes run it.

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
use oracle_protocol::{LATEST_RELEASE_PATH, REPORTS_PATH};
use tokio::net::TcpListener;
use tokio::sync::{Notify, Semaphore};
use tokio_util::task::TaskTracker;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use tracing::{Span, info, info_span, warn};

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
    /// `GUIDE_DIR`: the guide as `mdbook build docs/guide` writes it.
    pub guide_dir: PathBuf,
    /// `IMAGES_DIR`: the guide's pictures, `docs/guide/src/images`.
    pub images_dir: PathBuf,
    /// `GITHUB_TOKEN`: the owner's fine-grained token for [`Config::github_repo`], with Issues
    /// read/write and Contents read.
    pub github_token: Option<String>,
    /// `GITHUB_REPO`: `owner/name` of the repository issues go to and releases come from.
    pub github_repo: String,
    /// `TELEGRAM_TOKEN`: the owner's tg-inbox bot.
    pub telegram_token: Option<String>,
    /// `TELEGRAM_CHAT_ID`: the owner's chat with that bot.
    pub telegram_chat_id: Option<String>,
    /// `REDIS_URL`: where counters and rate limits are shared between replicas; in memory without.
    pub redis_url: Option<String>,
    /// GitHub's REST API. Not in the environment: only tests point it elsewhere.
    pub github_api: String,
    /// Telegram's Bot API. Not in the environment: only tests point it elsewhere.
    pub telegram_api: String,
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
            telegram_token: None,
            telegram_chat_id: None,
            redis_url: None,
            github_api: "https://api.github.com".to_owned(),
            telegram_api: "https://api.telegram.org".to_owned(),
        }
    }
}

impl Config {
    /// The configuration the environment gives, over [`Config::default`]. An empty variable
    /// counts as unset, so a deploy template can leave a secret blank.
    pub fn from_env() -> Result<Config, String> {
        let var = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        };
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
            config.github_repo = repo.trim().to_owned();
        }
        config.github_token = var("GITHUB_TOKEN").map(|token| token.trim().to_owned());
        config.telegram_token = var("TELEGRAM_TOKEN").map(|token| token.trim().to_owned());
        config.telegram_chat_id = var("TELEGRAM_CHAT_ID").map(|chat| chat.trim().to_owned());
        config.redis_url = var("REDIS_URL");
        Ok(config)
    }
}

/// How many reports the service holds in memory at once. Each can take up to ~20 MiB while it's
/// read and parsed (its raw 12 MiB and the diagnostics zip decoded from it), and a report with a
/// zip keeps up to 8 MiB after its answer, until the zip has gone to Telegram; the pod has
/// 256 MiB. A report arriving while every place is taken is refused with 503 and `Retry-After`.
pub const REPORTS_AT_ONCE: usize = 3;

/// Everything the handlers share.
pub struct App {
    site: site::Site,
    store: store::Store,
    github: github::GitHub,
    telegram: telegram::Telegram,
    releases: releases::Releases,
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
    pub fn new(config: Config) -> Result<Arc<App>, Box<dyn Error + Send + Sync>> {
        let http = upstream::client()?;
        let store = match &config.redis_url {
            Some(url) => store::Store::redis(url)?,
            None => store::Store::memory(),
        };
        Ok(Arc::new(App {
            site: site::Site::new(config.site_dir, config.guide_dir, config.images_dir),
            store,
            github: github::GitHub::new(
                http.clone(),
                config.github_api,
                config.github_repo,
                config.github_token,
            ),
            telegram: telegram::Telegram::new(
                http,
                config.telegram_api,
                config.telegram_token,
                config.telegram_chat_id,
            ),
            releases: releases::Releases::new(config.public_url),
            reports_in_memory: Arc::new(Semaphore::new(REPORTS_AT_ONCE)),
            background: TaskTracker::new(),
        }))
    }
}

/// The whole service's routes: the API, the release downloads, and the site behind them.
pub fn router(app: Arc<App>) -> Router {
    // Only the site's files are compressed: the release answer is a few hundred bytes with a
    // strong ETag, and the downloads are already-compressed installers sent with their exact
    // Content-Length.
    let site = Router::new()
        .fallback(site::serve)
        .layer(CompressionLayer::new())
        .layer(axum::middleware::map_response(site::weaken_encoded_etag));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(REPORTS_PATH, post(reports::submit))
        .route(LATEST_RELEASE_PATH, get(releases::latest))
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

/// Serves `config` until SIGTERM or Ctrl+C, with the morning digest running alongside.
pub async fn run(config: Config) -> Result<(), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind(("0.0.0.0", config.port)).await?;
    let public_url = config.public_url.clone();
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
        telegram = mode(app.telegram.configured()),
        counters,
        "serving"
    );
    let digest = tokio::spawn(stats::post_digests(app.clone()));
    let draining = Arc::new(Notify::new());
    let mut server = tokio::spawn(
        axum::serve(
            listener,
            router(app.clone()).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal(draining.clone()))
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
    info!("stopped");
    Ok(())
}

/// Resolves when the service should stop taking connections, and tells `draining` so.
async fn shutdown_signal(draining: Arc<Notify>) {
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
    info!("no longer taking connections; finishing the requests in flight");
    draining.notify_one();
}
