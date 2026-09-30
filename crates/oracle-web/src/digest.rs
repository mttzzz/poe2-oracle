//! The morning digest: at 09:00 in Moscow the service posts the day before to the owner's
//! Telegram, each number beside the same weekday a week earlier. It leads with the four numbers
//! that say how the project is doing -- active installs, new installs, updates taken, installers
//! downloaded from the site -- and lists the rest under them.
//!
//! A counter the service didn't keep on a day reads 0 there, which would pass for a real zero: the
//! distinct and per-request counts began with their deploy, the starts and updates with the app
//! release that reports them. Such a day's number is a dash ([`kept`], [`started`]).

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use tracing::{error, info, warn};

use crate::App;
use crate::moscow::{self, DAY_SECS, Day};
use crate::stats::{Snapshot, Source, Stat, snapshots};

/// The digest's hour in Moscow.
const DIGEST_HOUR: i64 = 9;
/// Tries at the digest, and the pause between them: a Redis or Telegram hiccup at nine shouldn't
/// cost the day's digest.
const DIGEST_TRIES: u32 = 3;
const DIGEST_RETRY: Duration = Duration::from_secs(10 * 60);

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
    for attempt in 1..=DIGEST_TRIES {
        match snapshots(&app.store, &[day, day.minus(7)], moscow::now()).await {
            Some(counts) => {
                let text = digest(day, &counts[0], &counts[1]);
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

/// Whether the counts that began with the service's deploy -- the app's distinct installs and
/// connections, the site's pages and visitors, the installer's downloads split by who took them --
/// were kept on `day`: one of them is counted then, on any day the service has visitors.
fn kept(day: &Snapshot) -> bool {
    [
        "page_view",
        "app_conn",
        "uniq_app_day",
        "uniq_site_day",
        "download_site",
        "download_update",
    ]
    .iter()
    .any(|name| day.get(name) > 0)
}

/// Whether the counts that come from the app's reports of its starts were kept on `day`: an app
/// that reports them started, and its starts were counted, even a developer's.
fn started(day: &Snapshot) -> bool {
    day.get("app_start") + day.get("app_start_dev") > 0
}

/// A number on the two days: `count` of each day `measured` says the counter was kept on.
type Pair = (Option<u64>, Option<u64>);

fn pair(
    now: &Snapshot,
    before: &Snapshot,
    measured: fn(&Snapshot) -> bool,
    count: impl Fn(&Snapshot) -> u64,
) -> Pair {
    (
        measured(now).then(|| count(now)),
        measured(before).then(|| count(before)),
    )
}

/// `label: <b>now</b> (before)`, a dash where the counter wasn't kept.
fn line(label: &str, (now, before): Pair) -> String {
    let show = |count: Option<u64>| count.map_or("—".to_owned(), |count| count.to_string());
    format!("{label}: <b>{}</b> ({})", show(now), show(before))
}

/// The digest of `day`; `before` is the same weekday a week earlier.
fn digest(day: Day, now: &Snapshot, before: &Snapshot) -> String {
    let (_, month, date) = day.date();
    let (_, week_ago_month, week_ago_date) = day.minus(7).date();
    let mut text = format!(
        "📊 <b>PoE2 Oracle за {}, {date} {}</b>\n<i>В скобках — неделей раньше, {week_ago_date} {}.</i>\n\n",
        WEEKDAYS[day.weekday()],
        MONTHS[month as usize - 1],
        MONTHS[week_ago_month as usize - 1],
    );
    let stat = |stat: Stat| pair(now, before, |_| true, move |day| day.get(&stat.name()));
    let named = |measured: fn(&Snapshot) -> bool, name: &'static str| {
        pair(now, before, measured, move |day| day.get(name))
    };
    let lead = [
        ("Активные установки", named(kept, "uniq_app_day")),
        ("Новые установки", named(started, "install_new")),
        (
            "Обновления применены",
            pair(now, before, started, |day| day.sum("update_applied_")),
        ),
        ("Скачали с сайта (кнопка)", named(kept, "download_site")),
    ];
    for (label, numbers) in lead {
        let _ = writeln!(text, "{}", line(label, numbers));
    }
    text.push('\n');

    let either = |measured: fn(&Snapshot) -> bool| measured(now) || measured(before);
    let mut rows = Vec::new();
    if either(kept) {
        rows.push(line(
            "Установщик скачан апдейтером",
            named(kept, "download_update"),
        ));
    }
    if either(started) {
        rows.push(line("Запуски программы", named(started, "app_start")));
    }
    rows.extend([
        line(
            "Подключения программы (запуски и переподключения)",
            stat(Stat::EventStream),
        ),
        line("Проверки обновлений", stat(Stat::UpdateCheck)),
        line(
            "Скачивания установщика (кнопка и обновления вместе)",
            stat(Stat::Download),
        ),
        line("Скачивания пакета данных", stat(Stat::DataDownload)),
        line(
            "Скачивания SHA256SUMS и подписей",
            stat(Stat::UpdateDownload),
        ),
    ]);
    if either(kept) {
        let show = |count: Option<u64>| count.map_or("—".to_owned(), |count| count.to_string());
        let (views, visitors) = (named(kept, "page_view"), named(kept, "uniq_site_day"));
        rows.push(format!(
            "Сайт: просмотров страниц <b>{}</b> ({}), посетителей <b>{}</b> ({})",
            show(views.0),
            show(views.1),
            show(visitors.0),
            show(visitors.1)
        ));
    }
    for row in rows {
        let _ = writeln!(text, "{row}");
    }

    let kinds = [
        (Stat::Report(oracle_protocol::ReportKind::Bug), "🐞 ошибки"),
        (Stat::Report(oracle_protocol::ReportKind::Idea), "💡 идеи"),
        (
            Stat::Report(oracle_protocol::ReportKind::Item),
            "💎 предметы",
        ),
        (
            Stat::Report(oracle_protocol::ReportKind::Crash),
            "💥 вылеты",
        ),
    ];
    let reports: Vec<(u64, u64)> = kinds
        .iter()
        .map(|(kind, _)| (now.get(&kind.name()), before.get(&kind.name())))
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
        .map(|((_, kind), (now, before))| format!("{kind} {now} ({before})"))
        .collect();
    text.push_str(&kinds.join(", "));

    // The versions active on either day, the newest first.
    if either(kept) {
        let versions = versions_line(now, before);
        if !versions.is_empty() {
            let _ = write!(text, "\nПо версиям (активные установки): {versions}");
        }
    }

    // Only the tags with a visit on either day: the others, on links not published yet or not
    // followed any more, would only be zeros.
    let visits: Vec<(&str, (u64, u64))> = Source::ALL
        .iter()
        .map(|source| {
            let name = Stat::Visit(*source).name();
            (source.tag(), (now.get(&name), before.get(&name)))
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

/// `0.1.3 30 (28), 0.1.2 8 (9), other 1 (0)`: each version with an active install on either day,
/// the newest first, `other` last.
fn versions_line(now: &Snapshot, before: &Snapshot) -> String {
    const PREFIX: &str = "uniq_app_day_v_";
    let mut versions: Vec<&str> = now
        .with(PREFIX)
        .chain(before.with(PREFIX))
        .map(|(name, _)| &name[PREFIX.len()..])
        .collect();
    versions.sort_unstable();
    versions.dedup();
    versions.sort_by_key(|version| {
        std::cmp::Reverse(
            semver::Version::parse(version)
                .ok()
                .map(|version| (version.major, version.minor, version.patch)),
        )
    });
    versions
        .iter()
        .map(|version| {
            let name = format!("{PREFIX}{version}");
            format!("{version} {} ({})", now.get(&name), before.get(&name))
        })
        .collect::<Vec<_>>()
        .join(", ")
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

    fn friday() -> Day {
        Day::of(SATURDAY).minus(1)
    }

    /// A day's counts as the service kept them, everything else 0.
    fn counts(day: Day, counts: &[(&str, u64)]) -> Snapshot {
        Snapshot::with_counts(day, counts)
    }

    /// The counters as they were before the service told anything apart: what an old day holds.
    const OLD: [(&str, u64); 5] = [
        ("download", 8),
        ("event_stream", 380),
        ("update_check", 290),
        ("data_download", 0),
        ("update_download", 22),
    ];

    #[test]
    fn the_digest_leads_with_the_four_numbers_each_beside_a_week_before() {
        let day = counts(
            friday(),
            &[
                ("download", 21),
                ("event_stream", 410),
                ("update_check", 7),
                ("data_download", 25),
                ("update_download", 30),
                ("report_bug", 3),
                ("report_item", 1),
                ("report_idea", 1),
                ("report_crash", 2),
                ("download_site", 12),
                ("download_update", 9),
                ("uniq_app_day", 41),
                ("app_start", 20),
                ("install_new", 3),
                ("page_view", 133),
                ("uniq_site_day", 40),
                ("uniq_app_day_v_0.1.10", 2),
                ("uniq_app_day_v_0.1.3", 30),
                ("uniq_app_day_v_0.1.2", 8),
                ("uniq_app_day_v_other", 1),
                ("update_applied_0.1.2_0.1.3", 4),
                ("update_applied_other_0.1.3", 1),
            ],
        );
        let week_ago = counts(
            friday().minus(7),
            &[
                ("download", 8),
                ("event_stream", 380),
                ("update_check", 290),
                ("update_download", 22),
                ("report_bug", 1),
                ("report_item", 0),
                ("report_idea", 1),
                ("download_site", 8),
                ("download_update", 0),
                ("uniq_app_day", 37),
                ("app_start", 18),
                ("install_new", 1),
                ("page_view", 90),
                ("uniq_site_day", 28),
                ("uniq_app_day_v_0.1.2", 37),
            ],
        );
        assert_eq!(
            digest(friday(), &day, &week_ago),
            "📊 <b>PoE2 Oracle за пятницу, 25 сентября</b>\n\
             <i>В скобках — неделей раньше, 18 сентября.</i>\n\
             \n\
             Активные установки: <b>41</b> (37)\n\
             Новые установки: <b>3</b> (1)\n\
             Обновления применены: <b>5</b> (0)\n\
             Скачали с сайта (кнопка): <b>12</b> (8)\n\
             \n\
             Установщик скачан апдейтером: <b>9</b> (0)\n\
             Запуски программы: <b>20</b> (18)\n\
             Подключения программы (запуски и переподключения): <b>410</b> (380)\n\
             Проверки обновлений: <b>7</b> (290)\n\
             Скачивания установщика (кнопка и обновления вместе): <b>21</b> (8)\n\
             Скачивания пакета данных: <b>25</b> (0)\n\
             Скачивания SHA256SUMS и подписей: <b>30</b> (22)\n\
             Сайт: просмотров страниц <b>133</b> (90), посетителей <b>40</b> (28)\n\
             Сообщения: <b>7</b> (2)\n\
             🐞 ошибки 3 (1), 💡 идеи 1 (1), 💎 предметы 1 (0), 💥 вылеты 2 (0)\n\
             По версиям (активные установки): 0.1.10 2 (0), 0.1.3 30 (0), 0.1.2 8 (37), other 1 (0)"
        );
    }

    #[test]
    fn a_number_the_service_did_not_keep_that_day_is_a_dash_not_a_zero() {
        // Deployed since the week before: the site's and the app's connections are kept, but no
        // release reports starts yet, so new installs and updates are unknown, not zero.
        let day = counts(
            friday(),
            &[
                ("uniq_app_day", 41),
                ("download_site", 12),
                ("page_view", 9),
            ],
        );
        let week_ago = counts(
            friday().minus(7),
            &[("uniq_app_day", 37), ("download_site", 0), ("page_view", 4)],
        );
        let text = digest(friday(), &day, &week_ago);
        assert!(
            text.contains(
                "Активные установки: <b>41</b> (37)\n\
                 Новые установки: <b>—</b> (—)\n\
                 Обновления применены: <b>—</b> (—)\n\
                 Скачали с сайта (кнопка): <b>12</b> (0)\n"
            ),
            "{text}"
        );
        assert!(!text.contains("Запуски программы"), "{text}");
    }

    #[test]
    fn before_the_new_counters_a_week_ago_is_a_dash_and_the_old_lines_stay() {
        let day = counts(
            friday(),
            &[
                ("uniq_app_day", 41),
                ("download_site", 12),
                ("page_view", 9),
                ("download", 12),
            ],
        );
        let old = counts(friday().minus(7), &OLD);
        let text = digest(friday(), &day, &old);
        assert!(
            text.contains(
                "Активные установки: <b>41</b> (—)\n\
                 Новые установки: <b>—</b> (—)\n\
                 Обновления применены: <b>—</b> (—)\n\
                 Скачали с сайта (кнопка): <b>12</b> (—)\n"
            ),
            "{text}"
        );
        // The old ones read as they always did, a week ago too.
        assert!(
            text.contains("Подключения программы (запуски и переподключения): <b>0</b> (380)\n"),
            "{text}"
        );
        assert!(
            text.contains("Скачивания установщика (кнопка и обновления вместе): <b>12</b> (8)\n"),
            "{text}"
        );
        // And when neither day has them, nothing of the new lines shows.
        let nothing = digest(friday(), &counts(friday(), &OLD), &old);
        for absent in [
            "Установщик скачан апдейтером",
            "Сайт: просмотров",
            "По версиям",
            "Запуски программы",
        ] {
            assert!(!nothing.contains(absent), "{absent}\n{nothing}");
        }
        assert!(
            nothing.contains("Активные установки: <b>—</b> (—)"),
            "{nothing}"
        );
    }

    #[test]
    fn the_digest_names_the_tags_that_brought_a_visit_on_either_day() {
        let day = counts(
            friday(),
            &[
                ("visit_reddit", 14),
                ("visit_discord", 3),
                ("visit_youtube", 0),
            ],
        );
        let week_ago = counts(
            friday().minus(7),
            &[
                ("visit_reddit", 0),
                ("visit_discord", 1),
                ("visit_youtube", 2),
            ],
        );
        let text = digest(friday(), &day, &week_ago);
        assert!(
            text.ends_with(
                "💥 вылеты 0 (0)\n\
                 Переходы на сайт по меткам: <b>17</b> (3)\n\
                 reddit 14 (0), discord 3 (1), youtube 0 (2)"
            ),
            "{text}"
        );
        // No visit through a tagged link on either day: the digest says nothing of them.
        let none = digest(
            friday(),
            &counts(friday(), &[]),
            &counts(friday().minus(7), &[]),
        );
        assert!(none.ends_with("💥 вылеты 0 (0)"), "{none}");
    }

    #[test]
    fn a_week_back_can_cross_a_month() {
        let first = Day::of(SATURDAY).minus(22);
        assert_eq!(first.to_string(), "2026-09-04");
        let text = digest(first, &counts(first, &[]), &counts(first.minus(7), &[]));
        assert!(text.starts_with("📊 <b>PoE2 Oracle за пятницу, 4 сентября</b>\n<i>В скобках — неделей раньше, 28 августа.</i>"), "{text}");
    }
}
