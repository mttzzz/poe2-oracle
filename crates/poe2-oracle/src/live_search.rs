//! Live search: a search the player watches ("Следить" in the results header) reports each new
//! listing the moment the trade site lists it, as a card in the trade overlay (`ui::trade_overlay`)
//! with its new-request sound. Each watch is the trade site's live search socket
//! (`trade_client::live` has the protocol), blocking on a thread of its own; the ids the sockets
//! hear go to one GPUI task, which fetches the listings through the price check's own fetch path
//! and rate limiter (`price_check::fetch_new_listings`).
//!
//! A socket that drops is opened again after 5 s, then 10, 20 ... up to 5 min between tries --
//! back to 5 s once one has stayed up a while. A refusal no retry can change (401: the site doesn't
//! take the session; 404: the search is gone) ends the watch and says so on a card. At most 20 at
//! once, the site's own limit per account. Watches need the session: signing out, or the site
//! refusing it, ends them all; quitting closes every socket; none is kept across launches.
//!
//! The sockets and the watch list build and are tested on every target; the GPUI side that
//! fetches and makes cards is Windows-only, like the price check it goes through.

use std::fmt;
use std::io;
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::{Arc, LazyLock};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::Sender;
use gpui::Global;
use parking_lot::{Condvar, Mutex};
use poe2_domain::{ItemRarity, ParsedItem};
use rustls::{ClientConfig, RootCertStore};
use trade_client::live::{self, LiveEnd, LiveMessage, MAX_LIVE_SEARCHES};
use trade_client::{FetchedItem, TradeSite};
use tungstenite::client::IntoClientRequest as _;
use tungstenite::http::HeaderValue;
use tungstenite::http::header::{COOKIE, ORIGIN, USER_AGENT};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Connector, HandshakeError, Message, WebSocket};

use crate::session::TradeSession;

/// The wait before a dropped socket's first new try; each further one waits twice as long, up to
/// `LAST_RETRY`.
const FIRST_RETRY: Duration = Duration::from_secs(5);
const LAST_RETRY: Duration = Duration::from_secs(5 * 60);
/// A socket that stayed up this long was fine: its next drop is tried again from `FIRST_RETRY`.
const STAYED_UP: Duration = Duration::from_secs(60);
/// How long a socket may hear nothing -- the site pings at least every 30 s -- before it counts as
/// lost. Its handshake gets as long.
const SILENCE: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The most listing cards the overlay keeps, newest first; a burst of new listings fetches only
/// that many, the newest.
pub const SHOWN_LISTINGS: usize = 3;

/// Why "Следить" was refused: the site's own limit per account.
const TOO_MANY: &str = "Слежение возможно не больше чем за 20 поисками сразу — снимите его с \
                        другого поиска";

/// A search as the trade site knows it: what a watch opens its socket for and fetches with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedSearch {
    pub site: TradeSite,
    pub league: String,
    /// The id the search returned: the socket's path, and its fetches' `query`.
    pub query_id: String,
    /// The search's page on the trade site.
    pub trade_url: String,
    /// What was searched for, as its cards name it (`watch_label`).
    pub label: String,
}

/// How a watch's cards name what it searches for: the checked item's name -- or, for a magic or
/// rare item, whose name is its own alone, its base type.
pub fn watch_label(item: &ParsedItem) -> String {
    match item.rarity {
        Some(ItemRarity::Magic | ItemRarity::Rare) => {
            item.base_type.clone().unwrap_or_else(|| item.name.clone())
        }
        _ => item.name.clone(),
    }
}

/// What a watch's thread reports.
#[derive(Debug)]
pub enum LiveEvent {
    /// Listings the search just got, by id.
    Listed { watch: u64, ids: Vec<String> },
    /// The site refused the socket for good (`LiveEnd::is_final`): the watch is over.
    Ended { watch: u64, end: LiveEnd },
}

/// What live search puts on the trade overlay.
#[derive(Debug, Clone)]
pub enum LiveCard {
    /// A new listing of a watched search.
    Listing(LiveListing),
    /// A watch the site ended for good, and why.
    Ended { label: String, reason: &'static str },
}

/// A new listing, as its card shows it.
#[derive(Debug, Clone)]
pub struct LiveListing {
    /// The watched search's label.
    pub search: String,
    /// The listed item: its name and base type.
    pub item: String,
    /// `(amount, currency id)`; `None` for a listing without a price.
    pub price: Option<(f64, String)>,
    pub seller: String,
    /// The message to the seller; `None` for an instant buyout, which needs none.
    pub whisper: Option<String>,
    /// The watched search on the trade site.
    pub trade_url: String,
}

impl LiveListing {
    pub fn new(search: &WatchedSearch, item: FetchedItem) -> LiveListing {
        let name = [item.name.as_str(), item.type_line.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        LiveListing {
            search: search.label.clone(),
            item: name,
            price: item.price,
            seller: item.account_name,
            whisper: item.whisper,
            trade_url: search.trade_url.clone(),
        }
    }
}

/// The watched searches: a GPUI global, which the panel's "Следить" toggles and whose observers
/// hear every change.
pub struct LiveSearches {
    watches: Vec<Watch>,
    next_id: u64,
    session: TradeSession,
    user_agent: &'static str,
    events: Sender<LiveEvent>,
    /// The search a watch was just refused for, and why: said beside its toggle until the next
    /// toggle.
    refused: Option<(String, &'static str)>,
}

struct Watch {
    id: u64,
    search: WatchedSearch,
    stop: Arc<Stop>,
}

impl Global for LiveSearches {}

impl LiveSearches {
    /// No watches yet. Their sockets will carry `session` and `user_agent`, and report on
    /// `events`.
    pub fn new(
        session: TradeSession,
        user_agent: &'static str,
        events: Sender<LiveEvent>,
    ) -> LiveSearches {
        LiveSearches {
            watches: Vec::new(),
            next_id: 0,
            session,
            user_agent,
            events,
            refused: None,
        }
    }

    /// How many searches are watched.
    pub fn count(&self) -> usize {
        self.watches.len()
    }

    pub fn is_watched(&self, query_id: &str) -> bool {
        self.watches
            .iter()
            .any(|watch| watch.search.query_id == query_id)
    }

    /// Why watching `query_id` was just refused, if it was.
    pub fn refusal(&self, query_id: &str) -> Option<&'static str> {
        self.refused
            .as_ref()
            .filter(|(refused, _)| refused == query_id)
            .map(|&(_, reason)| reason)
    }

    /// Watches `search`, or stops watching it.
    pub fn toggle(&mut self, search: &WatchedSearch) {
        self.refused = None;
        if let Some(at) = self
            .watches
            .iter()
            .position(|watch| watch.search.query_id == search.query_id)
        {
            let watch = self.watches.remove(at);
            watch.stop.stop();
            log::info!("live search {}: stopped", watch.id);
            return;
        }
        if let Err(reason) = self.watch(search.clone()) {
            self.refused = Some((search.query_id.clone(), reason));
        }
    }

    fn watch(&mut self, search: WatchedSearch) -> Result<(), &'static str> {
        if !self.session.is_signed_in() {
            return Err("Слежение работает только со входом на pathofexile.com");
        }
        if self.watches.len() >= MAX_LIVE_SEARCHES {
            return Err(TOO_MANY);
        }
        self.next_id += 1;
        let id = self.next_id;
        let stop = Arc::new(Stop::default());
        let socket = Socket {
            id,
            url: live::live_url(search.site, &search.league, &search.query_id),
            origin: search.site.origin(),
            session: self.session.clone(),
            user_agent: self.user_agent,
            events: self.events.clone(),
            stop: stop.clone(),
        };
        thread::Builder::new()
            .name(format!("live search {id}"))
            .spawn(move || socket.run())
            .map_err(|err| {
                log::warn!("starting a live search thread failed: {err}");
                "Не удалось запустить слежение"
            })?;
        log::info!(
            "live search {id}: watching {} in {} ({:?} site)",
            search.query_id,
            search.league,
            search.site
        );
        self.watches.push(Watch { id, search, stop });
        Ok(())
    }

    /// The search watch `id` is for, while it lasts.
    pub fn search(&self, id: u64) -> Option<&WatchedSearch> {
        self.watches
            .iter()
            .find(|watch| watch.id == id)
            .map(|watch| &watch.search)
    }

    /// Ends watch `id` -- the site refused its socket for good -- and hands back its search;
    /// `None` if it was stopped already.
    pub fn end(&mut self, id: u64) -> Option<WatchedSearch> {
        let at = self.watches.iter().position(|watch| watch.id == id)?;
        let watch = self.watches.remove(at);
        watch.stop.stop();
        Some(watch.search)
    }

    /// Stops every watch.
    pub fn stop_all(&mut self) {
        for watch in self.watches.drain(..) {
            watch.stop.stop();
        }
        self.refused = None;
    }
}

/// The waits between a dropped socket's tries: `FIRST_RETRY`, doubling up to `LAST_RETRY`.
struct Backoff {
    next: Duration,
}

impl Default for Backoff {
    fn default() -> Backoff {
        Backoff { next: FIRST_RETRY }
    }
}

impl Backoff {
    fn next_wait(&mut self) -> Duration {
        let wait = self.next;
        self.next = (wait * 2).min(LAST_RETRY);
        wait
    }
}

/// Stops one watch's thread: wakes it from a wait between tries, and shuts its connection down
/// under a blocked read.
#[derive(Default)]
struct Stop {
    state: Mutex<StopState>,
    woken: Condvar,
}

#[derive(Default)]
struct StopState {
    stopped: bool,
    /// The open connection's socket, for `Stop::stop` to shut down.
    connection: Option<TcpStream>,
}

impl Stop {
    fn stop(&self) {
        let mut state = self.state.lock();
        state.stopped = true;
        if let Some(connection) = state.connection.take() {
            let _ = connection.shutdown(Shutdown::Both);
        }
        self.woken.notify_all();
    }

    fn is_stopped(&self) -> bool {
        self.state.lock().stopped
    }

    /// Waits `wait`, less if stopped meanwhile; whether it was.
    fn sleep(&self, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        let mut state = self.state.lock();
        while !state.stopped {
            if self.woken.wait_until(&mut state, deadline).timed_out() {
                break;
            }
        }
        state.stopped
    }

    /// Keeps a handle on the connection's socket for `stop`; `false` when stopped already.
    fn hold(&self, connection: &TcpStream) -> bool {
        let mut state = self.state.lock();
        if state.stopped {
            return false;
        }
        state.connection = connection.try_clone().ok();
        true
    }

    fn release(&self) {
        self.state.lock().connection = None;
    }
}

/// One watch's socket, run on a thread of its own until it is stopped or refused for good.
struct Socket {
    id: u64,
    url: String,
    /// The site's own origin, which the handshake says it comes from.
    origin: &'static str,
    session: TradeSession,
    user_agent: &'static str,
    events: Sender<LiveEvent>,
    stop: Arc<Stop>,
}

impl Socket {
    fn run(self) {
        let mut backoff = Backoff::default();
        loop {
            // Signed out meanwhile: the watch is being stopped anyway.
            let Some(cookie) = self.session.cookie() else {
                return;
            };
            let opened = Instant::now();
            let end = self.listen(cookie);
            if self.stop.is_stopped() {
                return;
            }
            if end.is_final() {
                let _ = self.events.send_blocking(LiveEvent::Ended {
                    watch: self.id,
                    end,
                });
                return;
            }
            if opened.elapsed() >= STAYED_UP {
                backoff = Backoff::default();
            }
            let wait = backoff.next_wait();
            log::info!(
                "live search {}: {end:?}, trying again in {} s",
                self.id,
                wait.as_secs()
            );
            if self.stop.sleep(wait) {
                return;
            }
        }
    }

    /// Opens the socket and passes on what it hears until it closes; why it did.
    fn listen(&self, cookie: HeaderValue) -> LiveEnd {
        let mut socket = match self.open(cookie) {
            Ok(socket) => socket,
            Err(end) => return end,
        };
        log::info!("live search {}: connected", self.id);
        let end = loop {
            match socket.read() {
                Ok(Message::Text(text)) => match live::parse_message(text.as_str()) {
                    Ok(LiveMessage::New(ids)) => {
                        let listed = LiveEvent::Listed {
                            watch: self.id,
                            ids,
                        };
                        // Nobody left to tell: the app is closing.
                        if self.events.send_blocking(listed).is_err() {
                            break LiveEnd::Other;
                        }
                    }
                    Ok(LiveMessage::Other) => {}
                    Err(err) => log::warn!("live search {}: {err:#}", self.id),
                },
                Ok(Message::Close(frame)) => {
                    break frame.map_or(LiveEnd::Other, |frame| {
                        LiveEnd::from_close_code(frame.code.into())
                    });
                }
                // Pings are answered by tungstenite itself, with the next read.
                Ok(_) => {}
                Err(err) => {
                    if !self.stop.is_stopped() {
                        log::info!("live search {}: connection lost: {err}", self.id);
                    }
                    break LiveEnd::Other;
                }
            }
        };
        self.stop.release();
        end
    }

    /// Connects and shakes hands, the session in the `Cookie` header. The handshake's refusal is
    /// the site's answer (`LiveEnd::from_status`); anything else that fails is a lost connection.
    fn open(&self, cookie: HeaderValue) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, LiveEnd> {
        let failed = |what: &str, err: &dyn fmt::Display| {
            log::info!("live search {}: {what}: {err}", self.id);
            LiveEnd::Other
        };
        let mut request = self
            .url
            .as_str()
            .into_client_request()
            .map_err(|err| failed("bad address", &err))?;
        let uri = request.uri().clone();
        let headers = request.headers_mut();
        headers.insert(COOKIE, cookie);
        headers.insert(ORIGIN, HeaderValue::from_static(self.origin));
        headers.insert(USER_AGENT, HeaderValue::from_static(self.user_agent));
        let tls = uri.scheme_str() == Some("wss");
        let connector = if tls {
            Connector::Rustls(tls_config().map_err(|err| failed("TLS", &err))?)
        } else {
            Connector::Plain
        };
        let host = uri.host().unwrap_or_default();
        let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
        let stream = connect(host, port).map_err(|err| failed("connecting", &err))?;
        stream
            .set_read_timeout(Some(SILENCE))
            .and_then(|()| stream.set_nodelay(true))
            .map_err(|err| failed("socket options", &err))?;
        if !self.stop.hold(&stream) {
            return Err(LiveEnd::Other);
        }
        let shaken = tungstenite::client_tls_with_config(request, stream, None, Some(connector));
        match shaken {
            Ok((socket, _)) => Ok(socket),
            Err(HandshakeError::Failure(tungstenite::Error::Http(response))) => {
                self.stop.release();
                log::warn!(
                    "live search {}: refused with HTTP {}",
                    self.id,
                    response.status()
                );
                Err(LiveEnd::from_status(response.status().as_u16()))
            }
            Err(err) => {
                self.stop.release();
                Err(failed("handshake", &err))
            }
        }
    }
}

/// Every socket's TLS: the system's root certificates, and the aws-lc-rs provider named outright
/// -- the tree builds rustls with both of its providers, and it then picks neither by itself.
static TLS: LazyLock<Result<Arc<ClientConfig>, String>> = LazyLock::new(|| {
    let found = rustls_native_certs::load_native_certs();
    for err in &found.errors {
        log::warn!("reading a system root certificate failed: {err}");
    }
    let mut roots = RootCertStore::empty();
    let (added, _) = roots.add_parsable_certificates(found.certs);
    if added == 0 {
        return Err("no usable system root certificate".to_owned());
    }
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|err| err.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
});

fn tls_config() -> Result<Arc<ClientConfig>, String> {
    TLS.clone()
}

/// A TCP connection to `host`, trying each of its addresses.
fn connect(host: &str, port: u16) -> io::Result<TcpStream> {
    let mut failure = io::Error::new(io::ErrorKind::NotFound, format!("{host} has no address"));
    for address in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(err) => failure = err,
        }
    }
    Err(failure)
}

#[cfg(target_os = "windows")]
pub use app_side::init;

/// The GPUI side: fetching what the sockets hear, and the cards.
#[cfg(target_os = "windows")]
mod app_side {
    use async_channel::{Receiver, Sender};
    use gpui::{App, AsyncApp, Entity, WeakEntity};
    use trade_client::live::LiveEnd;

    use super::{LiveCard, LiveEvent, LiveListing, LiveSearches, SHOWN_LISTINGS};
    use crate::price_check::{self, PriceCheckApp};
    use crate::session::{self, SessionStatus, TradeSession};

    /// Starts live search: the global the panel's "Следить" drives, and the task that turns what
    /// the sockets hear into cards -- handed back as the channel the trade overlay reads. Needs
    /// the session's globals (`session::init`). Watches end with the session (signed out, or
    /// refused) and with the app.
    pub fn init(
        app: &Entity<PriceCheckApp>,
        user_agent: &'static str,
        cx: &mut App,
    ) -> Receiver<LiveCard> {
        let (events_tx, events) = async_channel::unbounded();
        let (cards_tx, cards) = async_channel::unbounded();
        let session = cx.global::<TradeSession>().clone();
        cx.set_global(LiveSearches::new(session, user_agent, events_tx));
        // The panel's toggle and count follow the watches and the session.
        let panel = app.downgrade();
        cx.observe_global::<LiveSearches>(move |cx| {
            panel.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
        let panel = app.downgrade();
        cx.observe_global::<SessionStatus>(move |cx| {
            if !cx.global::<SessionStatus>().signed_in() && cx.global::<LiveSearches>().count() > 0
            {
                cx.global_mut::<LiveSearches>().stop_all();
            }
            panel.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
        cx.on_app_quit(|cx| {
            cx.global_mut::<LiveSearches>().stop_all();
            async {}
        })
        .detach();
        let panel = app.downgrade();
        cx.spawn(async move |cx| deliver(panel, events, cards_tx, cx).await)
            .detach();
        cards
    }

    /// Fetches the listings the sockets report and hands them on as cards, with the watches the
    /// site ended; until the app closes.
    async fn deliver(
        panel: WeakEntity<PriceCheckApp>,
        events: Receiver<LiveEvent>,
        cards: Sender<LiveCard>,
        cx: &mut AsyncApp,
    ) {
        while let Ok(first) = events.recv().await {
            // Everything heard meanwhile too: one fetch per watch for a burst.
            let mut listed: Vec<(u64, Vec<String>)> = Vec::new();
            let heard = std::iter::once(first).chain(std::iter::from_fn(|| events.try_recv().ok()));
            for event in heard {
                match event {
                    LiveEvent::Listed { watch, ids } => {
                        match listed.iter_mut().find(|(listed, _)| *listed == watch) {
                            Some((_, all)) => all.extend(ids),
                            None => listed.push((watch, ids)),
                        }
                    }
                    LiveEvent::Ended { watch, end } => {
                        cx.update(|cx| end_watch(watch, end, &cards, cx));
                    }
                }
            }
            for (watch, ids) in listed {
                // Stopped meanwhile: nobody wants its listings any more.
                let Some(search) =
                    cx.update(|cx| cx.global::<LiveSearches>().search(watch).cloned())
                else {
                    continue;
                };
                let Some(view) = panel.upgrade() else {
                    return;
                };
                let newest = &ids[ids.len().saturating_sub(SHOWN_LISTINGS)..];
                let fetched = price_check::fetch_new_listings(
                    &view,
                    cx,
                    search.site,
                    &search.query_id,
                    newest,
                )
                .await;
                match fetched {
                    Ok(items) => {
                        log::info!(
                            "live search {watch}: {} new, {} fetched",
                            ids.len(),
                            items.len()
                        );
                        for item in items {
                            let _ =
                                cards.try_send(LiveCard::Listing(LiveListing::new(&search, item)));
                        }
                    }
                    Err(err) => {
                        log::warn!("live search {watch}: fetching the new listings failed: {err:#}")
                    }
                }
            }
        }
    }

    /// Watch `watch` was refused for good: it ends, the player is told why, and a refused session
    /// ends every other watch with it.
    fn end_watch(watch: u64, end: LiveEnd, cards: &Sender<LiveCard>, cx: &mut App) {
        let Some(search) = cx.global_mut::<LiveSearches>().end(watch) else {
            return;
        };
        log::warn!("live search {watch}: ended ({end:?})");
        let reason = if end == LiveEnd::Unauthorized {
            session::refused(cx);
            "сайт не принял вход — войдите на pathofexile.com заново и вставьте новый POESESSID \
             в настройках"
        } else {
            "поиска больше нет на сайте — повторите его и включите слежение снова"
        };
        let _ = cards.try_send(LiveCard::Ended {
            label: search.label,
            reason,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread::JoinHandle;

    use async_channel::Receiver;
    use tungstenite::handshake::server::{Request, Response};

    use super::*;

    const SESSION: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn retries_wait_twice_as_long_each_time_up_to_five_minutes() {
        let mut backoff = Backoff::default();
        let waits: Vec<u64> = (0..8).map(|_| backoff.next_wait().as_secs()).collect();
        assert_eq!(waits, [5, 10, 20, 40, 80, 160, 300, 300]);
    }

    #[test]
    fn stopping_ends_a_wait_between_tries_at_once() {
        let stop = Arc::new(Stop::default());
        let stopper = {
            let stop = stop.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(50));
                stop.stop();
            })
        };
        let started = Instant::now();
        assert!(stop.sleep(Duration::from_secs(60)));
        assert!(started.elapsed() < Duration::from_secs(10));
        stopper.join().unwrap();
    }

    /// A socket for watch 7 to a local stand-in for the trade site at `port`.
    fn local_socket(port: u16) -> (Socket, Receiver<LiveEvent>, Arc<Stop>) {
        let (events_tx, events) = async_channel::unbounded();
        let stop = Arc::new(Stop::default());
        let socket = Socket {
            id: 7,
            url: format!("ws://127.0.0.1:{port}/api/trade2/live/poe2/Standard/Q1"),
            origin: "https://www.pathofexile.com",
            session: TradeSession::new(Some(SESSION.to_owned())),
            user_agent: "PoE2 Oracle test",
            events: events_tx,
            stop: stop.clone(),
        };
        (socket, events, stop)
    }

    fn next_event(events: &Receiver<LiveEvent>) -> LiveEvent {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(event) = events.try_recv() {
                return event;
            }
            assert!(Instant::now() < deadline, "the socket reported nothing");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn finishes(thread: JoinHandle<()>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !thread.is_finished() {
            assert!(
                Instant::now() < deadline,
                "the socket's thread kept running"
            );
            thread::sleep(Duration::from_millis(10));
        }
        thread.join().unwrap();
    }

    #[test]
    // tungstenite's handshake callback hands its refusal back by value, however large.
    #[allow(clippy::result_large_err)]
    fn the_socket_carries_the_session_and_reports_new_listings() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let site = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut socket = tungstenite::accept_hdr(stream, |request: &Request, response| {
                for name in ["cookie", "origin", "user-agent"] {
                    let value = request
                        .headers()
                        .get(name)
                        .map(|value| value.to_str().unwrap());
                    seen.push(value.unwrap_or_default().to_owned());
                }
                Ok(response)
            })
            .unwrap();
            socket.send(Message::text(r#"{"auth": true}"#)).unwrap();
            socket
                .send(Message::text(r#"{"new": ["a1", "b2"], "x": 1}"#))
                .unwrap();
            // Held open until the watch goes.
            while socket.read().is_ok() {}
            seen
        });
        let (socket, events, stop) = local_socket(port);
        let watch = thread::spawn(move || socket.run());

        match next_event(&events) {
            LiveEvent::Listed { watch: 7, ids } => assert_eq!(ids, ["a1", "b2"]),
            other => panic!("expected the new listings, got {other:?}"),
        }
        stop.stop();
        finishes(watch);
        assert_eq!(
            site.join().unwrap(),
            [
                format!("POESESSID={SESSION}"),
                "https://www.pathofexile.com".to_owned(),
                "PoE2 Oracle test".to_owned(),
            ]
        );
    }

    #[test]
    #[allow(clippy::result_large_err)]
    fn a_refused_handshake_ends_the_watch_for_good() {
        for (status, end) in [(401, LiveEnd::Unauthorized), (404, LiveEnd::SearchGone)] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let site = thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                let refused = tungstenite::accept_hdr(stream, |_: &Request, _: Response| {
                    Err(Response::builder().status(status).body(None).unwrap())
                });
                assert!(refused.is_err());
            });
            let (socket, events, _stop) = local_socket(port);
            let watch = thread::spawn(move || socket.run());

            match next_event(&events) {
                LiveEvent::Ended {
                    watch: 7,
                    end: ended,
                } => assert_eq!(ended, end),
                other => panic!("expected the watch to end, got {other:?}"),
            }
            // No retry: the thread is done by itself.
            finishes(watch);
            site.join().unwrap();
        }
    }
}
