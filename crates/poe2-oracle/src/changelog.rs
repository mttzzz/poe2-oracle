//! The changelog the settings' «Что нового» shows: `CHANGELOG.md` and `CHANGELOG.ru.md`, built into
//! the app -- the section needs no internet -- and read into releases, each with its groups of
//! changes and their items ([`releases`], from the interface language's file). Pure and not
//! Windows-gated, so the native test pass covers it; `ui::settings_view` only draws it.
//!
//! The app reads what the files promise and no more:
//! - the releases are the lines between the ones starting `<!-- ANCHOR: releases` and
//!   `<!-- ANCHOR_END: releases`, the pair the guide's «Что нового» page includes between;
//! - a release starts at a heading `## [X.Y.Z] - YYYY-MM-DD`, newest first. `## [Unreleased]`, or
//!   any other heading of that level, is left out with everything under it, up to the next release;
//! - `### Added` (`### Добавлено`) opens a group of changes, and a line starting `- ` an item in
//!   it. The lines after that, up to a blank line, a new item or a heading, continue the item, each
//!   joined to it by one space, so a mark may run over a line break;
//! - in an item, `**bold**` and `` `code` `` are marks. A mark that never closes is plain text, as
//!   is anything else the format doesn't have, a link say: it is shown as written;
//! - prose between a release's heading and its first group, and an item outside any group, are not
//!   shown.

use std::sync::LazyLock;

use crate::i18n::{self, Lang};

/// The line that opens a changelog's releases, and the one that closes them.
const ANCHOR: &str = "<!-- ANCHOR: releases";
const ANCHOR_END: &str = "<!-- ANCHOR_END: releases";

/// A day, as a release is dated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

/// One release: its version, the day it came out and what it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// As the heading writes it: `0.1.7`.
    pub version: String,
    pub date: Date,
    pub groups: Vec<Group>,
}

/// The changes of one kind in a release, under the heading the language's file gives them:
/// `Added`, `Fixed`; `Добавлено`, `Исправлено`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub title: String,
    pub items: Vec<Item>,
}

/// One change: a bullet of the changelog, its words split at the marks inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub spans: Vec<Span>,
}

/// Words of an item, as they are marked. Side by side, the spans make the item's text with its
/// marks taken off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    /// Words with no mark.
    Text(String),
    /// Words between `**` and `**`.
    Strong(String),
    /// Words between backticks. Inside a bold stretch they cut it in two: the bold goes on after
    /// them.
    Code(String),
}

impl Span {
    /// The span's words, without the marks around them.
    pub fn text(&self) -> &str {
        match self {
            Span::Text(words) | Span::Strong(words) | Span::Code(words) => words,
        }
    }
}

/// The releases of the interface language's changelog, newest first.
pub fn releases() -> &'static [Release] {
    releases_in(i18n::lang())
}

/// The changelogs, as the repository's root holds them.
const ENGLISH_CHANGELOG: &str = include_str!("../../../CHANGELOG.md");
const RUSSIAN_CHANGELOG: &str = include_str!("../../../CHANGELOG.ru.md");

/// Each language's file, read once.
fn releases_in(lang: Lang) -> &'static [Release] {
    static ENGLISH: LazyLock<Vec<Release>> = LazyLock::new(|| parse(ENGLISH_CHANGELOG));
    static RUSSIAN: LazyLock<Vec<Release>> = LazyLock::new(|| parse(RUSSIAN_CHANGELOG));
    match lang {
        Lang::Russian => &RUSSIAN,
        Lang::English => &ENGLISH,
    }
}

/// The lines of `markdown` between the anchors; none without the opening one.
fn anchored(markdown: &str) -> impl Iterator<Item = &str> {
    markdown
        .lines()
        .skip_while(|line| !line.starts_with(ANCHOR))
        .skip(1)
        .take_while(|line| !line.starts_with(ANCHOR_END))
}

/// The releases `markdown`, a changelog, holds between its anchors, in its order. Text outside
/// the anchors is not read, and a changelog with no opening anchor has no releases.
pub fn parse(markdown: &str) -> Vec<Release> {
    let mut reader = Reader::default();
    for line in anchored(markdown) {
        reader.read(line);
    }
    reader.finish()
}

/// A changelog read line by line.
#[derive(Default)]
struct Reader {
    /// The releases read through.
    done: Vec<Release>,
    /// The release being read; `None` under a heading that isn't one, and before the first.
    current: Option<Release>,
    /// The item being read: the words of its lines so far, joined.
    item: Option<String>,
}

impl Reader {
    fn read(&mut self, line: &str) {
        let line = line.trim_end();
        if let Some((level, title)) = heading(line) {
            match level {
                1 | 2 => {
                    self.end_release();
                    self.current = if level == 2 { release(title) } else { None };
                }
                3 => {
                    self.end_item();
                    if let Some(current) = &mut self.current {
                        current.groups.push(Group {
                            title: title.to_owned(),
                            items: Vec::new(),
                        });
                    }
                }
                _ => self.end_item(),
            }
        } else if let Some(words) = line.strip_prefix("- ") {
            self.end_item();
            self.item = Some(words.to_owned());
        } else if line.is_empty() {
            self.end_item();
        } else if let Some(item) = &mut self.item {
            item.push(' ');
            item.push_str(line.trim_start());
        }
    }

    /// The item being read is whole: it goes to the latest group of the release being read, if
    /// there is one.
    fn end_item(&mut self) {
        let Some(words) = self.item.take() else {
            return;
        };
        let item = Item {
            spans: spans(words.trim()),
        };
        if item.spans.is_empty() {
            return;
        }
        if let Some(group) = self.current.as_mut().and_then(|r| r.groups.last_mut()) {
            group.items.push(item);
        }
    }

    fn end_release(&mut self) {
        self.end_item();
        self.done.extend(self.current.take());
    }

    fn finish(mut self) -> Vec<Release> {
        self.end_release();
        self.done
    }
}

/// A heading line's level, the count of its `#`s, and its text.
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.len() - line.trim_start_matches('#').len();
    let text = &line[level..];
    let spaced = text.is_empty() || text.starts_with([' ', '\t']);
    ((1..=6).contains(&level) && spaced).then(|| (level, text.trim()))
}

/// The release a level-2 heading's text names, `[X.Y.Z] - YYYY-MM-DD`, with no groups yet. `None`
/// for any other heading, `[Unreleased]` among them.
fn release(heading: &str) -> Option<Release> {
    let (version, date) = heading.strip_prefix('[')?.split_once("] - ")?;
    let numbered = version.starts_with(|c: char| c.is_ascii_digit())
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    if !numbered {
        return None;
    }
    let (year, rest) = date.split_once('-')?;
    let (month, day) = rest.split_once('-')?;
    let date = Date {
        year: digits(year, 4)?,
        month: digits(month, 2)?,
        day: digits(day, 2)?,
    };
    ((1..=12).contains(&date.month) && (1..=31).contains(&date.day)).then(|| Release {
        version: version.to_owned(),
        date,
        groups: Vec::new(),
    })
}

/// `text` as a number, if it is exactly `length` ASCII digits.
fn digits<T: std::str::FromStr>(text: &str, length: usize) -> Option<T> {
    if text.len() == length && text.bytes().all(|byte| byte.is_ascii_digit()) {
        text.parse().ok()
    } else {
        None
    }
}

/// A piece of an item's words, before the `**` marks are paired.
enum Piece<'a> {
    Words(&'a str),
    /// A `**`.
    Bold,
    Code(&'a str),
}

/// `words` cut at its `**`s and its code spans. A backtick with no partner after it, or none with
/// something between, is words; so is every `**` inside a code span.
fn split_marks(words: &str) -> Vec<Piece<'_>> {
    let bytes = words.as_bytes();
    let mut pieces = Vec::new();
    // `from` is where the words not yet cut off start, `at` the byte looked at. A mark is ASCII, as
    // no byte of a longer character is, so a cut at one is always between characters.
    let (mut from, mut at) = (0, 0);
    while at < bytes.len() {
        let mark = if bytes[at..].starts_with(b"**") {
            Some((Piece::Bold, 2))
        } else if bytes[at] == b'`' {
            words[at + 1..]
                .find('`')
                .filter(|&inside| inside > 0)
                .map(|inside| (Piece::Code(&words[at + 1..at + 1 + inside]), inside + 2))
        } else {
            None
        };
        let Some((piece, length)) = mark else {
            at += 1;
            continue;
        };
        if from < at {
            pieces.push(Piece::Words(&words[from..at]));
        }
        pieces.push(piece);
        at += length;
        from = at;
    }
    if from < words.len() {
        pieces.push(Piece::Words(&words[from..]));
    }
    pieces
}

/// The spans of an item's `words`. The `**`s pair up in order; the last of an odd number has no
/// partner, and stays in the words as `**`.
fn spans(words: &str) -> Vec<Span> {
    let pieces = split_marks(words);
    let bold_marks = pieces
        .iter()
        .filter(|piece| matches!(piece, Piece::Bold))
        .count();
    let paired = bold_marks - bold_marks % 2;
    let (mut strong, mut seen) = (false, 0);
    let mut spans = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Bold => {
                seen += 1;
                if seen <= paired {
                    strong = !strong;
                } else {
                    push_words(&mut spans, "**", false);
                }
            }
            Piece::Words(words) => push_words(&mut spans, words, strong),
            Piece::Code(code) => spans.push(Span::Code(code.to_owned())),
        }
    }
    spans
}

/// `words` onto the end of `spans`, into the span there when it is of the same kind.
fn push_words(spans: &mut Vec<Span>, words: &str, strong: bool) {
    match (spans.last_mut(), strong) {
        (Some(Span::Text(last)), false) | (Some(Span::Strong(last)), true) => last.push_str(words),
        (_, false) => spans.push(Span::Text(words.to_owned())),
        (_, true) => spans.push(Span::Strong(words.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::with_lang;

    fn text(words: &str) -> Span {
        Span::Text(words.to_owned())
    }

    fn strong(words: &str) -> Span {
        Span::Strong(words.to_owned())
    }

    fn code(words: &str) -> Span {
        Span::Code(words.to_owned())
    }

    fn day(year: u16, month: u8, day: u8) -> Date {
        Date { year, month, day }
    }

    /// An item's words with its marks taken off.
    fn plain(item: &Item) -> String {
        item.spans.iter().map(Span::text).collect()
    }

    /// Every item of `releases`, in order, as plain words.
    fn all_items(releases: &[Release]) -> Vec<String> {
        releases
            .iter()
            .flat_map(|release| &release.groups)
            .flat_map(|group| &group.items)
            .map(plain)
            .collect()
    }

    /// A changelog with something at every turn for the reader to leave out: text, a bullet and a
    /// release outside the anchors; the next release, `Unreleased`, with its items; a heading that
    /// is no release; a note under a release's heading; a bullet in no group; a paragraph after a
    /// blank line. What is left is two releases and five items.
    const CHANGELOG: &str = "\
# Changelog

Text before the anchors is not the changelog.

- A bullet before the anchors.

## [9.9.9] - 2020-01-01

### Added

- A release before the anchors.

<!-- ANCHOR: releases. The comment goes on after the name. -->

## [Unreleased]

### Added

- **Not out yet.** An item of the next release,
  which goes on here.

## [1.2.0] - 2026-10-05

A note under the heading, which belongs to no group.

### Added

- **First.** The beginning of the first item,
  and its second line,
    and a third, indented deeper.
- Second item, after a bullet.

  A paragraph after a blank line, which is no item.
- Third item, ended by a heading.
### Fixed

- **A bold phrase that
  wraps** and ends the fourth item.

## Not a release

### Added

- An item under a heading that is no release.

## [1.1.0] - 2026-09-30

- A bullet in no group.

### Changed

- Only item.
<!-- ANCHOR_END: releases. The comment goes on here too. -->
[1.2.0]: https://example.test/v1.2.0

## [0.0.1] - 2019-01-01

### Added

- A release after the anchors.
";

    #[test]
    fn releases_are_the_dated_sections_between_the_anchors() {
        let releases = parse(CHANGELOG);
        let shape: Vec<_> = releases
            .iter()
            .map(|release| {
                let groups: Vec<_> = release
                    .groups
                    .iter()
                    .map(|group| (group.title.as_str(), group.items.len()))
                    .collect();
                (release.version.clone(), release.date, groups)
            })
            .collect();
        assert_eq!(
            shape,
            [
                (
                    "1.2.0".to_owned(),
                    day(2026, 10, 5),
                    vec![("Added", 3), ("Fixed", 1)]
                ),
                ("1.1.0".to_owned(), day(2026, 9, 30), vec![("Changed", 1)]),
            ],
            "the release before the anchors, the Unreleased one, the heading that is no release \
             and the release after the anchors are left out, with their items"
        );
    }

    #[test]
    fn an_item_runs_on_over_the_lines_after_its_first() {
        let releases = parse(CHANGELOG);
        assert_eq!(
            all_items(&releases),
            [
                "First. The beginning of the first item, and its second line, and a third, \
                 indented deeper.",
                "Second item, after a bullet.",
                "Third item, ended by a heading.",
                "A bold phrase that wraps and ends the fourth item.",
                "Only item.",
            ],
            "a line is joined by one space whatever its indent; a blank line, a bullet or a \
             heading ends an item, and a paragraph after a blank line isn't one"
        );
        assert_eq!(
            releases[0].groups[1].items[0].spans,
            [
                strong("A bold phrase that wraps"),
                text(" and ends the fourth item.")
            ],
            "a mark runs over a line break"
        );
    }

    #[test]
    fn windows_line_endings_read_the_same() {
        // A checkout on Windows may turn the files' line feeds into CR LF, and the release is
        // built there.
        assert_eq!(parse(&CHANGELOG.replace('\n', "\r\n")), parse(CHANGELOG));
    }

    #[test]
    fn the_anchors_bound_the_releases() {
        let versions = |markdown: &str| -> Vec<String> {
            parse(markdown)
                .into_iter()
                .map(|release| release.version)
                .collect()
        };
        assert_eq!(versions(CHANGELOG), ["1.2.0", "1.1.0"]);
        // Without the opening anchor there is nothing to read.
        let unanchored = CHANGELOG.replace("<!-- ANCHOR: releases", "<!-- no anchor");
        assert!(versions(&unanchored).is_empty());
        assert!(versions("").is_empty());
        // Without the closing one the releases run to the end, as the guide's page includes them.
        let unclosed = CHANGELOG.replace("<!-- ANCHOR_END: releases", "<!-- no end");
        assert_eq!(versions(&unclosed), ["1.2.0", "1.1.0", "0.0.1"]);
    }

    #[test]
    fn a_heading_is_a_release_only_in_the_changelogs_form() {
        let read = |headings: &[&str]| -> Vec<String> {
            let mut markdown = String::from("<!-- ANCHOR: releases -->\n");
            for heading in headings {
                markdown.push_str(&format!("{heading}\n\n### Added\n\n- Item.\n\n"));
            }
            markdown.push_str("<!-- ANCHOR_END: releases -->\n");
            parse(&markdown)
                .into_iter()
                .map(|release| release.version)
                .collect()
        };
        assert_eq!(
            read(&[
                "## [0.1.0] - 2026-09-27",
                "## [0.2.0-rc.1] - 2026-09-28",
                "## [Unreleased]",
                "## [0.3.0]",
                "## [0.4.0] - 2026-9-28",
                "## [0.5.0] - 2026-13-01",
                "## [0.6.0] - 2026-09-32",
                "## [0.7.0] - 2026-09-28 [YANKED]",
                "## [x.y.z] - 2026-09-28",
                "### [0.8.0] - 2026-09-28",
                "## [0.9.0] - +026-09-28",
                "##[1.0.0] - 2026-09-28",
                "## [1.1.0] - 2026-09-28",
            ]),
            ["0.1.0", "0.2.0-rc.1", "1.1.0"]
        );
    }

    #[test]
    fn marks_split_the_words_where_they_stand() {
        assert_eq!(spans(""), Vec::<Span>::new());
        assert_eq!(spans("plain words"), [text("plain words")]);
        assert_eq!(
            spans("**Bold.** Then words, `code` and a mid-**sentence** mark, `end`"),
            [
                strong("Bold."),
                text(" Then words, "),
                code("code"),
                text(" and a mid-"),
                strong("sentence"),
                text(" mark, "),
                code("end"),
            ]
        );
        // Marks side by side, and a mark that is the whole item.
        assert_eq!(spans("`a``b`"), [code("a"), code("b")]);
        assert_eq!(spans("**a****b**"), [strong("ab")]);
        assert_eq!(spans("**all of it**"), [strong("all of it")]);
    }

    #[test]
    fn a_code_span_inside_a_bold_one_is_code_and_the_bold_goes_on_after_it() {
        // The shape of 0.1.0's first item, in both changelogs.
        assert_eq!(
            spans("**Price check on `Ctrl+E`.** Point at an item"),
            [
                strong("Price check on "),
                code("Ctrl+E"),
                strong("."),
                text(" Point at an item"),
            ]
        );
        // A `**` in code is the code's own.
        assert_eq!(
            spans("`a ** b` and **c**"),
            [code("a ** b"), text(" and "), strong("c")]
        );
    }

    #[test]
    fn marks_cut_between_characters_in_any_script() {
        assert_eq!(
            spans("**«Что нового» в настройках.** Новый раздел, `Ctrl+E` — «Ёлка» **и** `ещё`"),
            [
                strong("«Что нового» в настройках."),
                text(" Новый раздел, "),
                code("Ctrl+E"),
                text(" — «Ёлка» "),
                strong("и"),
                text(" "),
                code("ещё"),
            ]
        );
        assert_eq!(spans("é**é**é"), [text("é"), strong("é"), text("é")]);
    }

    #[test]
    fn a_mark_that_never_closes_stays_as_written() {
        assert_eq!(spans("An **unclosed bold"), [text("An **unclosed bold")]);
        assert_eq!(spans("An `unclosed code"), [text("An `unclosed code")]);
        assert_eq!(
            spans("Nothing between `` the ticks"),
            [text("Nothing between `` the ticks")]
        );
        assert_eq!(
            spans("A lone ** in the middle"),
            [text("A lone ** in the middle")]
        );
        assert_eq!(spans("2 * 3 = 6"), [text("2 * 3 = 6")]);
        // The `**`s pair in order, so the odd one out is the last.
        assert_eq!(spans("**a** and **b"), [strong("a"), text(" and **b")]);
        // Marks that do close keep working around one that doesn't.
        assert_eq!(
            spans("`code` then **bold** then `open"),
            [
                code("code"),
                text(" then "),
                strong("bold"),
                text(" then `open")
            ]
        );
    }

    /// The changelogs built into the app, with the files they come from.
    fn embedded() -> [(&'static str, &'static [Release]); 2] {
        [
            ("CHANGELOG.md", releases_in(Lang::English)),
            ("CHANGELOG.ru.md", releases_in(Lang::Russian)),
        ]
    }

    #[test]
    fn both_changelogs_list_the_same_releases_in_the_same_order() {
        let listed = |releases: &[Release]| {
            releases
                .iter()
                .map(|release| (release.version.clone(), release.date))
                .collect::<Vec<_>>()
        };
        let english = listed(releases_in(Lang::English));
        let russian = listed(releases_in(Lang::Russian));
        assert!(
            !english.is_empty(),
            "CHANGELOG.md has no release between its anchors"
        );
        assert_eq!(
            english, russian,
            "CHANGELOG.md (left) and CHANGELOG.ru.md (right) must document the same versions, \
             dated alike, in the same order: a version in one language only shows in that \
             language's «Что нового» alone"
        );
    }

    #[test]
    fn releases_run_from_the_newest_to_the_oldest() {
        /// A version's place in release order: its numbers, then a final version above a
        /// pre-release of the same numbers, `0.2.0` over `0.2.0-rc.1`.
        fn rank(version: &str) -> (Vec<u64>, bool) {
            let (numbers, pre_release) = match version.split_once('-') {
                Some((numbers, _)) => (numbers, true),
                None => (version, false),
            };
            let numbers = numbers
                .split('.')
                .map(|part| {
                    part.parse().unwrap_or_else(|_| {
                        panic!("version {version:?} isn't numbers and dots, then a -suffix maybe")
                    })
                })
                .collect();
            (numbers, !pre_release)
        }
        assert!(rank("0.10.0") > rank("0.9.9"), "numbers, not text");
        assert!(
            rank("0.2.0") > rank("0.2.0-rc.1"),
            "a final over its pre-release"
        );
        assert!(
            rank("0.2.0-rc.1") > rank("0.1.9"),
            "a pre-release over the version before"
        );
        for (file, releases) in embedded() {
            for pair in releases.windows(2) {
                let (newer, older) = (&pair[0], &pair[1]);
                assert!(
                    newer.version != older.version && rank(&newer.version) >= rank(&older.version),
                    "{file}: {} comes before {}, which isn't older",
                    newer.version,
                    older.version
                );
                assert!(
                    newer.date >= older.date,
                    "{file}: {} ({:?}) comes before {} ({:?}), which is dated later",
                    newer.version,
                    newer.date,
                    older.version,
                    older.date
                );
            }
        }
    }

    #[test]
    fn every_release_has_a_group_and_every_group_an_item() {
        for (file, releases) in embedded() {
            for release in releases {
                assert!(
                    !release.groups.is_empty(),
                    "{file}: {} has no `### ` group with `- ` items under it",
                    release.version
                );
                for group in &release.groups {
                    assert!(
                        !group.title.is_empty(),
                        "{file}: a `###` heading of {} has no title",
                        release.version
                    );
                    assert!(
                        !group.items.is_empty(),
                        "{file}: {} has no `- ` item under `### {}`",
                        release.version,
                        group.title
                    );
                }
            }
        }
    }

    #[test]
    fn the_newest_release_is_the_version_of_this_build() {
        let version = env!("CARGO_PKG_VERSION");
        // A pre-release build, `0.2.0-rc.1`, may go without a section of its own: release.yml
        // takes its notes from the final version's.
        let final_version = version.split('-').next().unwrap_or(version);
        for (file, releases) in embedded() {
            let newest = releases.first().map(|release| release.version.as_str());
            assert!(
                newest == Some(version) || newest == Some(final_version),
                "{file} must open with this build's version, {version}, as a dated `## [version] - \
                 YYYY-MM-DD` section: the settings mark that release as installed. It opens \
                 with {newest:?}"
            );
        }
    }

    #[test]
    fn no_mark_of_the_changelogs_is_left_written_out() {
        for (file, releases) in embedded() {
            for release in releases {
                for item in release.groups.iter().flat_map(|group| &group.items) {
                    for span in &item.spans {
                        let words = span.text();
                        assert!(
                            !words.contains("**") && !words.contains('`'),
                            "{file}: {} shows a `**` or a backtick as written, for a mark that \
                             never closes: {words:?}",
                            release.version
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_items_of_the_changelogs_are_whole_and_joined() {
        for (file, releases) in embedded() {
            for item in releases
                .iter()
                .flat_map(|release| &release.groups)
                .flat_map(|group| &group.items)
            {
                let words = plain(item);
                assert!(
                    !item.spans.is_empty() && item.spans.iter().all(|s| !s.text().is_empty()),
                    "{file}: an empty span in {words:?}"
                );
                assert_eq!(words, words.trim(), "{file}: untrimmed {words:?}");
                assert!(
                    !words.contains("  ") && !words.contains('\n'),
                    "{file}: lines not joined by one space in {words:?}"
                );
            }
        }
    }

    /// What «Что нового» would leave out of `markdown`'s releases, and why: the first line in a
    /// group that isn't a bullet or right under one, or that starts a list of its own. `None` for a
    /// changelog the app shows whole. Prose between a release's heading and its first group is
    /// allowed: the app has no place for it.
    fn left_out(markdown: &str) -> Option<String> {
        // Whether a group is open in the section being read, and whether the line before was part
        // of an item: a bullet, or a line right under one.
        let (mut in_group, mut in_item) = (false, false);
        for line in anchored(markdown) {
            let line = line.trim_end();
            match heading(line) {
                Some((level, _)) => {
                    match level {
                        1 | 2 => in_group = false,
                        3 => in_group = true,
                        _ => {}
                    }
                    in_item = false;
                }
                None if line.is_empty() => in_item = false,
                None if !in_group => {}
                None if line.starts_with("- ") => in_item = true,
                None if !in_item => {
                    return Some(format!(
                        "{line:?} is text in a group outside any bullet, or after a blank line: \
                         «Что нового» shows each bullet with the lines right under it, and \
                         nothing else"
                    ));
                }
                None if ["- ", "* ", "+ "]
                    .iter()
                    .any(|marker| line.trim_start().starts_with(marker)) =>
                {
                    return Some(format!(
                        "{line:?} starts a list inside a bullet, which «Что нового» doesn't \
                         read: it would join the bullet above as text"
                    ));
                }
                None => {}
            }
        }
        None
    }

    #[test]
    fn the_changelogs_hold_only_what_the_app_reads() {
        for (file, markdown) in [
            ("CHANGELOG.md", ENGLISH_CHANGELOG),
            ("CHANGELOG.ru.md", RUSSIAN_CHANGELOG),
        ] {
            if let Some(problem) = left_out(markdown) {
                panic!("{file}: {problem}");
            }
        }
    }

    #[test]
    fn text_the_app_would_leave_out_is_found() {
        let release = |group: &str| {
            format!(
                "<!-- ANCHOR: releases -->\n## [1.0.0] - 2026-01-01\n\nA note under the heading, \
                 which the app has no place for.\n\n### Added\n\n{group}\n\
                 <!-- ANCHOR_END: releases -->\n"
            )
        };
        assert_eq!(
            left_out(&release("- One\n  and two\n- Three\nand three")),
            None
        );
        assert_eq!(
            left_out(&release("- One\n- Two\n\n### Fixed\n\n- Three")),
            None
        );
        for group in [
            "- One\n\n  A second paragraph of the first item.",
            "Text right under the group's heading.",
            "- One\n  - A list inside the item.",
            "- One\n  * Another kind of list.",
        ] {
            assert!(left_out(&release(group)).is_some(), "{group:?}");
        }
        // The same text outside a group, or past the closing anchor, is not the app's business.
        let outside = "<!-- ANCHOR: releases -->\n## [Unreleased]\n\nA note.\n\n- One\n\n  Two.\n\
                       <!-- ANCHOR_END: releases -->\nText.\n\n### Added\n\nText.\n";
        assert_eq!(left_out(outside), None);
    }

    #[test]
    fn the_releases_follow_the_interface_language() {
        let titles = |releases: &[Release]| -> Vec<String> {
            releases
                .iter()
                .flat_map(|release| &release.groups)
                .map(|group| group.title.clone())
                .collect()
        };
        let russian = titles(with_lang(Lang::Russian, releases));
        let english = titles(with_lang(Lang::English, releases));
        assert!(
            !russian.is_empty()
                && russian
                    .iter()
                    .all(|title| title.chars().all(|c| matches!(c, '\u{400}'..='\u{4ff}'))),
            "Russian group titles: {russian:?}"
        );
        assert!(
            !english.is_empty() && english.iter().all(|title| title.is_ascii()),
            "English group titles: {english:?}"
        );
    }
}
