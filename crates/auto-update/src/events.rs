//! The service's event stream ([`EVENTS_PATH`]), followed for as long as the app runs:
//! [`follow_events`] holds a Server-Sent Events connection open and reports what it hears as
//! [`Link`]s -- above all the latest published [`Versions`], which the service sends as soon as a
//! connection opens and again whenever one of them changes.
//!
//! The first connection of a run that opens also tells the service the app has started, in its
//! query ([`Start`]). Every attempt carries it until one opens -- a start that met a refusal, or
//! no network, is told again -- and none after that: a reconnection is no new start.
//!
//! A connection that fails to open, ends, or hears nothing -- not even the service's ping comment
//! -- for three ping intervals is dropped, and the next attempt waits out a backoff: 5 s, doubling
//! per failure up to 5 min, each wait ±20 % at random, so that a restarted service doesn't get
//! every app back in the same second. A refusal's `Retry-After` (in seconds) stretches the wait,
//! up to those 5 min. A connection that brought versions and stayed up for a ping interval starts
//! the backoff over. A message on `wake` -- the network is back, say -- ends a wait at once.
//!
//! The stream is read as the HTML standard's "Interpreting an event stream" reads it
//! ([`EventParser`]): lines end in `\n`, `\r\n` or `\r`, split anywhere across reads; `data:` lines
//! join with `\n`; a blank line dispatches the event; a `:` line is a comment. `id:` and `retry:`
//! are read past: every connection starts with the full versions, so there is nothing to resume,
//! and the pace of reconnecting is the backoff's. Lines and events have caps; one over its cap is
//! dropped, and the stream read on.

use std::future::{pending, poll_fn};
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::task::Poll;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use async_channel::{Receiver, Sender};
use futures::AsyncReadExt as _;
use futures_timer::Delay;
use http_client::http::header::{CONTENT_TYPE, RETRY_AFTER};
use http_client::{
    AsyncBody, HttpClient, HttpRequestExt as _, RedirectPolicy, Request, Response, StatusCode,
};
use oracle_protocol::{EVENTS_PATH, EVENTS_PING_SECS, VERSIONS_EVENT, Versions};

use crate::USER_AGENT;
use crate::start::Start;

/// The wait after a first failure; each further one doubles it, up to [`LAST_RETRY`].
const FIRST_RETRY: Duration = Duration::from_secs(5);
/// The longest wait between attempts, before its jitter.
const LAST_RETRY: Duration = Duration::from_secs(5 * 60);
/// How far a wait may stray from its step either way, at random.
const JITTER: f64 = 0.2;
/// A connection that has heard nothing for this long -- three of the service's pings missed --
/// is dead, whatever its socket says.
const SILENCE: Duration = Duration::from_secs(3 * EVENTS_PING_SECS);
/// How long a connection that brought versions has to stay up to start the backoff over: one
/// that the service keeps closing at once must not be retried every 5 s.
const STEADY: Duration = Duration::from_secs(EVENTS_PING_SECS);
/// The longest line read, without its end. The service's lines are a few dozen bytes.
const MAX_LINE_BYTES: usize = 16 * 1024;
/// The most data an event carries, its lines joined.
const MAX_EVENT_BYTES: usize = 64 * 1024;
/// What one read of the stream takes at most.
const READ_BYTES: usize = 8 * 1024;
/// The name of an event without an `event:` field.
const DEFAULT_EVENT: &str = "message";
/// A byte order mark, which the standard skips at the stream's start.
const BOM: &[u8] = "\u{feff}".as_bytes();

/// What [`follow_events`] reports, in order: `Connected`, a `Versions` for every versions event
/// the connection brings, then `Disconnected` once it ends -- or `Disconnected` alone for an
/// attempt that never opened -- and so on for as long as it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// A connection is open: the service answered with its event stream.
    Connected,
    /// The latest published versions: the first right after connecting, then one per change.
    Versions(Versions),
    /// The connection failed to open or dropped; the next attempt comes `retry_in` from now, or
    /// at once on a message on `wake`.
    Disconnected { retry_in: Duration },
}

/// Follows the service's event stream until `links` closes -- every receiver dropped -- and
/// reports on it there ([`Link`]). Run it on a background executor: it returns only then, or
/// stops when the task running it is dropped.
///
/// `client` must not time reads out: the stream is quiet between the service's pings, and the
/// follower itself drops a connection that has heard nothing for three of them. A message on
/// `wake` ends the wait for the next attempt at once, and several make one attempt. One that
/// comes while an attempt is still connecting is kept for the wait after it, should it fail; one
/// that comes while connected is dropped -- the connection's watchdog tells a dead one.
///
/// `start` is what this run's first connection to open says of the app's start ([`Start`]): the
/// one `Arc` goes to every follower the run makes, so that following again -- updates turned off
/// and on -- is no second start. `None` says nothing.
pub fn follow_events(
    client: Arc<dyn HttpClient>,
    links: Sender<Link>,
    wake: Receiver<()>,
    start: Option<Arc<Start>>,
) -> impl Future<Output = ()> + Send + 'static {
    follow(client, links, wake, start, RealTime, fastrand::Rng::new())
}

/// Where the follower's time comes from: the real one, or a test's.
trait Clock {
    type Sleep: Future<Output = ()> + Unpin;

    fn now(&self) -> Instant;

    /// Completes once `duration` has passed.
    fn sleep(&self, duration: Duration) -> Self::Sleep;
}

/// Real time, through futures-timer's helper thread: it wakes the follower on any executor,
/// GPUI's included, without a runtime of its own.
struct RealTime;

impl Clock for RealTime {
    type Sleep = Delay;

    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, duration: Duration) -> Delay {
        Delay::new(duration)
    }
}

/// [`follow_events`] on `clock`, its jitter drawn from `rng`.
async fn follow(
    client: Arc<dyn HttpClient>,
    links: Sender<Link>,
    wake: Receiver<()>,
    start: Option<Arc<Start>>,
    clock: impl Clock,
    rng: fastrand::Rng,
) {
    let url = oracle_protocol::url(EVENTS_PATH);
    let mut closed = pin!(links.closed());
    let mut backoff = Backoff::new(rng);
    // The last failure logged: offline, the same one repeats every few minutes.
    let mut logged = None;
    loop {
        let Some(dropped) = listen(
            &*client,
            &url,
            start.as_deref(),
            &links,
            closed.as_mut(),
            &wake,
            &clock,
        )
        .await
        else {
            return;
        };
        if dropped.steady {
            backoff.reset();
        }
        let retry_in = backoff.next().max(dropped.retry_after.unwrap_or_default());
        let problem = format!("{:#}", dropped.problem);
        if dropped.connected || logged.as_ref() != Some(&problem) {
            log::warn!("{url}: {problem}; trying again in {} s", retry_in.as_secs());
        } else {
            log::debug!("{url}: {problem}; trying again in {} s", retry_in.as_secs());
        }
        logged = Some(problem);
        if links.send(Link::Disconnected { retry_in }).await.is_err() {
            return;
        }
        let mut wait = clock.sleep(retry_in);
        match race(woken(&wake), &mut wait, closed.as_mut()).await {
            Raced::LinksClosed => return,
            // The wakes that came meanwhile make this one attempt.
            Raced::Done(()) => drain(&wake),
            Raced::Alarm => {}
        }
    }
}

/// How a connection ended.
struct Dropped {
    /// It opened: the service answered with its event stream.
    connected: bool,
    /// It brought versions and stayed up for [`STEADY`]: the backoff starts over.
    steady: bool,
    /// How long the service asked to be left alone, at most [`LAST_RETRY`].
    retry_after: Option<Duration>,
    problem: anyhow::Error,
}

/// Opens one connection to `url` -- with `start`'s query, until one has opened -- and reads it
/// until it ends, sending on `links` what it brings; `None` once `links` has closed.
async fn listen(
    client: &dyn HttpClient,
    url: &str,
    start: Option<&Start>,
    links: &Sender<Link>,
    mut closed: Pin<&mut impl Future<Output = ()>>,
    wake: &Receiver<()>,
    clock: &impl Clock,
) -> Option<Dropped> {
    let failed = |problem, retry_after| {
        Some(Dropped {
            connected: false,
            steady: false,
            retry_after,
            problem,
        })
    };
    let request = match events_request(url, start.and_then(Start::query)) {
        Ok(request) => request,
        Err(problem) => return failed(problem, None),
    };
    // The watchdog, from the request on: the answer counts as something heard.
    let mut silence = clock.sleep(SILENCE);
    let response = match race(client.send(request), &mut silence, closed.as_mut()).await {
        Raced::LinksClosed => return None,
        Raced::Alarm => return failed(anyhow!("no answer in {} s", SILENCE.as_secs()), None),
        Raced::Done(Err(problem)) => return failed(problem.context("connecting"), None),
        Raced::Done(Ok(response)) => response,
    };
    if response.status() != StatusCode::OK {
        let problem = anyhow!("the service answered {}", response.status());
        return failed(problem, retry_after(&response));
    }
    if !is_event_stream(&response) {
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .map_or("no content type".into(), |value| {
                String::from_utf8_lossy(value.as_bytes())
            });
        let problem = anyhow!("the service answered {content_type}, not an event stream");
        return failed(problem, None);
    }
    // The service has the start whatever becomes of the connection from here on: the marker says
    // so, and no later attempt carries it.
    if let Some(start) = start {
        start.delivered();
    }
    if links.send(Link::Connected).await.is_err() {
        return None;
    }
    log::info!("{url}: connected");
    let opened = clock.now();
    // Moot now: they were for the attempt, and it made it.
    drain(wake);

    let mut body = response.into_body();
    let mut parser = EventParser::default();
    let mut buffer = vec![0; READ_BYTES];
    let mut delivered = false;
    let problem = loop {
        silence = clock.sleep(SILENCE);
        let read = match race(body.read(&mut buffer), &mut silence, closed.as_mut()).await {
            Raced::LinksClosed => return None,
            Raced::Alarm => break anyhow!("heard nothing for {} s", SILENCE.as_secs()),
            Raced::Done(Ok(0)) => break anyhow!("the service ended the stream"),
            Raced::Done(Ok(read)) => read,
            Raced::Done(Err(error)) => {
                break anyhow::Error::new(error).context("reading the stream");
            }
        };
        for event in parser.feed(&buffer[..read]) {
            if event.name != VERSIONS_EVENT {
                continue;
            }
            match serde_json::from_str(&event.data) {
                Ok(versions) => {
                    delivered = true;
                    if links.send(Link::Versions(versions)).await.is_err() {
                        return None;
                    }
                }
                Err(error) => log::warn!("{url}: skipping a {VERSIONS_EVENT} event: {error}"),
            }
        }
    };
    // Those that came while connected are moot too: the connection was being heard from.
    drain(wake);
    Some(Dropped {
        connected: true,
        steady: delivered && clock.now().saturating_duration_since(opened) >= STEADY,
        retry_after: None,
        problem,
    })
}

/// The request for the stream at `url`, with `query` -- what a start says -- when there is one.
fn events_request(url: &str, query: Option<&str>) -> Result<Request<AsyncBody>> {
    let request = match query {
        Some(query) => Request::get(format!("{url}?{query}")),
        None => Request::get(url),
    };
    // No timeout: the stream has no end, and the watchdog tells a dead one from a quiet one.
    Ok(request
        .header("User-Agent", USER_AGENT)
        .header("Accept", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .follow_redirects(RedirectPolicy::FollowAll)
        .body(AsyncBody::default())?)
}

/// Whether `response` is typed `text/event-stream`, whatever its parameters. Anything else --
/// the login page of a hotel's Wi-Fi, say -- is no connection to the service.
fn is_event_stream(response: &Response<AsyncBody>) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("text/event-stream"))
}

/// A refusal's `Retry-After` in seconds (the service sends no dates), at most [`LAST_RETRY`]: a
/// wrong one must not stop updates for days.
fn retry_after(response: &Response<AsyncBody>) -> Option<Duration> {
    let seconds = response.headers().get(RETRY_AFTER)?.to_str().ok()?;
    let seconds = seconds.trim().parse().ok()?;
    Some(Duration::from_secs(seconds).min(LAST_RETRY))
}

/// What ended a [`race`].
enum Raced<T> {
    /// The work finished first.
    Done(T),
    /// The alarm went off first.
    Alarm,
    /// `links` closed.
    LinksClosed,
}

/// Awaits `work` until `alarm` goes off or `closed` completes, whichever comes first. When more
/// than one is ready, `closed` wins, then `work`: what has arrived counts before the watchdog.
async fn race<T>(
    work: impl Future<Output = T>,
    alarm: &mut (impl Future<Output = ()> + Unpin),
    mut closed: Pin<&mut impl Future<Output = ()>>,
) -> Raced<T> {
    let mut work = pin!(work);
    poll_fn(|cx| {
        if closed.as_mut().poll(cx).is_ready() {
            Poll::Ready(Raced::LinksClosed)
        } else if let Poll::Ready(done) = work.as_mut().poll(cx) {
            Poll::Ready(Raced::Done(done))
        } else if Pin::new(&mut *alarm).poll(cx).is_ready() {
            Poll::Ready(Raced::Alarm)
        } else {
            Poll::Pending
        }
    })
    .await
}

/// Completes on the next message on `wake`; never, once nothing can send one.
async fn woken(wake: &Receiver<()>) {
    if wake.recv().await.is_err() {
        pending::<()>().await;
    }
}

/// Drops the messages waiting on `wake`.
fn drain(wake: &Receiver<()>) {
    while wake.try_recv().is_ok() {}
}

/// The waits between attempts: [`FIRST_RETRY`], doubling per failure up to [`LAST_RETRY`], each
/// off by up to ±[`JITTER`] at random.
struct Backoff {
    /// The next wait, before its jitter.
    step: Duration,
    rng: fastrand::Rng,
}

impl Backoff {
    fn new(rng: fastrand::Rng) -> Self {
        Self {
            step: FIRST_RETRY,
            rng,
        }
    }

    /// The wait before the next attempt.
    fn next(&mut self) -> Duration {
        let step = self.step;
        self.step = (step * 2).min(LAST_RETRY);
        step.mul_f64(1.0 + JITTER * (2.0 * self.rng.f64() - 1.0))
    }

    fn reset(&mut self) {
        self.step = FIRST_RETRY;
    }
}

/// One event of the stream, as the standard dispatches it.
#[derive(Debug, PartialEq, Eq)]
struct Event {
    /// Its `event:` field; [`DEFAULT_EVENT`] without one.
    name: String,
    /// Its `data:` lines, joined with `\n`.
    data: String,
}

/// Reads `text/event-stream` as the HTML standard reads it, from reads split anywhere: inside a
/// line, a character or a `\r\n`. A line over [`MAX_LINE_BYTES`] is skipped, and the event it
/// belongs to dropped unless it is a comment; so is an event whose data would come to more than
/// [`MAX_EVENT_BYTES`]. What an unfinished event has at the stream's end is never dispatched.
#[derive(Default)]
struct EventParser {
    /// The line so far, without its end.
    line: Vec<u8>,
    /// The last read ended in `\r`: a `\n` that starts the next one ends the same line.
    after_cr: bool,
    /// Past the stream's first line, the only one a byte order mark may start.
    past_first_line: bool,
    /// The line outgrew [`MAX_LINE_BYTES`]: the rest of it is skipped.
    skipping_line: bool,
    /// The event's name so far: its last `event:` field.
    name: String,
    /// The event's `data:` lines so far, each followed by `\n`.
    data: String,
    /// The event outgrew a cap: at its end it is dropped, not dispatched.
    oversized: bool,
}

impl EventParser {
    /// Reads `bytes`, the next of the stream, and returns the events they complete.
    fn feed(&mut self, mut bytes: &[u8]) -> Vec<Event> {
        let mut events = Vec::new();
        if self.after_cr && !bytes.is_empty() {
            self.after_cr = false;
            bytes = bytes.strip_prefix(b"\n").unwrap_or(bytes);
        }
        while let Some(end) = bytes
            .iter()
            .position(|&byte| byte == b'\n' || byte == b'\r')
        {
            self.extend_line(&bytes[..end]);
            let mut rest = &bytes[end + 1..];
            if bytes[end] == b'\r' {
                match rest.strip_prefix(b"\n") {
                    Some(after_crlf) => rest = after_crlf,
                    None => self.after_cr = rest.is_empty(),
                }
            }
            bytes = rest;
            events.extend(self.end_line());
        }
        self.extend_line(bytes);
        events
    }

    fn extend_line(&mut self, part: &[u8]) {
        if self.skipping_line || part.is_empty() {
            return;
        }
        if self.line.len() + part.len() <= MAX_LINE_BYTES {
            self.line.extend_from_slice(part);
            return;
        }
        // Whether the line is a comment shows in its first byte, past a leading byte order mark.
        let mut head: Vec<u8> = self
            .line
            .iter()
            .chain(part)
            .take(BOM.len() + 1)
            .copied()
            .collect();
        if !self.past_first_line && head.starts_with(BOM) {
            head.drain(..BOM.len());
        }
        if head.first() != Some(&b':') {
            self.oversized = true;
        }
        self.skipping_line = true;
        self.line.clear();
    }

    fn end_line(&mut self) -> Option<Event> {
        let first_line = !std::mem::replace(&mut self.past_first_line, true);
        if std::mem::take(&mut self.skipping_line) {
            return None;
        }
        let line = std::mem::take(&mut self.line);
        let mut text = line.as_slice();
        if first_line {
            text = text.strip_prefix(BOM).unwrap_or(text);
        }
        let event = self.read_line(text);
        // Its buffer serves the next line.
        self.line = line;
        self.line.clear();
        event
    }

    fn read_line(&mut self, line: &[u8]) -> Option<Event> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line[0] == b':' {
            return None;
        }
        let (field, value) = match line.iter().position(|&byte| byte == b':') {
            Some(colon) => {
                let value = &line[colon + 1..];
                (&line[..colon], value.strip_prefix(b" ").unwrap_or(value))
            }
            None => (line, &[][..]),
        };
        match field {
            b"event" => self.name = String::from_utf8_lossy(value).into_owned(),
            b"data" if !self.oversized => {
                let value = String::from_utf8_lossy(value);
                // The data dispatched: the lines so far with their `\n`, and this one.
                if self.data.len() + value.len() > MAX_EVENT_BYTES {
                    self.oversized = true;
                    self.data.clear();
                } else {
                    self.data.push_str(&value);
                    self.data.push('\n');
                }
            }
            // `id` and `retry` included: see the module's docs.
            _ => {}
        }
        None
    }

    /// The event the lines so far make, at a blank line: none without data.
    fn dispatch(&mut self) -> Option<Event> {
        let name = std::mem::take(&mut self.name);
        let mut data = std::mem::take(&mut self.data);
        if std::mem::take(&mut self.oversized) || data.is_empty() {
            return None;
        }
        data.pop();
        Some(Event {
            name: if name.is_empty() {
                DEFAULT_EVENT.to_owned()
            } else {
                name
            },
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::fs;
    use std::io;
    use std::path::Path;
    use std::rc::Rc;
    use std::task::{Context, Waker};

    use futures::TryStreamExt as _;
    use futures::channel::{mpsc, oneshot};
    use futures::executor::LocalPool;
    use futures::future::BoxFuture;
    use futures::task::LocalSpawnExt as _;
    use http_client::http::HeaderValue;
    use http_client::{RequestTimeout, Url};
    use oracle_protocol::DataVersion;

    use super::*;
    use crate::Version;

    const MS: Duration = Duration::from_millis(1);

    // --- The follower -----------------------------------------------------------------------

    /// Time that passes only when a test says so.
    #[derive(Clone)]
    struct TestClock(Rc<RefCell<TestTime>>);

    struct TestTime {
        start: Instant,
        elapsed: Duration,
        /// Whoever waits for time to pass.
        sleepers: Vec<Waker>,
    }

    impl TestClock {
        fn new() -> Self {
            Self(Rc::new(RefCell::new(TestTime {
                start: Instant::now(),
                elapsed: Duration::ZERO,
                sleepers: Vec::new(),
            })))
        }

        fn advance(&self, by: Duration) {
            let sleepers = {
                let mut time = self.0.borrow_mut();
                time.elapsed += by;
                std::mem::take(&mut time.sleepers)
            };
            sleepers.into_iter().for_each(Waker::wake);
        }
    }

    struct TestSleep {
        clock: TestClock,
        until: Duration,
    }

    impl Future for TestSleep {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            let mut time = self.clock.0.borrow_mut();
            if time.elapsed >= self.until {
                Poll::Ready(())
            } else {
                time.sleepers.push(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    impl Clock for TestClock {
        type Sleep = TestSleep;

        fn now(&self) -> Instant {
            let time = self.0.borrow();
            time.start + time.elapsed
        }

        fn sleep(&self, duration: Duration) -> TestSleep {
            let until = self.0.borrow().elapsed + duration;
            TestSleep {
                clock: self.clone(),
                until,
            }
        }
    }

    /// One connection attempt of the follower's, for the test to answer as the service would.
    /// Dropped unanswered, it's a connection that failed.
    struct Attempt {
        request: Request<AsyncBody>,
        answer: oneshot::Sender<Result<Response<AsyncBody>>>,
    }

    impl Attempt {
        /// Answers with the event stream, whose body the returned end sends.
        fn open(self) -> Stream {
            self.answer(200, &[("content-type", "text/event-stream")])
        }

        fn answer(self, status: u16, headers: &[(&str, &str)]) -> Stream {
            let (sender, body) = mpsc::unbounded::<io::Result<Vec<u8>>>();
            let mut response = Response::builder().status(status);
            for &(name, value) in headers {
                response = response.header(name, value);
            }
            let body = AsyncBody::from_reader(body.into_async_read());
            let _ = self.answer.send(Ok(response.body(body).unwrap()));
            Stream(sender)
        }

        /// Whether the follower gave up waiting for the answer.
        fn abandoned(&self) -> bool {
            self.answer.is_canceled()
        }
    }

    /// The service's end of an open stream. Dropped, it ends the stream cleanly.
    struct Stream(mpsc::UnboundedSender<io::Result<Vec<u8>>>);

    impl Stream {
        fn send(&self, text: &str) {
            let bytes = text.as_bytes().to_vec();
            self.0
                .unbounded_send(Ok(bytes))
                .expect("the follower reads");
        }

        /// Breaks the connection off.
        fn fail(&self) {
            let reset = io::Error::other("connection reset");
            self.0
                .unbounded_send(Err(reset))
                .expect("the follower reads");
        }

        /// Whether the follower let go of the connection.
        fn released(&self) -> bool {
            self.0.is_closed()
        }
    }

    /// Stands in for the service: each request the follower makes goes to the test, an
    /// [`Attempt`] to answer.
    struct StandIn(async_channel::Sender<Attempt>);

    impl HttpClient for StandIn {
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
            let (answer, answered) = oneshot::channel();
            self.0
                .try_send(Attempt { request, answer })
                .expect("the test listens");
            Box::pin(async move {
                answered
                    .await
                    .unwrap_or_else(|_| Err(anyhow!("connection refused")))
            })
        }
    }

    /// A follower running against the stand-in, on test time.
    struct Follower {
        pool: LocalPool,
        clock: TestClock,
        attempts: async_channel::Receiver<Attempt>,
        /// The app's end of `links`; `None` once dropped.
        links: Option<async_channel::Receiver<Link>>,
        /// The app's end of `wake`; `None` once dropped.
        wake: Option<async_channel::Sender<()>>,
        returned: Rc<Cell<bool>>,
    }

    impl Follower {
        /// Starts one that says nothing of a start: it makes its first attempt at once.
        fn start() -> Self {
            Self::following(None)
        }

        /// Starts one that says `start` on the first connection to open, as the app has it do.
        fn following(start: Option<Arc<Start>>) -> Self {
            let (stand_in, attempts) = async_channel::unbounded();
            let (links_sender, links) = async_channel::unbounded();
            let (wake, woken) = async_channel::unbounded();
            let clock = TestClock::new();
            let client = Arc::new(StandIn(stand_in));
            let rng = fastrand::Rng::with_seed(7);
            let following = follow(client, links_sender, woken, start, clock.clone(), rng);
            let returned = Rc::new(Cell::new(false));
            let pool = LocalPool::new();
            let done = returned.clone();
            let run = async move {
                following.await;
                done.set(true);
            };
            pool.spawner().spawn_local(run).unwrap();
            let mut follower = Self {
                pool,
                clock,
                attempts,
                links: Some(links),
                wake: Some(wake),
                returned,
            };
            follower.run();
            follower
        }

        /// Lets the follower run until it waits for something.
        fn run(&mut self) {
            self.pool.run_until_stalled();
        }

        /// Lets `time` pass, and the follower run.
        fn wait(&mut self, time: Duration) {
            self.clock.advance(time);
            self.run();
        }

        /// The attempt the follower is making.
        fn attempt(&mut self) -> Attempt {
            self.attempts
                .try_recv()
                .expect("the follower is connecting")
        }

        /// Whether the follower is making an attempt the test hasn't taken yet.
        fn attempted(&self) -> bool {
            !self.attempts.is_empty()
        }

        /// What the follower reported since last asked.
        fn links(&mut self) -> Vec<Link> {
            let links = self.links.as_ref().expect("links is open");
            std::iter::from_fn(|| links.try_recv().ok()).collect()
        }

        /// The wait reported once an attempt or a connection ended, the only report since.
        fn retry_in(&mut self) -> Duration {
            match self.links()[..] {
                [Link::Disconnected { retry_in }] => retry_in,
                ref links => panic!("one Disconnected expected, not {links:?}"),
            }
        }

        /// Sends `count` wakes at once, then lets the follower run.
        fn wake(&mut self, count: usize) {
            let wake = self.wake.as_ref().expect("wake is open");
            for _ in 0..count {
                wake.try_send(()).unwrap();
            }
            self.run();
        }

        /// Answers the attempt with the stream, and `versions` on it.
        fn connect(&mut self, versions: &Versions) -> Stream {
            let stream = self.attempt().open();
            stream.send(&versions_event(versions));
            self.run();
            let expected = [Link::Connected, Link::Versions(versions.clone())];
            assert_eq!(self.links(), expected);
            stream
        }

        /// Fails the attempt; the wait reported after it.
        fn refuse(&mut self) -> Duration {
            drop(self.attempt());
            self.run();
            self.retry_in()
        }

        /// Drops the app's end of `links`.
        fn close_links(&mut self) {
            self.links = None;
            self.run();
        }
    }

    fn versions(app: &str, data: DataVersion) -> Versions {
        Versions {
            app: Some(app.to_owned()),
            data: Some(data),
        }
    }

    fn versions_event(versions: &Versions) -> String {
        let json = serde_json::to_string(versions).unwrap();
        format!("event: {VERSIONS_EVENT}\ndata: {json}\n\n")
    }

    /// Whether `wait` is `step` give or take 20 %.
    fn within(wait: Duration, step: Duration) -> bool {
        wait >= step * 4 / 5 && wait <= step * 6 / 5
    }

    #[test]
    fn the_first_versions_and_every_change_are_reported() {
        let mut follower = Follower::start();
        let attempt = follower.attempt();
        let request = &attempt.request;
        assert_eq!(request.uri().to_string(), oracle_protocol::url(EVENTS_PATH));
        assert_eq!(request.headers()["accept"], "text/event-stream");
        assert_eq!(request.headers()["user-agent"], USER_AGENT);
        // A deadline for the whole request would cut the endless stream off.
        assert_eq!(request.extensions().get::<RequestTimeout>(), None);
        let stream = attempt.open();
        follower.run();
        assert_eq!(follower.links(), [Link::Connected]);

        // The service's first block, as it sends it.
        stream.send("event: versions\nretry: 15000\ndata: {\"app\":\"0.1.1\",\"data\":null}\n\n");
        follower.run();
        let first = Versions {
            app: Some("0.1.1".to_owned()),
            data: None,
        };
        assert_eq!(follower.links(), [Link::Versions(first)]);

        // Pings, other events and a versions event that holds none are no news...
        stream.send(": ping\n\nevent: other\ndata: {}\n\ndata: {\"app\":\"9.9.9\"}\n\n");
        stream.send("event: versions\ndata: not json\n\n");
        follower.run();
        assert_eq!(follower.links(), []);
        // ...a change is, however the stream cuts it.
        stream.send("event: versions\r\ndata: {\"app\":\"0.1.1\",");
        stream.send("\"data\":2026092601}\r\n");
        follower.run();
        assert_eq!(follower.links(), []);
        stream.send("\r\n");
        follower.run();
        let change = versions("0.1.1", 2026092601);
        assert_eq!(follower.links(), [Link::Versions(change)]);
    }

    #[test]
    fn a_connection_that_hears_nothing_for_three_pings_is_dropped() {
        let mut follower = Follower::start();
        // An attempt the service never answers...
        let attempt = follower.attempt();
        follower.wait(SILENCE - MS);
        assert!(!attempt.abandoned());
        follower.wait(MS);
        assert!(attempt.abandoned());
        let retry_in = follower.retry_in();
        follower.wait(retry_in);

        // ...and a connection gone quiet. The service's pings keep it up...
        let stream = follower.connect(&versions("0.1.1", 1));
        for _ in 0..4 {
            follower.wait(SILENCE - MS);
            stream.send(": ping\n\n");
            follower.run();
        }
        assert_eq!(follower.links(), []);
        // ...but three intervals without one end it.
        follower.wait(SILENCE - MS);
        assert!(!stream.released());
        follower.wait(MS);
        assert!(stream.released());
        follower.retry_in();
    }

    #[test]
    fn an_ended_stream_is_followed_by_a_new_connection_after_the_wait() {
        let mut follower = Follower::start();
        let stream = follower.connect(&versions("0.1.1", 1));
        follower.wait(STEADY);
        // The service shutting down ends the stream cleanly.
        drop(stream);
        follower.run();
        let retry_in = follower.retry_in();
        assert!(within(retry_in, FIRST_RETRY), "{retry_in:?}");

        follower.wait(retry_in - MS);
        assert!(!follower.attempted());
        follower.wait(MS);
        follower.connect(&versions("0.1.2", 1));
    }

    #[test]
    fn the_backoff_doubles_to_five_minutes_and_starts_over_after_a_steady_connection() {
        let mut follower = Follower::start();
        for step in [5, 10, 20, 40, 80, 160, 300, 300] {
            let retry_in = follower.refuse();
            let step = Duration::from_secs(step);
            assert!(within(retry_in, step), "{retry_in:?} for {step:?}");
            follower.wait(retry_in);
        }

        // Not starting over: a connection with versions that broke off within a ping interval...
        let stream = follower.connect(&versions("0.1.1", 1));
        follower.wait(STEADY - MS);
        stream.fail();
        follower.run();
        let retry_in = follower.retry_in();
        assert!(within(retry_in, LAST_RETRY), "{retry_in:?}");
        follower.wait(retry_in);
        // ...one that stayed up without any -- a service that doesn't know them yet...
        let stream = follower.attempt().open();
        follower.run();
        assert_eq!(follower.links(), [Link::Connected]);
        follower.wait(STEADY);
        drop(stream);
        follower.run();
        let retry_in = follower.retry_in();
        assert!(within(retry_in, LAST_RETRY), "{retry_in:?}");
        follower.wait(retry_in);

        // ...unlike one that brought versions and stayed up.
        let stream = follower.connect(&versions("0.1.1", 1));
        follower.wait(STEADY);
        drop(stream);
        follower.run();
        let retry_in = follower.retry_in();
        assert!(within(retry_in, FIRST_RETRY), "{retry_in:?}");
        follower.wait(retry_in);
        let retry_in = follower.refuse();
        assert!(within(retry_in, FIRST_RETRY * 2), "{retry_in:?}");
    }

    #[test]
    fn the_waits_spread_over_the_whole_jitter() {
        let mut backoff = Backoff::new(fastrand::Rng::with_seed(1));
        let waits: Vec<_> = (0..1000)
            .map(|_| {
                backoff.reset();
                backoff.next()
            })
            .collect();
        let shortest = *waits.iter().min().unwrap();
        let longest = *waits.iter().max().unwrap();
        assert!(
            shortest >= Duration::from_secs(4) && shortest < Duration::from_millis(4100),
            "{shortest:?}"
        );
        assert!(
            longest <= Duration::from_secs(6) && longest > Duration::from_millis(5900),
            "{longest:?}"
        );
    }

    #[test]
    fn a_refusal_or_an_answer_other_than_the_stream_is_a_failed_attempt() {
        let mut follower = Follower::start();
        // The service is full and asks for two minutes: longer than the backoff's first wait.
        let full = [("content-type", "text/plain"), ("retry-after", "120")];
        follower.attempt().answer(503, &full);
        follower.run();
        assert_eq!(follower.retry_in(), Duration::from_secs(120));
        follower.wait(Duration::from_secs(120));

        // A hotel Wi-Fi's login page, whatever its status, is no event stream.
        follower
            .attempt()
            .answer(200, &[("content-type", "text/html")]);
        follower.run();
        let retry_in = follower.retry_in();
        assert!(within(retry_in, FIRST_RETRY * 2), "{retry_in:?}");
        follower.wait(retry_in);

        // A day's Retry-After is held to the backoff's longest wait.
        follower.attempt().answer(429, &[("retry-after", "86400")]);
        follower.run();
        assert_eq!(follower.retry_in(), LAST_RETRY);
    }

    #[test]
    fn a_wake_ends_the_wait_and_several_make_one_attempt() {
        let mut follower = Follower::start();
        follower.refuse();
        // The network is back, and back again: one attempt, at once.
        follower.wake(3);
        let attempt = follower.attempt();
        assert!(!follower.attempted());
        drop(attempt);
        follower.run();
        // The wakes are spent: the next wait runs its course.
        let retry_in = follower.retry_in();
        follower.wait(retry_in - MS);
        assert!(!follower.attempted());
        follower.wait(MS);
        assert!(follower.attempted());
    }

    #[test]
    fn a_wake_while_connecting_retries_at_once_should_that_attempt_fail() {
        let mut follower = Follower::start();
        let attempt = follower.attempt();
        follower.wake(1);
        // The attempt goes on...
        assert!(!attempt.abandoned());
        assert!(!follower.attempted());
        drop(attempt);
        follower.run();
        // ...and once it has failed, the next one comes at once.
        follower.retry_in();
        assert!(follower.attempted());
    }

    #[test]
    fn a_wake_while_connected_changes_nothing() {
        let mut follower = Follower::start();
        let stream = follower.connect(&versions("0.1.1", 1));
        follower.wake(1);
        assert_eq!(follower.links(), []);
        assert!(!stream.released());
        assert!(!follower.attempted());

        // Nor does it cut short the wait once that connection ends.
        stream.fail();
        follower.run();
        let retry_in = follower.retry_in();
        follower.wait(retry_in - MS);
        assert!(!follower.attempted());
        follower.wait(MS);
        assert!(follower.attempted());
    }

    #[test]
    fn with_nothing_left_to_wake_it_the_waits_run_their_course() {
        let mut follower = Follower::start();
        follower.wake = None;
        let retry_in = follower.refuse();
        assert!(!follower.attempted());
        follower.wait(retry_in - MS);
        assert!(!follower.attempted());
        follower.wait(MS);
        assert!(follower.attempted());
    }

    #[test]
    fn the_follower_returns_once_links_closes_whatever_it_was_doing() {
        // Connecting: the attempt is given up.
        let mut follower = Follower::start();
        let attempt = follower.attempt();
        follower.close_links();
        assert!(follower.returned.get());
        assert!(attempt.abandoned());

        // Connected: the connection is let go.
        let mut follower = Follower::start();
        let stream = follower.connect(&versions("0.1.1", 1));
        follower.close_links();
        assert!(follower.returned.get());
        assert!(stream.released());

        // Waiting: no attempt follows.
        let mut follower = Follower::start();
        follower.refuse();
        follower.close_links();
        assert!(follower.returned.get());
        follower.wait(LAST_RETRY * 2);
        assert!(!follower.attempted());
    }

    /// What a new install of 0.1.3, its app speaking Russian, says of its start.
    const START_QUERY: &str = "start=1&first=1&lang=ru";

    fn new_install(dir: &Path) -> Arc<Start> {
        Start::new(dir, &Version::new(0, 1, 3), "ru", false, None)
    }

    /// What `last-run-version` in `dir` says, if it is there.
    fn told(dir: &Path) -> Option<String> {
        fs::read_to_string(dir.join("last-run-version")).ok()
    }

    #[test]
    fn the_first_connection_to_open_says_the_start_and_a_reconnection_says_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut follower = Follower::following(Some(new_install(dir.path())));
        let attempt = follower.attempt();
        let url = format!("{}?{START_QUERY}", oracle_protocol::url(EVENTS_PATH));
        assert_eq!(attempt.request.uri().to_string(), url);
        // Still connecting: the service has not been told.
        assert_eq!(told(dir.path()), None);

        // It is when the stream opens...
        let stream = attempt.open();
        stream.send(&versions_event(&versions("0.1.3", 1)));
        follower.run();
        assert_eq!(told(dir.path()).as_deref(), Some("0.1.3"));
        follower.links();

        // ...and the connection after a broken one asks for the stream, and nothing more.
        stream.fail();
        follower.run();
        let retry_in = follower.retry_in();
        follower.wait(retry_in);
        let url = oracle_protocol::url(EVENTS_PATH);
        assert_eq!(follower.attempt().request.uri().to_string(), url);
    }

    #[test]
    fn a_start_that_did_not_get_through_is_said_again_until_a_connection_opens() {
        let dir = tempfile::tempdir().unwrap();
        let mut follower = Follower::following(Some(new_install(dir.path())));
        // No network, a refusal and a hotel Wi-Fi's page: none is a connection, each says it.
        for case in 0..3 {
            let attempt = follower.attempt();
            assert_eq!(attempt.request.uri().query(), Some(START_QUERY), "{case}");
            match case {
                0 => drop(attempt),
                1 => drop(attempt.answer(503, &[("retry-after", "10")])),
                _ => drop(attempt.answer(200, &[("content-type", "text/html")])),
            }
            follower.run();
            let retry_in = follower.retry_in();
            assert_eq!(told(dir.path()), None, "{case}");
            follower.wait(retry_in);
        }

        let attempt = follower.attempt();
        assert_eq!(attempt.request.uri().query(), Some(START_QUERY));
        let _stream = attempt.open();
        follower.run();
        assert_eq!(told(dir.path()).as_deref(), Some("0.1.3"));
    }

    #[test]
    fn a_follower_made_again_says_the_start_only_if_no_connection_opened_before() {
        let dir = tempfile::tempdir().unwrap();
        let start = new_install(dir.path());
        // Updates turned off before the service could be told, and on again: it is still to tell.
        let mut follower = Follower::following(Some(start.clone()));
        drop(follower.attempt());
        follower.run();
        drop(follower);
        let mut follower = Follower::following(Some(start.clone()));
        let attempt = follower.attempt();
        assert_eq!(attempt.request.uri().query(), Some(START_QUERY));
        let _stream = attempt.open();
        follower.run();
        drop(follower);

        // Turned off and on after a connection opened: it has been told.
        let mut follower = Follower::following(Some(start));
        assert_eq!(follower.attempt().request.uri().query(), None);
    }

    // --- The parser -------------------------------------------------------------------------

    fn event(name: &str, data: &str) -> Event {
        Event {
            name: name.to_owned(),
            data: data.to_owned(),
        }
    }

    fn parse(reads: &[&[u8]]) -> Vec<Event> {
        let mut parser = EventParser::default();
        reads.iter().flat_map(|read| parser.feed(read)).collect()
    }

    /// The events in `stream` read whole, the same as read a byte at a time.
    fn parse_every_way(stream: &[u8]) -> Vec<Event> {
        let whole = parse(&[stream]);
        let bytes: Vec<&[u8]> = stream.chunks(1).collect();
        assert_eq!(parse(&bytes), whole, "read a byte at a time");
        whole
    }

    #[test]
    fn lines_end_in_lf_crlf_or_cr_split_anywhere() {
        let expected = [event("versions", "one\ntwo"), event("message", "three")];
        for end in ["\n", "\r\n", "\r"] {
            let lines = [
                "event: versions",
                "data: one",
                "data: two",
                "",
                "data: three",
                "",
                "",
            ];
            let stream = lines.join(end);
            assert_eq!(parse_every_way(stream.as_bytes()), expected, "{end:?}");
        }
        // A `\r\n` split between reads ends one line, not two.
        let reads: [&[u8]; 4] = [b"data: one\r", b"\ndata: two\r", b"\n", b"\r\n"];
        assert_eq!(parse(&reads), [event("message", "one\ntwo")]);
    }

    #[test]
    fn fields_are_read_as_the_standard_reads_them() {
        let stream = concat!(
            ": a comment\n",
            "data:no space\n",
            "data:  two spaces\n",
            "data\n",
            "data: a: colon\n",
            "id: 7\nretry: 15000\nunknown: field\nData: not data\n",
            "\n",
            // No data, no event; and its name doesn't carry over to the next.
            "event: versions\n\n",
            "data: plain\n\n",
            "event:\ndata: unnamed\n\n",
            // Blank lines alone dispatch nothing...
            "\n\n",
            // ...nor does an event the stream ends in the middle of.
            "event: versions\ndata: unfinished\n",
        );
        assert_eq!(
            parse_every_way(stream.as_bytes()),
            [
                event("message", "no space\n two spaces\n\na: colon"),
                event("message", "plain"),
                event("message", "unnamed"),
            ]
        );
    }

    #[test]
    fn a_byte_order_mark_is_skipped_at_the_start_only() {
        // Further on it makes its line's field `\u{feff}data`, which is no field.
        let stream = "\u{feff}event: versions\ndata: 1\n\n\u{feff}data: 2\n\n";
        assert_eq!(parse_every_way(stream.as_bytes()), [event("versions", "1")]);
    }

    #[test]
    fn text_is_whole_however_its_characters_are_split() {
        let stream = "event: versions\ndata: Путь изгнанника 2\n\n";
        assert_eq!(
            parse_every_way(stream.as_bytes()),
            [event("versions", "Путь изгнанника 2")]
        );
        // Bytes that aren't UTF-8 come out as replacement characters, not as a lost event.
        assert_eq!(parse(&[b"data: \xff\n\n"]), [event("message", "\u{fffd}")]);
    }

    #[test]
    fn a_line_or_an_event_over_its_cap_is_dropped_and_the_stream_read_on() {
        let next = "event: versions\ndata: next\n\n";
        let data_line = |length: usize| format!("data:{}\n", "x".repeat(length - "data:".len()));
        // A line at the cap is read; a byte more drops its event, and that one only.
        let at_the_cap = data_line(MAX_LINE_BYTES) + "\n" + next;
        let data = "x".repeat(MAX_LINE_BYTES - "data:".len());
        assert_eq!(
            parse_every_way(at_the_cap.as_bytes()),
            [event("message", &data), event("versions", "next")]
        );
        let over_the_cap = data_line(MAX_LINE_BYTES + 1) + "data: its event\n\n" + next;
        assert_eq!(
            parse_every_way(over_the_cap.as_bytes()),
            [event("versions", "next")]
        );
        // A long comment is no event to drop.
        let comment = format!(":{}\n", "x".repeat(MAX_LINE_BYTES)) + next;
        assert_eq!(
            parse_every_way(comment.as_bytes()),
            [event("versions", "next")]
        );

        // An event's data joined from lines of 10 000 bytes at most, `total` bytes in all.
        let event_of = |total: usize| {
            let mut data = String::new();
            while data.len() < total {
                if !data.is_empty() {
                    data.push('\n');
                }
                let piece = (total - data.len()).min(10_000);
                data.push_str(&"y".repeat(piece));
            }
            let lines: String = data
                .split('\n')
                .map(|line| format!("data:{line}\n"))
                .collect();
            (lines + "\n" + next, data)
        };
        let (at_the_cap, data) = event_of(MAX_EVENT_BYTES);
        assert_eq!(
            parse(&[at_the_cap.as_bytes()]),
            [event("message", &data), event("versions", "next")]
        );
        let (over_the_cap, _) = event_of(MAX_EVENT_BYTES + 1);
        assert_eq!(
            parse(&[over_the_cap.as_bytes()]),
            [event("versions", "next")]
        );
    }
}
