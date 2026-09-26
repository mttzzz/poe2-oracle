//! `GET /api/v1/events`: the stream the app stays connected to, to hear of a new app release or
//! data pack as soon as the service lists it ([`crate::releases`]).
//!
//! Server-Sent Events, as [`oracle_protocol::EVENTS_PATH`] describes them: a [`VERSIONS_EVENT`]
//! event with the latest [`Versions`] as soon as the service knows them -- at once, unless GitHub
//! hasn't answered since the service started -- and another whenever they change; `: ping` after
//! [`EVENTS_PING_SECS`] of silence, which keeps the gateway and the load balancer from closing the
//! connection and tells the app the service is still there. The first event carries `retry: 15000`
//! for clients that reconnect the way browsers do. Nothing on the stream is compressed: only the
//! site's files are.
//!
//! A stream holds its connection for as long as the app runs, so the streams are capped: at most
//! [`PER_CLIENT`] from one client address ([`limits::client_key`]), more answered 429, and at most
//! [`AT_ONCE`] in all, more answered 503, both with `Retry-After`. When the service stops taking
//! connections, every stream ends at once rather than hold up the shutdown: the app reconnects, and
//! the gateway sends it to the pod taking over.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::{self, Stream};
use oracle_protocol::{EVENTS_PING_SECS, VERSIONS_EVENT, Versions};
use parking_lot::Mutex;
use rustix::process::{Resource, getrlimit};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::stats::{self, Stat};
use crate::{App, limits};

/// The most streams one client address holds at once: a household's PCs, and the streams of
/// connections that died without a word -- the service hears of those only when the gateway gives
/// up on them, which can take minutes.
pub const PER_CLIENT: usize = 8;
/// The most streams at once, unless tests say otherwise ([`crate::Config::event_streams`]).
/// Measured on the release build in the dev lane, an idle stream takes 18.6 KiB of the process's
/// memory (its connection's buffers, task, timer and request span), with one worker thread or
/// four, plus about 4.5 KiB of kernel socket memory, which the pod's limit counts too; the process
/// keeps what closed streams freed for the next ones. 4000 streams take about
/// 90 MiB of the pod's 256, beside the release files kept in memory (40 MiB), three reports being
/// read (60 MiB) and the rest of the service (10 MiB).
pub const AT_ONCE: usize = 4000;
/// The open files kept for everything but the streams: the listener, the site's files, other
/// requests, and the calls to GitHub, Telegram and Redis.
const OTHER_FILES: u64 = 256;
/// How long a refused client is told to wait.
const RETRY_AFTER_SECS: u64 = 60;
/// The reconnection delay the first event suggests.
const RECONNECT: Duration = Duration::from_millis(15_000);

/// The open streams, counted per client address.
pub struct Streams {
    /// The most at once: as many as asked for, or as the open-file limit leaves room for.
    at_once: usize,
    open: Mutex<Open>,
    /// Cancelled when the service stops taking connections: every stream ends.
    stopping: CancellationToken,
}

#[derive(Default)]
struct Open {
    total: usize,
    by_client: HashMap<String, usize>,
}

/// A stream's place among the open ones, given back when the stream ends or its connection closes.
struct Place {
    streams: Arc<Streams>,
    client: String,
}

impl Drop for Place {
    fn drop(&mut self) {
        let mut open = self.streams.open.lock();
        open.total -= 1;
        if let Some(count) = open.by_client.get_mut(&self.client) {
            *count -= 1;
            if *count == 0 {
                open.by_client.remove(&self.client);
            }
        }
    }
}

/// Why a stream was refused.
enum Full {
    /// Its address holds [`PER_CLIENT`] streams already.
    Client,
    /// The service holds its most.
    Service,
}

impl Streams {
    /// Room for `wanted` streams at once, or as many as the open-file limit leaves room for.
    pub fn new(wanted: usize) -> Streams {
        Streams {
            at_once: wanted.min(file_room()),
            open: Mutex::default(),
            stopping: CancellationToken::new(),
        }
    }

    /// The most streams at once.
    pub fn at_once(&self) -> usize {
        self.at_once
    }

    /// Ends every stream, and any opened from now on as soon as it has begun.
    pub fn stop(&self) {
        self.stopping.cancel();
    }

    fn admit(self: &Arc<Self>, client: String) -> Result<Place, Full> {
        let mut open = self.open.lock();
        let held = open.by_client.get(&client).copied().unwrap_or(0);
        if held >= PER_CLIENT {
            return Err(Full::Client);
        }
        if open.total >= self.at_once {
            return Err(Full::Service);
        }
        open.total += 1;
        open.by_client.insert(client.clone(), held + 1);
        Ok(Place {
            streams: self.clone(),
            client,
        })
    }
}

/// How many streams the open-file limit leaves room for, besides [`OTHER_FILES`].
fn file_room() -> usize {
    getrlimit(Resource::Nofile)
        .current
        .and_then(|limit| usize::try_from(limit.saturating_sub(OTHER_FILES)).ok())
        .unwrap_or(usize::MAX)
}

/// `GET /api/v1/events`: the event stream, or 429 or 503 while the streams are at their most.
pub async fn follow(State(app): State<Arc<App>>, request: Request) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip());
    let client = limits::client_key(request.headers(), peer);
    let place = match app.streams.admit(client) {
        Ok(place) => place,
        Err(Full::Client) => {
            info!("event stream refused: its address holds {PER_CLIENT} already");
            return refused(
                StatusCode::TOO_MANY_REQUESTS,
                "too many update streams from this address",
            );
        }
        Err(Full::Service) => {
            warn!(
                at_once = app.streams.at_once,
                "event stream refused: the service holds its most"
            );
            return refused(StatusCode::SERVICE_UNAVAILABLE, "too many update streams");
        }
    };
    stats::count(&app, Stat::EventStream);
    let events = events(app.releases.versions(), app.streams.stopping.clone(), place);
    Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(EVENTS_PING_SECS))
                .text("ping"),
        )
        .into_response()
}

/// A stream's events: the versions once they're known, and again whenever they change, until
/// `stopping`. The stream keeps `place` for as long as it lasts.
fn events(
    mut versions: watch::Receiver<Option<Versions>>,
    stopping: CancellationToken,
    place: Place,
) -> impl Stream<Item = Result<Event, Infallible>> {
    // What the service knows now is news to a stream that has just begun.
    versions.mark_changed();
    stream::unfold(
        (versions, stopping, place, true),
        |(mut versions, stopping, place, first)| async move {
            loop {
                tokio::select! {
                    biased;
                    () = stopping.cancelled() => return None,
                    changed = versions.changed() => changed.ok()?,
                }
                let Some(latest) = versions.borrow_and_update().clone() else {
                    continue;
                };
                let mut event = Event::default().event(VERSIONS_EVENT);
                if first {
                    event = event.retry(RECONNECT);
                }
                let event = event.json_data(latest).expect("versions serialize");
                return Some((Ok(event), (versions, stopping, place, false)));
            }
        },
    )
}

fn refused(status: StatusCode, why: &str) -> Response {
    (
        status,
        [(header::RETRY_AFTER, HeaderValue::from(RETRY_AFTER_SECS))],
        format!("{why}; try again in {RETRY_AFTER_SECS} s"),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use http_body_util::BodyExt as _;
    use tokio::time::Instant;

    use super::*;
    use crate::Config;

    /// The next piece of `body` the service writes, as text.
    async fn next(body: &mut Body) -> String {
        let frame = body.frame().await.expect("the stream goes on").unwrap();
        String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn a_quiet_stream_pings_every_25_seconds() {
        // A dry run: no token, so the service knows it offers no release. The service outlives
        // its streams, as the server's state does.
        let app = App::new(Config::default()).unwrap();
        let response = follow(State(app.clone()), Request::new(Body::empty())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body();
        let start = Instant::now();
        assert_eq!(
            next(&mut body).await,
            "event: versions\nretry: 15000\ndata: {\"app\":null,\"data\":null}\n\n"
        );
        for pings in 1..=2 {
            assert_eq!(next(&mut body).await, ": ping\n\n");
            assert_eq!(start.elapsed(), Duration::from_secs(25 * pings));
        }
    }
}
