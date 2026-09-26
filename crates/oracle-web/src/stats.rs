//! What the service counts, per Moscow day, and the digest that tells the owner every morning.
//!
//! The counters are `oracle:stat:<name>:<YYYY-MM-DD>`, kept for 120 days after their day, and hold
//! numbers only: no address, no version, nothing about who. At 09:00 in Moscow the digest posts
//! the day before to the owner's Telegram, each number beside the same weekday a week earlier.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use oracle_protocol::ReportKind;
use tracing::{error, info, warn};

use crate::App;
use crate::moscow::{self, DAY_SECS, Day};

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
}

/// Every counter, in the order the digest reads them.
const ALL: [Stat; 9] = [
    Stat::Download,
    Stat::EventStream,
    Stat::UpdateCheck,
    Stat::DataDownload,
    Stat::UpdateDownload,
    Stat::Report(ReportKind::Bug),
    Stat::Report(ReportKind::Idea),
    Stat::Report(ReportKind::Item),
    Stat::Report(ReportKind::Crash),
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
        }
    }

    fn key(self, day: Day) -> String {
        format!("oracle:stat:{}:{day}", self.name())
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
    let reports: Vec<(u64, u64)> = (lines.len()..ALL.len()).map(count).collect();
    let (total, total_before) = reports
        .iter()
        .fold((0, 0), |(now, before), (day, week_ago)| {
            (now + day, before + week_ago)
        });
    let _ = writeln!(text, "Сообщения: <b>{total}</b> ({total_before})");
    let kinds: Vec<String> = ["🐞 ошибки", "💡 идеи", "💎 предметы", "💥 вылеты"]
        .iter()
        .zip(&reports)
        .map(|(kind, (now, before))| format!("{kind} {now} ({before})"))
        .collect();
    text.push_str(&kinds.join(", "));
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
    use crate::store::Store;

    /// 2026-09-26 00:00 in Moscow, a Saturday.
    const SATURDAY: i64 = 1_790_370_000;

    #[test]
    fn the_digest_reads_a_day_against_a_week_before() {
        let friday = Day::of(SATURDAY).minus(1);
        let values = [
            12, 8, 410, 380, 7, 290, 25, 0, 30, 22, 3, 1, 1, 1, 1, 0, 0, 0,
        ];
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
}
