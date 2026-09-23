//! Reporting a problem from inside the app: the repository's GitHub issue forms
//! (`.github/ISSUE_TEMPLATE/bug_report.yml`, `item_problem.yml`) opened in the player's browser
//! with what the app already knows filled in -- its version, the game client's language, the
//! item's text -- the app's own words there in the interface language (the forms' labels are in
//! English and Russian both). The app sends nothing itself: the player reads the whole form and
//! submits it from their own GitHub account, and attaches the diagnostics report
//! (`diagnostics::write_report`) by dragging it in.
//!
//! The query parameters are the forms' field ids; renaming a field in a form breaks its prefill
//! here, so both change together.
//!
//! What would name the player is masked in all a report carries ([`Masker`]): the diagnostics
//! report is meant for a public issue, and a Windows user name is often a real name.

use std::cmp::Reverse;

use item_parser::ItemLanguage;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::tr;

/// The repository's new-issue page; `template` picks the form.
const NEW_ISSUE_URL: &str = "https://github.com/mttzzz/poe2-oracle/issues/new";

/// Longest URL the forms are opened with. GitHub answers `414 URI Too Long` somewhere past 8 KB,
/// and an item text in Russian takes six characters per letter once encoded, so a long one is
/// cut to fit ([`fit_item_text`]).
const MAX_URL_LEN: usize = 7500;

/// Encoded in a query value: everything but RFC 3986's unreserved characters.
pub(crate) const QUERY_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// The general bug form: the version and client language filled in, and the diagnostics field
/// naming the report to drag in -- `report_file`, the zip just written, or why none was.
pub fn bug_report_url(language: Option<ItemLanguage>, report: Result<&str, &str>) -> String {
    let diagnostics = match report {
        Ok(file) => tr!(
            "The report {file} is saved on your desktop and shown in File Explorer: drag it here.",
            file = file
        ),
        Err(error) => tr!("The report couldn't be collected: {error}", error = error),
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
        ("title", tr!("Item: {name}", name = name)),
        ("version", env!("CARGO_PKG_VERSION").to_owned()),
    ];
    fields.extend(language.map(|language| ("game-language", language_label(language).to_owned())));
    let without_text = form_url("item_problem.yml", &fields);
    let room = MAX_URL_LEN.saturating_sub(without_text.len() + "&item-text=".len());
    fields.push(("item-text", fit_item_text(item_text, room)));
    form_url("item_problem.yml", &fields)
}

/// Writes the diagnostics report off the main thread, shows it in Explorer and opens the bug
/// form naming it: the tray's "Сообщить об ошибке" and the settings window's "Сообщить ↗".
/// `summary` is the app's side of the report (`PriceCheckApp::diagnostics_summary`).
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
                let file = path
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                // The name only: the folder is the player's desktop, and the log goes into the
                // next report.
                log::info!("diagnostics report written: {file}");
                diagnostics::reveal(&path);
                Ok(file)
            }
            Err(err) => {
                log::warn!("writing the diagnostics report failed: {err:#}");
                // Into the form, whose error names the folder the report couldn't go to.
                Err(Masker::for_this_user().mask(&format!("{err:#}")))
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
        ItemLanguage::English => tr!("English client"),
        ItemLanguage::Russian => tr!("Russian client"),
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
/// long, at a character -- so that it takes at most `room` characters once encoded, with a note
/// after the cut saying so.
fn fit_item_text(text: &str, room: usize) -> String {
    let text = text.replace("\r\n", "\n");
    let encoded_len = |text: &str| utf8_percent_encode(text, QUERY_VALUE).to_string().len();
    let text = text.trim_end();
    if encoded_len(text) <= room {
        return text.to_owned();
    }
    let cut_note = tr!("\n… (text cut short; the diagnostics report has all of it)");
    let room = room.saturating_sub(encoded_len(cut_note));
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
    format!("{}{cut_note}", kept.trim_end())
}

/// Either slash: Windows takes both in a path.
const SEPARATORS: [char; 2] = ['\\', '/'];

/// A user name shorter than this is part of too many other words to mask on its own.
const MIN_MASKED_NAME: usize = 3;

/// Replaces what would name the player in a text: their user folder with `%USERPROFILE%`; their
/// Desktop, Documents and AppData folders -- wherever Windows keeps them, another drive or a
/// OneDrive folder named after an employer -- with `%DESKTOP%`, `%DOCUMENTS%`, `%APPDATA%` and
/// `%LOCALAPPDATA%`; and their user name, wherever else it stands, with `%USERNAME%`. In any
/// letter case and with either slash, as Windows reads a path. A match counts only as a whole
/// word: the user name "anna" leaves "Hanna" alone.
pub(crate) struct Masker {
    /// Each text with its stand-in, longest first: at one place, the most specific one masks.
    masks: Vec<(String, &'static str)>,
}

impl Masker {
    /// This Windows user's own folders and names.
    #[cfg(target_os = "windows")]
    pub(crate) fn for_this_user() -> Masker {
        use std::path::{Path, PathBuf};

        let base = directories::BaseDirs::new();
        let user = directories::UserDirs::new();
        let profile = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| Some(base.as_ref()?.home_dir().to_path_buf()));
        let text = |folder: Option<&Path>| Some(folder?.to_string_lossy().into_owned());
        let folders = [
            (text(profile.as_deref()), "%USERPROFILE%"),
            (
                text(base.as_ref().map(|dirs| dirs.home_dir())),
                "%USERPROFILE%",
            ),
            (text(base.as_ref().map(|dirs| dirs.data_dir())), "%APPDATA%"),
            (
                text(base.as_ref().map(|dirs| dirs.data_local_dir())),
                "%LOCALAPPDATA%",
            ),
            (
                text(user.as_ref().and_then(|dirs| dirs.desktop_dir())),
                "%DESKTOP%",
            ),
            (
                text(user.as_ref().and_then(|dirs| dirs.document_dir())),
                "%DOCUMENTS%",
            ),
        ];
        // The user folder's own name too: a renamed account keeps its old folder.
        let names = [
            std::env::var("USERNAME").ok(),
            profile
                .as_deref()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned()),
        ];
        Masker::new(
            folders
                .into_iter()
                .filter_map(|(folder, stand_in)| Some((folder?, stand_in))),
            names.into_iter().flatten(),
        )
    }

    fn new<F: AsRef<str>, N: AsRef<str>>(
        folders: impl IntoIterator<Item = (F, &'static str)>,
        names: impl IntoIterator<Item = N>,
    ) -> Masker {
        let folders = folders.into_iter().filter_map(|(folder, stand_in)| {
            let folder = folder.as_ref().trim_end_matches(SEPARATORS);
            // A drive's root is in every path on that drive.
            folder
                .contains(SEPARATORS)
                .then(|| (folder.to_owned(), stand_in))
        });
        let names = names.into_iter().filter_map(|name| {
            let name = name.as_ref();
            (name.chars().count() >= MIN_MASKED_NAME).then(|| (name.to_owned(), "%USERNAME%"))
        });
        let mut masks: Vec<_> = folders.chain(names).collect();
        masks.sort_by_key(|(text, _)| Reverse(text.chars().count()));
        Masker { masks }
    }

    pub(crate) fn mask(&self, text: &str) -> String {
        let mut masked = String::with_capacity(text.len());
        let mut rest = text;
        // Whether `rest` follows a letter or a digit, where no match may start.
        let mut in_word = false;
        'text: while let Some(c) = rest.chars().next() {
            if !in_word {
                for (mask, stand_in) in &self.masks {
                    if let Some(len) = match_len(rest, mask)
                        && !rest[len..].starts_with(char::is_alphanumeric)
                    {
                        masked.push_str(stand_in);
                        rest = &rest[len..];
                        continue 'text;
                    }
                }
            }
            masked.push(c);
            in_word = c.is_alphanumeric();
            rest = &rest[c.len_utf8()..];
        }
        masked
    }
}

/// The length of the start of `text` that reads as `mask`, in any letter case and with either
/// slash for a separator.
fn match_len(text: &str, mask: &str) -> Option<usize> {
    let mut chars = text.char_indices();
    for expected in mask.chars() {
        let (_, actual) = chars.next()?;
        let same = actual == expected
            || (SEPARATORS.contains(&actual) && SEPARATORS.contains(&expected))
            || actual.to_lowercase().eq(expected.to_lowercase());
        if !same {
            return None;
        }
    }
    Some(chars.next().map_or(text.len(), |(index, _)| index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{self, Lang};

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
        let title = param(&url, "title").expect("title");
        assert!(title.contains("Вампирский захват"), "{title}");
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
        let (body, _) = kept.split_once("\n…").expect("says it was cut");
        assert!(body.lines().all(|kept| kept == line.trim_end()), "{body}");
    }

    #[test]
    fn the_bug_form_names_the_report_to_attach_and_the_client() {
        let url = bug_report_url(None, Ok("PoE2-Oracle-report-2026-09-23.zip"));
        assert!(url.contains("template=bug_report.yml"));
        assert_eq!(param(&url, "game-language"), None);
        assert!(
            param(&url, "diagnostics")
                .expect("diagnostics")
                .contains("PoE2-Oracle-report-2026-09-23.zip")
        );
        let english = bug_report_url(Some(ItemLanguage::English), Err("disk full"));
        assert!(
            param(&english, "diagnostics")
                .expect("diagnostics")
                .contains("disk full")
        );
        let russian = bug_report_url(Some(ItemLanguage::Russian), Err("disk full"));
        let clients = [&english, &russian].map(|url| param(url, "game-language"));
        assert!(
            clients.iter().all(Option::is_some) && clients[0] != clients[1],
            "{clients:?}"
        );
    }

    #[test]
    fn the_forms_are_filled_in_the_interface_language() {
        let russian = |text: &String| {
            text.chars()
                .any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
        };
        // An item text with no Russian in it, so that only the app's own words could have some;
        // long enough to be cut, so that the note saying so is among them. The two forms name
        // different clients, so both names are checked.
        let filled = || {
            let bug = bug_report_url(Some(ItemLanguage::Russian), Ok("report.zip"));
            let failed = bug_report_url(None, Err("disk full"));
            let item =
                item_problem_url(Some(ItemLanguage::English), "Ring", &"Ring\n".repeat(2000));
            [
                param(&bug, "diagnostics"),
                param(&bug, "game-language"),
                param(&failed, "diagnostics"),
                param(&item, "title"),
                param(&item, "game-language"),
                param(&item, "item-text"),
            ]
            .map(|value| value.expect("filled in"))
        };
        let english = i18n::with_lang(Lang::English, filled);
        assert!(!english.iter().any(russian), "{english:#?}");
        let in_russian = i18n::with_lang(Lang::Russian, filled);
        assert!(in_russian.iter().all(russian), "{in_russian:#?}");
    }

    /// Kiril's folders: the user folder, AppData in it, the desktop moved to another drive and the
    /// documents in a work OneDrive.
    fn kirils_masker() -> Masker {
        Masker::new(
            [
                (r"C:\Users\Kiril", "%USERPROFILE%"),
                (r"C:\Users\Kiril\AppData\Roaming", "%APPDATA%"),
                (r"C:\Users\Kiril\AppData\Local\", "%LOCALAPPDATA%"),
                (r"D:\Kiril\Desktop", "%DESKTOP%"),
                (
                    r"C:\Users\Kiril\OneDrive - Contoso\Documents",
                    "%DOCUMENTS%",
                ),
            ],
            ["Kiril"],
        )
    }

    #[test]
    fn the_player_s_folders_are_masked_as_windows_reads_a_path() {
        let masker = kirils_masker();
        for (text, masked) in [
            // The most specific folder, whatever the letter case or the slash.
            (
                r"log: C:\Users\Kiril\AppData\Local\poe2-oracle\data\logs",
                r"log: %LOCALAPPDATA%\poe2-oracle\data\logs",
            ),
            (r"c:\users\KIRIL\Saved Games", r"%USERPROFILE%\Saved Games"),
            (
                "C:/Users/kiril/AppData/Roaming/poe2-oracle",
                "%APPDATA%/poe2-oracle",
            ),
            (
                r"written to D:\Kiril\Desktop\PoE2-Oracle-report.zip",
                r"written to %DESKTOP%\PoE2-Oracle-report.zip",
            ),
            (
                r"C:\Users\Kiril\OneDrive - Contoso\Documents\My Games\Path of Exile 2",
                r"%DOCUMENTS%\My Games\Path of Exile 2",
            ),
            // Another user's folder that merely starts the same.
            (r"C:\Users\Kirill\Desktop", r"C:\Users\Kirill\Desktop"),
        ] {
            assert_eq!(masker.mask(text), masked, "{text}");
        }
    }

    #[test]
    fn the_user_name_is_masked_wherever_it_stands_as_a_word() {
        let masker = kirils_masker();
        assert_eq!(
            masker.mask(r"E:\Games\kiril\PoE2 and backup_Kiril.zip"),
            r"E:\Games\%USERNAME%\PoE2 and backup_%USERNAME%.zip"
        );
        assert_eq!(
            masker.mask("Kirill, Kirilov and MrKiril keep their names"),
            "Kirill, Kirilov and MrKiril keep their names"
        );
        // Cyrillic letter case too: a Russian player's folder is often named in Russian.
        let masker = Masker::new([(r"C:\Users\Кирилл", "%USERPROFILE%")], ["Кирилл"]);
        assert_eq!(
            masker.mask(r"C:\USERS\КИРИЛЛ\Desktop, кирилл"),
            r"%USERPROFILE%\Desktop, %USERNAME%"
        );
        // Two letters are in too many words to be told apart.
        let masker = Masker::new([(r"C:\Users\Al", "%USERPROFILE%")], ["Al"]);
        assert_eq!(
            masker.mask(r"C:\Users\Al\Desktop: Al"),
            r"%USERPROFILE%\Desktop: Al"
        );
    }

    #[test]
    fn a_folder_at_a_drive_s_root_is_left_alone() {
        // Documents moved to the root of D: would otherwise mask every path on that drive.
        let masker = Masker::new([(r"D:\", "%DOCUMENTS%")], std::iter::empty::<&str>());
        assert_eq!(
            masker.mask(r"D:\Games\Path of Exile 2"),
            r"D:\Games\Path of Exile 2"
        );
    }
}
