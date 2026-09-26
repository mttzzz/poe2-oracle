//! A report as a GitHub issue: its title, labels and Markdown body, in Russian for the owner.
//!
//! Whatever the sender wrote is shown literally. Free text goes into fenced code blocks whose
//! fence is longer than any run of backticks inside, so no line of it can close the block early;
//! one-line values go into code spans built the same way. Neither renders Markdown, links, HTML or
//! @mentions, so a report can't ping strangers on GitHub or restyle the issue around it.

use std::fmt::Write as _;

use oracle_protocol::{Report, ReportKind, ReportSource};

/// GitHub refuses a longer issue body. Counted here in UTF-16 units, never fewer than the
/// characters GitHub counts.
const BODY_LIMIT: usize = 65_536;
/// Kept free under [`BODY_LIMIT`] for the notes on what was cut.
const BODY_MARGIN: usize = 512;
/// Titles are cut to this many characters, to stay readable in the issue list.
const TITLE_CHARS: usize = 80;
/// One-line values -- the version, the league, the contact -- are cut to this many characters,
/// which the protocol's limits ([`oracle_protocol::MAX_CONTEXT_CHARS`], the contact's) keep them
/// within anyway.
const VALUE_CHARS: usize = 200;

/// The kind's name, as titles and messages show it.
pub fn kind_name(kind: ReportKind) -> &'static str {
    match kind {
        ReportKind::Bug => "Ошибка",
        ReportKind::Idea => "Идея",
        ReportKind::Item => "Предмет",
        ReportKind::Crash => "Вылет",
    }
}

/// The labels an issue gets: its kind, and where it was written.
pub fn labels(report: &Report) -> [&'static str; 2] {
    let kind = match report.kind {
        ReportKind::Bug => "bug",
        ReportKind::Idea => "enhancement",
        ReportKind::Item => "item",
        ReportKind::Crash => "crash",
    };
    let source = match report.source {
        ReportSource::App => "from-app",
        ReportSource::Site => "from-site",
    };
    [kind, source]
}

/// `[Ошибка] <first line of the text>`, `[Предмет] <item name>` or `[Вылет] <first line of the
/// panic>`, at most [`TITLE_CHARS`] characters.
pub fn title(report: &Report) -> String {
    let subject = match report.kind {
        ReportKind::Item => report.item.as_ref().map(|item| one_line(&item.name)),
        ReportKind::Crash => report.crash.as_deref().map(first_line),
        ReportKind::Bug | ReportKind::Idea => None,
    }
    .filter(|subject| !subject.is_empty())
    .unwrap_or_else(|| first_line(&report.text));
    let subject = if subject.is_empty() {
        "без описания".to_owned()
    } else {
        subject
    };
    let prefix = format!("[{}] ", kind_name(report.kind));
    let room = TITLE_CHARS - prefix.chars().count();
    prefix + &shorten(&subject, room)
}

/// The issue's body. `telegram` says whether the report's files also go to Telegram, which the
/// body tells about the diagnostics zip and about texts cut here.
pub fn body(report: &Report, telegram: bool) -> String {
    let mut head = String::new();
    let source = match report.source {
        ReportSource::App => "сообщение из программы",
        ReportSource::Site => "сообщение с сайта",
    };
    let _ = writeln!(head, "**{}** — {source}\n", kind_name(report.kind));
    if let Some(app) = &report.app {
        fact(&mut head, "Версия", Some(&app.version));
        fact(&mut head, "Язык интерфейса", Some(&app.interface_language));
        fact(
            &mut head,
            "Язык клиента игры",
            app.client_language.as_deref(),
        );
        fact(&mut head, "Windows", app.windows.as_deref());
        fact(&mut head, "Лига", app.league.as_deref());
        if let Some(scale) = app.ui_scale {
            let _ = writeln!(head, "- Масштаб интерфейса: {scale}");
        }
    }
    match report
        .contact
        .as_deref()
        .map(one_line)
        .filter(|contact| !contact.is_empty())
    {
        Some(contact) => fact(&mut head, "Контакт", Some(&contact)),
        None => head.push_str("- Контакт: не оставлен\n"),
    }
    let text = report.text.trim();
    if !text.is_empty() {
        head.push_str("\n#### Текст\n\n");
        // At most 8000 characters: never cut in practice, whatever its backticks.
        head.push_str(&fenced(text, BODY_LIMIT / 2, ""));
    }

    let item = report.item.as_ref().map(|item| {
        (
            format!("\n#### Предмет: {}\n\n", code(&item.name)),
            item.text.as_str(),
        )
    });
    let crash = report
        .crash
        .as_deref()
        .map(|crash| ("\n#### Вылет\n\n".to_owned(), crash));
    let footer = footer(report, telegram);
    let fixed = width(&head)
        + item.as_ref().map_or(0, |(heading, _)| width(heading))
        + crash.as_ref().map_or(0, |(heading, _)| width(heading))
        + 1
        + width(&footer);
    // The item's text and the crash share what's left, and each goes whole to Telegram as a file.
    let room = BODY_LIMIT.saturating_sub(fixed + BODY_MARGIN);
    let wants = [
        item.as_ref()
            .map_or(0, |(_, text)| width(&fenced(text, usize::MAX, ""))),
        crash
            .as_ref()
            .map_or(0, |(_, text)| width(&fenced(text, usize::MAX, ""))),
    ];
    let [item_room, crash_room] = share(room, wants);
    let whole = if telegram {
        "; целиком — в Telegram"
    } else {
        ""
    };

    let mut body = head;
    if let Some((heading, text)) = item {
        body.push_str(&heading);
        body.push_str(&fenced(text, item_room, whole));
    }
    if let Some((heading, text)) = crash {
        body.push_str(&heading);
        body.push_str(&fenced(text, crash_room, whole));
    }
    body.push('\n');
    body.push_str(&footer);
    body
}

/// Where the diagnostics zip went. The issue can't carry it: GitHub's API takes no attachments.
fn footer(report: &Report, telegram: bool) -> String {
    match (&report.diagnostics, telegram) {
        (Some(zip), true) => format!(
            "📎 Диагностика (zip, {}) — в Telegram, ответом на сообщение об этом отчёте.\n",
            size(zip.len())
        ),
        (Some(zip), false) => format!(
            "📎 Диагностика (zip, {}) не сохранилась: Telegram не настроен.\n",
            size(zip.len())
        ),
        (None, _) if report.source == ReportSource::App => "Диагностику не приложили.\n".to_owned(),
        (None, _) => String::new(),
    }
}

/// `- label: value` with the value as a code span; nothing for a missing or blank value.
fn fact(out: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        let _ = writeln!(out, "- {label}: {}", code(value));
    }
}

/// `value` on one line, cut to [`VALUE_CHARS`], as a Markdown code span: delimited by more
/// backticks than any run inside it, and padded with spaces when it starts or ends with one.
fn code(value: &str) -> String {
    let value = shorten(&one_line(value), VALUE_CHARS);
    let ticks = "`".repeat(longest_run(&value, '`') + 1);
    let pad = if value.starts_with('`') || value.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{ticks}{pad}{value}{pad}{ticks}")
}

/// `content` as a fenced `text` block of at most `room` UTF-16 units. Cut at the end when the
/// whole doesn't fit, with a note on how much is shown, which `whole` completes.
fn fenced(content: &str, room: usize, whole: &str) -> String {
    let content = content
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_end()
        .trim_start_matches('\n')
        .to_owned();
    let fence = "`".repeat(longest_run(&content, '`').max(2) + 1);
    let block = |text: &str| format!("{fence}text\n{text}\n{fence}\n");
    let full = block(&content);
    if width(&full) <= room {
        return full;
    }
    let budget = room.saturating_sub(width(&block("")) + 120 + width(whole));
    if budget < 100 {
        return format!("_Не поместилось{whole}._\n");
    }
    let kept = cut(&content, budget);
    format!(
        "{}_…обрезано: показано {} из {} символов{whole}._\n",
        block(kept),
        kept.chars().count(),
        content.chars().count()
    )
}

/// Splits `room` between two parts that want `wants`: each gets what it wants if both fit; else
/// one wanting at most half gets all of it and the other the rest; else half each.
fn share(room: usize, wants: [usize; 2]) -> [usize; 2] {
    let [first, second] = wants;
    let half = room / 2;
    if first + second <= room {
        wants
    } else if first <= half {
        [first, room - first]
    } else if second <= half {
        [room - second, second]
    } else {
        [half, room - half]
    }
}

/// The start of `text` that fits `budget` UTF-16 units, ending at a line break when one is in its
/// second half.
fn cut(text: &str, budget: usize) -> &str {
    let mut used = 0;
    let mut end = 0;
    for (at, char) in text.char_indices() {
        used += char.len_utf16();
        if used > budget {
            break;
        }
        end = at + char.len_utf8();
    }
    let kept = &text[..end];
    match kept.rfind('\n') {
        Some(line_end) if line_end >= end / 2 => &kept[..line_end],
        _ => kept,
    }
}

/// The first line of `text` that has something on it, as [`one_line`] cleans it.
fn first_line(text: &str) -> String {
    text.lines()
        .map(one_line)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

/// `text` on one line: every run of whitespace a single space, other control characters gone.
fn one_line(text: &str) -> String {
    let mut line = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !line.is_empty() {
            line.push(' ');
        }
        line.extend(word.chars().filter(|char| !char.is_control()));
    }
    line
}

/// `text` cut to `max` characters, the last of them an ellipsis when it was longer.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

fn longest_run(text: &str, of: char) -> usize {
    let (mut longest, mut run) = (0, 0);
    for char in text.chars() {
        run = if char == of { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

/// Length as GitHub's limit is checked here: UTF-16 units.
fn width(text: &str) -> usize {
    text.encode_utf16().count()
}

/// A file size for the owner: `512 Б`, `340 КБ`, `1,2 МБ`.
pub fn size(bytes: usize) -> String {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * 1024;
    if bytes < KIB {
        format!("{bytes} Б")
    } else if bytes < MIB {
        format!("{} КБ", (bytes + KIB / 2) / KIB)
    } else {
        let tenths = (bytes * 10 + MIB / 2) / MIB;
        format!("{},{} МБ", tenths / 10, tenths % 10)
    }
}

#[cfg(test)]
mod tests {
    use oracle_protocol::{
        AppContext, MAX_CONTACT_CHARS, MAX_CRASH_BYTES, MAX_ITEM_TEXT_BYTES, MAX_TEXT_CHARS,
        ReportItem,
    };

    use super::*;

    fn report(kind: ReportKind, text: &str) -> Report {
        Report {
            kind,
            source: ReportSource::App,
            text: text.to_owned(),
            contact: None,
            app: Some(AppContext {
                version: "0.1.0".to_owned(),
                interface_language: "ru".to_owned(),
                client_language: Some("en".to_owned()),
                windows: Some("Windows 11 Pro 24H2 (26100.4061)".to_owned()),
                league: Some("Forbidden Rites".to_owned()),
                ui_scale: Some(1.25),
            }),
            item: None,
            crash: None,
            diagnostics: None,
            website: String::new(),
        }
    }

    #[test]
    fn titles_take_the_first_line_and_stay_short() {
        let bug = report(
            ReportKind::Bug,
            "\n  \n  Цена  не\tпоявилась \nподробности ниже",
        );
        assert_eq!(title(&bug), "[Ошибка] Цена не появилась");

        let long = report(ReportKind::Idea, &"очень длинная идея ".repeat(10));
        let long_title = title(&long);
        assert_eq!(long_title.chars().count(), TITLE_CHARS);
        assert!(long_title.starts_with("[Идея] очень длинная идея"));
        assert!(long_title.ends_with('…'));

        let mut item = report(ReportKind::Item, "цена странная");
        item.item = Some(ReportItem {
            name: "Жуть\nшлема".to_owned(),
            text: "Item Class: Helmets".to_owned(),
        });
        assert_eq!(title(&item), "[Предмет] Жуть шлема");

        let mut crash = report(ReportKind::Crash, "");
        crash.crash = Some("\nthread 'main' panicked at src/app.rs:12:5:\nboom".to_owned());
        assert_eq!(
            title(&crash),
            "[Вылет] thread 'main' panicked at src/app.rs:12:5:"
        );
        crash.crash = Some(String::new());
        assert_eq!(title(&crash), "[Вылет] без описания");
    }

    #[test]
    fn a_code_fence_in_the_text_cannot_close_the_block() {
        let text = "до\n```\n# Заголовок @mttzzz <b>html</b>\n```\nпосле";
        let body = body(&report(ReportKind::Bug, text), true);
        assert!(
            body.contains(&format!("\n````text\n{text}\n````\n")),
            "{body}"
        );
        // The longest run inside decides the fence, whatever it is.
        let body = super::body(&report(ReportKind::Bug, "a ````` b"), true);
        assert!(body.contains("``````text\na ````` b\n``````\n"), "{body}");
    }

    #[test]
    fn one_line_values_are_code_spans_that_hold_their_backticks() {
        assert_eq!(code("Forbidden Rites"), "`Forbidden Rites`");
        assert_eq!(code("a`b"), "``a`b``");
        assert_eq!(code("`x`"), "`` `x` ``");
        assert_eq!(code("tg: @nick\n# heading"), "`tg: @nick # heading`");
        let mut with_contact = report(ReportKind::Bug, "текст");
        with_contact.contact = Some("  ```@evil```  ".to_owned());
        assert!(body(&with_contact, true).contains("- Контакт: ```` ```@evil``` ````\n"));
    }

    #[test]
    fn the_body_lists_the_context_and_where_the_zip_went() {
        let mut bug = report(ReportKind::Bug, "Не открывается панель");
        bug.contact = Some("tg @player".to_owned());
        bug.diagnostics = Some(vec![0; 1_300_000]);
        let body = body(&bug, true);
        for line in [
            "**Ошибка** — сообщение из программы\n",
            "- Версия: `0.1.0`\n",
            "- Язык интерфейса: `ru`\n",
            "- Язык клиента игры: `en`\n",
            "- Windows: `Windows 11 Pro 24H2 (26100.4061)`\n",
            "- Лига: `Forbidden Rites`\n",
            "- Масштаб интерфейса: 1.25\n",
            "- Контакт: `tg @player`\n",
            "📎 Диагностика (zip, 1,2 МБ) — в Telegram, ответом на сообщение об этом отчёте.\n",
        ] {
            assert!(body.contains(line), "no {line:?} in\n{body}");
        }

        let mut site = report(ReportKind::Idea, "Идея");
        site.source = ReportSource::Site;
        site.app = None;
        let body = super::body(&site, true);
        assert!(body.starts_with("**Идея** — сообщение с сайта\n\n- Контакт: не оставлен\n"));
        assert!(!body.contains("Диагностик"));
    }

    #[test]
    fn the_largest_report_fits_github_with_cuts_noted() {
        let mut crash = report(ReportKind::Crash, &"т".repeat(MAX_TEXT_CHARS));
        crash.contact = Some("к".repeat(MAX_CONTACT_CHARS));
        crash.app.as_mut().unwrap().league = Some("`".repeat(5000));
        crash.item = Some(ReportItem {
            name: "`".repeat(200),
            text: "Rarity: Rare\n".repeat(MAX_ITEM_TEXT_BYTES / 13),
        });
        // Four-byte characters: two UTF-16 units each.
        crash.crash = Some("😀".repeat(MAX_CRASH_BYTES / 4));
        crash.diagnostics = Some(vec![0; 10]);
        let body = body(&crash, true);
        assert!(width(&body) <= BODY_LIMIT, "{} units", width(&body));
        assert!(body.contains("символов; целиком — в Telegram._\n"));
        // The text itself is never cut.
        assert!(body.contains(&"т".repeat(MAX_TEXT_CHARS)));
    }

    #[test]
    fn a_report_that_fits_is_not_cut() {
        let mut item = report(ReportKind::Item, "дорого");
        let text = "Item Class: Rings\nRarity: Rare\n".repeat(400);
        item.item = Some(ReportItem {
            name: "Кольцо".to_owned(),
            text: text.clone(),
        });
        let body = body(&item, true);
        assert!(body.contains(&format!(
            "#### Предмет: `Кольцо`\n\n```text\n{}\n```\n",
            text.trim_end()
        )));
        assert!(!body.contains("обрезано"));
    }

    #[test]
    fn two_parts_share_the_room_fairly() {
        assert_eq!(share(100, [30, 50]), [30, 50]);
        assert_eq!(share(100, [30, 90]), [30, 70]);
        assert_eq!(share(100, [90, 30]), [70, 30]);
        assert_eq!(share(100, [60, 90]), [50, 50]);
        assert_eq!(share(100, [0, 500]), [0, 100]);
    }

    #[test]
    fn cuts_prefer_a_line_break_and_count_utf16() {
        assert_eq!(cut("abc\ndef\nghi", 9), "abc\ndef");
        assert_eq!(cut("abcdefghij", 4), "abcd");
        assert_eq!(cut("😀😀😀", 5), "😀😀");
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(size(512), "512 Б");
        assert_eq!(size(340 * 1024 + 100), "340 КБ");
        assert_eq!(size(1_300_000), "1,2 МБ");
        assert_eq!(size(8 * 1024 * 1024), "8,0 МБ");
    }
}
