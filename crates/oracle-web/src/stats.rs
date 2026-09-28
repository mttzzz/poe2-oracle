//! What the service counts, per Moscow day, and the digest that tells the owner every morning.
//!
//! The counters are `oracle:stat:<name>:<YYYY-MM-DD>`, kept for 120 days after their day, and hold
//! numbers only: no address, no version, nothing about who. At 09:00 in Moscow the digest posts
//! the day before to the owner's Telegram, each number beside the same weekday a week earlier.
//! `oracle-web stats` prints every counter of the last days as JSON ([`read_out`]).

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use oracle_protocol::ReportKind;
use serde::ser::SerializeMap as _;
use serde::{Serialize, Serializer};
use tracing::{error, info, warn};

use crate::App;
use crate::moscow::{self, DAY_SECS, Day};
use crate::store::Store;

/// How long a day's counters are kept after it.
const KEEP_DAYS: i64 = 120;
/// The digest's hour in Moscow.
const DIGEST_HOUR: i64 = 9;
/// Tries at the digest, and the pause between them: a Redis or Telegram hiccup at nine shouldn't
/// cost the day's digest.
const DIGEST_TRIES: u32 = 3;
const DIGEST_RETRY: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Copy, Debug)]
pub enum Stat {
    /// The installer, downloaded from the site's button or by the updater.
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

/// Every counter, in the order the digest reads them.
const ALL: [Stat; 18] = [
    Stat::Download,
    Stat::EventStream,
    Stat::UpdateCheck,
    Stat::DataDownload,
    Stat::UpdateDownload,
    Stat::Report(ReportKind::Bug),
    Stat::Report(ReportKind::Idea),
    Stat::Report(ReportKind::Item),
    Stat::Report(ReportKind::Crash),
    Stat::Visit(Source::Reddit),
    Stat::Visit(Source::Forum),
    Stat::Visit(Source::Discord),
    Stat::Visit(Source::YouTube),
    Stat::Visit(Source::Steam),
    Stat::Visit(Source::Wiki),
    Stat::Visit(Source::Lists),
    Stat::Visit(Source::Creators),
    Stat::Visit(Source::Article),
];

impl Stat {
    fn name(self) -> &'static str {
        match self {
            Stat::Download => "download",
            Stat::EventStream => "event_stream",
            Stat::UpdateCheck => "update_check",
            Stat::DataDownload => "data_download",
            Stat::UpdateDownload => "update_download",
            Stat::Report(ReportKind::Bug) => "report_bug",
            Stat::Report(ReportKind::Idea) => "report_idea",
            Stat::Report(ReportKind::Item) => "report_item",
            Stat::Report(ReportKind::Crash) => "report_crash",
            Stat::Visit(Source::Reddit) => "visit_reddit",
            Stat::Visit(Source::Forum) => "visit_forum",
            Stat::Visit(Source::Discord) => "visit_discord",
            Stat::Visit(Source::YouTube) => "visit_youtube",
            Stat::Visit(Source::Steam) => "visit_steam",
            Stat::Visit(Source::Wiki) => "visit_wiki",
            Stat::Visit(Source::Lists) => "visit_lists",
            Stat::Visit(Source::Creators) => "visit_creators",
            Stat::Visit(Source::Article) => "visit_article",
        }
    }

    fn key(self, day: Day) -> String {
        format!("oracle:stat:{}:{day}", self.name())
    }
}

impl Source {
    /// The source a link's tag names, as the link carries it: one of those [`ALL`] counts, else
    /// `None`.
    pub fn named(tag: &str) -> Option<Source> {
        ALL.iter().find_map(|stat| match *stat {
            Stat::Visit(source) if source.tag() == tag => Some(source),
            _ => None,
        })
    }

    /// Its tag: its counter's name after `visit_`.
    fn tag(self) -> &'static str {
        let name = Stat::Visit(self).name();
        name.strip_prefix("visit_").unwrap_or(name)
    }
}

/// Counts one `stat` today, in the background: no answer waits on the count.
pub fn count(app: &Arc<App>, stat: Stat) {
    let app = app.clone();
    tokio::spawn(async move {
        let now = moscow::now();
        let (key, expires_at) = counter(stat, now);
        app.store.increment(&key, expires_at, now).await;
    });
}

/// The counter `stat` counts in at `now`, and when it expires: [`KEEP_DAYS`] after its day.
fn counter(stat: Stat, now: i64) -> (String, i64) {
    let day = Day::of(now);
    (stat.key(day), day.end() + KEEP_DAYS * DAY_SECS)
}

/// Every counter's count on each of the last days, as `oracle-web stats` prints it. As JSON:
/// `{"generated_at":"2026-09-28T09:10:00Z","days":[{"day":"2026-09-28","counts":{…}},…]}`, the
/// days today first, each count under its counter's name.
#[derive(Serialize)]
pub struct Readout {
    /// When the counters were read, in UTC.
    generated_at: String,
    /// Today in Moscow first, then each day before it.
    days: Vec<DayCounts>,
}

/// One day of a [`Readout`].
#[derive(Serialize)]
struct DayCounts {
    day: Day,
    counts: Counts,
}

/// A day's count of each of [`ALL`] in turn, written as an object keyed by their names, in that
/// order.
struct Counts([u64; ALL.len()]);

impl Serialize for Counts {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut counts = serializer.serialize_map(Some(ALL.len()))?;
        for (stat, count) in ALL.iter().zip(&self.0) {
            counts.serialize_entry(stat.name(), count)?;
        }
        counts.end()
    }
}

/// Every counter of [`ALL`] on the Moscow day of `now` and on the `days - 1` days before it, read
/// in one call; `None` when Redis fails.
pub async fn read_out(store: &Store, days: u32, now: i64) -> Option<Readout> {
    let today = Day::of(now);
    let keys: Vec<String> = (0..i64::from(days))
        .flat_map(|back| ALL.iter().map(move |stat| stat.key(today.minus(back))))
        .collect();
    let values = store.values(&keys, now).await?;
    let (per_day, _) = values.as_chunks::<{ ALL.len() }>();
    Some(Readout {
        generated_at: moscow::rfc3339(now),
        days: per_day
            .iter()
            .zip(0..)
            .map(|(counts, back)| DayCounts {
                day: today.minus(back),
                counts: Counts(*counts),
            })
            .collect(),
    })
}

/// Posts each morning's digest, forever.
pub async fn post_digests(app: Arc<App>) {
    let mut after = moscow::now();
    loop {
        let at = moscow::next_hour_of_day(after, DIGEST_HOUR);
        let wait = (at - moscow::now()).max(0);
        tokio::time::sleep(Duration::from_secs(wait as u64)).await;
        post_digest(&app, Day::of(at).minus(1)).await;
        // From the planned moment, not the clock: waking a hair early can't repeat a day.
        after = at;
    }
}

async fn post_digest(app: &App, day: Day) {
    // With Redis, every replica wakes at nine; the first to take the day's key posts.
    let lock = format!("oracle:digest:{day}");
    if app.telegram.configured()
        && !app
            .store
            .claim(&lock, day.end() + 2 * DAY_SECS, moscow::now())
            .await
    {
        info!(%day, "another replica posts this digest");
        return;
    }
    let keys: Vec<String> = ALL
        .iter()
        .flat_map(|stat| [stat.key(day), stat.key(day.minus(7))])
        .collect();
    for attempt in 1..=DIGEST_TRIES {
        match app.store.values(&keys, moscow::now()).await {
            Some(values) => {
                let text = digest(day, &values);
                match app.telegram.send_message(&text).await {
                    Ok(_) => {
                        info!(%day, "digest posted");
                        return;
                    }
                    Err(problem) => warn!(%problem, attempt, "Telegram didn't take the digest"),
                }
            }
            None => warn!(attempt, "no counters for the digest"),
        }
        if attempt < DIGEST_TRIES {
            tokio::time::sleep(DIGEST_RETRY).await;
        }
    }
    error!(%day, "the digest wasn't posted");
}

/// The digest of `day`. `values` holds, for each of [`ALL`] in turn, its count on `day` and on
/// the same weekday a week earlier.
fn digest(day: Day, values: &[u64]) -> String {
    let count = |stat: usize| (values[2 * stat], values[2 * stat + 1]);
    let (_, month, date) = day.date();
    let (_, week_ago_month, week_ago_date) = day.minus(7).date();
    let mut text = format!(
        "📊 <b>PoE2 Oracle за {}, {date} {}</b>\n<i>В скобках — неделей раньше, {week_ago_date} {}.</i>\n\n",
        WEEKDAYS[day.weekday()],
        MONTHS[month as usize - 1],
        MONTHS[week_ago_month as usize - 1],
    );
    let lines = [
        "Скачивания установщика",
        "Подключения программы (запуски и переподключения)",
        "Проверки обновлений",
        "Скачивания пакета данных",
        "Скачивания SHA256SUMS и подписей",
    ];
    for (stat, label) in lines.iter().enumerate() {
        let (now, before) = count(stat);
        let _ = writeln!(text, "{label}: <b>{now}</b> ({before})");
    }
    let kinds = ["🐞 ошибки", "💡 идеи", "💎 предметы", "💥 вылеты"];
    let reports: Vec<(u64, u64)> = (lines.len()..lines.len() + kinds.len())
        .map(count)
        .collect();
    let (total, total_before) = reports
        .iter()
        .fold((0, 0), |(now, before), (day, week_ago)| {
            (now + day, before + week_ago)
        });
    let _ = writeln!(text, "Сообщения: <b>{total}</b> ({total_before})");
    let kinds: Vec<String> = kinds
        .iter()
        .zip(&reports)
        .map(|(kind, (now, before))| format!("{kind} {now} ({before})"))
        .collect();
    text.push_str(&kinds.join(", "));
    // Only the tags with a visit on either day: the others, on links not published yet or not
    // followed any more, would only be zeros.
    let visits: Vec<(&str, (u64, u64))> = ALL
        .iter()
        .enumerate()
        .filter_map(|(stat, counted)| match counted {
            Stat::Visit(source) => Some((source.tag(), count(stat))),
            _ => None,
        })
        .filter(|(_, (now, before))| *now > 0 || *before > 0)
        .collect();
    if !visits.is_empty() {
        let total: u64 = visits.iter().map(|(_, (now, _))| now).sum();
        let total_before: u64 = visits.iter().map(|(_, (_, before))| before).sum();
        let _ = write!(
            text,
            "\nПереходы на сайт по меткам: <b>{total}</b> ({total_before})\n"
        );
        let tags: Vec<String> = visits
            .iter()
            .map(|(tag, (now, before))| format!("{tag} {now} ({before})"))
            .collect();
        text.push_str(&tags.join(", "));
    }
    text
}

/// Weekdays after «за»: the accusative.
const WEEKDAYS: [&str; 7] = [
    "понедельник",
    "вторник",
    "среду",
    "четверг",
    "пятницу",
    "субботу",
    "воскресенье",
];
/// Months after a date: the genitive.
const MONTHS: [&str; 12] = [
    "января",
    "февраля",
    "марта",
    "апреля",
    "мая",
    "июня",
    "июля",
    "августа",
    "сентября",
    "октября",
    "ноября",
    "декабря",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-09-26 00:00 in Moscow, a Saturday.
    const SATURDAY: i64 = 1_790_370_000;

    #[test]
    fn the_digest_reads_a_day_against_a_week_before() {
        let friday = Day::of(SATURDAY).minus(1);
        // No visit through a tagged link on either day: the digest says nothing of them.
        let mut values = [0; 2 * ALL.len()];
        values[..18].copy_from_slice(&[
            12, 8, 410, 380, 7, 290, 25, 0, 30, 22, 3, 1, 1, 1, 1, 0, 0, 0,
        ]);
        assert_eq!(
            digest(friday, &values),
            "📊 <b>PoE2 Oracle за пятницу, 25 сентября</b>\n\
             <i>В скобках — неделей раньше, 18 сентября.</i>\n\
             \n\
             Скачивания установщика: <b>12</b> (8)\n\
             Подключения программы (запуски и переподключения): <b>410</b> (380)\n\
             Проверки обновлений: <b>7</b> (290)\n\
             Скачивания пакета данных: <b>25</b> (0)\n\
             Скачивания SHA256SUMS и подписей: <b>30</b> (22)\n\
             Сообщения: <b>5</b> (2)\n\
             🐞 ошибки 3 (1), 💡 идеи 1 (1), 💎 предметы 1 (0), 💥 вылеты 0 (0)"
        );
    }

    #[test]
    fn the_digest_names_the_tags_that_brought_a_visit_on_either_day() {
        let friday = Day::of(SATURDAY).minus(1);
        let mut values = [0; 2 * ALL.len()];
        for (source, now, before) in [
            (Source::Reddit, 14, 0),
            (Source::Discord, 3, 1),
            (Source::YouTube, 0, 2),
        ] {
            let stat = ALL
                .iter()
                .position(|stat| matches!(stat, Stat::Visit(visit) if *visit == source))
                .unwrap();
            values[2 * stat] = now;
            values[2 * stat + 1] = before;
        }
        let text = digest(friday, &values);
        assert!(
            text.ends_with(
                "💥 вылеты 0 (0)\n\
                 Переходы на сайт по меткам: <b>17</b> (3)\n\
                 reddit 14 (0), discord 3 (1), youtube 0 (2)"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_week_back_can_cross_a_month() {
        let first = Day::of(SATURDAY).minus(22);
        assert_eq!(first.to_string(), "2026-09-04");
        let text = digest(first, &[0; 2 * ALL.len()]);
        assert!(text.starts_with("📊 <b>PoE2 Oracle за пятницу, 4 сентября</b>\n<i>В скобках — неделей раньше, 28 августа.</i>"), "{text}");
    }

    #[tokio::test]
    async fn counts_land_on_their_moscow_day_and_last_120_days() {
        let store = Store::memory();
        // 23:59 on Friday, then 00:00 and 00:01 on Saturday, in Moscow.
        for now in [SATURDAY - 60, SATURDAY, SATURDAY + 60] {
            let (key, expires_at) = counter(Stat::Report(ReportKind::Item), now);
            store.increment(&key, expires_at, now).await;
        }
        let (friday, _) = counter(Stat::Report(ReportKind::Item), SATURDAY - 1);
        let (saturday, expires_at) = counter(Stat::Report(ReportKind::Item), SATURDAY);
        assert_eq!(friday, "oracle:stat:report_item:2026-09-25");
        assert_eq!(saturday, "oracle:stat:report_item:2026-09-26");
        let keys = [friday, saturday];
        assert_eq!(store.values(&keys, SATURDAY + 120).await, Some(vec![1, 2]));
        assert_eq!(expires_at, SATURDAY + (1 + 120) * DAY_SECS);
        assert_eq!(
            store.values(&keys[1..], expires_at - 1).await,
            Some(vec![2])
        );
        assert_eq!(store.values(&keys[1..], expires_at).await, Some(vec![0]));
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
            let (key, expires_at) = counter(stat, now);
            store.increment(&key, expires_at, now).await;
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
}
