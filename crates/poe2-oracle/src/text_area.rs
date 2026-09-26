//! The report window's text box, apart from how it is drawn (`ui::text_area`): the text, the
//! selection, what each edit does to them and how undo takes it back. Pure and platform-free, so
//! the native test pass covers it. Offsets are byte offsets into the text, always on a char
//! boundary; the length limit counts chars, as the report's own check does.
//!
//! Where a rule depends on how the text is laid out -- up and down, Home and End -- it asks a
//! [`Layout`]: the box builds one from GPUI's wrapped lines, the tests from a pretend font. A
//! soft-wrapped row ends where the next one begins, so that one offset shows in two places; the
//! caret remembers which (`upstream`): the end of the upper row after End, the start of the lower
//! one after anything else.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::ops::Range;

use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// How many steps undo can take back.
const UNDO_STEPS: usize = 200;

/// One row of the text as laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// The offset of its first character.
    pub start: usize,
    /// The offset past its last one: where the next row starts if this one wraps, else its
    /// paragraph's end -- a `\n`, or the end of the text.
    pub end: usize,
    /// The paragraph goes on in the next row: it was wrapped at the box's width.
    pub wraps: bool,
}

/// The text as laid out on screen.
pub trait Layout {
    /// The rows, top to bottom: at least one, and together covering the whole text.
    fn rows(&self) -> &[Row];
    /// How far from its row's left edge the caret at `offset`, an offset within `row`, sits.
    fn x(&self, row: usize, offset: usize) -> f32;
    /// The offset within `row` whose caret sits nearest to `x`: see [`nearest_stop`].
    fn offset_at(&self, row: usize, x: f32) -> usize;
}

/// The row that shows the caret at `offset`: the last one starting at or before it -- or, on a
/// soft wrap and `upstream`, the row the wrap ends.
pub fn row_of(rows: &[Row], offset: usize, upstream: bool) -> usize {
    let row = rows
        .partition_point(|row| row.start <= offset)
        .saturating_sub(1);
    if upstream && row > 0 && rows[row].start == offset && rows[row - 1].wraps {
        row - 1
    } else {
        row
    }
}

/// The offset within `row` whose caret sits nearest to `x`, of `stops`: the caret positions of
/// the row's paragraph -- each glyph's start, and the paragraph's end -- as offsets, each with how
/// far along the paragraph's unwrapped line it sits, as `x` is too. The end is weighed like any
/// glyph's start; a stop on another row of the paragraph comes back as this row's start or end.
/// Of two as near, the one listed first.
pub fn nearest_stop(row: Row, stops: impl IntoIterator<Item = (usize, f32)>, x: f32) -> usize {
    stops
        .into_iter()
        .min_by(|(_, a), (_, b)| (a - x).abs().total_cmp(&(b - x).abs()))
        .map_or(row.start, |(offset, _)| offset)
        .clamp(row.start, row.end)
}

/// A line break other than a CR: `\n`, and the rarer ones -- a vertical tab, a form feed, NEL,
/// and Unicode's line and paragraph separators. GPUI splits the text into paragraphs only at
/// `\n`, while DirectWrite breaks a line at any of them.
fn is_line_break(character: char) -> bool {
    matches!(
        character,
        '\n' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// `text` as the box keeps it: Windows' CRLF line breaks, lone CRs and every other line break
/// as `\n`; tabs as spaces; no other control characters, which GPUI can't draw.
pub fn clean(text: &str) -> Cow<'_, str> {
    if !text
        .chars()
        .any(|character| character != '\n' && (character.is_control() || is_line_break(character)))
    {
        return Cow::Borrowed(text);
    }
    let mut cleaned = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                characters.next_if_eq(&'\n');
                cleaned.push('\n');
            }
            '\t' => cleaned.push(' '),
            character if is_line_break(character) => cleaned.push('\n'),
            character if character.is_control() => {}
            character => cleaned.push(character),
        }
    }
    Cow::Owned(cleaned)
}

/// The longest start of `text` of at most `room` chars that ends between graphemes: what of a
/// paste fits under the length limit, without an accent torn off its letter.
fn fit(text: &str, room: usize) -> &str {
    if text.len() <= room {
        return text;
    }
    let mut chars = 0;
    let mut end = 0;
    for grapheme in text.graphemes(true) {
        chars += grapheme.chars().count();
        if chars > room {
            break;
        }
        end += grapheme.len();
    }
    &text[..end]
}

/// The offset `utf16` UTF-16 code units into `text` reach -- the unit Windows' text input counts
/// in. One that falls inside a character counts up to its end.
pub fn offset_from_utf16(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (offset, character) in text.char_indices() {
        if units >= utf16 {
            return offset;
        }
        units += character.len_utf16();
    }
    text.len()
}

/// How many UTF-16 code units of `text` come before `offset`.
pub fn offset_to_utf16(text: &str, offset: usize) -> usize {
    text[..offset].chars().map(char::len_utf16).sum()
}

/// A word-boundary segment made only of spaces.
fn is_space(segment: &str) -> bool {
    segment.chars().all(char::is_whitespace)
}

/// A word-boundary segment that is a word, not spaces or punctuation.
fn is_word(segment: &str) -> bool {
    segment.chars().any(char::is_alphanumeric)
}

/// What an edit was, for folding a run of alike edits into one undo step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Typed text -- a newline included -- and what an IME composes.
    Typing,
    /// Backspace: the character, or the word, before the caret.
    Backspace,
    /// Delete: the one after it.
    Delete,
    /// Anything else, each a step of its own: a paste, a cut, a selection deleted.
    Other,
}

/// One undo step: at `at`, `removed` became `inserted`.
struct Edit {
    kind: Kind,
    at: usize,
    removed: String,
    inserted: String,
    /// The selection, as (anchor, head), before the step and after it.
    before: (usize, usize),
    after: (usize, usize),
}

impl Edit {
    /// Takes `next` into this step if it goes on with it: more typing where this typing left
    /// off -- up to a new word, so that undo takes text back a word at a time -- an IME
    /// rewriting what it composed, or Backspace or Delete pressed again.
    fn absorb(&mut self, next: &Edit) -> bool {
        if next.kind != self.kind {
            return false;
        }
        match self.kind {
            Kind::Typing => {
                let end = self.at + self.inserted.len();
                let within = self.at <= next.at && next.at + next.removed.len() <= end;
                let new_word = next.at == end
                    && next.removed.is_empty()
                    && self.inserted.ends_with(char::is_whitespace)
                    && next
                        .inserted
                        .starts_with(|character: char| !character.is_whitespace());
                if !within || new_word {
                    return false;
                }
                let from = next.at - self.at;
                self.inserted
                    .replace_range(from..from + next.removed.len(), &next.inserted);
            }
            Kind::Backspace => {
                if !next.inserted.is_empty() || next.at + next.removed.len() != self.at {
                    return false;
                }
                self.removed.insert_str(0, &next.removed);
                self.at = next.at;
            }
            Kind::Delete => {
                if !next.inserted.is_empty() || next.at != self.at {
                    return false;
                }
                self.removed.push_str(&next.removed);
            }
            Kind::Other => return false,
        }
        self.after = next.after;
        true
    }
}

/// The text, its selection and its undo history.
pub struct Model {
    text: String,
    /// The text's length in chars, the unit of the limit.
    chars: usize,
    max_chars: usize,
    /// Where the selection started; `head` is where it ends and the caret is. The two are the
    /// same when nothing is selected.
    anchor: usize,
    head: usize,
    /// On a soft wrap, the caret shows at the end of the upper row, not the start of the lower.
    upstream: bool,
    /// Where up and down aim, so that a run of them keeps its column across shorter rows.
    goal_x: Option<f32>,
    /// The text an IME is composing.
    marked: Option<Range<usize>>,
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    /// The last undo step may still take in the next edit: nothing but edits came since.
    open: bool,
}

impl Model {
    /// `text`, cleaned and cut to `max_chars`, with the caret at its end.
    pub fn new(text: String, max_chars: usize) -> Model {
        let mut model = Model {
            text: String::new(),
            chars: 0,
            max_chars,
            anchor: 0,
            head: 0,
            upstream: false,
            goal_x: None,
            marked: None,
            undo: VecDeque::new(),
            redo: Vec::new(),
            open: false,
        };
        model.set_text(text);
        model
    }

    /// Puts `text`, cleaned and cut to the limit, in place of the whole text, with the caret at
    /// its end. Not an edit: undo forgets everything before it.
    pub fn set_text(&mut self, text: String) {
        let cleaned = match clean(&text) {
            Cow::Borrowed(_) => None,
            Cow::Owned(cleaned) => Some(cleaned),
        };
        self.text = cleaned.unwrap_or(text);
        let fits = fit(&self.text, self.max_chars).len();
        self.text.truncate(fits);
        self.chars = self.text.chars().count();
        self.anchor = self.text.len();
        self.head = self.text.len();
        self.upstream = false;
        self.goal_x = None;
        self.marked = None;
        self.undo.clear();
        self.redo.clear();
        self.open = false;
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text's length in chars.
    pub fn chars(&self) -> usize {
        self.chars
    }

    /// Where the caret is.
    pub fn head(&self) -> usize {
        self.head
    }

    /// What's selected, start to end: empty when nothing is.
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    /// The selection was made backwards: the caret is at its start.
    pub fn reversed(&self) -> bool {
        self.head < self.anchor
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    /// The text an IME is composing, if it is.
    pub fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    /// The row the caret shows on.
    pub fn caret_row(&self, layout: &impl Layout) -> usize {
        row_of(layout.rows(), self.head, self.upstream)
    }

    /// `offset` brought back within the text and onto a char boundary: an offset from a layout
    /// may be a frame older than the last edit.
    pub fn snap(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    /// Puts the caret at `offset` -- on a soft wrap, at the end of the upper row if `upstream`
    /// -- selecting from the anchor if `extend`.
    pub fn move_to(&mut self, offset: usize, upstream: bool, extend: bool) {
        self.head = self.snap(offset);
        if !extend {
            self.anchor = self.head;
        }
        self.upstream = upstream;
        self.goal_x = None;
        self.open = false;
    }

    /// ←: a character back, or with `word` to the start of a word. A selection left without
    /// `extend` just collapses to its start.
    pub fn move_left(&mut self, word: bool, extend: bool) {
        let selection = self.selection();
        let offset = if word {
            self.word_start_before(self.head)
        } else if !extend && !selection.is_empty() {
            selection.start
        } else {
            self.grapheme_before(self.head)
        };
        self.move_to(offset, false, extend);
    }

    /// →: a character on, or with `word` to the start of the next word. A selection left
    /// without `extend` just collapses to its end.
    pub fn move_right(&mut self, word: bool, extend: bool) {
        let selection = self.selection();
        let offset = if word {
            self.word_start_after(self.head)
        } else if !extend && !selection.is_empty() {
            selection.end
        } else {
            self.grapheme_after(self.head)
        };
        self.move_to(offset, false, extend);
    }

    /// Home: the start of the caret's row.
    pub fn move_home(&mut self, layout: &impl Layout, extend: bool) {
        let row = layout.rows()[self.caret_row(layout)];
        self.move_to(row.start, false, extend);
    }

    /// End: the end of the caret's row -- a wrapped one's end, not the next row's start.
    pub fn move_end(&mut self, layout: &impl Layout, extend: bool) {
        let row = layout.rows()[self.caret_row(layout)];
        self.move_to(row.end, row.wraps, extend);
    }

    /// Ctrl+Home, Ctrl+End: the start or the end of the text.
    pub fn move_to_edge(&mut self, end: bool, extend: bool) {
        let offset = if end { self.text.len() } else { 0 };
        self.move_to(offset, false, extend);
    }

    /// ↑, ↓: the row above or below, as near the column the run of them started from as that row
    /// allows; past the first or last row, the start or end of the text.
    pub fn move_vertically(&mut self, layout: &impl Layout, down: bool, extend: bool) {
        let rows = layout.rows();
        let row = self.caret_row(layout);
        let x = self.goal_x.unwrap_or_else(|| layout.x(row, self.head));
        let target = if down {
            Some(row + 1).filter(|&target| target < rows.len())
        } else {
            row.checked_sub(1)
        };
        let Some(target) = target else {
            self.move_to_edge(down, extend);
            return;
        };
        let offset = layout.offset_at(target, x);
        let row = rows[target];
        self.move_to(offset, row.wraps && offset == row.end, extend);
        self.goal_x = Some(x);
    }

    /// Ctrl+A.
    pub fn select_all(&mut self) {
        self.move_to(0, false, false);
        self.move_to(self.text.len(), false, true);
    }

    /// A double click at `offset`: selects the word there.
    pub fn select_word(&mut self, offset: usize) {
        let word = self.word_at(self.snap(offset));
        self.move_to(word.start, false, false);
        self.move_to(word.end, false, true);
    }

    /// Typed text, or what an IME settled on: `text` over `range` if one is given, else over the
    /// composition in progress, else over the selection.
    pub fn type_text(&mut self, range: Option<Range<usize>>, text: &str) -> bool {
        let range = self.target(range);
        self.replace(range, &clean(text), Kind::Typing)
    }

    /// An IME's composition so far: `text` over `range`, or the composition, or the selection --
    /// marked, and with `selected`, offsets within `text`, selected.
    pub fn compose(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
    ) -> bool {
        let range = self.target(range);
        let start = range.start;
        let changed = self.replace(range, &clean(text), Kind::Typing);
        let end = self.head;
        self.marked = (end > start).then_some(start..end);
        if let Some(selected) = selected {
            self.anchor = self.snap((start + selected.start).min(end));
            self.head = self.snap((start + selected.end).min(end));
        }
        changed
    }

    /// The composition is over: its text stays as typed.
    pub fn unmark(&mut self) {
        self.marked = None;
    }

    /// Enter.
    pub fn newline(&mut self) -> bool {
        self.replace(self.selection(), "\n", Kind::Typing)
    }

    /// Ctrl+V: `text` over the selection, line breaks and all.
    pub fn paste(&mut self, text: &str) -> bool {
        self.replace(self.selection(), &clean(text), Kind::Other)
    }

    /// Ctrl+X: takes the selection out and hands it over; nothing with nothing selected.
    pub fn cut(&mut self) -> Option<String> {
        let selection = self.selection();
        if selection.is_empty() {
            return None;
        }
        let cut = self.text[selection.clone()].to_owned();
        self.replace(selection, "", Kind::Other);
        Some(cut)
    }

    /// Backspace: the selection, else the character -- or with `word`, the word -- before the
    /// caret.
    pub fn backspace(&mut self, word: bool) -> bool {
        let selection = self.selection();
        if !selection.is_empty() {
            return self.replace(selection, "", Kind::Other);
        }
        let start = if word {
            self.word_start_before(self.head)
        } else {
            self.grapheme_before(self.head)
        };
        self.replace(start..self.head, "", Kind::Backspace)
    }

    /// Delete: the selection, else the character after the caret -- or with `word`, all up to
    /// the next word.
    pub fn delete(&mut self, word: bool) -> bool {
        let selection = self.selection();
        if !selection.is_empty() {
            return self.replace(selection, "", Kind::Other);
        }
        let end = if word {
            self.word_start_after(self.head)
        } else {
            self.grapheme_after(self.head)
        };
        self.replace(self.head..end, "", Kind::Delete)
    }

    /// Ctrl+Z: takes back the last step, and puts back the selection it was made on.
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop_back() else {
            return false;
        };
        self.splice(edit.at..edit.at + edit.inserted.len(), &edit.removed);
        (self.anchor, self.head) = edit.before;
        self.redo.push(edit);
        true
    }

    /// Ctrl+Y, Ctrl+Shift+Z: makes the last step undone again.
    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        self.splice(edit.at..edit.at + edit.removed.len(), &edit.inserted);
        (self.anchor, self.head) = edit.after;
        self.undo.push_back(edit);
        true
    }

    /// What an edit from the platform's text input applies to.
    fn target(&self, range: Option<Range<usize>>) -> Range<usize> {
        range
            .map(|range| {
                let (start, end) = (self.snap(range.start), self.snap(range.end));
                start.min(end)..start.max(end)
            })
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection())
    }

    /// Puts `text`, already clean, over `range`, cut to what the limit leaves room for, and the
    /// caret after it; records the undo step. Whether the text changed.
    fn replace(&mut self, range: Range<usize>, text: &str, kind: Kind) -> bool {
        let removed_chars = self.text[range.clone()].chars().count();
        let text = fit(
            text,
            self.max_chars.saturating_sub(self.chars - removed_chars),
        );
        let before = (self.anchor, self.head);
        let end = range.start + text.len();
        self.anchor = end;
        self.head = end;
        self.upstream = false;
        self.goal_x = None;
        self.marked = None;
        if self.text[range.clone()] == *text {
            return false;
        }
        let edit = Edit {
            kind,
            at: range.start,
            removed: self.text[range.clone()].to_owned(),
            inserted: text.to_owned(),
            before,
            after: (end, end),
        };
        self.text.replace_range(range, text);
        self.chars = self.chars - removed_chars + text.chars().count();
        self.redo.clear();
        if !(self.open && self.undo.back_mut().is_some_and(|last| last.absorb(&edit))) {
            if self.undo.len() == UNDO_STEPS {
                self.undo.pop_front();
            }
            self.undo.push_back(edit);
        }
        self.open = true;
        true
    }

    /// Puts `text` in for `range` as undo and redo do: the text was this way before, so there's
    /// no limit to mind and nothing to record.
    fn splice(&mut self, range: Range<usize>, text: &str) {
        self.chars = self.chars - self.text[range.clone()].chars().count() + text.chars().count();
        self.text.replace_range(range, text);
        self.upstream = false;
        self.goal_x = None;
        self.marked = None;
        self.open = false;
    }

    fn grapheme_before(&self, offset: usize) -> usize {
        GraphemeCursor::new(offset, self.text.len(), true)
            .prev_boundary(&self.text, 0)
            .ok()
            .flatten()
            .unwrap_or(0)
    }

    fn grapheme_after(&self, offset: usize) -> usize {
        GraphemeCursor::new(offset, self.text.len(), true)
            .next_boundary(&self.text, 0)
            .ok()
            .flatten()
            .unwrap_or(self.text.len())
    }

    /// The paragraph -- the text between line breaks -- around `offset`.
    fn paragraph(&self, offset: usize) -> Range<usize> {
        let start = self.text[..offset]
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        let end = self.text[offset..]
            .find('\n')
            .map_or(self.text.len(), |newline| offset + newline);
        start..end
    }

    /// Where Ctrl+← goes from `offset`: over any spaces to the start of the word before them. A
    /// line break is a stop of its own.
    fn word_start_before(&self, offset: usize) -> usize {
        if self.text[..offset].ends_with('\n') {
            return offset - 1;
        }
        let start = self.paragraph(offset).start;
        let mut word_start = offset;
        for (index, segment) in self.text[start..offset].split_word_bound_indices().rev() {
            word_start = start + index;
            if !is_space(segment) {
                break;
            }
        }
        word_start
    }

    /// Where Ctrl+→ goes from `offset`: past the word there and the spaces after it, to the start
    /// of the next word. A line break is a stop of its own.
    fn word_start_after(&self, offset: usize) -> usize {
        if self.text[offset..].starts_with('\n') {
            return offset + 1;
        }
        let end = self.paragraph(offset).end;
        let mut word_start = offset;
        for (index, segment) in self.text[offset..end].split_word_bound_indices() {
            if index > 0 && !is_space(segment) {
                break;
            }
            word_start = offset + index + segment.len();
        }
        word_start
    }

    /// What a double click at `offset` selects: the word there, or right before it -- a caret
    /// placed at a word's end still means that word -- else the spaces or mark there.
    fn word_at(&self, offset: usize) -> Range<usize> {
        let paragraph = self.paragraph(offset);
        let mut before = None;
        let mut after = None;
        for (index, segment) in self.text[paragraph.clone()].split_word_bound_indices() {
            let range = paragraph.start + index..paragraph.start + index + segment.len();
            if range.end == offset {
                before = Some((range, is_word(segment)));
            } else if range.contains(&offset) {
                after = Some((range, is_word(segment)));
                break;
            }
        }
        [after, before]
            .into_iter()
            .flatten()
            .min_by_key(|(_, word)| !word)
            .map_or(offset..offset, |(range, _)| range)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pretend layout of `text` in `rows`, in a font that draws every char a unit wide but a
    /// `W`, a quarter wider: enough to put a caret between the other letters' columns. It
    /// hit-tests as the box does, over every stop of the row's paragraph.
    struct Pretend<'a> {
        text: &'a str,
        rows: Vec<Row>,
    }

    impl Pretend<'_> {
        /// How wide `range` of the text is drawn.
        fn width(&self, range: Range<usize>) -> f32 {
            self.text[range]
                .chars()
                .map(|character| if character == 'W' { 1.25 } else { 1. })
                .sum()
        }
    }

    impl Layout for Pretend<'_> {
        fn rows(&self) -> &[Row] {
            &self.rows
        }

        fn x(&self, row: usize, offset: usize) -> f32 {
            self.width(self.rows[row].start..offset)
        }

        fn offset_at(&self, row: usize, x: f32) -> usize {
            let row = self.rows[row];
            let start = self.text[..row.start]
                .rfind('\n')
                .map_or(0, |newline| newline + 1);
            let end = self.text[row.start..]
                .find('\n')
                .map_or(self.text.len(), |newline| row.start + newline);
            let stops = self.text[start..end]
                .char_indices()
                .map(|(index, _)| start + index)
                .chain([end])
                .map(|offset| (offset, self.width(start..offset)));
            nearest_stop(row, stops, self.width(start..row.start) + x)
        }
    }

    /// `text` laid out in `rows`: each row's text, and whether it wraps into the next.
    fn pretend<'a>(text: &'a str, rows: &[(&str, bool)]) -> Pretend<'a> {
        let mut start = 0;
        let rows = rows
            .iter()
            .map(|&(row_text, wraps)| {
                let end = start + row_text.len();
                assert_eq!(&text[start..end], row_text);
                let row = Row { start, end, wraps };
                start = if wraps { end } else { end + 1 };
                row
            })
            .collect();
        Pretend { text, rows }
    }

    fn model(text: &str) -> Model {
        Model::new(text.to_owned(), 8000)
    }

    fn type_chars(model: &mut Model, text: &str) {
        for character in text.chars() {
            model.type_text(None, character.encode_utf8(&mut [0; 4]));
        }
    }

    #[test]
    fn cleaning_turns_windows_line_breaks_and_tabs_into_what_the_box_draws() {
        assert_eq!(clean("a\r\nb\rc\td\u{7}e\n"), "a\nb\nc de\n");
        assert!(matches!(clean("один\nдва"), Cow::Borrowed(_)));
    }

    #[test]
    fn cleaning_turns_every_other_line_break_into_a_newline() {
        // Unicode's separators, which aren't control characters.
        assert_eq!(clean("a\u{2028}b\u{2029}c"), "a\nb\nc");
        // A vertical tab, a form feed and NEL, which are.
        assert_eq!(clean("c\u{0B}d\u{0C}e\u{85}f"), "c\nd\ne\nf");
    }

    #[test]
    fn a_paste_keeps_its_line_breaks_as_lf() {
        let mut model = model("");
        assert!(model.paste("first\r\nsecond\r\n\r\nлиния"));
        assert_eq!(model.text(), "first\nsecond\n\nлиния");
        assert_eq!(model.head(), model.text().len());
    }

    #[test]
    fn the_limit_counts_chars_and_cuts_what_does_not_fit() {
        let mut model = Model::new("abcdef".to_owned(), 4);
        assert_eq!(model.text(), "abcd");
        assert!(!model.type_text(None, "e"));
        assert_eq!(model.text(), "abcd");

        let mut model = Model::new(String::new(), 3);
        assert!(model.paste("привет"));
        assert_eq!(model.text(), "при");
        assert_eq!(model.chars(), 3);

        // A paste over a selection has the selection's room too.
        let mut model = Model::new("abcd".to_owned(), 5);
        model.select_all();
        assert!(model.paste("vwxyz12"));
        assert_eq!(model.text(), "vwxyz");
    }

    #[test]
    fn a_cut_paste_ends_between_graphemes() {
        let mut model = Model::new(String::new(), 3);
        model.paste("abe\u{301}c");
        assert_eq!(model.text(), "ab");
    }

    #[test]
    fn arrows_and_backspace_step_over_whole_characters() {
        let mut model = model("привет");
        model.move_left(false, false);
        assert_eq!(model.head(), "прив".len() + "е".len());
        assert!(model.backspace(false));
        assert_eq!(model.text(), "привт");
        assert!(model.delete(false));
        assert_eq!(model.text(), "прив");

        let mut model = Model::new("e\u{301}x".to_owned(), 8000);
        model.move_to(0, false, false);
        model.move_right(false, false);
        assert_eq!(model.head(), "e\u{301}".len());
        assert!(model.backspace(false));
        assert_eq!(model.text(), "x");
    }

    #[test]
    fn arrows_without_shift_collapse_a_selection() {
        let mut model = model("hello");
        model.move_to(1, false, false);
        model.move_to(4, false, true);
        model.move_left(false, false);
        assert_eq!((model.selection(), model.head()), (1..1, 1));
        model.move_to(4, false, true);
        model.move_right(false, false);
        assert_eq!((model.selection(), model.head()), (4..4, 4));
        model.move_left(false, true);
        assert_eq!(model.selection(), 3..4);
        assert!(model.reversed());
    }

    #[test]
    fn ctrl_arrows_stop_at_word_starts_marks_and_line_breaks() {
        let text = "Привет, мир!  Hello\nworld";
        let mut model = model(text);
        let mut stops = Vec::new();
        model.move_to_edge(false, false);
        while model.head() < text.len() {
            model.move_right(true, false);
            stops.push(model.head());
        }
        let at = |part: &str| text.find(part).unwrap();
        assert_eq!(
            stops,
            [
                at(","),
                at("мир"),
                at("!"),
                at("Hello"),
                at("\n"),
                at("world"),
                text.len()
            ]
        );

        let mut back = Vec::new();
        while model.head() > 0 {
            model.move_left(true, false);
            back.push(model.head());
        }
        assert_eq!(
            back,
            [
                at("world"),
                at("\n"),
                at("Hello"),
                at("!"),
                at("мир"),
                at(","),
                0
            ]
        );
    }

    #[test]
    fn ctrl_backspace_and_ctrl_delete_take_a_word_with_its_spaces() {
        let mut model = model("hello brave  new world");
        assert!(model.backspace(true));
        assert_eq!(model.text(), "hello brave  new ");
        assert!(model.backspace(true));
        assert_eq!(model.text(), "hello brave  ");
        model.move_to(0, false, false);
        assert!(model.delete(true));
        assert_eq!(model.text(), "brave  ");
        assert!(model.delete(true));
        assert_eq!(model.text(), "");
        assert!(!model.delete(true));
    }

    #[test]
    fn a_double_click_selects_the_word_it_lands_on() {
        let text = "привет мир  x,y\n\nend";
        let model = model(text);
        let at = |part: &str| text.find(part).unwrap();
        let word = |offset: usize| model.word_at(offset);
        assert_eq!(word(2), 0..at(" "));
        // At a word's end, still that word, not the space after it.
        assert_eq!(word(at(" ")), 0..at(" "));
        assert_eq!(word(at("мир") + 2), at("мир")..at("мир") + "мир".len());
        assert_eq!(word(at("  ") + 1), at("  ")..at("  ") + 2);
        assert_eq!(word(at(",")), at("x")..at(",")); // x, the word right before the comma
        assert_eq!(word(at("\n\n") + 1), at("\n\n") + 1..at("\n\n") + 1);
        assert_eq!(word(text.len()), at("end")..text.len());
    }

    #[test]
    fn typing_undoes_a_word_at_a_time() {
        let mut model = model("");
        type_chars(&mut model, "hello wor");
        assert!(model.newline());
        type_chars(&mut model, "ld");
        assert_eq!(model.text(), "hello wor\nld");
        assert!(model.undo());
        assert_eq!(model.text(), "hello wor\n");
        assert!(model.undo());
        assert_eq!(model.text(), "hello ");
        assert!(model.undo());
        assert_eq!(model.text(), "");
        assert!(!model.undo());
        assert!(model.redo());
        assert!(model.redo());
        assert_eq!(model.text(), "hello wor\n");
        assert_eq!(model.head(), model.text().len());
    }

    #[test]
    fn moving_the_caret_ends_a_typing_step() {
        let mut model = model("");
        type_chars(&mut model, "ab");
        model.move_left(false, false);
        type_chars(&mut model, "X");
        assert_eq!(model.text(), "aXb");
        model.undo();
        assert_eq!(model.text(), "ab");
        model.undo();
        assert_eq!(model.text(), "");
    }

    #[test]
    fn backspaces_and_deletes_in_a_row_undo_as_one_step() {
        let mut model = model("привет мир");
        for _ in 0..4 {
            model.backspace(false);
        }
        model.backspace(true);
        assert_eq!(model.text(), "");
        assert!(model.undo());
        assert_eq!(model.text(), "привет мир");
        assert_eq!(model.head(), model.text().len());

        model.move_to(0, false, false);
        model.delete(false);
        model.delete(false);
        assert_eq!(model.text(), "ивет мир");
        assert!(model.undo());
        assert_eq!((model.text(), model.head()), ("привет мир", 0));
    }

    #[test]
    fn undo_puts_back_the_selection_an_edit_replaced() {
        let mut model = model("hello world");
        model.select_word(8);
        assert_eq!(model.selected_text(), "world");
        model.type_text(None, "x");
        assert_eq!(model.text(), "hello x");
        model.undo();
        assert_eq!(model.text(), "hello world");
        assert_eq!(model.selection(), 6..11);
        model.redo();
        assert_eq!((model.text(), model.selection()), ("hello x", 7..7));
    }

    #[test]
    fn a_paste_is_a_step_of_its_own_and_a_new_edit_ends_redo() {
        let mut model = model("");
        type_chars(&mut model, "ab");
        model.paste("cd");
        model.undo();
        assert_eq!(model.text(), "ab");
        type_chars(&mut model, "e");
        assert!(!model.redo());
        assert_eq!(model.text(), "abe");
    }

    #[test]
    fn undo_goes_back_a_bounded_number_of_steps() {
        let mut model = model("");
        for _ in 0..=UNDO_STEPS {
            model.paste("x");
        }
        let mut undone = 0;
        while model.undo() {
            undone += 1;
        }
        assert_eq!(undone, UNDO_STEPS);
        assert_eq!(model.text(), "x");
    }

    #[test]
    fn an_ime_composition_undoes_as_one_step() {
        let mut model = model("");
        type_chars(&mut model, "a ");
        assert!(model.compose(None, "k", None));
        assert!(model.compose(None, "か", None));
        assert!(model.compose(None, "かな", Some(3..3)));
        assert_eq!(model.marked(), Some(2..8));
        assert_eq!(model.head(), 5);
        assert!(model.type_text(None, "仮名"));
        assert_eq!((model.text(), model.marked()), ("a 仮名", None));
        assert!(model.undo());
        assert_eq!(model.text(), "a ");
    }

    #[test]
    fn up_and_down_keep_their_column_across_short_rows() {
        let text = "abcdef ghij\nxy\nklmnopqr";
        let layout = pretend(
            text,
            &[
                ("abcdef ", true),
                ("ghij", false),
                ("xy", false),
                ("klmnopqr", false),
            ],
        );
        let mut model = model(text);
        model.move_to(5, false, false);
        model.move_vertically(&layout, true, false);
        assert_eq!(model.head(), "abcdef ghij".len());
        model.move_vertically(&layout, true, false);
        assert_eq!(model.head(), "abcdef ghij\nxy".len());
        model.move_vertically(&layout, true, true);
        assert_eq!(model.head(), text.find('p').unwrap());
        assert_eq!(
            model.selection(),
            "abcdef ghij\nxy".len()..text.find('p').unwrap()
        );
        model.move_vertically(&layout, false, false);
        model.move_vertically(&layout, false, false);
        model.move_vertically(&layout, false, false);
        assert_eq!(model.head(), 5);
        model.move_vertically(&layout, false, false);
        assert_eq!(model.head(), 0);
        model.move_to_edge(true, false);
        model.move_vertically(&layout, true, false);
        assert_eq!(model.head(), text.len());
    }

    #[test]
    fn past_the_last_letter_s_start_the_nearest_boundary_still_wins() {
        // After the wide W the caret sits at 1.25: nearer the boundary between x and y than the
        // paragraph's end past y.
        let text = "Wab\nxy";
        let layout = pretend(text, &[("Wab", false), ("xy", false)]);
        let mut model = model(text);
        model.move_to(1, false, false);
        model.move_vertically(&layout, true, false);
        assert_eq!(model.head(), text.find('y').unwrap());
        // A point on the last letter's left half is before it; on its right half, after it.
        assert_eq!(layout.offset_at(1, 1.4), text.find('y').unwrap());
        assert_eq!(layout.offset_at(1, 1.6), text.len());
    }

    #[test]
    fn end_on_a_wrapped_row_stays_on_that_row() {
        let text = "ab cdefgh";
        let layout = pretend(text, &[("ab ", true), ("cdefgh", false)]);
        let mut model = model(text);
        model.move_to(1, false, false);
        model.move_end(&layout, false);
        assert_eq!((model.head(), model.caret_row(&layout)), (3, 0));
        // Home from there is this row's start, not the next row's.
        model.move_home(&layout, false);
        assert_eq!(model.head(), 0);
        // The same offset with the caret moved there any other way is the lower row's start.
        model.move_to(3, false, false);
        assert_eq!(model.caret_row(&layout), 1);
        model.move_end(&layout, false);
        assert_eq!(model.head(), text.len());
        // Up from past the upper row's end lands on its end, and stays on it.
        model.move_vertically(&layout, false, false);
        assert_eq!((model.head(), model.caret_row(&layout)), (3, 0));
    }

    #[test]
    fn rows_resolve_a_soft_wrap_by_the_caret_s_side() {
        let rows = [
            Row {
                start: 0,
                end: 3,
                wraps: true,
            },
            Row {
                start: 3,
                end: 5,
                wraps: false,
            },
            Row {
                start: 6,
                end: 6,
                wraps: false,
            },
        ];
        assert_eq!(row_of(&rows, 3, false), 1);
        assert_eq!(row_of(&rows, 3, true), 0);
        assert_eq!(row_of(&rows, 5, true), 1);
        assert_eq!(row_of(&rows, 6, false), 2);
        assert_eq!(row_of(&rows, 0, true), 0);
    }

    #[test]
    fn utf16_offsets_count_surrogate_pairs_and_cyrillic() {
        let text = "aя😀b";
        assert_eq!(offset_to_utf16(text, text.find('b').unwrap()), 4);
        assert_eq!(offset_from_utf16(text, 4), text.find('b').unwrap());
        assert_eq!(offset_from_utf16(text, 2), text.find('😀').unwrap());
        assert_eq!(offset_from_utf16(text, 99), text.len());
    }
}
