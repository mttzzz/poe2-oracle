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
//! [`PER_CLIENT`] from one client address ([`limits::client_key`]) and at most [`AT_ONCE`] in all.
//! A stream past either cap gets no place, yet still the news: one event with the versions the
//! service knows now (none while it knows none) and `retry: 60000`, then the stream ends, with no
//! ping, and counts as no stream opened ([`Stat::EventStream`]). The app takes the versions and
//! reconnects on its backoff, which only a connection that brought versions and stayed up for a
//! ping interval starts over. A refused one never does, so a refused app asks again after about 5,
//! 10, 20, 40, 80 and 160 s, then every 5 min (each ±20 %), and hears of a release at most about
//! 6 min after the apps holding a stream. When the service stops taking connections, every stream
//! ends at once rather than hold up the shutdown: the app reconnects, and the gateway sends it to
//! the pod taking over.
//!
//! An app's stream is also what counts its install as active ([`usage`]): when it opens, whether it
//! gets a place or not, and again after each Moscow midnight for as long as it stays open
//! ([`Rollover`]) -- it holds one connection for days and asks nothing meanwhile. The flags of a
//! start ([`Start`]) that an app's first connection of a run carries count once, opened or refused.

use std::collections::HashMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Request, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::{self, Stream};
use oracle_protocol::{EVENTS_PING_SECS, VERSIONS_EVENT, Versions};
use parking_lot::Mutex;
use rustix::process::{Resource, getrlimit};
use tokio::sync::watch;
use tokio::time::{Instant, Sleep};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::agent::{AppAgent, Start};
use crate::distinct::Who;
use crate::moscow::{self, Day};
use crate::stats::{self, Stat};
use crate::{App, usage};

/// The most streams one client address holds at once: a computer club, an office or a dorm behind
/// one address, or a carrier's NAT, which puts hundreds of subscribers behind one; and the streams
/// of connections that died without a word -- the service hears of those only when the gateway
/// gives up on them, which can take minutes. A client past it loses only the push: it gets the
/// versions each time it asks, on its backoff. The cap keeps one address -- a script, a stuck
/// client -- from taking every place: it holds under 2 % of [`AT_ONCE`].
pub const PER_CLIENT: usize = 64;
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
/// The reconnection delay the first event suggests.
const RECONNECT: Duration = Duration::from_millis(15_000);
/// The reconnection delay a refused stream's event suggests to clients that reconnect the way
/// browsers do. The app goes by its backoff.
const REFUSED_RECONNECT: Duration = Duration::from_secs(60);

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

/// `GET /api/v1/events`: the event stream, or while the streams are at their most, the versions
/// alone ([`refusal`]).
pub async fn follow(State(app): State<Arc<App>>, request: Request) -> Response {
    let who = Who::of(&request);
    let agent = AppAgent::of(request.headers());
    // The flags of a start, which only the app's first connection of a run carries.
    let start = agent
        .as_ref()
        .and_then(|_| Start::of(request.uri().query()));
    let place = match app.streams.admit(who.client().to_owned()) {
        Ok(place) => place,
        Err(full) => {
            match full {
                Full::Client => {
                    info!("event stream refused: its address holds {PER_CLIENT} already");
                }
                Full::Service => warn!(
                    at_once = app.streams.at_once,
                    "event stream refused: the service holds its most"
                ),
            }
            if let Some(agent) = agent {
                usage::app_stream(&app, who, agent, start, false);
            }
            let versions = app.releases.versions().borrow().clone();
            return Sse::new(refusal(versions)).into_response();
        }
    };
    stats::count(&app, Stat::EventStream);
    // A developer's build is counted as nothing but its start, so nothing keeps it counted.
    let rollover = agent
        .as_ref()
        .filter(|_| !start.as_ref().is_some_and(|start| start.dev))
        .map(|agent| {
            Rollover::new(
                app.clone(),
                who.clone(),
                agent.clone(),
                Arc::new(moscow::now),
            )
        });
    if let Some(agent) = agent {
        usage::app_stream(&app, who, agent, start, true);
    }
    let events = events(
        app.releases.versions(),
        app.streams.stopping.clone(),
        place,
        rollover,
    );
    Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(EVENTS_PING_SECS))
                .text("ping"),
        )
        .into_response()
}

/// What keeps an app's stream counted on the days after the one it opened. The stream lives as long
/// as the app runs -- days, when the app isn't restarted -- and asks nothing meanwhile, so without
/// this an install would be missing from every day but its first. A few minutes after each Moscow
/// midnight, the wait at random so that the streams that have been open all night don't all count
/// at once, the stream counts its install as active in the day that has begun.
struct Rollover {
    app: Arc<App>,
    who: Who,
    agent: AppAgent,
    /// Unix seconds, now: the clock, or a test's.
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    /// The most seconds after midnight it waits.
    spread: u64,
    /// When it counts next.
    wake: Pin<Box<Sleep>>,
}

/// The most seconds after a Moscow midnight an open stream waits before counting itself.
const ROLLOVER_SPREAD: u64 = 300;

impl Rollover {
    fn new(
        app: Arc<App>,
        who: Who,
        agent: AppAgent,
        clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    ) -> Rollover {
        let spread = getrandom::u32().map_or(0, |random| u64::from(random) % ROLLOVER_SPREAD);
        let wake = Box::pin(tokio::time::sleep(wait_for_midnight(clock(), spread)));
        Rollover {
            app,
            who,
            agent,
            clock,
            spread,
            wake,
        }
    }

    /// The moment has come: counts the install in the day it is now, and waits for the next.
    fn turn(&mut self) {
        let now = (self.clock)();
        usage::app_active_at(&self.app, self.who.clone(), self.agent.clone(), now);
        let wait = wait_for_midnight(now, self.spread);
        self.wake.as_mut().reset(Instant::now() + wait);
    }
}

/// How long after `now` a stream waits to count itself in the next day: until the next Moscow
/// midnight, and `spread` seconds more.
fn wait_for_midnight(now: i64, spread: u64) -> Duration {
    Duration::from_secs((Day::of(now).end() - now).max(0) as u64 + spread)
}

/// Resolves when the stream's rollover is due; never for a stream that has none.
async fn rollover_due(rollover: &mut Option<Rollover>) {
    match rollover {
        Some(rollover) => rollover.wake.as_mut().await,
        None => std::future::pending().await,
    }
}

/// A stream's events: the versions once they're known, and again whenever they change, until
/// `stopping`. The stream keeps `place` for as long as it lasts.
fn events(
    mut versions: watch::Receiver<Option<Versions>>,
    stopping: CancellationToken,
    place: Place,
    rollover: Option<Rollover>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    // What the service knows now is news to a stream that has just begun.
    versions.mark_changed();
    stream::unfold(
        (versions, stopping, place, true, rollover),
        |(mut versions, stopping, place, first, mut rollover)| async move {
            loop {
                tokio::select! {
                    biased;
                    () = stopping.cancelled() => return None,
                    changed = versions.changed() => changed.ok()?,
                    () = rollover_due(&mut rollover) => {
                        if let Some(rollover) = rollover.as_mut() {
                            rollover.turn();
                        }
                        continue;
                    }
                }
                let Some(latest) = versions.borrow_and_update().clone() else {
                    continue;
                };
                let mut event = Event::default().event(VERSIONS_EVENT);
                if first {
                    event = event.retry(RECONNECT);
                }
                let event = event.json_data(latest).expect("versions serialize");
                return Some((Ok(event), (versions, stopping, place, false, rollover)));
            }
        },
    )
}

/// A refused stream: one event with `versions`, the latest the service knows -- none while it
/// knows none -- and `retry:` at [`REFUSED_RECONNECT`], then the end. No place, no ping.
fn refusal(versions: Option<Versions>) -> impl Stream<Item = Result<Event, Infallible>> {
    let event = match versions {
        Some(versions) => Event::default()
            .event(VERSIONS_EVENT)
            .retry(REFUSED_RECONNECT)
            .json_data(versions)
            .expect("versions serialize"),
        None => Event::default().retry(REFUSED_RECONNECT),
    };
    stream::iter([Ok(event)])
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{HeaderValue, StatusCode, header};
    use http_body_util::BodyExt as _;
    use tokio::time::{Instant, timeout};

    use super::*;
    use crate::{Config, moscow};

    /// The next piece of `body` the service writes, as text.
    async fn next(body: &mut Body) -> String {
        let frame = body.frame().await.expect("the stream goes on").unwrap();
        String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap()
    }

    /// All `response` writes, once it has ended -- at once: the test fails after a second.
    async fn whole(response: Response) -> String {
        let body = timeout(Duration::from_secs(1), response.into_body().collect())
            .await
            .expect("the stream ends at once")
            .unwrap()
            .to_bytes();
        String::from_utf8(body.to_vec()).unwrap()
    }

    /// A request for the stream from `client`, as the gateway forwards it.
    fn from(client: &str) -> Request {
        let mut request = Request::new(Body::empty());
        request
            .headers_mut()
            .insert("x-forwarded-for", HeaderValue::from_str(client).unwrap());
        request
    }

    /// The streams opened today, as the morning digest counts them.
    async fn counted_today(app: &App) -> u64 {
        let now = moscow::now();
        let key = format!("oracle:stat:event_stream:{}", moscow::Day::of(now));
        app.store.values(&[key], now).await.unwrap()[0]
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

    #[tokio::test(start_paused = true)]
    async fn a_stream_past_a_cap_gets_the_versions_and_ends_holding_no_place() {
        // A dry run: the service knows at once that it offers no release.
        let app = App::new(Config {
            event_streams: PER_CLIENT + 1,
            ..Config::default()
        })
        .unwrap();
        let mut held = Vec::new();
        for _ in 0..PER_CLIENT {
            held.push(follow(State(app.clone()), from("203.0.113.1")).await);
        }
        // Past the address's cap; then, the last place taken by another address, past the
        // service's.
        let past_address = follow(State(app.clone()), from("203.0.113.1")).await;
        held.push(follow(State(app.clone()), from("203.0.113.2")).await);
        let past_service = follow(State(app.clone()), from("203.0.113.3")).await;
        for refused in [past_address, past_service] {
            assert_eq!(refused.status(), StatusCode::OK);
            assert_eq!(refused.headers()[header::CONTENT_TYPE], "text/event-stream");
            assert_eq!(
                whole(refused).await,
                "event: versions\nretry: 60000\ndata: {\"app\":null,\"data\":null}\n\n"
            );
        }
        {
            let open = app.streams.open.lock();
            assert_eq!(open.total, PER_CLIENT + 1);
            assert_eq!(
                open.by_client,
                HashMap::from([
                    ("203.0.113.1".to_owned(), PER_CLIENT),
                    ("203.0.113.2".to_owned(), 1),
                ])
            );
        }
        // The counts are made in the background: in once the runtime has nothing else to do.
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert_eq!(
            counted_today(&app).await,
            PER_CLIENT as u64 + 1,
            "a refused stream counts as none"
        );

        drop(held);
        let open = app.streams.open.lock();
        assert_eq!(open.total, 0);
        assert!(open.by_client.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_stream_ends_at_once_while_the_versions_are_unknown() {
        // A token, and no listing yet: the service doesn't know the versions, and the stream with
        // the place waits for them.
        let app = App::new(Config {
            github_token: Some("token".to_owned()),
            event_streams: 1,
            ..Config::default()
        })
        .unwrap();
        let _held = follow(State(app.clone()), from("203.0.113.1")).await;
        let refused = timeout(
            Duration::from_secs(1),
            follow(State(app), from("203.0.113.2")),
        )
        .await
        .expect("answered at once");
        assert_eq!(whole(refused).await, "retry: 60000\n\n");
    }

    /// A request for the stream at `uri` from the app of `version`, behind `client`.
    fn app_asking(client: &str, version: &str, uri: &str) -> Request {
        let agent = format!("PoE2-Oracle/{version}");
        Request::builder()
            .uri(uri)
            .header("x-forwarded-for", client)
            .header("user-agent", agent)
            .body(Body::empty())
            .unwrap()
    }

    /// Today's counts, once the background tasks that made them are in.
    async fn snapshot_of_today(app: &App) -> crate::stats::Snapshot {
        tokio::time::sleep(Duration::from_millis(1)).await;
        let now = moscow::now();
        crate::stats::snapshots(&app.store, &[moscow::Day::of(now)], now)
            .await
            .unwrap()
            .remove(0)
    }

    #[tokio::test(start_paused = true)]
    async fn an_apps_first_connection_of_a_run_counts_its_start_and_a_reconnection_does_not() {
        let app = App::new(Config::default()).unwrap();
        app.releases
            .listed(&crate::releases::published(&["v0.1.3", "v0.1.2"]));
        let stream = "/api/v1/events";
        let browser = Request::builder()
            .uri(format!("{stream}?start=1&first=1&lang=ru"))
            .header("x-forwarded-for", "203.0.113.5")
            .header("user-agent", "Mozilla/5.0 Firefox/130.0")
            .body(Body::empty())
            .unwrap();
        for request in [
            // A new install, in Russian.
            app_asking(
                "203.0.113.1",
                "0.1.3",
                &format!("{stream}?start=1&first=1&lang=ru"),
            ),
            // An update from 0.1.2, in English; and that app's reconnection, which says nothing.
            app_asking(
                "203.0.113.2",
                "0.1.3",
                &format!("{stream}?start=1&from=0.1.2&lang=en"),
            ),
            app_asking("203.0.113.2", "0.1.3", stream),
            // A developer's build, whose flags count for nothing but itself.
            app_asking(
                "203.0.113.3",
                "0.1.3",
                &format!("{stream}?start=1&first=1&dev=1&lang=ru"),
            ),
            // Flags without the start, and flags from a client that is not the app.
            app_asking("203.0.113.4", "0.1.3", &format!("{stream}?first=1&lang=ru")),
            browser,
        ] {
            assert_eq!(
                follow(State(app.clone()), request).await.status(),
                StatusCode::OK
            );
        }
        let today = snapshot_of_today(&app).await;
        assert_eq!(today.get("app_start"), 2);
        assert_eq!(today.get("install_new"), 1);
        assert_eq!(today.get("update_applied_0.1.2_0.1.3"), 1);
        assert_eq!(today.sum("update_applied_"), 1);
        assert_eq!(today.get("app_start_lang_ru"), 1);
        assert_eq!(today.get("app_start_lang_en"), 1);
        assert_eq!(today.get("app_start_dev"), 1);
        // Four streams of the app opened, the developer's not among them; the browser's isn't the
        // app's. The old counter still reads every stream, as it always did.
        assert_eq!(today.get("app_conn"), 4);
        assert_eq!(today.get("app_conn_v_0.1.3"), 4);
        assert_eq!(today.get("event_stream"), 6);
        assert_eq!(
            today.get("uniq_app_day"),
            3,
            "the developer's build isn't one"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_start_refused_a_place_still_counts_and_its_stream_does_not() {
        let app = App::new(Config {
            event_streams: 1,
            ..Config::default()
        })
        .unwrap();
        app.releases
            .listed(&crate::releases::published(&["v0.1.3"]));
        let flags = "/api/v1/events?start=1&first=1&lang=ru";
        let _held = follow(
            State(app.clone()),
            app_asking("203.0.113.1", "0.1.3", flags),
        )
        .await;
        let refused = follow(
            State(app.clone()),
            app_asking("203.0.113.2", "0.1.3", flags),
        )
        .await;
        assert_eq!(refused.status(), StatusCode::OK);
        let today = snapshot_of_today(&app).await;
        assert_eq!(today.get("app_start"), 2);
        assert_eq!(today.get("install_new"), 2);
        assert_eq!(today.get("app_conn"), 1, "the refused stream opened none");
        assert_eq!(today.get("event_stream"), 1);
        assert_eq!(
            today.get("uniq_app_day"),
            2,
            "the refused install is active all the same"
        );
    }

    #[test]
    fn an_open_stream_waits_for_midnight_and_a_spread_after_it() {
        // 2026-09-26 12:00 in Moscow: twelve hours to the turn of the day.
        let noon = 1_790_413_200;
        assert_eq!(wait_for_midnight(noon, 0), Duration::from_secs(12 * 3600));
        assert_eq!(
            wait_for_midnight(noon, 299),
            Duration::from_secs(12 * 3600 + 299)
        );
        // A moment before midnight, and at it: the wait is to the midnight that ends the day.
        let midnight = moscow::Day::of(noon).end();
        assert_eq!(wait_for_midnight(midnight - 1, 5), Duration::from_secs(6));
        assert_eq!(
            wait_for_midnight(midnight, 5),
            Duration::from_secs(24 * 3600 + 5)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_left_open_counts_its_install_again_on_each_new_day() {
        use futures_util::StreamExt as _;

        let app = App::new(Config::default()).unwrap();
        // 2026-09-26 12:00 in Moscow, on a clock that goes with the paused time.
        let noon = 1_790_413_200;
        let opened = Instant::now();
        let clock: Arc<dyn Fn() -> i64 + Send + Sync> =
            Arc::new(move || noon + opened.elapsed().as_secs() as i64);
        let who = Who::test("203.0.113.1", "PoE2-Oracle/0.1.3");
        let agent =
            AppAgent::of(&app_asking("203.0.113.1", "0.1.3", "/").headers().clone()).unwrap();
        let place = app.streams.admit("203.0.113.1".to_owned()).ok().unwrap();
        let rollover = Rollover::new(app.clone(), who, agent, clock);
        let mut stream = Box::pin(events(
            app.releases.versions(),
            app.streams.stopping.clone(),
            place,
            Some(rollover),
        ));
        let _versions = stream
            .next()
            .await
            .expect("the versions come first")
            .expect("an event, not an error");
        let holding = tokio::spawn(async move { while stream.next().await.is_some() {} });

        let installs = |days_on: i64| {
            let day = moscow::Day::of(noon).minus(-days_on);
            let key = format!("oracle:stat:uniq_app_day:{day}");
            let store = &app.store;
            async move {
                store
                    .distinct_values(&[key], noon + days_on * 86_400)
                    .await
                    .unwrap()[0]
            }
        };
        // Nothing counted it on the day it opened -- the app's connection counts itself, not this.
        assert_eq!(installs(1).await, 0);
        // Past the midnight and its spread: the stream has counted its install in the new day.
        tokio::time::advance(Duration::from_secs(12 * 3600 + ROLLOVER_SPREAD + 1)).await;
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert_eq!(installs(1).await, 1);
        assert_eq!(installs(2).await, 0);
        // And once more a day on, and it is the same install: still one.
        tokio::time::advance(Duration::from_secs(24 * 3600)).await;
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert_eq!(installs(2).await, 1);
        assert_eq!(installs(1).await, 1);
        holding.abort();
    }
}
