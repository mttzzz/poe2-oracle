//! Keeping the app current by itself, while the player allows it (`Settings::check_updates`,
//! «Обновлять автоматически»). The updater stays connected to the app's service at
//! oracle.pushka.biz (`auto_update::events`), which announces the latest app and game data
//! versions as they come out, and fetches whichever is newer (`update_rules::next_fetch`): the
//! app's signed installer, or a signed game data pack, unpacked for the next start (`data_pack`).
//!
//! A fetched update goes in at once -- the installer replaces the app and starts it again; a pack
//! takes a restart of the app (`instance::relaunch`) -- but never while one of the app's windows
//! is up or a hotkey's work is under way (`update_rules::Showing`): the restart waits for them to
//! be over. It leaves a marker for the next start (`update_rules::Marker`), which says on a plate
//! over the game that the update went in (`ui::toast`), and the XP overlay's tracker, which the
//! next start carries on with (`ui::xp_overlay::carry_over`). A failed fetch is tried again later,
//! quietly; a failed start -- the update kept fetched -- a few times, after waits of its own
//! (`update_rules::Failures`). A version whose update didn't take, or wouldn't start, is left
//! alone until the next launch.
//!
//! The connection starts a few seconds after launch, once the trade catalogs are in, and comes
//! back by itself when it drops: after its backoff, or at once when Windows says the Internet is
//! back (`platform::network`). The settings window's «Обновления» say what the updater does
//! ([`UpdateStatus`]), and «Проверить сейчас» ([`check_now`]) cuts its wait short. Turning the
//! setting off drops the connection and whatever was fetched.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, ensure};
use auto_update::Version;
use auto_update::events::{self, Link};
use gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, Global, Subscription, Task, WeakEntity,
};
use http_client::HttpClient;
use oracle_protocol::{DataVersion, EVENTS_PATH, Versions};
use reqwest_client::ReqwestClient;

use crate::data_pack;
use crate::login::Login;
use crate::paths;
use crate::platform::instance;
use crate::platform::network::{self, NetworkWatch};
use crate::price_check::{BootstrapState, PriceCheckApp};
use crate::tr;
use crate::ui::report_view::ReportView;
use crate::ui::{toast, tour, xp_overlay};
use crate::update_rules::{self, Failures, Marker, Outcome, Retry, Showing, Target};

/// How long after launch the connection starts, at the earliest: the price catalogs load first,
/// since they matter more.
const START_DELAY: Duration = Duration::from_secs(10);
/// How often the start looks again whether the catalogs are in, once [`START_DELAY`] is over.
const CATALOG_POLL: Duration = Duration::from_secs(2);
/// The longest reason a failure is shown with in the settings window, in characters.
const MAX_REASON_CHARS: usize = 160;

/// What the updater is doing, as the settings window's «Обновления» say it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStatus {
    /// «Обновлять автоматически» is on.
    pub on: bool,
    pub link: LinkState,
    /// The service's versions came, and nothing newer is being fetched, waits or failed.
    pub up_to_date: bool,
    /// The update under way, if any.
    pub work: Option<Work>,
}

impl Global for UpdateStatus {}

/// The connection to the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// Not connected yet: the first attempt comes a few seconds after launch.
    Connecting,
    Connected,
    /// It failed or dropped: tried again later, and at once when the Internet is back.
    Offline,
}

/// What the updater does about an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Work {
    /// Downloading it.
    Fetching(Target),
    /// Fetched: it goes in once the app's windows are closed.
    Waiting(Target),
    /// The last try failed, for this reason: tried again later.
    Failed(Target, String),
}

/// The running updater, for [`check_now`].
struct Running(Entity<Updater>);

impl Global for Running {}

/// The update fetched and waiting for the restart.
enum Ready {
    App {
        version: Version,
        installer: PathBuf,
    },
    /// The pack is unpacked for the next start already.
    Data(DataVersion),
}

impl Ready {
    fn target(&self) -> Target {
        match self {
            Ready::App { version, .. } => Target::App(version.clone()),
            Ready::Data(version) => Target::Data(*version),
        }
    }
}

/// Why a fetch didn't bring an update.
enum FetchError {
    /// Something on the way failed -- the network, the service, a signature: tried again later.
    Failed(anyhow::Error),
    /// The data pack arrived but this app can't use it: left alone until the next launch, since
    /// fetching it again would bring the same pack.
    Refused(anyhow::Error),
}

/// The connection while it's kept: dropping it ends the stream, its listener and the network
/// watch.
struct Following {
    wake: async_channel::Sender<()>,
    _stream: Task<()>,
    _listener: Task<()>,
    _network: Option<NetworkWatch>,
}

struct Updater {
    app: WeakEntity<PriceCheckApp>,
    /// The app's HTTP client, for the release answers and the downloads: its read timeout cuts a
    /// stalled one short.
    client: Arc<dyn HttpClient>,
    running: Version,
    /// The setting, as last seen.
    on: bool,
    /// [`START_DELAY`] is over and the catalogs are in: the connection may start.
    started: bool,
    following: Option<Following>,
    link: LinkState,
    /// The service's last word.
    latest: Option<Versions>,
    fetching: Option<(Target, Task<()>)>,
    ready: Option<Ready>,
    /// The app is quitting for an update.
    restarting: bool,
    /// The last failure, until something newer happens.
    failure: Option<(Target, String)>,
    /// The failed fetches in a row, and the failed starts of each version.
    failures: Failures,
    retry: Option<Task<()>>,
    /// The wait before the fetched update is started again, after its start failed: it holds the
    /// restart until it's over.
    start_retry: Option<Task<()>>,
    /// Versions whose update the last run found didn't take, whose pack this app refused, or
    /// whose fetched update wouldn't start: fetched again only at the next launch.
    left_alone: Vec<Target>,
    _subscriptions: Vec<Subscription>,
}

/// Starts the updater: the plate an update left for this start, then -- [`START_DELAY`] on, once
/// the catalogs are in -- the connection, while the setting allows it. Call once, at launch.
pub fn init(app: &Entity<PriceCheckApp>, cx: &mut App) {
    let running = Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is semver");
    let mut left_alone = Vec::new();
    let mut failure = None;
    if let Some(marker) = Marker::take(&paths::update_marker_file()) {
        match marker.outcome(&running, data_pack::active_version()) {
            Outcome::Updated(target) => {
                log::info!("updated to {}", describe(&target));
                let text = match &target {
                    Target::App(version) => {
                        tr!("PoE2 Oracle updated to {version}", version = version)
                    }
                    Target::Data(_) => tr!("Game data updated").to_owned(),
                };
                let ui_scale = app.read(cx).settings.ui_scale;
                toast::show(text, app.downgrade(), ui_scale, cx);
            }
            Outcome::Failed(target) => {
                log::warn!(
                    "the update to {} didn't take: left alone until the next launch",
                    describe(&target)
                );
                let reason = match target {
                    Target::App(_) => tr!("the new version didn't start"),
                    Target::Data(_) => tr!("the new data didn't load"),
                };
                failure = Some((target.clone(), reason.to_owned()));
                left_alone.push(target);
            }
            Outcome::Passed => {}
        }
    }
    let client = cx.http_client();
    let updater = cx.new(|cx| {
        let mut updater = Updater::new(app, client, running, cx);
        updater.left_alone = left_alone;
        updater.failure = failure;
        updater
    });
    updater.update(cx, |updater, cx| updater.publish(cx));
    cx.set_global(Running(updater.clone()));
    let this = updater.downgrade();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(START_DELAY).await;
        while catalogs_loading(&this, cx) {
            cx.background_executor().timer(CATALOG_POLL).await;
        }
        this.update(cx, |updater, cx| updater.start(cx)).ok();
    })
    .detach();
}

/// «Проверить сейчас»: the connection's wait for its next attempt ends at once, a fetch that
/// failed goes again now, and a start that failed as soon as the app's windows let it.
pub fn check_now(cx: &mut App) {
    if let Some(updater) = cx.try_global::<Running>().map(|running| running.0.clone()) {
        updater.update(cx, |updater, cx| updater.check_now(cx));
    }
}

/// Whether the trade catalogs are still loading -- `false` once the updater or the app is gone.
fn catalogs_loading(updater: &WeakEntity<Updater>, cx: &mut AsyncApp) -> bool {
    updater
        .read_with(cx, |updater, cx| {
            updater
                .app
                .upgrade()
                .is_some_and(|app| matches!(app.read(cx).bootstrap, BootstrapState::Loading))
        })
        .unwrap_or(false)
}

impl Updater {
    fn new(
        app: &Entity<PriceCheckApp>,
        client: Arc<dyn HttpClient>,
        running: Version,
        cx: &mut Context<Self>,
    ) -> Updater {
        let this = cx.weak_entity();
        // A fetched update waits for the app's windows and a hotkey's work: each of these may say
        // the last of them is over.
        let subscriptions = vec![
            cx.observe(app, |updater, app, cx| updater.app_changed(&app, cx)),
            cx.observe_global::<Login>(|updater, cx| updater.try_apply(cx)),
            cx.on_window_closed(move |cx, _| {
                this.update(cx, |updater, cx| updater.try_apply(cx)).ok();
            }),
        ];
        Updater {
            app: app.downgrade(),
            client,
            running,
            on: app.read(cx).settings.check_updates,
            started: false,
            following: None,
            link: LinkState::Connecting,
            latest: None,
            fetching: None,
            ready: None,
            restarting: false,
            failure: None,
            failures: Failures::default(),
            retry: None,
            start_retry: None,
            left_alone: Vec::new(),
            _subscriptions: subscriptions,
        }
    }

    /// The wait after launch is over: connects, while the setting allows it.
    fn start(&mut self, cx: &mut Context<Self>) {
        if self.started {
            return;
        }
        self.started = true;
        if self.on {
            self.connect(cx);
        }
        self.publish(cx);
    }

    /// The app changed: the setting may have been turned on or off, a window closed, or a
    /// hotkey's work ended.
    fn app_changed(&mut self, app: &Entity<PriceCheckApp>, cx: &mut Context<Self>) {
        let on = app.read(cx).settings.check_updates;
        if on != self.on {
            self.on = on;
            if !on {
                self.stop();
            } else if self.started {
                self.connect(cx);
            }
            self.publish(cx);
        }
        self.try_apply(cx);
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        // No read timeout: the stream is quiet between the service's pings, and the follower
        // itself drops one that has heard nothing for too long.
        let client: Arc<dyn HttpClient> =
            match ReqwestClient::proxy_and_user_agent(None, crate::brand::USER_AGENT) {
                Ok(client) => Arc::new(client),
                Err(err) => {
                    log::warn!("building the client the updates come through failed: {err:#}");
                    return;
                }
            };
        let (links_to, links) = async_channel::unbounded();
        let (wake, woken) = async_channel::bounded(1);
        let stream = cx
            .background_executor()
            .spawn(events::follow_events(client, links_to, woken));
        let listener = cx.spawn(async move |this, cx| {
            while let Ok(link) = links.recv().await {
                if this
                    .update(cx, |updater, cx| updater.on_link(link, cx))
                    .is_err()
                {
                    return;
                }
            }
        });
        let network = network::watch(wake.clone())
            .inspect_err(|err| log::info!("{err:#}"))
            .ok();
        self.following = Some(Following {
            wake,
            _stream: stream,
            _listener: listener,
            _network: network,
        });
        self.link = LinkState::Connecting;
        log::info!(
            "automatic updates on: following {}",
            oracle_protocol::url(EVENTS_PATH)
        );
    }

    /// The setting was turned off: the connection goes, and whatever was fetched with it.
    fn stop(&mut self) {
        log::info!("automatic updates off");
        self.following = None;
        self.link = LinkState::Connecting;
        self.latest = None;
        self.fetching = None;
        self.ready = None;
        self.retry = None;
        self.start_retry = None;
        self.failures.fetches_over();
        self.failure = self
            .failure
            .take()
            .filter(|(target, _)| self.left_alone.contains(target));
    }

    fn on_link(&mut self, link: Link, cx: &mut Context<Self>) {
        match link {
            Link::Connected => {
                log::info!("connected to the update service");
                self.link = LinkState::Connected;
            }
            Link::Versions(versions) => {
                log::info!(
                    "the service's latest: app {}, game data {}",
                    versions.app.as_deref().unwrap_or("none"),
                    versions
                        .data
                        .map_or_else(|| "none".to_owned(), |data| data.to_string())
                );
                self.link = LinkState::Connected;
                self.latest = Some(versions);
                self.consider(cx);
            }
            Link::Disconnected { .. } => self.link = LinkState::Offline,
        }
        self.publish(cx);
    }

    fn check_now(&mut self, cx: &mut Context<Self>) {
        if !self.started {
            self.start(cx);
            return;
        }
        let Some(following) = &self.following else {
            return;
        };
        log::info!("checking for updates now");
        let _ = following.wake.try_send(());
        if self.link == LinkState::Offline {
            self.link = LinkState::Connecting;
        }
        self.retry = None;
        self.start_retry = None;
        self.consider(cx);
        self.publish(cx);
    }

    /// Goes by the service's last word: drops a fetched release it no longer announces, fetches
    /// what's newer -- a failed version at its retry, a newer one at once -- and installs what's
    /// fetched once the windows let it.
    fn consider(&mut self, cx: &mut Context<Self>) {
        let Some(latest) = self.latest.clone() else {
            return;
        };
        if let Some(ready) = &self.ready
            && !update_rules::still_wanted(&ready.target(), &latest)
        {
            log::info!(
                "{} is no longer the latest: not installing it",
                describe(&ready.target())
            );
            self.ready = None;
        }
        if self.fetching.is_none() && !self.restarting {
            let ready = self.ready.as_ref().map(Ready::target);
            let next = update_rules::next_fetch(
                &latest,
                &self.running,
                data_pack::active_version(),
                ready.as_ref(),
                &self.left_alone,
            );
            match next {
                Some(target) => {
                    let failed = self
                        .failure
                        .as_ref()
                        .is_some_and(|(failed, _)| *failed == target);
                    if !(failed && self.retry.is_some()) {
                        self.retry = None;
                        self.fetch(target, cx);
                    }
                }
                // Nothing to fetch: a failure of what's no longer announced is over -- not the
                // failed start of the update still waiting, nor one of a version left alone.
                None => {
                    self.retry = None;
                    self.failures.fetches_over();
                    self.failure = self.failure.take().filter(|(target, _)| {
                        self.left_alone.contains(target) || ready.as_ref() == Some(target)
                    });
                }
            }
        }
        self.try_apply(cx);
    }

    fn fetch(&mut self, target: Target, cx: &mut Context<Self>) {
        log::info!("fetching {}", describe(&target));
        let client = self.client.clone();
        let running = self.running.clone();
        let fetched = target.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = match &fetched {
                Target::App(version) => fetch_app(client, running, version.clone(), cx).await,
                Target::Data(version) => fetch_data(client, *version, cx).await,
            };
            this.update(cx, |updater, cx| updater.fetched(fetched, result, cx))
                .ok();
        });
        self.fetching = Some((target, task));
        self.publish(cx);
    }

    fn fetched(
        &mut self,
        target: Target,
        result: Result<Ready, FetchError>,
        cx: &mut Context<Self>,
    ) {
        self.fetching = None;
        match result {
            Ok(ready) => {
                log::info!(
                    "{} fetched: it goes in once the app's windows are closed",
                    describe(&target)
                );
                // A pack is unpacked for the next start already: that start says so, even when
                // the player quits before the restart this run would make.
                if let Ready::Data(_) = ready {
                    let marker = Marker::new(&target, &self.running, data_pack::active_version());
                    if let Err(err) = marker.save(&paths::update_marker_file()) {
                        log::warn!("{err:#}");
                    }
                }
                self.failures.fetches_over();
                self.failure = None;
                self.ready = Some(ready);
            }
            Err(FetchError::Refused(err)) => {
                log::warn!(
                    "{} can't be used: {err:#} -- left alone until the next launch",
                    describe(&target)
                );
                self.failure = Some((target.clone(), reason(&err)));
                self.left_alone.push(target);
            }
            Err(FetchError::Failed(err)) => {
                let delay = self.failures.fetch_failed();
                log::warn!(
                    "fetching {} failed, trying again in {} min: {err:#}",
                    describe(&target),
                    delay.as_secs() / 60
                );
                self.failure = Some((target, reason(&err)));
                self.retry_in(delay, cx);
            }
        }
        self.consider(cx);
        self.publish(cx);
    }

    fn retry_in(&mut self, delay: Duration, cx: &mut Context<Self>) {
        self.retry = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |updater, cx| {
                updater.retry = None;
                updater.consider(cx);
                updater.publish(cx);
            })
            .ok();
        }));
    }

    /// Installs the fetched update when none of the app's windows is up and no hotkey's work is
    /// under way, and not before a failed start's wait is over; otherwise it waits for the next of
    /// them to end.
    fn try_apply(&mut self, cx: &mut Context<Self>) {
        if self.ready.is_none() || self.restarting || self.start_retry.is_some() {
            return;
        }
        let Some(app) = self.app.upgrade() else {
            return;
        };
        if !showing(&app, cx).quiet() {
            return;
        }
        let Some(ready) = self.ready.take() else {
            return;
        };
        let target = ready.target();
        let marker_file = paths::update_marker_file();
        let marker = Marker::new(&target, &self.running, data_pack::active_version());
        let started = marker.save(&marker_file).and_then(|()| match &ready {
            Ready::App { installer, .. } => auto_update::apply_update(installer),
            Ready::Data(_) => instance::relaunch(),
        });
        match started {
            Ok(()) => {
                log::info!("restarting for {}", describe(&target));
                self.restarting = true;
                xp_overlay::carry_over(cx);
                crate::app::quit(cx);
            }
            Err(err) => {
                // An installer that never ran can't have taken. A pack is installed for the next
                // start already, which says so (`fetched`).
                if let Ready::App { .. } = ready {
                    let _ = std::fs::remove_file(&marker_file);
                }
                self.start_failed(ready, err, cx);
            }
        }
    }

    /// Starting the fetched update `ready` failed: it stays fetched, and only the start goes
    /// again, after a wait of its own -- or, once its version has failed to start
    /// [`update_rules::MAX_FAILED_STARTS`] times, not before the next launch.
    fn start_failed(&mut self, ready: Ready, err: anyhow::Error, cx: &mut Context<Self>) {
        let target = ready.target();
        self.failure = Some((target.clone(), reason(&err)));
        match self.failures.start_failed(&target) {
            Retry::After(delay) => {
                log::warn!(
                    "starting {} failed, trying again in {} min: {err:#}",
                    describe(&target),
                    delay.as_secs() / 60
                );
                self.ready = Some(ready);
                self.start_retry = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(delay).await;
                    this.update(cx, |updater, cx| {
                        updater.start_retry = None;
                        updater.try_apply(cx);
                        updater.publish(cx);
                    })
                    .ok();
                }));
            }
            Retry::NextLaunch => {
                log::warn!(
                    "starting {} failed again: {err:#} -- left alone until the next launch",
                    describe(&target)
                );
                self.left_alone.push(target);
            }
        }
        self.consider(cx);
        self.publish(cx);
    }

    /// Says what the updater does to the settings window, when that changed. A failed start
    /// shows over the update it keeps waiting.
    fn publish(&self, cx: &mut Context<Self>) {
        let ready = self.ready.as_ref().map(Ready::target);
        let work = match (&self.fetching, &self.failure) {
            (Some((target, _)), _) => Some(Work::Fetching(target.clone())),
            (None, Some((target, reason)))
                if ready.as_ref().is_none_or(|ready| ready == target) =>
            {
                Some(Work::Failed(target.clone(), reason.clone()))
            }
            (None, _) => ready.map(Work::Waiting),
        };
        let status = UpdateStatus {
            on: self.on,
            link: self.link,
            up_to_date: self.latest.is_some() && work.is_none(),
            work,
        };
        if cx.try_global::<UpdateStatus>() != Some(&status) {
            cx.set_global(status);
        }
    }
}

/// Asks the service for the release `announced` and downloads its verified installer.
async fn fetch_app(
    client: Arc<dyn HttpClient>,
    running: Version,
    announced: Version,
    cx: &mut AsyncApp,
) -> Result<Ready, FetchError> {
    let dir = paths::updates_dir();
    let update = auto_update::check_for_update(&client, &running, &dir)
        .await
        .and_then(|update| {
            let update = update.context("the service has no release newer than this version")?;
            ensure!(
                update.version.cmp_precedence(&announced).is_eq(),
                "the service offers {} instead of {announced}",
                update.version
            );
            Ok(update)
        })
        .map_err(FetchError::Failed)?;
    let version = update.version.clone();
    let installer = cx
        .background_executor()
        .spawn(async move { auto_update::download_update(&client, &update, &dir).await })
        .await
        .map_err(FetchError::Failed)?;
    Ok(Ready::App { version, installer })
}

/// Asks the service for the data pack `announced`, downloads it verified and unpacks it for the
/// next start. The zip goes once unpacked -- or refused.
async fn fetch_data(
    client: Arc<dyn HttpClient>,
    announced: DataVersion,
    cx: &mut AsyncApp,
) -> Result<Ready, FetchError> {
    let dir = paths::updates_dir();
    let update = auto_update::check_for_data(&client, data_pack::active_version(), &dir)
        .await
        .and_then(|update| {
            let update = update.context("the service has no game data newer than the app's")?;
            ensure!(
                update.version == announced,
                "the service offers game data {} instead of {announced}",
                update.version
            );
            Ok(update)
        })
        .map_err(FetchError::Failed)?;
    cx.background_executor()
        .spawn(async move {
            let zip = auto_update::download_data(&client, &update, &dir)
                .await
                .map_err(FetchError::Failed)?;
            let installed = data_pack::install(&zip).map_err(FetchError::Refused);
            if let Err(err) = std::fs::remove_file(&zip) {
                log::warn!("deleting {} failed: {err}", zip.display());
            }
            installed.map(Ready::Data)
        })
        .await
}

/// The app's windows that are up, and whether a hotkey's work is under way.
fn showing(app: &Entity<PriceCheckApp>, cx: &App) -> Showing {
    let state = app.read(cx);
    Showing {
        panel: state.visible,
        settings: state.settings_window().is_some(),
        report: cx
            .windows()
            .iter()
            .any(|window| window.downcast::<ReportView>().is_some()),
        sign_in: cx.try_global::<Login>().is_some_and(Login::is_open),
        tour: tour::under_way(cx),
        hotkey: state.hotkey_busy,
    }
}

/// An update, as the log names it.
fn describe(target: &Target) -> String {
    match target {
        Target::App(version) => format!("version {version}"),
        Target::Data(version) => format!("game data {version}"),
    }
}

/// Why `err` happened, as the settings window says it: its innermost cause -- what the system or
/// the service said -- cut to [`MAX_REASON_CHARS`].
fn reason(err: &anyhow::Error) -> String {
    let cause = err.root_cause().to_string();
    match cause.char_indices().nth(MAX_REASON_CHARS) {
        Some((end, _)) => format!("{}…", &cause[..end]),
        None => cause,
    }
}
