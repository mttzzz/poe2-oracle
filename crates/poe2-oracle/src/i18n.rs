//! The interface language: the language of the app's own words -- labels, messages, tooltips and
//! number formats. Game text keeps the language it arrives in: item names and mod lines as the
//! client copied them, league names as the trade site gives them.
//!
//! The words are written in English in the code and translated through tables, as EE2's
//! `app_i18n.json`: `assets/i18n/ru/*.json` map each English text to its Russian, one file per
//! area of the app (settings, panel, overlays, tour, report) so a change to one area touches one
//! file. A new language is new tables, not a code change.
//!
//! - [`tr!`]`("Search")` is the text in the interface language, and
//!   `tr!("Found: {count}", count = total)` fills in named placeholders.
//! - [`tr_n!`]`(total, "{n} listing|{n} listings")` also picks the plural form for the count:
//!   English has two forms, `|`-separated; Russian's table entry has three
//!   (`"{n} лот|{n} лота|{n} лотов"`). `{n}` is the count.
//! - [`decimal`] writes a number with the language's decimal separator; [`number`] and
//!   [`compact`] write prices and rates the way the panel shows them, [`integer`] a count,
//!   [`percent`] a percentage, [`duration`] and [`duration_secs`] a length of time, [`day_month`]
//!   a date.
//!
//! A test reads every `tr!`/`tr_n!` in the sources and checks that each has a Russian text in
//! exactly one file, that the tables hold nothing else, and that every placeholder survives the
//! translation.
//!
//! One process-wide language, read anywhere without a context through [`lang`]. [`apply`] sets it
//! from the settings on start and on every change; the windows then redraw. A test speaks either
//! language on its own thread through `with_lang`.

use std::collections::HashMap;
use std::fmt::{Display, Write as _};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use trade_client::TradeSite;

use crate::settings::InterfaceLanguage;

/// A language the app's own words come in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Russian,
    English,
}

impl Lang {
    /// The language's code, as the game's config and the trade sites name it.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Russian => "ru",
            Lang::English => "en",
        }
    }

    /// The trade site in this language, where the interface's league names come from:
    /// `ru.pathofexile.com` for Russian, `www` for English.
    pub fn trade_site(self) -> TradeSite {
        match self {
            Lang::Russian => TradeSite::Russian,
            Lang::English => TradeSite::International,
        }
    }

    /// Which of `forms` a count takes: English `1 listing`, `2 listings`; Russian `1 лот`,
    /// `2 лота`, `5 лотов` (and `21 лот`, `11 лотов`). A table with fewer forms than the language
    /// has gets its last one.
    fn plural_form<'a>(self, forms: &[&'a str], count: u64) -> &'a str {
        let index = match self {
            Lang::English => usize::from(count != 1),
            Lang::Russian => match (count % 10, count % 100) {
                (1, rest) if rest != 11 => 0,
                (2..=4, rest) if !(12..=14).contains(&rest) => 1,
                _ => 2,
            },
        };
        forms[index.min(forms.len() - 1)]
    }
}

/// The current language: `Lang::Russian` as `0`, `Lang::English` as `1`. Russian until
/// [`apply`] runs, as the app spoke before it had a choice.
static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The language the app's words are in now.
pub fn lang() -> Lang {
    #[cfg(test)]
    if let Some(lang) = TEST_LANG.get() {
        return lang;
    }
    match CURRENT.load(Ordering::Relaxed) {
        0 => Lang::Russian,
        _ => Lang::English,
    }
}

#[cfg(test)]
thread_local! {
    /// The language [`with_lang`] makes a test's thread speak, whatever the process-wide one is.
    static TEST_LANG: std::cell::Cell<Option<Lang>> = const { std::cell::Cell::new(None) };
}

/// `f`'s result with this thread speaking `lang`: the tests run in parallel, and one checking the
/// English words must not flip the language under another checking the Russian ones.
#[cfg(test)]
pub fn with_lang<T>(lang: Lang, f: impl FnOnce() -> T) -> T {
    let previous = TEST_LANG.replace(Some(lang));
    let result = f();
    TEST_LANG.set(previous);
    result
}

#[cfg(target_os = "windows")]
fn set_lang(lang: Lang) {
    let value = match lang {
        Lang::Russian => 0,
        Lang::English => 1,
    };
    CURRENT.store(value, Ordering::Relaxed);
}

/// What the settings' choice means. «Авто» follows the game client's language, which its config
/// names (`[LANGUAGE] language=ru`); before the game's first run there is no config, and the
/// Windows display language decides. Russian is the only translation so far, so every other
/// language gets English.
pub fn resolve(
    choice: InterfaceLanguage,
    game_language: Option<&str>,
    windows_is_russian: bool,
) -> Lang {
    match choice {
        InterfaceLanguage::Russian => Lang::Russian,
        InterfaceLanguage::English => Lang::English,
        InterfaceLanguage::Auto => match game_language {
            Some(code) if code.eq_ignore_ascii_case("ru") => Lang::Russian,
            Some(_) => Lang::English,
            None if windows_is_russian => Lang::Russian,
            None => Lang::English,
        },
    }
}

/// What «Авто» stands for now: the game client's language from its config, or -- before the
/// game's first run -- the Windows display language ([`resolve`]).
#[cfg(target_os = "windows")]
pub fn auto() -> Lang {
    let (game_language, windows_is_russian) = system_languages();
    resolve(
        InterfaceLanguage::Auto,
        game_language.as_deref(),
        windows_is_russian,
    )
}

/// The game client's language as its config names it, and whether the Windows display language
/// is Russian: what «Авто» goes by.
#[cfg(target_os = "windows")]
fn system_languages() -> (Option<String>, bool) {
    let game_language = crate::platform::game_config::read().language;
    // `GetUserDefaultUILanguage` is the display language; its low 10 bits are the primary
    // language, and 0x19 is LANG_RUSSIAN (`winnt.h`).
    let windows_is_russian =
        unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() } & 0x3ff == 0x19;
    (game_language, windows_is_russian)
}

/// Makes `choice` the app's language: [`resolve`]d against the game's config and the Windows
/// display language as they are now. Whether the language changed -- the windows need a redraw
/// then.
#[cfg(target_os = "windows")]
pub fn apply(choice: InterfaceLanguage) -> bool {
    let (game_language, windows_is_russian) = system_languages();
    let new = resolve(choice, game_language.as_deref(), windows_is_russian);
    let changed = new != lang();
    set_lang(new);
    if changed {
        log::info!("interface language: {}", new.code());
    }
    changed
}

/// The Russian tables, each area's file as (its name, its JSON).
const RUSSIAN_FILES: [(&str, &str); 5] = [
    ("settings", include_str!("../assets/i18n/ru/settings.json")),
    ("panel", include_str!("../assets/i18n/ru/panel.json")),
    ("overlays", include_str!("../assets/i18n/ru/overlays.json")),
    ("tour", include_str!("../assets/i18n/ru/tour.json")),
    ("report", include_str!("../assets/i18n/ru/report.json")),
];

/// Each Russian table's entries, by file: English text -> Russian text.
fn russian_files() -> impl Iterator<Item = (&'static str, HashMap<String, String>)> {
    RUSSIAN_FILES.into_iter().map(|(name, json)| {
        let entries = serde_json::from_str(json)
            .unwrap_or_else(|err| panic!("assets/i18n/ru/{name}.json: {err}"));
        (name, entries)
    })
}

/// The Russian tables merged: English text -> Russian text.
static RUSSIAN: LazyLock<HashMap<String, String>> =
    LazyLock::new(|| russian_files().flat_map(|(_, entries)| entries).collect());

/// `english` in the current language: itself in English; its table entry otherwise, or the
/// English again when the table misses it (the sources test keeps that from shipping).
pub fn text(english: &'static str) -> &'static str {
    match lang() {
        Lang::English => english,
        Lang::Russian => RUSSIAN.get(english).map_or(english, String::as_str),
    }
}

/// The form of `forms` (`|`-separated, as [`tr_n!`] writes them) that `count` takes in the
/// current language.
pub fn plural(forms: &str, count: u64) -> &str {
    let forms: Vec<&str> = forms.split('|').collect();
    lang().plural_form(&forms, count)
}

/// `template` with each `{name}` replaced by its value from `args`. A name `args` doesn't have
/// stays as written; `{{` and `}}` are literal braces.
pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 8);
    let mut rest = template;
    while let Some(open) = rest.find(['{', '}']) {
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if let Some(close) = tail.find('}').filter(|_| tail.starts_with('{')) {
            let name = &tail[1..close];
            if let Some((_, value)) = args.iter().find(|(arg, _)| *arg == name) {
                let _ = write!(out, "{value}");
                rest = &tail[close + 1..];
                continue;
            }
        }
        out.push_str(&tail[..1]);
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// `value` with `places` decimals and the language's separator: `15,1` in Russian, `15.1` in
/// English.
pub fn decimal(value: f64, places: usize) -> String {
    separated(format!("{value:.places$}"))
}

/// `text`, a number Rust wrote, with the language's decimal separator.
fn separated(text: String) -> String {
    match lang() {
        Lang::English => text,
        Lang::Russian => text.replace('.', ","),
    }
}

/// A price or a rate the way the panel writes it (PoE Overlay II's style): trailing zeros
/// dropped, two decimals under 10, one under 100, none above -- `1,72`, `20,2`, `350` -- and two
/// significant digits below 1, so a cheap currency never reads as zero: `0,15`, `0,0041`. The
/// language's decimal separator (`1.72` in English).
pub fn number(value: f64) -> String {
    let places = if value >= 100.0 {
        0
    } else if value >= 10.0 {
        1
    } else if value >= 1.0 || value <= 0.0 {
        2
    } else {
        (1 - value.log10().floor() as i32).clamp(2, 8) as usize
    };
    let fixed = format!("{value:.places$}");
    let trimmed = if fixed.contains('.') {
        fixed.trim_end_matches('0').trim_end_matches('.')
    } else {
        &fixed
    };
    separated(trimmed.to_owned())
}

/// A [`number`] in short form: `891`, `4,1k`, `159k`, `1,2M` (`4.1k` in English).
pub fn compact(value: f64) -> String {
    if value >= 1e6 {
        format!("{}M", number(value / 1e6))
    } else if value >= 1e3 {
        format!("{}k", number(value / 1e3))
    } else {
        number(value)
    }
}

/// A count, such as a search's matches: English groups its thousands, `12,345`; the Russian
/// interface writes the digits alone, `12345`, as it always has.
pub fn integer(value: u64) -> String {
    let digits = value.to_string();
    match lang() {
        Lang::Russian => digits,
        Lang::English => {
            let mut out = String::with_capacity(digits.len() + digits.len() / 3);
            for (index, digit) in digits.chars().enumerate() {
                if index > 0 && (digits.len() - index).is_multiple_of(3) {
                    out.push(',');
                }
                out.push(digit);
            }
            out
        }
    }
}

/// A day of the year without its year: `22.09` in Russian, `Sep 22` in English.
pub fn day_month(day: u16, month: u16) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    match lang() {
        Lang::Russian => format!("{day:02}.{month:02}"),
        Lang::English => match MONTHS.get(usize::from(month).wrapping_sub(1)) {
            Some(name) => format!("{name} {day}"),
            None => format!("{month}/{day}"),
        },
    }
}

/// `value`, a number already written, as a percentage: Russian spaces the sign off, `15,1 %`;
/// English doesn't, `15.1%`.
pub fn percent(value: impl Display) -> String {
    match lang() {
        Lang::Russian => format!("{value} %"),
        Lang::English => format!("{value}%"),
    }
}

/// A time to level or a pause's length, to the nearest minute, in its two largest units:
/// `1 ч 32 мин`, `45 мин`, `2 д 3 ч`, `1 д`, `< 1 мин` in Russian; `1h 32m`, `45m`, `2d 3h`,
/// `1d`, `<1m` in English.
pub fn duration(duration: Duration) -> String {
    let minutes = (duration.as_secs_f64() / 60.0).round() as u64;
    let hours = minutes / 60;
    match minutes {
        0 => match lang() {
            Lang::Russian => "< 1 мин".to_owned(),
            Lang::English => "<1m".to_owned(),
        },
        1..60 => units(&[(minutes, TimeUnit::Minute)]),
        _ if hours < 24 => units(&[(hours, TimeUnit::Hour), (minutes % 60, TimeUnit::Minute)]),
        _ => units(&[(hours / 24, TimeUnit::Day), (hours % 24, TimeUnit::Hour)]),
    }
}

/// A wait to the second, in minutes and seconds: `45 с`, `10 мин`, `9 мин 50 с` in Russian;
/// `45s`, `10m`, `9m 50s` in English.
pub fn duration_secs(secs: u64) -> String {
    match secs / 60 {
        0 => units(&[(secs, TimeUnit::Second)]),
        minutes => units(&[(minutes, TimeUnit::Minute), (secs % 60, TimeUnit::Second)]),
    }
}

/// A unit a length of time is counted in.
#[derive(Clone, Copy)]
enum TimeUnit {
    Day,
    Hour,
    Minute,
    Second,
}

impl TimeUnit {
    /// What follows a count of it: Russian's short word after a space, English's letter.
    fn suffix(self, lang: Lang) -> &'static str {
        match (lang, self) {
            (Lang::Russian, TimeUnit::Day) => " д",
            (Lang::Russian, TimeUnit::Hour) => " ч",
            (Lang::Russian, TimeUnit::Minute) => " мин",
            (Lang::Russian, TimeUnit::Second) => " с",
            (Lang::English, TimeUnit::Day) => "d",
            (Lang::English, TimeUnit::Hour) => "h",
            (Lang::English, TimeUnit::Minute) => "m",
            (Lang::English, TimeUnit::Second) => "s",
        }
    }
}

/// `parts`, largest first, each count with its unit, space-separated; a part after the first that
/// counts zero is left out: `1 ч`, not `1 ч 0 мин`.
fn units(parts: &[(u64, TimeUnit)]) -> String {
    let lang = lang();
    let mut out = String::new();
    for (index, &(count, unit)) in parts.iter().enumerate() {
        if index > 0 && count == 0 {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        let _ = write!(out, "{count}{}", unit.suffix(lang));
    }
    out
}

/// The app's words in the interface language (see the module doc): `tr!("Search")` is a
/// `&'static str`; with placeholders, `tr!("Found: {count}", count = total)`, a `String`.
#[macro_export]
macro_rules! tr {
    ($english:literal) => {
        $crate::i18n::text($english)
    };
    ($english:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::text($english),
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+],
        )
    };
}

/// A counted phrase in the interface language, in the form the count takes:
/// `tr_n!(total, "{n} listing|{n} listings")`, with more placeholders after it as in [`tr!`].
/// `{n}` is the count.
#[macro_export]
macro_rules! tr_n {
    ($count:expr, $english:literal $(, $name:ident = $value:expr)* $(,)?) => {{
        let count: u64 = $count;
        $crate::i18n::fill(
            $crate::i18n::plural($crate::i18n::text($english), count),
            &[("n", &count as &dyn ::std::fmt::Display) $(, (stringify!($name), &$value as &dyn ::std::fmt::Display))*],
        )
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// The English texts `tr!` and `tr_n!` name in `source`: each call's first string literal --
    /// `tr!`'s first argument, `tr_n!`'s second (its count comes first).
    fn keys_in(source: &str) -> Vec<String> {
        let mut keys = Vec::new();
        for (start, _) in source.match_indices("tr") {
            // Only a macro call, not `attr!(` or the like.
            if start > 0 && source[..start].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let after = &source[start..];
            let Some(args) = after
                .strip_prefix("tr!(")
                .or_else(|| after.strip_prefix("tr_n!("))
            else {
                continue;
            };
            let Some(literal) = args.find('"').map(|quote| &args[quote + 1..]) else {
                continue;
            };
            let mut key = String::new();
            let mut chars = literal.chars();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => match chars.next() {
                        Some('n') => key.push('\n'),
                        Some('\n') => {
                            // A `\` line continuation drops the newline and the next line's
                            // leading spaces.
                            let rest: String = chars.clone().collect();
                            let skipped = rest.len() - rest.trim_start().len();
                            for _ in 0..rest[..skipped].chars().count() {
                                chars.next();
                            }
                        }
                        Some(other) => key.push(other),
                        None => break,
                    },
                    other => key.push(other),
                }
            }
            keys.push(key);
        }
        keys
    }

    fn source_keys() -> BTreeSet<String> {
        fn walk(dir: &Path, keys: &mut BTreeSet<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, keys);
                } else if path.extension().is_some_and(|ext| ext == "rs")
                    && !path.ends_with("i18n.rs")
                {
                    keys.extend(keys_in(&std::fs::read_to_string(&path).unwrap()));
                }
            }
        }
        let mut keys = BTreeSet::new();
        walk(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut keys,
        );
        keys
    }

    fn placeholders(text: &str) -> BTreeSet<&str> {
        text.match_indices('{')
            .filter_map(|(open, _)| {
                let rest = &text[open + 1..];
                let close = rest.find('}')?;
                let name = &rest[..close];
                name.chars()
                    .all(|c| c.is_alphanumeric() || c == '_')
                    .then_some(name)
            })
            .filter(|name| !name.is_empty())
            .collect()
    }

    #[test]
    fn every_text_in_the_sources_has_a_russian_translation_and_nothing_else_is_in_the_table() {
        let keys = source_keys();
        let table: BTreeSet<String> = RUSSIAN.keys().cloned().collect();
        let missing: Vec<_> = keys.difference(&table).collect();
        let unused: Vec<_> = table.difference(&keys).collect();
        assert!(
            missing.is_empty(),
            "no Russian text in assets/i18n/ru/*.json for {missing:#?}"
        );
        assert!(
            unused.is_empty(),
            "assets/i18n/ru/*.json hold texts no code uses: {unused:#?}"
        );
        let mut seen: HashMap<String, &str> = HashMap::new();
        for (file, entries) in russian_files() {
            for english in entries.into_keys() {
                if let Some(first) = seen.insert(english.clone(), file) {
                    panic!("{english:?} is in both ru/{first}.json and ru/{file}.json");
                }
            }
        }
    }

    #[test]
    fn a_translation_keeps_every_placeholder_and_its_plural_forms() {
        for (english, russian) in RUSSIAN.iter() {
            let english_forms = english.split('|').count();
            let russian_forms = russian.split('|').count();
            if english_forms > 1 {
                assert_eq!(
                    russian_forms, 3,
                    "{english:?} -> {russian:?}: Russian has 3 forms"
                );
            } else {
                assert_eq!(russian_forms, 1, "{english:?} -> {russian:?}: not a plural");
            }
            for form in russian.split('|') {
                assert_eq!(
                    placeholders(form),
                    placeholders(english),
                    "{english:?} -> {russian:?}: the placeholders differ"
                );
            }
        }
    }

    #[test]
    fn the_sources_scan_reads_every_way_a_text_is_written() {
        let source = r#"
            div().child(tr!("Search"));
            let a = tr!( "Found: {count}", count = total);
            let b = tr_n!(total, "{n} listing|{n} listings");
            let c = tr!("A long \
                         line \"quoted\"");
            let d = attr!("not a key");
        "#;
        assert_eq!(
            keys_in(source),
            [
                "Search",
                "Found: {count}",
                "{n} listing|{n} listings",
                "A long line \"quoted\""
            ]
        );
    }

    #[test]
    fn auto_follows_the_game_then_windows() {
        let auto = InterfaceLanguage::Auto;
        assert_eq!(resolve(auto, Some("ru"), false), Lang::Russian);
        assert_eq!(
            resolve(auto, Some("en"), true),
            Lang::English,
            "the game wins"
        );
        assert_eq!(
            resolve(auto, Some("de"), true),
            Lang::English,
            "no German yet"
        );
        assert_eq!(resolve(auto, None, true), Lang::Russian);
        assert_eq!(resolve(auto, None, false), Lang::English);
        assert_eq!(
            resolve(InterfaceLanguage::English, Some("ru"), true),
            Lang::English
        );
        assert_eq!(
            resolve(InterfaceLanguage::Russian, Some("en"), false),
            Lang::Russian
        );
    }

    #[test]
    fn plural_forms_follow_each_language() {
        let english = ["{n} listing", "{n} listings"];
        let russian = ["{n} лот", "{n} лота", "{n} лотов"];
        let pick = |lang: Lang, forms: &[&'static str], counts: &[u64]| {
            counts
                .iter()
                .map(|&count| lang.plural_form(forms, count))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            pick(Lang::English, &english, &[0, 1, 2, 21]),
            [
                "{n} listings",
                "{n} listing",
                "{n} listings",
                "{n} listings"
            ]
        );
        assert_eq!(
            pick(
                Lang::Russian,
                &russian,
                &[1, 2, 5, 11, 12, 21, 22, 25, 111, 104]
            ),
            [
                "{n} лот",
                "{n} лота",
                "{n} лотов",
                "{n} лотов",
                "{n} лотов",
                "{n} лот",
                "{n} лота",
                "{n} лотов",
                "{n} лотов",
                "{n} лота"
            ]
        );
    }

    #[test]
    fn fill_replaces_named_placeholders_and_keeps_the_rest() {
        let total = 214;
        let args: [(&str, &dyn Display); 2] = [("count", &total), ("league", &"Standard")];
        assert_eq!(
            fill("Found {count} in {league}, {missing} {{literal}}", &args),
            "Found 214 in Standard, {missing} {literal}"
        );
        assert_eq!(fill("no braces", &args), "no braces");
        assert_eq!(fill("{count}", &args), "214");
    }

    /// What `f` writes in Russian, then in English.
    fn in_both(f: impl Fn() -> String) -> (String, String) {
        (with_lang(Lang::Russian, &f), with_lang(Lang::English, &f))
    }

    #[test]
    fn numbers_take_each_languages_separator_and_percent_sign() {
        assert_eq!(
            in_both(|| [1.72, 20.24, 350.4, 0.15, 0.0041, 3.0, 0.0]
                .map(number)
                .join(" ")),
            (
                "1,72 20,2 350 0,15 0,0041 3 0".to_owned(),
                "1.72 20.2 350 0.15 0.0041 3 0".to_owned()
            )
        );
        assert_eq!(
            in_both(|| [891.0, 4_120.0, 159_000.0, 1_240_000.0]
                .map(compact)
                .join(" ")),
            (
                "891 4,12k 159k 1,24M".to_owned(),
                "891 4.12k 159k 1.24M".to_owned()
            )
        );
        assert_eq!(
            in_both(|| percent(decimal(15.06, 1))),
            ("15,1 %".to_owned(), "15.1%".to_owned())
        );
        assert_eq!(
            in_both(|| [7, 999, 1_000, 12_345, 1_234_567].map(integer).join(" ")),
            (
                "7 999 1000 12345 1234567".to_owned(),
                "7 999 1,000 12,345 1,234,567".to_owned()
            )
        );
        assert_eq!(
            in_both(|| format!("{} · {}", day_month(22, 9), day_month(1, 12))),
            ("22.09 · 01.12".to_owned(), "Sep 22 · Dec 1".to_owned())
        );
    }

    #[test]
    fn durations_keep_their_two_largest_units() {
        let minutes = |secs: u64| duration(Duration::from_secs(secs));
        assert_eq!(
            in_both(
                || [20, 45 * 60, 92 * 60, 3588, 26 * 3600 + 10 * 60, 48 * 3600]
                    .map(minutes)
                    .join(" · ")
            ),
            (
                "< 1 мин · 45 мин · 1 ч 32 мин · 1 ч · 1 д 2 ч · 2 д".to_owned(),
                "<1m · 45m · 1h 32m · 1h · 1d 2h · 2d".to_owned()
            ),
            "59.8 minutes round up to a whole hour, not 60 minutes"
        );
        assert_eq!(
            in_both(|| [45, 600, 590].map(duration_secs).join(" · ")),
            (
                "45 с · 10 мин · 9 мин 50 с".to_owned(),
                "45s · 10m · 9m 50s".to_owned()
            )
        );
    }
}
