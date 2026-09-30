//! What the service counts, per Moscow day, and what it reads out of the counts.
//!
//! The counters are `oracle:stat:<name>:<YYYY-MM-DD>`, kept for 120 days after their day, and hold
//! numbers only: no address, no id, nothing about who. Most have a name of their own ([`Stat`],
//! [`Uniq`]); those named for a version ([`Versioned`], [`Uniq::AppDayVersion`]) are also listed by
//! name in `oracle:names:<YYYY-MM-DD>`, where a readout that knows only Redis finds them. The
//! distinct counts -- how many different installs and visitors, not how many times ([`Uniq`]) --
//! are HyperLogLog sketches of hashes under a salt of their period ([`crate::distinct`]). Which
//! request moves which counter is [`crate::usage`]'s. At 09:00 in Moscow the digest
//! ([`crate::digest`]) posts the day before to the owner's Telegram, each number beside the same
//! weekday a week earlier. `oracle-web stats` prints every counter of the last days as JSON
//! ([`read_out`]).

use std::borrow::Cow;
use std::sync::{Arc, LazyLock};

use oracle_protocol::ReportKind;
use serde::ser::SerializeMap as _;
use serde::{Serialize, Serializer};

use crate::App;
use crate::agent::{Lang, Version};
use crate::distinct::{Uniq, week_key};
use crate::moscow::{self, DAY_SECS, Day, Week};
use crate::store::{Named, Store, What, Write};

/// How long a day's counters are kept after it.
pub const KEEP_DAYS: i64 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stat {
    /// The installer, downloaded from the site's button or by the updater: both together, as it
    /// was counted before the two were told apart ([`Stat::DownloadSite`],
    /// [`Stat::DownloadUpdate`]).
    Download,
    /// An event stream opened: the app connects when it starts, and again after losing the
    /// connection.
    EventStream,
    /// An update check: the app's latest release or the latest data pack asked for.
    UpdateCheck,
    /// A data pack, downloaded by the updater.
    DataDownload,
    /// Any other release file: the `SHA256SUMS` of a release and its signature, which only the
    /// updater fetches.
    UpdateDownload,
    Report(ReportKind),
    /// A page of the site loaded from a link tagged with where it was published.
    Visit(Source),
    /// The installer, downloaded by anything but the updater: the site's button, or a link to the
    /// file.
    DownloadSite,
    /// The installer, downloaded by the updater (a User-Agent of the app's).
    DownloadUpdate,
    /// A [`Stat::DownloadSite`] whose request carried the landing tag of a [`Source`]: the site's
    /// script adds it to the download links.
    DownloadSiteFrom(Source),
    /// An event stream opened by the app (a User-Agent of the app's), starts of developer builds
    /// left out.
    AppConn,
    /// A page of the site served: HTML only, no file a page uses, no 404.
    PageView,
    /// The first connection of an app's run ([`crate::agent::Start`]).
    AppStart,
    /// A start that is its installation's first.
    InstallNew,
    /// A start by the interface language it reports: the three sum to [`Stat::AppStart`].
    AppStartLang(Lang),
    /// A start of a build the developer made for testing: counted as this and nothing else.
    AppStartDev,
}

/// Where a link to the site was published. The owner's links carry its tag, `?from=reddit`, which
/// is its counter's name after `visit_`. A tag that isn't one of these counts nothing, so a
/// stranger can't make up counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Reddit,
    /// The pathofexile.com forum.
    Forum,
    Discord,
    YouTube,
    /// Steam's community hub.
    Steam,
    /// poe2wiki.
    Wiki,
    /// Lists of tools: exile.party, awesome-poe-2, awesome-gpui.
    Lists,
    /// Messages to video creators.
    Creators,
    /// The developer article, and where it's shared: This Week in Rust, Zed's discussions.
    Article,
}

impl Source {
    pub const ALL: [Source; 9] = [
        Source::Reddit,
        Source::Forum,
        Source::Discord,
        Source::YouTube,
        Source::Steam,
        Source::Wiki,
        Source::Lists,
        Source::Creators,
        Source::Article,
    ];

    /// Its tag, as a link carries it and a counter's name ends with it.
    pub fn tag(self) -> &'static str {
        match self {
            Source::Reddit => "reddit",
            Source::Forum => "forum",
            Source::Discord => "discord",
            Source::YouTube => "youtube",
            Source::Steam => "steam",
            Source::Wiki => "wiki",
            Source::Lists => "lists",
            Source::Creators => "creators",
            Source::Article => "article",
        }
    }

    /// The source a link's tag names, as the link carries it; `None` for any other.
    pub fn named(tag: &str) -> Option<Source> {
        Source::ALL.into_iter().find(|source| source.tag() == tag)
    }

    /// The source a query's `from` tag names: of its parameters, the first `from` whose value is a
    /// source's tag.
    pub fn in_query(query: &str) -> Option<Source> {
        query
            .split('&')
            .filter_map(|parameter| parameter.split_once('='))
            .filter(|(name, _)| *name == "from")
            .find_map(|(_, value)| Source::named(value))
    }
}

impl Stat {
    pub fn name(self) -> Cow<'static, str> {
        match self {
            Stat::Download => "download".into(),
            Stat::EventStream => "event_stream".into(),
            Stat::UpdateCheck => "update_check".into(),
            Stat::DataDownload => "data_download".into(),
            Stat::UpdateDownload => "update_download".into(),
            Stat::Report(ReportKind::Bug) => "report_bug".into(),
            Stat::Report(ReportKind::Idea) => "report_idea".into(),
            Stat::Report(ReportKind::Item) => "report_item".into(),
            Stat::Report(ReportKind::Crash) => "report_crash".into(),
            Stat::Visit(source) => format!("visit_{}", source.tag()).into(),
            Stat::DownloadSite => "download_site".into(),
            Stat::DownloadUpdate => "download_update".into(),
            Stat::DownloadSiteFrom(source) => format!("download_site_from_{}", source.tag()).into(),
            Stat::AppConn => "app_conn".into(),
            Stat::PageView => "page_view".into(),
            Stat::AppStart => "app_start".into(),
            Stat::InstallNew => "install_new".into(),
            Stat::AppStartLang(lang) => format!("app_start_lang_{}", lang.name()).into(),
            Stat::AppStartDev => "app_start_dev".into(),
        }
    }

    /// One more of this counter, on the day of `now`.
    pub fn write(self, now: i64) -> Write {
        write(&self.name(), What::Count, false, now)
    }
}

/// A counter named for versions: the name holds a [`Version`], so it is one of the newest published
/// releases' or `other`, whatever a request said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Versioned {
    /// An event stream opened by the app, by the version it runs.
    AppConn(Version),
    /// The installer downloaded by the updater, by the version downloaded.
    DownloadUpdate(Version),
    /// A start on a newer version right after a start on an older one: an update taken.
    UpdateApplied { from: Version, to: Version },
}

impl Versioned {
    pub fn name(&self) -> String {
        match self {
            Versioned::AppConn(version) => format!("app_conn_v_{version}"),
            Versioned::DownloadUpdate(version) => format!("download_update_v_{version}"),
            Versioned::UpdateApplied { from, to } => format!("update_applied_{from}_{to}"),
        }
    }

    /// One more of this counter, on the day of `now`.
    pub fn write(&self, now: i64) -> Write {
        write(&self.name(), What::Count, true, now)
    }
}

/// `what` under the counter `name` of the day of `now`, kept [`KEEP_DAYS`] after that day. A name
/// made of what requests bring is `listed` in the day's list of names, for the readout.
pub fn write(name: &str, what: What, listed: bool, now: i64) -> Write {
    let day = Day::of(now);
    Write {
        key: format!("oracle:stat:{name}:{day}"),
        what,
        expires_at: day.end() + KEEP_DAYS * DAY_SECS,
        named: listed.then(|| Named {
            index: names_key(day),
            name: name.to_owned(),
        }),
    }
}

/// Where the names of the day's listed counters are kept.
fn names_key(day: Day) -> String {
    format!("oracle:names:{day}")
}

/// Counts one `stat` today, in the background: no answer waits on the count.
pub fn count(app: &Arc<App>, stat: Stat) {
    let app = app.clone();
    tokio::spawn(async move {
        let now = moscow::now();
        app.store.record(&[stat.write(now)], now).await;
    });
}

/// A counter every readout has, its count 0 when nothing was counted.
enum Fixed {
    Count(Stat),
    Uniq(Uniq),
}

/// Every fixed counter, in the order the readout prints them: the counters as they were before
/// any was added come first, in their old order.
static FIXED: LazyLock<Vec<Fixed>> = LazyLock::new(|| {
    let mut fixed: Vec<Fixed> = [
        Stat::Download,
        Stat::EventStream,
        Stat::UpdateCheck,
        Stat::DataDownload,
        Stat::UpdateDownload,
        Stat::Report(ReportKind::Bug),
        Stat::Report(ReportKind::Idea),
        Stat::Report(ReportKind::Item),
        Stat::Report(ReportKind::Crash),
    ]
    .map(Fixed::Count)
    .into();
    fixed.extend(Source::ALL.map(|source| Fixed::Count(Stat::Visit(source))));
    fixed.extend([Stat::DownloadSite, Stat::DownloadUpdate].map(Fixed::Count));
    fixed.extend(Source::ALL.map(|source| Fixed::Count(Stat::DownloadSiteFrom(source))));
    fixed.extend(
        [
            Stat::AppConn,
            Stat::PageView,
            Stat::AppStart,
            Stat::InstallNew,
            Stat::AppStartDev,
        ]
        .map(Fixed::Count),
    );
    fixed.extend(Lang::ALL.map(|lang| Fixed::Count(Stat::AppStartLang(lang))));
    fixed.push(Fixed::Uniq(Uniq::AppDay));
    fixed.push(Fixed::Uniq(Uniq::SiteDay));
    fixed.extend(Source::ALL.map(|source| Fixed::Uniq(Uniq::SiteDayFrom(source))));
    fixed
});

impl Fixed {
    fn name(&self) -> String {
        match self {
            Fixed::Count(stat) => stat.name().into_owned(),
            Fixed::Uniq(uniq) => uniq.name(),
        }
    }

    fn distinct(&self) -> bool {
        matches!(self, Fixed::Uniq(_))
    }
}

/// One counter asked of the store: what it is called, where it is kept, how it is read.
struct Ask {
    name: String,
    key: String,
    distinct: bool,
    /// Always in the readout, else only when counted.
    fixed: bool,
}

/// One day's counts by name, as [`snapshots`] reads them.
pub struct Snapshot {
    pub day: Day,
    /// The fixed counters in the readout's order, then the counted ones named for versions,
    /// sorted by name.
    counts: Vec<(String, u64)>,
}

impl Snapshot {
    /// A counter's count that day, 0 when it isn't there.
    pub fn get(&self, name: &str) -> u64 {
        self.counts
            .iter()
            .find(|(counted, _)| counted == name)
            .map_or(0, |(_, count)| *count)
    }

    /// The counters whose names start with `prefix`, with their counts.
    pub fn with<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = (&'a str, u64)> + 'a {
        self.counts
            .iter()
            .filter(move |(name, _)| name.starts_with(prefix))
            .map(|(name, count)| (name.as_str(), *count))
    }

    /// The sum of the counters whose names start with `prefix`.
    pub fn sum(&self, prefix: &str) -> u64 {
        self.with(prefix).map(|(_, count)| count).sum()
    }

    /// A day's counts as given, everything else 0, for tests.
    #[cfg(test)]
    pub fn with_counts(day: Day, counts: &[(&str, u64)]) -> Snapshot {
        Snapshot {
            day,
            counts: counts
                .iter()
                .map(|(name, count)| ((*name).to_owned(), *count))
                .collect(),
        }
    }
}

/// Whether a name read back from the store is one this service could have written.
fn plausible(name: &str) -> bool {
    name.len() <= 80
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
}

/// The counts of each of `days`, read in three calls: the day's lists of names, then the plain
/// counters, then the sketches. `None` when Redis fails.
pub async fn snapshots(store: &Store, days: &[Day], now: i64) -> Option<Vec<Snapshot>> {
    let indexes: Vec<String> = days.iter().map(|day| names_key(*day)).collect();
    let listed = store.names(&indexes, now).await?;
    let (mut plain, mut sketches) = (Vec::new(), Vec::new());
    let mut asked: Vec<Vec<Ask>> = Vec::with_capacity(days.len());
    for (day, names) in days.iter().zip(&listed) {
        let mut wanted: Vec<Ask> = FIXED
            .iter()
            .map(|fixed| Ask {
                name: fixed.name(),
                key: String::new(),
                distinct: fixed.distinct(),
                fixed: true,
            })
            .collect();
        wanted.extend(names.iter().filter(|name| plausible(name)).map(|name| Ask {
            name: name.clone(),
            key: String::new(),
            distinct: name.starts_with("uniq_"),
            fixed: false,
        }));
        for ask in &mut wanted {
            ask.key = format!("oracle:stat:{}:{day}", ask.name);
            if ask.distinct {
                sketches.push(ask.key.clone());
            } else {
                plain.push(ask.key.clone());
            }
        }
        asked.push(wanted);
    }
    let mut plain = store.values(&plain, now).await?.into_iter();
    let mut sketches = store.distinct_values(&sketches, now).await?.into_iter();
    Some(
        days.iter()
            .zip(asked)
            .map(|(day, wanted)| Snapshot {
                day: *day,
                counts: wanted
                    .into_iter()
                    .filter_map(|ask| {
                        let count = if ask.distinct {
                            sketches.next()
                        } else {
                            plain.next()
                        }
                        .unwrap_or_default();
                        (ask.fixed || count > 0).then_some((ask.name, count))
                    })
                    .collect(),
            })
            .collect(),
    )
}

/// Every counter's count on each of the last days, as `oracle-web stats` prints it. As JSON:
/// `{"generated_at":"2026-09-28T09:10:00Z","days":[{"day":"2026-09-28","counts":{…}},…],
/// "weeks":[{"week":"2026-W40","from":"2026-09-28","counts":{"uniq_app_week":44}},…]}`, the days
/// and weeks this one first, each count under its counter's name. Every fixed counter is in each
/// day's counts, 0 when nothing was counted -- on a day before the counter existed too; the
/// counters named for versions (`app_conn_v_0.1.3`, `uniq_app_day_v_0.1.3`,
/// `update_applied_0.1.2_0.1.3`, `download_update_v_0.1.3`) only when counted.
#[derive(Serialize)]
pub struct Readout {
    /// When the counters were read, in UTC.
    generated_at: String,
    /// Today in Moscow first, then each day before it.
    days: Vec<DayCounts>,
    /// The ISO weeks the days touch, this one first.
    weeks: Vec<WeekCounts>,
}

/// One day of a [`Readout`].
#[derive(Serialize)]
struct DayCounts {
    day: Day,
    counts: Counts,
}

/// One week of a [`Readout`]: the sketches that count by the week.
#[derive(Serialize)]
struct WeekCounts {
    week: Week,
    /// Its Monday.
    from: Day,
    counts: Counts,
}

/// Counts written as an object keyed by their names, in the order given.
struct Counts(Vec<(String, u64)>);

impl Serialize for Counts {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut counts = serializer.serialize_map(Some(self.0.len()))?;
        for (name, count) in &self.0 {
            counts.serialize_entry(name, count)?;
        }
        counts.end()
    }
}

/// Every counter on the Moscow day of `now` and on the `days - 1` days before it, and the weekly
/// counts of the weeks they touch, read from `store`; `None` when Redis fails.
pub async fn read_out(store: &Store, days: u32, now: i64) -> Option<Readout> {
    let today = Day::of(now);
    let listed: Vec<Day> = (0..i64::from(days)).map(|back| today.minus(back)).collect();
    let snapshots = snapshots(store, &listed, now).await?;
    let first = listed.last().map_or(today, |oldest| *oldest).week();
    let mut weeks = vec![today.week()];
    while weeks.last().is_some_and(|week| *week != first) {
        weeks.push(weeks[weeks.len() - 1].minus(1));
    }
    let keys: Vec<String> = weeks.iter().map(|week| week_key(*week)).collect();
    let installs = store.distinct_values(&keys, now).await?;
    Some(Readout {
        generated_at: moscow::rfc3339(now),
        days: snapshots
            .into_iter()
            .map(|snapshot| DayCounts {
                day: snapshot.day,
                counts: Counts(snapshot.counts),
            })
            .collect(),
        weeks: weeks
            .into_iter()
            .zip(installs)
            .map(|(week, installs)| WeekCounts {
                week,
                from: week.monday(),
                counts: Counts(vec![(Uniq::AppWeek.name(), installs)]),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-09-26 00:00 in Moscow, a Saturday.
    const SATURDAY: i64 = 1_790_370_000;

    fn version(text: &str) -> Version {
        Version::bound(Some(text), &[text.to_owned()])
    }

    #[test]
    fn a_tag_names_a_source_only_when_it_is_one_of_the_nine() {
        for source in Source::ALL {
            assert_eq!(Source::named(source.tag()), Some(source));
        }
        for tag in ["", "Reddit", "reddit ", "redd", "evil", "visit_reddit"] {
            assert_eq!(Source::named(tag), None, "{tag:?}");
        }
        // Of a query's parameters the first `from` naming a source counts.
        for (query, source) in [
            ("from=reddit", Some(Source::Reddit)),
            ("utm_source=feed&from=forum", Some(Source::Forum)),
            ("from=evil&from=creators", Some(Source::Creators)),
            ("from=article&from=reddit", Some(Source::Article)),
            ("from=evil", None),
            ("source=reddit", None),
            ("xfrom=reddit&q=from=reddit", None),
            ("from", None),
            ("", None),
        ] {
            assert_eq!(Source::in_query(query), source, "{query}");
        }
    }

    #[tokio::test]
    async fn counts_land_on_their_moscow_day_and_last_120_days() {
        let store = Store::memory();
        // 23:59 on Friday, then 00:00 and 00:01 on Saturday, in Moscow.
        for now in [SATURDAY - 60, SATURDAY, SATURDAY + 60] {
            let write = Stat::Report(ReportKind::Item).write(now);
            store.record(&[write], now).await;
        }
        let friday = Stat::Report(ReportKind::Item).write(SATURDAY - 1);
        let saturday = Stat::Report(ReportKind::Item).write(SATURDAY);
        assert_eq!(friday.key, "oracle:stat:report_item:2026-09-25");
        assert_eq!(saturday.key, "oracle:stat:report_item:2026-09-26");
        let keys = [friday.key, saturday.key];
        assert_eq!(store.values(&keys, SATURDAY + 120).await, Some(vec![1, 2]));
        assert_eq!(saturday.expires_at, SATURDAY + (1 + 120) * DAY_SECS);
        assert_eq!(
            store.values(&keys[1..], saturday.expires_at - 1).await,
            Some(vec![2])
        );
        assert_eq!(
            store.values(&keys[1..], saturday.expires_at).await,
            Some(vec![0])
        );
    }

    #[test]
    fn counters_are_named_as_the_readout_and_the_dashboard_read_them() {
        for (stat, name) in [
            (Stat::Download, "download"),
            (Stat::DownloadSite, "download_site"),
            (Stat::DownloadUpdate, "download_update"),
            (
                Stat::DownloadSiteFrom(Source::YouTube),
                "download_site_from_youtube",
            ),
            (Stat::Visit(Source::Wiki), "visit_wiki"),
            (Stat::AppConn, "app_conn"),
            (Stat::PageView, "page_view"),
            (Stat::AppStart, "app_start"),
            (Stat::InstallNew, "install_new"),
            (Stat::AppStartLang(Lang::Ru), "app_start_lang_ru"),
            (Stat::AppStartLang(Lang::Other), "app_start_lang_other"),
            (Stat::AppStartDev, "app_start_dev"),
        ] {
            assert_eq!(stat.name(), name);
        }
        let (old, new) = (version("0.1.2"), version("0.1.3"));
        assert_eq!(Versioned::AppConn(new.clone()).name(), "app_conn_v_0.1.3");
        assert_eq!(
            Versioned::DownloadUpdate(new.clone()).name(),
            "download_update_v_0.1.3"
        );
        assert_eq!(
            Versioned::UpdateApplied { from: old, to: new }.name(),
            "update_applied_0.1.2_0.1.3"
        );
        assert_eq!(
            Versioned::AppConn(Version::bound(None, &[])).name(),
            "app_conn_v_other"
        );
    }

    #[tokio::test]
    async fn the_readout_holds_each_moscow_day_today_first() {
        let store = Store::memory();
        // Two installers late on Friday and one at midnight, then a visit from Reddit on Saturday
        // morning, in Moscow.
        for (stat, now) in [
            (Stat::Download, SATURDAY - 3600),
            (Stat::Download, SATURDAY - 1),
            (Stat::Download, SATURDAY),
            (Stat::Visit(Source::Reddit), SATURDAY + 9 * 3600),
        ] {
            store.record(&[stat.write(now)], now).await;
        }
        // 11:05 on Saturday in Moscow.
        let readout = read_out(&store, 3, SATURDAY + 11 * 3600 + 5 * 60).await;
        let readout = serde_json::to_value(readout.unwrap()).unwrap();
        assert_eq!(readout["generated_at"], "2026-09-26T08:05:00Z");
        let days: Vec<(&str, u64, u64, u64)> = readout["days"]
            .as_array()
            .unwrap()
            .iter()
            .map(|day| {
                let count = |name: &str| day["counts"][name].as_u64().unwrap();
                (
                    day["day"].as_str().unwrap(),
                    count("download"),
                    count("visit_reddit"),
                    count("report_crash"),
                )
            })
            .collect();
        assert_eq!(
            days,
            [
                ("2026-09-26", 1, 1, 0),
                ("2026-09-25", 2, 0, 0),
                ("2026-09-24", 0, 0, 0),
            ]
        );
    }

    /// The store after a day and a half of an app's and a site's traffic, at 11:05 on Saturday.
    async fn traffic() -> (Store, i64) {
        let store = Store::memory();
        let now = SATURDAY + 11 * 3600 + 5 * 60;
        let yesterday = now - DAY_SECS;
        let (old, new) = (version("0.1.2"), version("0.1.3"));
        let mut writes = vec![
            Stat::DownloadSite.write(now),
            Stat::DownloadSiteFrom(Source::Reddit).write(now),
            Stat::DownloadUpdate.write(yesterday),
            Stat::PageView.write(now),
            Stat::PageView.write(now),
            Stat::AppStart.write(now),
            Versioned::AppConn(new.clone()).write(now),
            Versioned::AppConn(new.clone()).write(now),
            Versioned::AppConn(old.clone()).write(yesterday),
            Versioned::DownloadUpdate(new.clone()).write(yesterday),
            Versioned::UpdateApplied {
                from: old.clone(),
                to: new.clone(),
            }
            .write(yesterday),
        ];
        // Three installs today, two of them on 0.1.3, and two visitors, one from Reddit.
        for (install, on) in [(1, &new), (2, &new), (3, &old)] {
            writes.push(Uniq::AppDay.write([install; 32], now));
            writes.push(Uniq::AppDayVersion(on.clone()).write([install; 32], now));
            writes.push(Uniq::AppWeek.write([install; 32], now));
        }
        // An install seen on the Sunday before is in the week before's sketch.
        writes.push(Uniq::AppWeek.write([4; 32], SATURDAY - 6 * DAY_SECS + 3600));
        writes.push(Uniq::SiteDay.write([5; 32], now));
        writes.push(Uniq::SiteDay.write([6; 32], now));
        writes.push(Uniq::SiteDayFrom(Source::Reddit).write([5; 32], now));
        store.record(&writes, now).await;
        (store, now)
    }

    #[tokio::test]
    async fn the_readout_keeps_the_old_keys_and_adds_the_distinct_and_versioned_ones() {
        let (store, now) = traffic().await;
        let readout = read_out(&store, 2, now).await.unwrap();
        let text = serde_json::to_string(&readout).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let (today, yesterday) = (&json["days"][0], &json["days"][1]);
        assert_eq!(today["day"], "2026-09-26");
        assert_eq!(yesterday["day"], "2026-09-25");

        let fixed: Vec<&str> = [
            "download",
            "event_stream",
            "update_check",
            "data_download",
            "update_download",
            "report_bug",
            "report_idea",
            "report_item",
            "report_crash",
            "visit_reddit",
            "visit_article",
            "download_site",
            "download_update",
            "download_site_from_reddit",
            "download_site_from_article",
            "app_conn",
            "page_view",
            "app_start",
            "install_new",
            "app_start_dev",
            "app_start_lang_en",
            "app_start_lang_ru",
            "app_start_lang_other",
            "uniq_app_day",
            "uniq_site_day",
            "uniq_site_day_from_reddit",
            "uniq_site_day_from_article",
        ]
        .into();
        for name in &fixed {
            assert!(today["counts"][name].is_u64(), "{name}");
            assert!(yesterday["counts"][name].is_u64(), "{name}");
        }
        // The old counters print first, in their old order, and the new ones after them.
        let at = |name: &str| text.find(&format!("\"{name}\":")).unwrap();
        assert!(at("download") < at("event_stream") && at("event_stream") < at("update_check"));
        assert!(
            at("report_crash") < at("visit_reddit") && at("visit_article") < at("download_site")
        );
        assert!(at("uniq_site_day_from_article") < at("app_conn_v_0.1.3"));

        let counts = &today["counts"];
        for (name, count) in [
            ("download_site", 1),
            ("download_site_from_reddit", 1),
            ("download_update", 0),
            ("page_view", 2),
            ("app_start", 1),
            ("app_conn_v_0.1.3", 2),
            ("uniq_app_day", 3),
            ("uniq_app_day_v_0.1.3", 2),
            ("uniq_app_day_v_0.1.2", 1),
            ("uniq_site_day", 2),
            ("uniq_site_day_from_reddit", 1),
            ("uniq_site_day_from_forum", 0),
        ] {
            assert_eq!(counts[name], count, "{name}");
        }
        // A counter named for a version is there only on the days it was counted.
        assert!(counts.get("app_conn_v_0.1.2").is_none());
        assert!(counts.get("update_applied_0.1.2_0.1.3").is_none());
        let before = &yesterday["counts"];
        assert_eq!(before["app_conn_v_0.1.2"], 1);
        assert_eq!(before["download_update_v_0.1.3"], 1);
        assert_eq!(before["update_applied_0.1.2_0.1.3"], 1);
        assert_eq!(before["download_update"], 1);
        assert!(before.get("app_conn_v_0.1.3").is_none());

        // The weekly sketches: this week's three installs, and the Sunday's one before it, which
        // the two days read touch.
        assert_eq!(
            json["weeks"],
            serde_json::json!([
                {"week": "2026-W39", "from": "2026-09-21", "counts": {"uniq_app_week": 3}},
            ])
        );
        let wider = read_out(&store, 8, now).await.unwrap();
        let wider = serde_json::to_value(wider).unwrap();
        assert_eq!(wider["days"].as_array().unwrap().len(), 8);
        assert_eq!(
            wider["weeks"],
            serde_json::json!([
                {"week": "2026-W39", "from": "2026-09-21", "counts": {"uniq_app_week": 3}},
                {"week": "2026-W38", "from": "2026-09-14", "counts": {"uniq_app_week": 1}},
            ])
        );
    }

    #[tokio::test]
    async fn a_snapshot_finds_a_counter_by_name_or_by_prefix() {
        let (store, now) = traffic().await;
        let days = [Day::of(now), Day::of(now).minus(1)];
        let snapshots = snapshots(&store, &days, now).await.unwrap();
        let (today, yesterday) = (&snapshots[0], &snapshots[1]);
        assert_eq!(today.get("page_view"), 2);
        assert_eq!(today.get("no_such_counter"), 0);
        assert_eq!(today.sum("app_conn_v_"), 2);
        assert_eq!(yesterday.sum("update_applied_"), 1);
        assert_eq!(today.sum("update_applied_"), 0);
        assert_eq!(
            yesterday.with("app_conn_v_").collect::<Vec<_>>(),
            [("app_conn_v_0.1.2", 1)]
        );
    }

    #[tokio::test]
    async fn a_name_the_service_could_not_have_written_is_not_read() {
        let store = Store::memory();
        let now = SATURDAY + 60;
        let mut write = Versioned::AppConn(version("0.1.3")).write(now);
        write.named.as_mut().unwrap().name = "app_conn_v_\"}{evil".to_owned();
        store.record(&[write], now).await;
        let readout = serde_json::to_string(&read_out(&store, 1, now).await.unwrap()).unwrap();
        assert!(!readout.contains("evil"), "{readout}");
    }
}
