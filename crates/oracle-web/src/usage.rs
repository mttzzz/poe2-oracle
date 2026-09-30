//! Which counters a request moves. The handlers say what happened -- the app opened a stream or
//! checked for updates, a page was served, an installer went out -- and this decides what it counts
//! as, in the background and in one write to the store: no answer waits on a count.
//!
//! The app is a request whose User-Agent is its own ([`AppAgent`]); everything else is a visitor.
//! Every name made of a version goes through [`Version::bound`] with the versions of the newest
//! published releases, so a request can't make counters up. A developer's build says so on its
//! first connection ([`Start::dev`]), and that start is counted as nothing but a developer's
//! start.

use std::sync::Arc;

use axum::http::HeaderMap;

use crate::App;
use crate::agent::{AppAgent, Start, Version};
use crate::distinct::{Uniq, Who};
use crate::moscow;
use crate::stats::{Source, Stat, Versioned};
use crate::store::Write;

/// The app opened an event stream, or was turned away for want of a place (`opened` false),
/// with the `start` its request reported -- none on a reconnection.
///
/// The install counts as active whether or not it got a place; the stream counts only when it did,
/// as [`Stat::EventStream`] does. The start's flags count either way: a start is the app's fact,
/// not the stream's.
pub fn app_stream(app: &Arc<App>, who: Who, agent: AppAgent, start: Option<Start>, opened: bool) {
    let app = app.clone();
    tokio::spawn(async move {
        let now = moscow::now();
        let writes = stream_writes(&app, &who, &agent, start.as_ref(), opened, now).await;
        app.store.record(&writes, now).await;
    });
}

async fn stream_writes(
    app: &App,
    who: &Who,
    agent: &AppAgent,
    start: Option<&Start>,
    opened: bool,
    now: i64,
) -> Vec<Write> {
    if start.is_some_and(|start| start.dev) {
        return vec![Stat::AppStartDev.write(now)];
    }
    let known = app.releases.known_versions().await;
    let version = Version::bound(agent.version(), &known);
    let mut writes = active_writes(app, who, &version, now).await;
    if opened {
        writes.push(Stat::AppConn.write(now));
        writes.push(Versioned::AppConn(version.clone()).write(now));
    }
    if let Some(start) = start {
        writes.push(Stat::AppStart.write(now));
        writes.push(Stat::AppStartLang(start.lang).write(now));
        if start.first {
            writes.push(Stat::InstallNew.write(now));
        }
        if let (Some(from), Some(to)) = (start.from.as_deref(), agent.version())
            && update_taken(from, to)
        {
            let from = Version::bound(Some(from), &known);
            writes.push(Versioned::UpdateApplied { from, to: version }.write(now));
        }
    }
    writes
}

/// Whether a start on `to` right after one on `from` is an update: both are versions and `to` is
/// the newer. Going back to an older one isn't.
fn update_taken(from: &str, to: &str) -> bool {
    match (semver::Version::parse(from), semver::Version::parse(to)) {
        (Ok(from), Ok(to)) => from.cmp_precedence(&to).is_lt(),
        _ => false,
    }
}

/// The app asked something -- checked for updates -- or is still holding a stream as a new day
/// begins ([`crate::events`]): its install counts as active in the day and the week.
pub fn app_active(app: &Arc<App>, who: Who, agent: AppAgent) {
    app_active_at(app, who, agent, moscow::now());
}

/// [`app_active`] at `now`, which is the clock's, or a test's.
pub fn app_active_at(app: &Arc<App>, who: Who, agent: AppAgent, now: i64) {
    let app = app.clone();
    tokio::spawn(async move {
        let writes = active(&app, &who, &agent, now).await;
        app.store.record(&writes, now).await;
    });
}

async fn active(app: &App, who: &Who, agent: &AppAgent, now: i64) -> Vec<Write> {
    let known = app.releases.known_versions().await;
    let version = Version::bound(agent.version(), &known);
    active_writes(app, who, &version, now).await
}

/// An active install's share of the distinct counts: the day's, the week's and its version's.
/// None of them when the store can't give the salts.
async fn active_writes(app: &App, who: &Who, version: &Version, now: i64) -> Vec<Write> {
    let mut writes = Vec::new();
    if let Some(salt) = app
        .salts
        .of(&app.store, Uniq::AppDay.period(now), now)
        .await
    {
        let install = who.install(&salt);
        writes.push(Uniq::AppDay.write(install, now));
        writes.push(Uniq::AppDayVersion(version.clone()).write(install, now));
    }
    if let Some(salt) = app
        .salts
        .of(&app.store, Uniq::AppWeek.period(now), now)
        .await
    {
        writes.push(Uniq::AppWeek.write(who.install(&salt), now));
    }
    writes
}

/// A page of the site was served to a GET, from a link tagged with `source` or not.
pub fn page_loaded(app: &Arc<App>, who: Who, source: Option<Source>) {
    let app = app.clone();
    tokio::spawn(async move {
        let now = moscow::now();
        let writes = page_writes(&app, &who, source, now).await;
        app.store.record(&writes, now).await;
    });
}

async fn page_writes(app: &App, who: &Who, source: Option<Source>, now: i64) -> Vec<Write> {
    let mut writes = vec![Stat::PageView.write(now)];
    writes.extend(source.map(|source| Stat::Visit(source).write(now)));
    if let Some(salt) = app
        .salts
        .of(&app.store, Uniq::SiteDay.period(now), now)
        .await
    {
        let visitor = who.visitor(&salt);
        writes.push(Uniq::SiteDay.write(visitor, now));
        writes.extend(source.map(|source| Uniq::SiteDayFrom(source).write(visitor, now)));
    }
    writes
}

/// The app's installer, release `version`, went out to a request with `headers` and `query`: to
/// the updater when its User-Agent is the app's, else to whoever pressed a link, the site's
/// button or another -- which counts, by the landing tag the query carries, where they came from.
pub fn installer_downloaded(
    app: &Arc<App>,
    headers: &HeaderMap,
    version: &str,
    query: Option<&str>,
) {
    let by_updater = AppAgent::of(headers).is_some();
    let source = query.and_then(Source::in_query);
    let (app, version) = (app.clone(), version.to_owned());
    tokio::spawn(async move {
        let now = moscow::now();
        let writes = installer_writes(&app, by_updater, &version, source, now).await;
        app.store.record(&writes, now).await;
    });
}

async fn installer_writes(
    app: &App,
    by_updater: bool,
    version: &str,
    source: Option<Source>,
    now: i64,
) -> Vec<Write> {
    if by_updater {
        let known = app.releases.known_versions().await;
        let version = Version::bound(Some(version), &known);
        return vec![
            Stat::DownloadUpdate.write(now),
            Versioned::DownloadUpdate(version).write(now),
        ];
    }
    let mut writes = vec![Stat::DownloadSite.write(now)];
    writes.extend(source.map(|source| Stat::DownloadSiteFrom(source).write(now)));
    writes
}

#[cfg(test)]
mod tests {
    use crate::Config;
    use crate::moscow::{DAY_SECS, Day};
    use crate::stats::{Snapshot, snapshots};

    use super::*;

    /// 2026-09-26 12:00 in Moscow.
    const NOON: i64 = 1_790_413_200;

    /// The service that has listed releases `tags`, a dry run otherwise.
    fn app_knowing(tags: &[&str]) -> Arc<App> {
        let app = App::new(Config::default()).unwrap();
        app.releases.listed(&crate::releases::published(tags));
        app
    }

    fn who(client: &str) -> Who {
        Who::test(client, "PoE2-Oracle/0.1.3")
    }

    fn app_at(version: &str) -> AppAgent {
        let mut headers = HeaderMap::new();
        let agent = format!("PoE2-Oracle/{version}");
        headers.insert("user-agent", agent.parse().unwrap());
        AppAgent::of(&headers).unwrap()
    }

    fn start(query: &str) -> Start {
        Start::of(Some(query)).unwrap()
    }

    /// What `writes` did, as a day's counts read back.
    async fn counted(app: &App, writes: &[Write], now: i64) -> Snapshot {
        app.store.record(writes, now).await;
        snapshots(&app.store, &[Day::of(now)], now)
            .await
            .unwrap()
            .remove(0)
    }

    #[tokio::test]
    async fn an_install_is_active_once_a_day_and_an_update_does_not_make_it_two() {
        let app = app_knowing(&["v0.1.3", "v0.1.2"]);
        let (old, new) = (app_at("0.1.2"), app_at("0.1.3"));
        let mut writes = Vec::new();
        // The same install asks four times, on the old version and then on the new one; another
        // one on the new version, behind another address; a third on a version never released.
        for (client, agent) in [
            ("203.0.113.1", &old),
            ("203.0.113.1", &old),
            ("203.0.113.1", &new),
            ("203.0.113.1", &new),
            ("203.0.113.2", &new),
            ("203.0.113.3", &app_at("9.9.9")),
        ] {
            writes.extend(active(&app, &who(client), agent, NOON).await);
        }
        let today = counted(&app, &writes, NOON).await;
        assert_eq!(today.get("uniq_app_day"), 3, "three addresses");
        assert_eq!(today.get("uniq_app_day_v_0.1.2"), 1);
        assert_eq!(
            today.get("uniq_app_day_v_0.1.3"),
            2,
            "an update is in both versions"
        );
        assert_eq!(today.get("uniq_app_day_v_other"), 1);
        // Nothing was opened or started: asking is not connecting.
        assert_eq!(today.get("app_conn"), 0);
        assert_eq!(today.get("app_start"), 0);
    }

    #[tokio::test]
    async fn the_week_counts_an_install_once_however_many_days_it_was_active() {
        let app = app_knowing(&["v0.1.3"]);
        let mut writes = Vec::new();
        // Monday to Saturday of one ISO week, one install every day, another on Friday only.
        let monday = Day::of(NOON).week().monday().start() + 3600;
        for day in 0..6 {
            writes.extend(
                active(
                    &app,
                    &who("203.0.113.1"),
                    &app_at("0.1.3"),
                    monday + day * DAY_SECS,
                )
                .await,
            );
        }
        writes.extend(
            active(
                &app,
                &who("203.0.113.2"),
                &app_at("0.1.3"),
                monday + 4 * DAY_SECS,
            )
            .await,
        );
        app.store.record(&writes, NOON).await;
        let key = crate::distinct::week_key(Day::of(NOON).week());
        assert_eq!(app.store.distinct_values(&[key], NOON).await, Some(vec![2]));
    }

    #[tokio::test]
    async fn a_stream_counts_as_a_connection_only_when_it_opened() {
        let app = app_knowing(&["v0.1.3"]);
        let install = who("203.0.113.1");
        let opened = stream_writes(&app, &install, &app_at("0.1.3"), None, true, NOON).await;
        let refused = stream_writes(
            &app,
            &who("203.0.113.2"),
            &app_at("0.1.3"),
            None,
            false,
            NOON,
        )
        .await;
        let today = counted(&app, &[opened, refused].concat(), NOON).await;
        assert_eq!(today.get("app_conn"), 1);
        assert_eq!(today.get("app_conn_v_0.1.3"), 1);
        assert_eq!(
            today.get("uniq_app_day"),
            2,
            "the refused install is active too"
        );
        // A reconnection carries no start.
        assert_eq!(today.get("app_start"), 0);
    }

    /// The writes of a start whose request said `query`, from `client`, running `running`.
    async fn started(app: &App, query: &str, running: &str, client: &str) -> Vec<Write> {
        let (agent, start) = (app_at(running), start(query));
        stream_writes(app, &who(client), &agent, Some(&start), true, NOON).await
    }

    #[tokio::test]
    async fn a_start_counts_its_flags_once_checked() {
        let app = app_knowing(&["v0.1.3", "v0.1.2"]);
        let mut writes = Vec::new();
        // A new install, in Russian; an update from 0.1.2 to 0.1.3 in English; an existing install
        // that only restarted; one whose language is none of the two.
        for (query, client) in [
            ("start=1&first=1&lang=ru", "203.0.113.1"),
            ("start=1&from=0.1.2&lang=en", "203.0.113.2"),
            ("start=1&lang=ru", "203.0.113.3"),
            ("start=1&lang=de", "203.0.113.4"),
        ] {
            writes.extend(started(&app, query, "0.1.3", client).await);
        }
        let today = counted(&app, &writes, NOON).await;
        assert_eq!(today.get("app_start"), 4);
        assert_eq!(today.get("install_new"), 1);
        assert_eq!(today.get("update_applied_0.1.2_0.1.3"), 1);
        assert_eq!(today.sum("update_applied_"), 1);
        assert_eq!(today.get("app_start_lang_ru"), 2);
        assert_eq!(today.get("app_start_lang_en"), 1);
        assert_eq!(today.get("app_start_lang_other"), 1);
        assert_eq!(today.get("app_conn"), 4, "each start opened a stream");
        assert_eq!(today.get("uniq_app_day"), 4);
    }

    #[tokio::test]
    async fn only_a_real_update_between_known_releases_counts_as_one() {
        let app = app_knowing(&["v0.1.3", "v0.1.2"]);
        let mut writes = Vec::new();
        // (the version the last start ran, the version running now)
        for (from, running) in [
            // Not older: the same version again, a step back.
            ("0.1.3", "0.1.3"),
            ("0.1.3", "0.1.2"),
            // From a release the service no longer knows, and to one it doesn't yet.
            ("0.0.9", "0.1.3"),
            ("0.1.2", "0.1.4"),
            // The app names no version of its own.
            ("0.1.2", "0.2.0-rc.1"),
        ] {
            let agent = if running.contains('-') {
                let mut headers = HeaderMap::new();
                headers.insert(
                    "user-agent",
                    format!("PoE2-Oracle/{running}").parse().unwrap(),
                );
                AppAgent::of(&headers).unwrap()
            } else {
                app_at(running)
            };
            let flags = start(&format!("start=1&from={from}"));
            writes.extend(
                stream_writes(&app, &who("203.0.113.1"), &agent, Some(&flags), true, NOON).await,
            );
        }
        let today = counted(&app, &writes, NOON).await;
        assert_eq!(today.get("app_start"), 5);
        assert_eq!(today.get("update_applied_other_0.1.3"), 1);
        assert_eq!(today.get("update_applied_0.1.2_other"), 1);
        assert_eq!(today.sum("update_applied_"), 2, "the rest were no update");
    }

    #[tokio::test]
    async fn a_developers_start_counts_as_nothing_but_a_developers_start() {
        let app = app_knowing(&["v0.1.3"]);
        let flags = start("start=1&first=1&from=0.1.2&lang=ru&dev=1");
        let writes = stream_writes(
            &app,
            &who("203.0.113.1"),
            &app_at("0.1.3"),
            Some(&flags),
            true,
            NOON,
        )
        .await;
        let today = counted(&app, &writes, NOON).await;
        assert_eq!(today.get("app_start_dev"), 1);
        for name in [
            "app_start",
            "install_new",
            "app_conn",
            "uniq_app_day",
            "app_start_lang_ru",
            "app_conn_v_0.1.3",
            "uniq_app_day_v_0.1.3",
        ] {
            assert_eq!(today.get(name), 0, "{name}");
        }
        assert_eq!(today.sum("update_applied_"), 0);
        // Not even the week's install count.
        let key = crate::distinct::week_key(Day::of(NOON).week());
        assert_eq!(app.store.distinct_values(&[key], NOON).await, Some(vec![0]));
    }

    #[tokio::test]
    async fn without_a_known_release_every_version_counts_as_other() {
        let app = app_knowing(&[]);
        let writes = stream_writes(
            &app,
            &who("203.0.113.1"),
            &app_at("0.1.3"),
            None,
            true,
            NOON,
        )
        .await;
        let today = counted(&app, &writes, NOON).await;
        assert_eq!(today.get("app_conn_v_other"), 1);
        assert_eq!(today.get("uniq_app_day_v_other"), 1);
        assert_eq!(today.get("app_conn_v_0.1.3"), 0);
    }

    #[tokio::test]
    async fn the_installer_goes_to_the_site_or_to_the_updater_and_the_tag_only_counts_for_the_site()
    {
        let app = app_knowing(&["v0.1.3"]);
        let mut writes = Vec::new();
        // Two button presses from Reddit and one from a tag nobody published, one from no tag; the
        // updater fetching the release, and once more with a tag it never has.
        for (by_updater, version, source) in [
            (false, "0.1.3", Source::in_query("from=reddit")),
            (false, "0.1.3", Source::in_query("x=1&from=reddit")),
            (false, "0.1.3", Source::in_query("from=evil")),
            (false, "0.1.3", None),
            (true, "0.1.3", None),
            (true, "0.1.3", Source::in_query("from=reddit")),
            (true, "9.9.9", None),
        ] {
            writes.extend(installer_writes(&app, by_updater, version, source, NOON).await);
        }
        let today = counted(&app, &writes, NOON).await;
        assert_eq!(today.get("download_site"), 4);
        assert_eq!(today.get("download_site_from_reddit"), 2);
        assert_eq!(today.sum("download_site_from_"), 2);
        assert_eq!(today.get("download_update"), 3);
        assert_eq!(today.get("download_update_v_0.1.3"), 2);
        assert_eq!(today.get("download_update_v_other"), 1);
        assert_eq!(today.get("download"), 0, "the old counter is the handler's");
    }

    #[tokio::test]
    async fn a_page_counts_its_view_and_its_visitor_and_a_tagged_one_its_tag() {
        let app = app_knowing(&[]);
        let agent = "Mozilla/5.0 Firefox/130.0";
        let mut writes = Vec::new();
        // One visitor loads three pages, the last from a link tagged Reddit; another loads one.
        writes.extend(page_writes(&app, &Who::test("203.0.113.1", agent), None, NOON).await);
        writes.extend(page_writes(&app, &Who::test("203.0.113.1", agent), None, NOON + 60).await);
        writes.extend(
            page_writes(
                &app,
                &Who::test("203.0.113.1", agent),
                Some(Source::Reddit),
                NOON + 120,
            )
            .await,
        );
        writes.extend(page_writes(&app, &Who::test("203.0.113.2", agent), None, NOON + 180).await);
        let today = counted(&app, &writes, NOON).await;
        assert_eq!(today.get("page_view"), 4);
        assert_eq!(today.get("uniq_site_day"), 2);
        assert_eq!(today.get("visit_reddit"), 1);
        assert_eq!(today.get("uniq_site_day_from_reddit"), 1);
        assert_eq!(today.get("uniq_site_day_from_forum"), 0);
    }
}
