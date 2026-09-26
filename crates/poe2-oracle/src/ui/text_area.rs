//! A multi-line text box: where the report window takes the player's own words. GPUI ships none
//! (its `examples/input.rs` is a one-line editor), so this one is built the way that example is:
//! characters -- typed, composed by an IME, or made with AltGr and dead keys -- arrive through
//! GPUI's platform text input (`EntityInputHandler`, registered each time the box paints), and
//! the keys that move or edit through the box's key-down listener, which stops them there so
//! that Windows doesn't also turn them into characters. The box leaves Tab, Esc and Ctrl+Enter to
//! its window -- Esc should first take the keyboard away from the box -- and any other key it
//! doesn't use goes on to the window's own listeners, printable keys included: those listeners
//! must let such a key go on too, or its character never reaches the box.
//!
//! The text wraps at the box's width, and the box grows with it from [`MIN_HEIGHT`] to
//! [`MAX_HEIGHT`], then scrolls, keeping the caret in view. The editing rules themselves --
//! selection, word boundaries, rows and columns, undo, the length limit -- are
//! `crate::text_area`'s, apart from GPUI. The box looks like the settings window's fields: a dark
//! field whose edge warms to gold under the pointer and glows gold while it has the keyboard.

use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, ContentMask, Context, DispatchPhase, Element,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, GlobalElementId, InspectorElementId, IntoElement, KeyDownEvent, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, Rgba,
    ScrollWheelEvent, SharedString, Style, Subscription, Task, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, WrappedLine, div, fill, point, prelude::*, px, relative, rgb, size,
};

use crate::text_area::{
    Layout, Model, Row, nearest_stop, offset_from_utf16, offset_to_utf16, row_of,
};
use crate::ui::style::{CONTROL_RADIUS, alpha, ease_hover, ease_state, glow};
use crate::ui::theme::{BG_FIELD, BORDER_FIELD, GOLD, TEXT, TEXT_MUTED, blend, rems_from_px};

/// The text's size and line spacing: the settings window's.
const TEXT_SIZE: f32 = 14.;
const LINE_HEIGHT: f32 = 1.4;
/// The text's height at 100% UI scale, px: the least the box takes, and the most it grows to
/// before it scrolls -- six and fourteen lines.
const MIN_HEIGHT: f32 = 120.;
const MAX_HEIGHT: f32 = 280.;
/// Between the box's edge and its text, px at 100% UI scale.
const PADDING_X: f32 = 10.;
const PADDING_Y: f32 = 6.;
/// How long the caret stays shown, then hidden: Windows' own caret blink time.
const BLINK: Duration = Duration::from_millis(530);
/// The selection's gold, and what's left of it while the box doesn't have the keyboard.
const SELECTION_OPACITY: f32 = 0.35;
const SELECTION_OPACITY_UNFOCUSED: f32 = 0.18;

pub struct TextArea {
    model: Model,
    /// The text as GPUI shapes it, remade on each edit rather than copied every frame.
    shared: SharedString,
    /// What the empty box shows, worded each time it's shown: it follows the interface
    /// language.
    placeholder: fn() -> &'static str,
    focus_handle: FocusHandle,
    /// The text as last painted, for the mouse, the IME and moves by row; none before the first
    /// paint.
    shown: Option<Rc<Shown>>,
    /// How far the text is scrolled up.
    scroll_top: Pixels,
    /// The caret moved or the text changed: the next paint scrolls the caret into view.
    reveal: bool,
    /// The left button went down on the text and hasn't come up: moving it selects.
    selecting: bool,
    /// The caret's blink phase.
    caret_on: bool,
    /// Blinks the caret while the box has the keyboard and its window is active.
    blink: Option<Task<()>>,
    _subscriptions: [Subscription; 3],
}

/// What a [`TextArea`] tells its owner.
pub enum TextAreaEvent {
    /// The player changed the text: [`TextArea::text`] has it.
    Changed,
}

impl EventEmitter<TextAreaEvent> for TextArea {}

impl TextArea {
    /// A box holding `text` -- line breaks as `\n`, cut to `max_chars` chars -- with the caret at
    /// its end. `placeholder` words what the box shows while empty.
    pub fn new(
        text: impl Into<String>,
        placeholder: fn() -> &'static str,
        max_chars: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TextArea {
        let model = Model::new(text.into(), max_chars);
        let focus_handle = cx.focus_handle();
        let subscriptions = [
            cx.on_focus(&focus_handle, window, |area, window, cx| {
                area.restart_blink(window, cx);
                cx.notify();
            }),
            cx.on_blur(&focus_handle, window, |area, window, cx| {
                area.selecting = false;
                area.model.unmark();
                area.restart_blink(window, cx);
                cx.notify();
            }),
            cx.observe_window_activation(window, |area, window, cx| {
                area.restart_blink(window, cx);
                cx.notify();
            }),
        ];
        TextArea {
            shared: model.text().to_owned().into(),
            model,
            placeholder,
            focus_handle,
            shown: None,
            scroll_top: Pixels::ZERO,
            reveal: true,
            selecting: false,
            caret_on: true,
            blink: None,
            _subscriptions: subscriptions,
        }
    }

    /// The text, line breaks as `\n`.
    pub fn text(&self) -> &str {
        self.model.text()
    }

    /// Puts `text` in the box in place of what it holds, caret at its end, as [`TextArea::new`]
    /// takes it. Not the player's edit: no [`TextAreaEvent::Changed`], and undo can't take it
    /// back.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.model.set_text(text.into());
        self.shared = self.model.text().to_owned().into();
        self.reveal = true;
        cx.notify();
    }

    /// What the empty box shows.
    pub fn set_placeholder(&mut self, placeholder: fn() -> &'static str, cx: &mut Context<Self>) {
        self.placeholder = placeholder;
        cx.notify();
    }

    /// The text's length in chars -- the unit of the limit.
    pub fn char_count(&self) -> usize {
        self.model.chars()
    }

    /// After an edit: the text as shaped follows it, and the owner hears of it.
    fn edited(&mut self, cx: &mut Context<Self>) {
        self.shared = self.model.text().to_owned().into();
        cx.emit(TextAreaEvent::Changed);
    }

    /// After anything the player did in the box: the caret shows, steadily for a moment, and in
    /// view.
    fn acted(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.reveal = true;
        self.restart_blink(window, cx);
        cx.notify();
    }

    /// Shows the caret and starts it blinking afresh -- or stops it, if the box doesn't have the
    /// keyboard or its window isn't active.
    fn restart_blink(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.caret_on = true;
        self.blink =
            (self.focus_handle.is_focused(window) && window.is_window_active()).then(|| {
                cx.spawn(async move |area, cx| {
                    loop {
                        cx.background_executor().timer(BLINK).await;
                        let blinked = area.update(cx, |area, cx| {
                            area.caret_on = !area.caret_on;
                            cx.notify();
                        });
                        if blinked.is_err() {
                            break;
                        }
                    }
                })
            });
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        // Alt, AltGr (Windows reports it as Ctrl+Alt) and the Windows key make characters or
        // belong to the window.
        if modifiers.alt || modifiers.platform {
            return;
        }
        let (ctrl, shift) = (modifiers.control, modifiers.shift);
        let shown = self.shown.clone();
        let edited = match keystroke.key.as_str() {
            "left" => {
                self.model.move_left(ctrl, shift);
                false
            }
            "right" => {
                self.model.move_right(ctrl, shift);
                false
            }
            key @ ("up" | "down") if !ctrl => {
                if let Some(shown) = &shown {
                    self.model.move_vertically(&**shown, key == "down", shift);
                }
                false
            }
            key @ ("home" | "end") if ctrl => {
                self.model.move_to_edge(key == "end", shift);
                false
            }
            "home" => {
                if let Some(shown) = &shown {
                    self.model.move_home(&**shown, shift);
                }
                false
            }
            "end" => {
                if let Some(shown) = &shown {
                    self.model.move_end(&**shown, shift);
                }
                false
            }
            "backspace" => self.model.backspace(ctrl),
            "delete" => self.model.delete(ctrl),
            "enter" if !ctrl => self.model.newline(),
            "a" if ctrl && !shift => {
                self.model.select_all();
                false
            }
            "c" if ctrl && !shift => {
                let selected = self.model.selected_text();
                if !selected.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(selected.to_owned()));
                }
                false
            }
            "x" if ctrl && !shift => match self.model.cut() {
                Some(cut) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(cut));
                    true
                }
                None => false,
            },
            "v" if ctrl && !shift => cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .is_some_and(|pasted| self.model.paste(&pasted)),
            "z" if ctrl => {
                if shift {
                    self.model.redo()
                } else {
                    self.model.undo()
                }
            }
            "y" if ctrl && !shift => self.model.redo(),
            _ => return,
        };
        cx.stop_propagation();
        if edited {
            self.edited(cx);
        }
        self.acted(window, cx);
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some((offset, upstream)) = self.hit(event.position) else {
            return;
        };
        if event.click_count >= 2 {
            self.model.select_word(offset);
        } else {
            self.model.move_to(offset, upstream, event.modifiers.shift);
        }
        self.selecting = event.click_count == 1;
        self.acted(window, cx);
    }

    /// The pointer moved anywhere in the window while [`TextArea::selecting`].
    fn drag(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.selecting = false;
            return;
        }
        let Some((offset, upstream)) = self.hit(event.position) else {
            return;
        };
        let (head, selection) = (self.model.head(), self.model.selection());
        self.model.move_to(offset, upstream, true);
        if (self.model.head(), self.model.selection()) != (head, selection) {
            self.acted(window, cx);
        }
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(shown) = &self.shown else {
            return;
        };
        let delta = event.delta.pixel_delta(shown.line_height).y;
        let top = (self.scroll_top - delta)
            .min(shown.scroll_range())
            .max(Pixels::ZERO);
        // At either end the wheel is the window's, to scroll whatever holds the box.
        if top != self.scroll_top {
            self.scroll_top = top;
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// The caret position under `position`, a point in the window: its offset, and whether it's
    /// at the end of a wrapped row. Points off the text find its nearest row and column.
    fn hit(&self, position: Point<Pixels>) -> Option<(usize, bool)> {
        let shown = self.shown.as_ref()?;
        let local = position - shown.bounds.origin;
        let y = local.y + self.scroll_top;
        let row = if y < Pixels::ZERO {
            0
        } else {
            ((y / shown.line_height) as usize).min(shown.rows.len() - 1)
        };
        let offset = self.model.snap(shown.offset_at(row, local.x.as_f32()));
        let Row { end, wraps, .. } = shown.rows[row];
        Some((offset, wraps && offset == end))
    }

    /// What the box shows this frame: the text, the IME's composition underlined, or the
    /// placeholder, dimmed, while there's no text.
    fn display(&self, window: &Window) -> Display {
        let style = window.text_style();
        if self.model.text().is_empty() {
            let text: SharedString = (self.placeholder)().into();
            let run = TextRun {
                color: rgb(TEXT_MUTED).into(),
                ..style.to_run(text.len())
            };
            return Display {
                text,
                runs: vec![run],
                placeholder: true,
            };
        }
        let text = self.shared.clone();
        let run = style.to_run(text.len());
        let runs = match self.model.marked() {
            Some(marked) => {
                let underline = UnderlineStyle {
                    thickness: px(1.),
                    color: Some(run.color),
                    wavy: false,
                };
                [
                    (marked.start, None),
                    (marked.len(), Some(underline)),
                    (text.len() - marked.end, None),
                ]
                .into_iter()
                .filter(|&(len, _)| len > 0)
                .map(|(len, underline)| TextRun {
                    len,
                    underline,
                    ..run.clone()
                })
                .collect()
            }
            None => vec![run],
        };
        Display {
            text,
            runs,
            placeholder: false,
        }
    }

    /// Lays out this frame's painting from the text as shaped at its bounds: scrolls the caret
    /// into view if it moved, and places the selection and the caret.
    fn frame(&mut self, shown: Rc<Shown>, window: &Window, overhang: Pixels) -> Frame {
        let bounds = shown.bounds;
        let line_height = shown.line_height;
        if self.reveal {
            let top = line_height * self.model.caret_row(&*shown) as f32;
            if top < self.scroll_top {
                self.scroll_top = top;
            } else if top + line_height > self.scroll_top + bounds.size.height {
                self.scroll_top = top + line_height - bounds.size.height;
            }
            self.reveal = false;
        }
        self.scroll_top = self.scroll_top.min(shown.scroll_range()).max(Pixels::ZERO);
        let first = (self.scroll_top / line_height) as usize;
        let last = ((self.scroll_top + bounds.size.height) / line_height).ceil() as usize;
        let visible = first..last.min(shown.painted_rows);
        let focused = self.focus_handle.is_focused(window);
        let selection = self.selection_bounds(&shown, visible.clone());
        let caret = (focused
            && window.is_window_active()
            && self.caret_on
            && self.model.selection().is_empty())
        .then(|| self.caret_bounds(&shown));
        self.shown = Some(shown.clone());
        Frame {
            shown,
            visible,
            scroll_top: self.scroll_top,
            selection,
            selection_color: alpha(
                GOLD,
                if focused {
                    SELECTION_OPACITY
                } else {
                    SELECTION_OPACITY_UNFOCUSED
                },
            ),
            caret,
            // Glyphs may overhang the text's left and right edges a little, into the padding.
            clip: Bounds::new(
                point(bounds.left() - overhang, bounds.top()),
                size(bounds.size.width + overhang * 2., bounds.size.height),
            ),
            selecting: self.selecting,
        }
    }

    /// The selection's band on each visible row, in the window. A line break selected shows as a
    /// sliver past its row's end.
    fn selection_bounds(&self, shown: &Shown, visible: Range<usize>) -> Vec<Bounds<Pixels>> {
        let selection = self.model.selection();
        if selection.is_empty() {
            return Vec::new();
        }
        let line_height = shown.line_height;
        let sliver = line_height / 4.;
        visible
            .filter_map(|index| {
                let row = *shown.rows.get(index)?;
                if selection.end < row.start || selection.start > row.end {
                    return None;
                }
                let left = if selection.start > row.start {
                    px(shown.x(index, selection.start))
                } else {
                    Pixels::ZERO
                };
                let right = if selection.end > row.end {
                    px(shown.x(index, row.end)) + if row.wraps { Pixels::ZERO } else { sliver }
                } else {
                    px(shown.x(index, selection.end))
                };
                (right > left).then(|| {
                    let top = shown.bounds.top() + line_height * index as f32 - self.scroll_top;
                    Bounds::new(
                        point(shown.bounds.left() + left, top),
                        size(right - left, line_height),
                    )
                })
            })
            .collect()
    }

    /// The caret's bar, in the window: as tall as the glyphs, centred on its row as they are.
    fn caret_bounds(&self, shown: &Shown) -> Bounds<Pixels> {
        const WIDTH: Pixels = px(1.);
        let row = self.model.caret_row(shown);
        let x = px(shown.x(row, self.model.head())).min(shown.bounds.size.width - WIDTH);
        let top = shown.bounds.top() + shown.line_height * row as f32 - self.scroll_top
            + (shown.line_height - shown.glyph_height) / 2.;
        Bounds::new(
            point(shown.bounds.left() + x, top),
            size(WIDTH, shown.glyph_height),
        )
    }

    /// The platform's `range` of UTF-16 code units as offsets into the text, start first.
    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let text = self.model.text();
        let (start, end) = (
            offset_from_utf16(text, range.start),
            offset_from_utf16(text, range.end),
        );
        start.min(end)..start.max(end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let text = self.model.text();
        offset_to_utf16(text, range.start)..offset_to_utf16(text, range.end)
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range);
        adjusted_range.replace(self.range_to_utf16(&range));
        Some(self.model.text()[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.model.selection()),
            reversed: self.model.reversed(),
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.model
            .marked()
            .map(|marked| self.range_to_utf16(&marked))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.model.unmark();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.map(|range| self.range_from_utf16(&range));
        if self.model.type_text(range, text) {
            self.edited(cx);
        }
        self.acted(window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.map(|range| self.range_from_utf16(&range));
        let selected = new_selected_range.map(|selected| {
            offset_from_utf16(new_text, selected.start)..offset_from_utf16(new_text, selected.end)
        });
        if self.model.compose(range, new_text, selected) {
            self.edited(cx);
        }
        self.acted(window, cx);
    }

    /// Where the IME puts its candidate window: under the range's start.
    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let shown = self.shown.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = self.model.snap(range.start);
        let row = row_of(&shown.rows, start, false);
        let end = self
            .model
            .snap(range.end)
            .min(shown.rows[row].end)
            .max(start);
        let (left, right) = (shown.x(row, start), shown.x(row, end));
        Some(Bounds::new(
            point(
                element_bounds.left() + px(left),
                element_bounds.top() + shown.line_height * row as f32 - self.scroll_top,
            ),
            size(px(right - left), shown.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let (offset, _) = self.hit(point)?;
        Some(offset_to_utf16(self.model.text(), offset))
    }
}

impl Focusable for TextArea {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let field = div()
            .id("text-area")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::key_down))
            // The press itself moves the keyboard here (`track_focus`).
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_scroll_wheel(cx.listener(Self::scroll))
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .px(rems_from_px(PADDING_X))
            .py(rems_from_px(PADDING_Y))
            .rounded(rems_from_px(CONTROL_RADIUS))
            .bg(rgb(BG_FIELD))
            .border_1()
            .text_size(rems_from_px(TEXT_SIZE))
            .line_height(relative(LINE_HEIGHT))
            .text_color(rgb(TEXT))
            .cursor_text()
            .child(TextElement { area: cx.entity() })
            // The keyboard's gold ring, easing in and out over the edge.
            .child(ease_state(
                "focus",
                focused,
                div()
                    .absolute()
                    .inset_0()
                    .rounded(rems_from_px(CONTROL_RADIUS))
                    .border_1()
                    .border_color(rgb(GOLD)),
                |ring, on| ring.opacity(on).shadow(glow(GOLD, 0.6 * on)),
            ));
        ease_hover("text-area", field, |field, hover| {
            field
                .border_color(rgb(blend(BORDER_FIELD, GOLD, 0.7 * hover)))
                .shadow(glow(GOLD, 0.4 * hover))
        })
    }
}

/// The text as painted: GPUI's wrapped lines, one per paragraph, and the rows they make.
struct Shown {
    lines: Vec<WrappedLine>,
    /// Each paragraph's first row.
    line_rows: Vec<usize>,
    /// The rows the caret moves through: the text's, or one empty row under the placeholder.
    rows: Vec<Row>,
    /// Each row's paragraph, where that paragraph starts in the text, and how far along its
    /// unwrapped line the row begins. None under the placeholder.
    row_lines: Vec<RowLine>,
    /// How many rows are painted: the text's, or the placeholder's.
    painted_rows: usize,
    line_height: Pixels,
    /// The font's height -- its ascent and descent -- which the caret takes.
    glyph_height: Pixels,
    /// Where the text went, in the window: the box less its padding.
    bounds: Bounds<Pixels>,
}

struct RowLine {
    line: usize,
    line_start: usize,
    x: Pixels,
}

impl Shown {
    /// `lines`, `display`'s paragraphs as GPUI shaped them, in rows. Each paragraph starts where
    /// the text puts it, not where the shaped line before it ends: a line DirectWrite failed to
    /// lay out comes back empty, and would shift every row after it.
    fn new(
        lines: Vec<WrappedLine>,
        display: &Display,
        line_height: Pixels,
        glyph_height: Pixels,
        bounds: Bounds<Pixels>,
    ) -> Shown {
        let mut line_rows = Vec::with_capacity(lines.len());
        let mut rows = Vec::new();
        let mut row_lines = Vec::new();
        let mut painted_rows = 0;
        let mut line_start = 0;
        for (line, (wrapped, paragraph)) in lines.iter().zip(display.text.split('\n')).enumerate() {
            line_rows.push(painted_rows);
            painted_rows += wrapped.wrap_boundaries().len() + 1;
            if display.placeholder {
                continue;
            }
            let (mut start, mut x) = (0, Pixels::ZERO);
            for boundary in wrapped.wrap_boundaries() {
                let glyph =
                    &wrapped.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix];
                rows.push(Row {
                    start: line_start + start,
                    end: line_start + glyph.index,
                    wraps: true,
                });
                row_lines.push(RowLine {
                    line,
                    line_start,
                    x,
                });
                (start, x) = (glyph.index, glyph.position.x);
            }
            rows.push(Row {
                start: line_start + start,
                end: line_start + paragraph.len(),
                wraps: false,
            });
            row_lines.push(RowLine {
                line,
                line_start,
                x,
            });
            line_start += paragraph.len() + 1;
        }
        if rows.is_empty() {
            rows.push(Row {
                start: 0,
                end: 0,
                wraps: false,
            });
        }
        Shown {
            lines,
            line_rows,
            rows,
            row_lines,
            painted_rows: painted_rows.max(1),
            line_height,
            glyph_height,
            bounds,
        }
    }

    /// How far the text can scroll: its height past the box's.
    fn scroll_range(&self) -> Pixels {
        (self.line_height * self.painted_rows as f32 - self.bounds.size.height).max(Pixels::ZERO)
    }
}

impl Layout for Shown {
    fn rows(&self) -> &[Row] {
        &self.rows
    }

    fn x(&self, row: usize, offset: usize) -> f32 {
        let Some(row_line) = self.row_lines.get(row) else {
            return 0.;
        };
        let layout = &self.lines[row_line.line].unwrapped_layout;
        (layout.x_for_index(offset.saturating_sub(row_line.line_start)) - row_line.x).as_f32()
    }

    fn offset_at(&self, row: usize, x: f32) -> usize {
        let Some(row_line) = self.row_lines.get(row) else {
            return self.rows[row].start;
        };
        let layout = &self.lines[row_line.line].unwrapped_layout;
        // Each glyph's start and the paragraph's end. GPUI's `closest_index_for_x` weighs only
        // the starts: past the last one it takes the end, however much nearer that start is.
        let stops = layout
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|glyph| (glyph.index, glyph.position.x))
            .chain([(layout.len, layout.width)])
            .map(|(index, x)| (row_line.line_start + index, x.as_f32()));
        nearest_stop(self.rows[row], stops, row_line.x.as_f32() + x)
    }
}

/// The box's text: laid out at its width, scrolled, with the selection and the caret.
struct TextElement {
    area: Entity<TextArea>,
}

/// What the box shows: its text or its placeholder, as runs for GPUI to shape.
struct Display {
    text: SharedString,
    runs: Vec<TextRun>,
    placeholder: bool,
}

/// A frame's painting, worked out before it's painted.
struct Frame {
    shown: Rc<Shown>,
    /// The rows in view.
    visible: Range<usize>,
    scroll_top: Pixels,
    selection: Vec<Bounds<Pixels>>,
    selection_color: Rgba,
    caret: Option<Bounds<Pixels>>,
    clip: Bounds<Pixels>,
    selecting: bool,
}

/// `display` shaped into wrapped lines, one per paragraph, wrapped at `width` if there is one.
fn shape(
    window: &Window,
    display: &Display,
    font_size: Pixels,
    width: Option<Pixels>,
) -> Vec<WrappedLine> {
    match window.text_system().shape_text(
        display.text.clone(),
        font_size,
        &display.runs,
        width,
        None,
    ) {
        Ok(lines) => lines.into_vec(),
        Err(error) => {
            log::error!("Couldn't lay out the text box's text: {error:#}");
            Vec::new()
        }
    }
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = Rc<Display>;
    type PrepaintState = Frame;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    /// As wide as the box, and as tall as the text wrapped at that width, within the box's
    /// least and most.
    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let display = Rc::new(self.area.read(cx).display(window));
        let rem_size = window.rem_size();
        let font_size = window.text_style().font_size.to_pixels(rem_size);
        let line_height = window.line_height();
        let least = rems_from_px(MIN_HEIGHT).to_pixels(rem_size);
        let most = rems_from_px(MAX_HEIGHT).to_pixels(rem_size);
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let measured = display.clone();
        let layout_id =
            window.request_measured_layout(style, move |known, available, window, _| {
                let width = known.width.or(match available.width {
                    AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });
                let rows: usize = shape(window, &measured, font_size, width)
                    .iter()
                    .map(|line| line.wrap_boundaries().len() + 1)
                    .sum();
                let height = known
                    .height
                    .unwrap_or_else(|| (line_height * rows.max(1) as f32).max(least).min(most));
                size(width.unwrap_or_default(), height)
            });
        (layout_id, display)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        display: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let rem_size = window.rem_size();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(rem_size);
        let lines = shape(window, display, font_size, Some(bounds.size.width));
        // The caret takes the font's height, not the first line's: GPUI shapes an empty line
        // with no font, which leaves it no height. The font's descent comes negative, a y below
        // the baseline.
        let text_system = window.text_system();
        let font = text_system.resolve_font(&style.font());
        let glyph_height =
            text_system.ascent(font, font_size) + text_system.descent(font, font_size).abs();
        let shown = Rc::new(Shown::new(
            lines,
            display,
            window.line_height(),
            glyph_height,
            bounds,
        ));
        let overhang = rems_from_px(PADDING_X).to_pixels(rem_size);
        self.area
            .update(cx, |area, _| area.frame(shown, window, overhang))
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _display: &mut Self::RequestLayoutState,
        frame: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.area.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.area.clone()),
            cx,
        );
        // A drag that started on the text goes on selecting wherever the pointer goes.
        if frame.selecting {
            let area = self.area.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    area.update(cx, |area, cx| area.drag(event, window, cx));
                }
            });
            let area = self.area.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _window, cx| {
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                    area.update(cx, |area, _| area.selecting = false);
                }
            });
        }
        let shown = &frame.shown;
        let line_height = shown.line_height;
        window.with_content_mask(Some(ContentMask { bounds: frame.clip }), |window| {
            for band in &frame.selection {
                window.paint_quad(fill(*band, frame.selection_color));
            }
            for (line, &first_row) in shown.lines.iter().zip(&shown.line_rows) {
                let last_row = first_row + line.wrap_boundaries().len();
                if last_row < frame.visible.start || first_row >= frame.visible.end {
                    continue;
                }
                let origin = point(
                    bounds.left(),
                    bounds.top() + line_height * first_row as f32 - frame.scroll_top,
                );
                if let Err(error) =
                    line.paint(origin, line_height, TextAlign::Left, None, window, cx)
                {
                    log::error!("Couldn't paint the text box's text: {error:#}");
                }
            }
            if let Some(caret) = frame.caret {
                window.paint_quad(fill(caret, rgb(GOLD)));
            }
        });
    }
}
