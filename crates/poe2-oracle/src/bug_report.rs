//! Reporting a problem from inside the app: the repository's GitHub issue forms
//! (`.github/ISSUE_TEMPLATE/bug_report.yml`, `item_problem.yml`) opened in the player's browser
//! with what the app already knows filled in -- its version, the game client's language, the
//! item's text. The app sends nothing itself: the player reads the whole form and submits it from
//! their own GitHub account, and attaches the diagnostics report (`diagnostics::write_report`)
//! by dragging it in.
//!
//! The query parameters are the forms' field ids; renaming a field in a form breaks its prefill
//! here, so both change together.

use item_parser::ItemLanguage;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// The repository's new-issue page; `template` picks the form.
const NEW_ISSUE_URL: &str = "https://github.com/mttzzz/poe2-oracle/issues/new";

/// Longest URL the forms are opened with. GitHub answers `414 URI Too Long` somewhere past 8 KB,
/// and an item text in Russian takes six characters per letter once encoded, so a long one is
/// cut to fit ([`fit_item_text`]).
const MAX_URL_LEN: usize = 7500;

/// Encoded in a query value: everything but RFC 3986's unreserved characters.
const QUERY_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Appended where [`fit_item_text`] cut an item text short.
const CUT_NOTE: &str = "\n… (текст обрезан, полностью он есть в отчёте диагностики)";

/// The general bug form: the version and client language filled in, and the diagnostics field
/// naming the report to drag in -- `report_file`, the zip just written, or why none was.
pub fn bug_report_url(language: Option<ItemLanguage>, report: Result<&str, &str>) -> String {
    let diagnostics = match report {
        Ok(file) => format!(
            "Отчёт {file} сохранён на рабочем столе и открыт в Проводнике: перетащите его сюда."
        ),
        Err(error) => format!("Отчёт собрать не удалось: {error}"),
    };
    let mut fields = vec![("version", env!("CARGO_PKG_VERSION").to_owned())];
    fields.extend(language.map(|language| ("game-language", language_label(language).to_owned())));
    fields.push(("diagnostics", diagnostics));
    form_url("bug_report.yml", &fields)
}

/// The item form for an item the app misread or mispriced: its text, as the game copied it, is
/// what reproduces the problem. `name` titles the issue.
pub fn item_problem_url(language: Option<ItemLanguage>, name: &str, item_text: &str) -> String {
    let mut fields = vec![
        ("title", format!("Предмет: {name}")),
        ("version", env!("CARGO_PKG_VERSION").to_owned()),
    ];
    fields.extend(language.map(|language| ("game-language", language_label(language).to_owned())));
    let without_text = form_url("item_problem.yml", &fields);
    let room = MAX_URL_LEN.saturating_sub(without_text.len() + "&item-text=".len());
    fields.push(("item-text", fit_item_text(item_text, room)));
    form_url("item_problem.yml", &fields)
}

/// Writes the diagnostics report off the main thread, shows it in Explorer and opens the bug
/// form naming it: the tray's and the settings window's "Сообщить об ошибке". `summary` is the
/// app's side of the report (`PriceCheckApp::diagnostics_summary`).
#[cfg(target_os = "windows")]
pub fn report_bug(summary: String, language: Option<ItemLanguage>, cx: &mut gpui::App) {
    use crate::diagnostics;

    cx.spawn(async move |cx| {
        let written = cx
            .background_executor()
            .spawn(async move { diagnostics::write_report(&summary) })
            .await;
        let report = match written {
            Ok(path) => {
                log::info!("diagnostics report written to {}", path.display());
                diagnostics::reveal(&path);
                Ok(path
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned()))
            }
            Err(err) => {
                log::warn!("writing the diagnostics report failed: {err:#}");
                Err(format!("{err:#}"))
            }
        };
        let url = bug_report_url(language, report.as_deref().map_err(String::as_str));
        cx.update(|cx| cx.open_url(&url));
    })
    .detach();
}

/// How the form's client-language field names the client an item text came from.
fn language_label(language: ItemLanguage) -> &'static str {
    match language {
        ItemLanguage::English => "English client",
        ItemLanguage::Russian => "Русский клиент",
    }
}

fn form_url(template: &str, fields: &[(&str, String)]) -> String {
    let mut url = format!("{NEW_ISSUE_URL}?template={template}");
    for (id, value) in fields {
        url.push('&');
        url.push_str(id);
        url.push('=');
        url.extend(utf8_percent_encode(value, QUERY_VALUE));
    }
    url
}

/// `text` with Windows line breaks made plain, cut at a line break -- or, for a single line too
/// long, at a character -- so that it takes at most `room` characters once encoded, with
/// [`CUT_NOTE`] saying so.
fn fit_item_text(text: &str, room: usize) -> String {
    let text = text.replace("\r\n", "\n");
    let encoded_len = |text: &str| utf8_percent_encode(text, QUERY_VALUE).to_string().len();
    let text = text.trim_end();
    if encoded_len(text) <= room {
        return text.to_owned();
    }
    let room = room.saturating_sub(encoded_len(CUT_NOTE));
    let mut kept = String::new();
    for line in text.split_inclusive('\n') {
        if encoded_len(&kept) + encoded_len(line) > room {
            break;
        }
        kept.push_str(line);
    }
    if kept.is_empty() {
        for c in text.chars() {
            if encoded_len(&kept) + encoded_len(c.encode_utf8(&mut [0; 4])) > room {
                break;
            }
            kept.push(c);
        }
    }
    format!("{}{CUT_NOTE}", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The value of query parameter `id` in `url`, decoded.
    fn param(url: &str, id: &str) -> Option<String> {
        let query = url.split_once('?')?.1;
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == id).then(|| {
                percent_encoding::percent_decode_str(value)
                    .decode_utf8()
                    .expect("utf-8")
                    .into_owned()
            })
        })
    }

    #[test]
    fn the_item_form_carries_the_text_the_game_copied() {
        let text = "Класс предмета: Кольца\r\nРедкость: Редкий\r\nВампирский захват\r\n\
                    Кольцо с сапфиром\r\n--------\r\n+9 к ловкости (15% & 7#)\r\n";
        let url = item_problem_url(Some(ItemLanguage::Russian), "Вампирский захват", text);
        assert!(url.starts_with(
            "https://github.com/mttzzz/poe2-oracle/issues/new?template=item_problem.yml&"
        ));
        assert_eq!(
            param(&url, "item-text").as_deref(),
            Some(
                "Класс предмета: Кольца\nРедкость: Редкий\nВампирский захват\nКольцо с сапфиром\n\
                 --------\n+9 к ловкости (15% & 7#)"
            )
        );
        assert_eq!(
            param(&url, "game-language").as_deref(),
            Some("Русский клиент")
        );
        assert_eq!(
            param(&url, "title").as_deref(),
            Some("Предмет: Вампирский захват")
        );
        assert_eq!(
            param(&url, "version").as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn a_long_item_text_is_cut_at_a_line_to_fit_the_url() {
        let line = "Добавляет от 10 до 20 урона от молнии к атакам\n";
        let text = line.repeat(100);
        let url = item_problem_url(Some(ItemLanguage::Russian), "Кольцо", &text);
        assert!(url.len() <= MAX_URL_LEN, "{}", url.len());
        let kept = param(&url, "item-text").expect("item text");
        let (body, note) = kept.split_once("\n…").expect("says it was cut");
        assert!(body.lines().all(|kept| kept == line.trim_end()), "{body}");
        assert!(note.contains("обрезан"));
    }

    #[test]
    fn the_bug_form_names_the_report_to_attach() {
        let url = bug_report_url(None, Ok("PoE2-Oracle-report-2026-09-23.zip"));
        assert!(url.contains("template=bug_report.yml"));
        assert_eq!(param(&url, "game-language"), None);
        assert!(
            param(&url, "diagnostics")
                .expect("diagnostics")
                .contains("PoE2-Oracle-report-2026-09-23.zip")
        );
        let failed = bug_report_url(Some(ItemLanguage::English), Err("disk full"));
        assert_eq!(
            param(&failed, "game-language").as_deref(),
            Some("English client")
        );
        assert!(
            param(&failed, "diagnostics")
                .expect("diagnostics")
                .contains("disk full")
        );
    }
}
