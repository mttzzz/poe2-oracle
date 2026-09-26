//! The report window: the player writes to the developer from inside the app -- a problem, an
//! idea, an item the price panel read or priced wrong, or the crash the last run ended with --
//! and the app sends it to its service (`crate::report`). In the settings window's game-styled
//! look (`ui::style`): a title bar that drags the window, the kind as a segmented choice, a
//! multi-line box for the player's words (`ui::text_area`) with a count against the service's
//! limit, a contact to be answered at, the diagnostics toggle, what's attached, what leaves the
//! computer, and Send.
//!
//! Send stays off until the report passes the service's own rules (`Report::check`). Sending
//! collects the diagnostics off the main thread, when they're attached, and shows the service's
//! answer in place of the form: the report's number, or why it didn't go -- with Try again, and
//! Save to desktop for passing it on another way.
//!
//! One window at a time (`app::open_report`): another request brings it forward, and it takes
//! the request over while a report is being written or was just sent ([`ReportView::take`]). Esc
//! takes the keyboard from the text box first, then closes the window, as ×, Cancel, Close and
//! `WM_CLOSE` do -- without asking about what's typed, and not while a report is on its way: its
//! answer shows in the window. The window stays above the game, as the settings window does.
//!
//! It follows the UI scale as the settings window does: everything in it is sized in rems, whose
//! size the scale sets, and it opens sized for the scale (`app::open_report`).

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, Render, SharedString, Window, WindowControlArea, div, prelude::*,
    px, relative, rgb,
};
use oracle_protocol::{MAX_CONTACT_CHARS, MAX_TEXT_CHARS, Report, ReportKind};

use crate::diagnostics::{self, WINDOWS_VERSION};
use crate::i18n;
use crate::platform::win32::Win32Overlay;
use crate::price_check::PriceCheckApp;
use crate::report::{self, Crash, Draft, Failure, Form, Request, Stage, Takeover};
use crate::tr;
use crate::ui::fonts::{self, NameFont};
use crate::ui::style::{
    ButtonKind, alpha, appear, button, diamond, ease_hover, game_frame, heading, link, segmented,
    switch, title_button, title_gradient,
};
use crate::ui::text_area::{TextArea, TextAreaEvent};
use crate::ui::text_field::{Committed, TextField};
use crate::ui::theme::{
    BASE_REM_SIZE, BG_PANEL, BORDER_GOLD, GOLD, GOLD_LIGHT, TEXT, TEXT_DIM, TEXT_MUTED,
    TEXT_WARNING, rems_from_px,
};

/// The window's size at 100 % UI scale -- room for the text box at its tallest -- and the least
/// the player can size it to, the form then scrolling; both grow and shrink with the scale, as its
/// content does.
pub(crate) const WINDOW_SIZE: (f32, f32) = (620., 720.);
pub(crate) const WINDOW_MIN_SIZE: (f32, f32) = (520., 560.);

const TITLE_HEIGHT: f32 = 40.;
/// Inset of the window's content from its edges, and the gap between the form's parts.
const INSET: f32 = 20.;
const GAP: f32 = 14.;
/// How much a control out of reach -- sending, or Send before the report can go -- keeps.
const INERT_OPACITY: f32 = 0.45;

pub struct ReportView {
    app: Entity<PriceCheckApp>,
    /// The window's own focus: Esc closes from here, and sending moves the keyboard here.
    focus_handle: FocusHandle,
    /// The report apart from its words: where it is, its kind, what it can attach, and the
    /// diagnostics toggle. The crash in it is forgotten once the window is done with it
    /// (`report::forget_crash`).
    form: Form<Crash>,
    text: Entity<TextArea>,
    contact: Entity<TextField>,
    /// The report the fields make passes the service's rules: Send is on.
    ready: bool,
    /// A save to the desktop -- «Что внутри», or an unsent report -- is under way.
    saving: bool,
    /// Why the last save to the desktop failed.
    save_error: Option<String>,
}

impl ReportView {
    pub fn new(
        app: Entity<PriceCheckApp>,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text =
            cx.new(|cx| TextArea::new("", placeholder(request.kind), MAX_TEXT_CHARS, window, cx));
        cx.subscribe(&text, |view, _, _: &TextAreaEvent, cx| view.edited(cx))
            .detach();
        let contact = cx.new(|cx| TextField::new("", || "", window, cx));
        // The field tells only when it's left, but redraws with every key: Send follows that.
        cx.observe(&contact, |view, _, cx| view.edited(cx)).detach();
        cx.subscribe(&contact, |_, field, _: &Committed, cx| {
            cut_contact(&field, cx)
        })
        .detach();

        // Anything that closes the window through `WM_CLOSE` -- Alt+F4, the taskbar -- closes it
        // the way × does (see `SettingsView::new`).
        let view = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            view.update(cx, |view, cx| view.close("WM_CLOSE", window, cx))
                .is_err()
        });
        // Nothing focused -- the box or the field left -- is the window's own focus again, where
        // Esc closes.
        cx.on_focus_lost(window, |view, window, cx| {
            view.focus_handle.focus(window, cx)
        })
        .detach();
        // The UI scale and the league come from the app as they stand.
        cx.observe(&app, |_, _, cx| cx.notify()).detach();

        match Win32Overlay::from_window(window) {
            Ok(overlay) => {
                // Windows 11's rounded corners and outline would cut the frame's corner diamonds.
                if let Err(err) = overlay.disable_dwm_frame() {
                    log::warn!("{err:#}");
                }
                // Above the game, as the settings window is. `SetWindowPos` sends messages into
                // GPUI's window procedure: not from inside this update.
                cx.spawn(async move |_, _| {
                    if let Err(err) = overlay.set_topmost(true) {
                        log::warn!("{err:#}");
                    }
                })
                .detach();
            }
            Err(err) => log::warn!("the report window's handle is unavailable: {err:#}"),
        }
        window.focus(&text.focus_handle(cx), cx);

        let Request { kind, item, crash } = request;
        let mut view = ReportView {
            app,
            focus_handle: cx.focus_handle(),
            form: Form::new(kind, item, crash),
            text,
            contact,
            ready: false,
            saving: false,
            save_error: None,
        };
        view.ready = view.report(None, cx).check().is_ok();
        view
    }

    /// Takes another request over (`app::open_report`) while a report is being written -- its
    /// kind, and its item in place of the one before; what's typed and the diagnostics toggle stay
    /// -- or was just sent, when the window starts a new one (`report::Form::take`). Sending, or
    /// saying why a report didn't go, it keeps to that.
    pub fn take(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        let Request { kind, item, crash } = request;
        match self.form.take(item, crash) {
            Takeover::Refused => return,
            Takeover::Joined => {}
            Takeover::Restarted => self.text.update(cx, |text, cx| text.set_text("", cx)),
        }
        self.pick(kind, cx);
        window.focus(&self.text.focus_handle(cx), cx);
        self.edited(cx);
    }

    /// Closes the window: ×, Esc, Cancel, Close and `WM_CLOSE` all come here, `why` naming which
    /// for the log -- but not while the report is on its way: the window stays for the answer
    /// (`report::Form::closable`). A crash it was opened for is forgotten -- the player has seen
    /// its report. Hidden at once, then let go of by GPUI once what the hiding reported has run
    /// (`app::close_window`).
    pub fn close(&mut self, why: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.form.closable() {
            log::info!("the report window stays while its report is on its way: {why}");
            return;
        }
        log::info!("report window closed: {why}");
        if self.form.crash.take().is_some() {
            report::forget_crash();
        }
        crate::app::close_window(window, cx);
    }

    fn pick(&mut self, kind: ReportKind, cx: &mut Context<Self>) {
        if kind == self.form.kind {
            return;
        }
        self.form.kind = kind;
        self.text
            .update(cx, |text, cx| text.set_placeholder(placeholder(kind), cx));
        self.edited(cx);
    }

    fn toggle_diagnostics(&mut self, cx: &mut Context<Self>) {
        self.form.toggle_diagnostics();
        cx.notify();
    }

    /// The report the fields make as they stand (`report::Draft`), with `diagnostics`.
    fn report(&self, diagnostics: Option<Vec<u8>>, cx: &App) -> Report {
        let state = self.app.read(cx);
        let context = report::app_context(
            i18n::lang(),
            state.item_language(),
            Some(WINDOWS_VERSION.clone()),
            state.league(),
            state.settings.ui_scale,
        );
        Draft {
            kind: self.form.kind,
            text: self.text.read(cx).text(),
            contact: self.contact.read(cx).text(),
            item: self.form.item.as_ref(),
            crash: self.form.crash.as_ref().map(|crash| crash.text.as_str()),
        }
        .report(context, diagnostics)
    }

    /// The fields changed: whether the report can go now, by the service's rules.
    fn edited(&mut self, cx: &mut Context<Self>) {
        self.ready = self.report(None, cx).check().is_ok();
        cx.notify();
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.form.stage, Stage::Compose) || !self.ready {
            return;
        }
        // Nothing typed from here on would go with it.
        self.focus_handle.focus(window, cx);
        let summary = self
            .form
            .attaches_diagnostics()
            .then(|| self.app.read(cx).diagnostics_summary());
        let outgoing = Arc::new(self.report(None, cx));
        self.submit(outgoing, summary, cx);
    }

    /// Sends `outgoing` -- with the diagnostics collected first when `summary`, the app's side of
    /// them, is given -- and shows the service's answer.
    fn submit(
        &mut self,
        mut outgoing: Arc<Report>,
        summary: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.form.stage = Stage::Sending;
        self.save_error = None;
        cx.notify();
        cx.spawn(async move |view, cx| {
            // Off the main thread: the logs are read and masked, and megabytes of zip go into the
            // JSON as base64.
            let (outgoing, body) = cx
                .background_executor()
                .spawn(async move {
                    if let Some(summary) = summary {
                        match diagnostics::collect(&summary) {
                            Ok(zip) => Arc::make_mut(&mut outgoing).diagnostics = Some(zip),
                            Err(err) => log::warn!(
                                "collecting the diagnostics failed, sending without them: {err:#}"
                            ),
                        }
                    }
                    let body = serde_json::to_vec(&*outgoing).expect("a report serializes");
                    (outgoing, body)
                })
                .await;
            let answer = report::send(body).await;
            view.update(cx, |view, cx| view.answered(outgoing, answer, cx))
                .ok();
        })
        .detach();
    }

    fn answered(
        &mut self,
        sent: Arc<Report>,
        answer: Result<u64, Failure>,
        cx: &mut Context<Self>,
    ) {
        if self.form.answered(sent, answer).is_some() {
            report::forget_crash();
        }
        cx.notify();
    }

    /// «Повторить»: the same report again, diagnostics and all.
    fn retry(&mut self, cx: &mut Context<Self>) {
        if let Stage::Failed(unsent, _) = &self.form.stage {
            let unsent = unsent.clone();
            self.submit(unsent, None, cx);
        }
    }

    /// «Сохранить на рабочий стол»: the report the service didn't take, as one zip
    /// (`report::unsent_zip`), shown in Explorer.
    fn save_unsent(&mut self, cx: &mut Context<Self>) {
        let Stage::Failed(unsent, _) = &self.form.stage else {
            return;
        };
        let unsent = unsent.clone();
        self.save(cx, move || {
            diagnostics::save_to_desktop("PoE2-Oracle-message", &report::unsent_zip(&unsent)?)
        });
    }

    /// «Что внутри»: the diagnostics as they'd go, saved to the desktop and shown in Explorer.
    fn show_diagnostics(&mut self, cx: &mut Context<Self>) {
        let summary = self.app.read(cx).diagnostics_summary();
        self.save(cx, move || diagnostics::write_report(&summary));
    }

    /// Runs `write` -- a save to the desktop -- off the main thread, then shows what it wrote in
    /// Explorer, or says why it couldn't.
    fn save(
        &mut self,
        cx: &mut Context<Self>,
        write: impl FnOnce() -> anyhow::Result<PathBuf> + Send + 'static,
    ) {
        if self.saving {
            return;
        }
        self.saving = true;
        self.save_error = None;
        cx.notify();
        cx.spawn(async move |view, cx| {
            let saved = cx.background_executor().spawn(async move { write() }).await;
            let save_error = match saved {
                Ok(path) => {
                    // The name only: the folder is the player's desktop.
                    let name = path.file_name().unwrap_or_default().display();
                    log::info!("saved to the desktop: {name}");
                    diagnostics::reveal(&path);
                    None
                }
                Err(err) => {
                    log::warn!("saving to the desktop failed: {err:#}");
                    Some(format!("{err:#}"))
                }
            };
            view.update(cx, |view, cx| {
                view.saving = false;
                view.save_error = save_error;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Esc takes the keyboard from the text box first, then closes the window. The contact field
    /// takes its Esc itself.
    fn escape(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "escape" {
            return;
        }
        if self.text.focus_handle(cx).contains_focused(window, cx) {
            self.focus_handle.focus(window, cx);
        } else {
            self.close("Esc", window, cx);
        }
    }

    fn render_title_bar(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(rems_from_px(TITLE_HEIGHT))
            .bg(title_gradient())
            .border_b_1()
            .border_color(rgb(BORDER_GOLD))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .gap(rems_from_px(10.))
                    .pl(rems_from_px(20.))
                    .window_control_area(WindowControlArea::Drag)
                    .child(diamond(8., GOLD))
                    .child(
                        heading(face)
                            .text_size(rems_from_px(15.))
                            .text_color(rgb(GOLD_LIGHT))
                            .child("PoE2 Oracle"),
                    )
                    .child(div().text_color(rgb(TEXT_MUTED)).child("·"))
                    .child(
                        heading(face)
                            .text_size(rems_from_px(15.))
                            .text_color(rgb(TEXT))
                            .child(tr!("Report")),
                    ),
            )
            .child(title_button(
                "close",
                "×",
                46.,
                cx.listener(|view, _: &MouseDownEvent, window, cx| {
                    view.close("title bar ×", window, cx)
                }),
            ))
    }

    /// The form: what the report is and says, while it's written and while it's on its way.
    fn render_form(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        let sending = matches!(self.form.stage, Stage::Sending);
        let kinds = self.form.kinds();
        let labels: Vec<SharedString> = kinds.iter().map(|&kind| kind_label(kind).into()).collect();
        let picked = kinds
            .iter()
            .position(|&kind| kind == self.form.kind)
            .unwrap_or(0);
        let count = self.text.read(cx).char_count();
        let attach = self.form.attaches_diagnostics();

        let text = div()
            .flex()
            .flex_col()
            .gap(rems_from_px(4.))
            .child(self.text.clone())
            .child(
                div()
                    .flex()
                    .justify_end()
                    .text_size(rems_from_px(12.))
                    .text_color(rgb(if count >= MAX_TEXT_CHARS {
                        TEXT_WARNING
                    } else {
                        TEXT_MUTED
                    }))
                    .child(format!("{count} / {MAX_TEXT_CHARS}")),
            );
        let contact = div()
            .flex()
            .flex_col()
            .gap(rems_from_px(6.))
            .child(tr!("Contact (optional)"))
            .child(div().flex().child(self.contact.clone()))
            .child(note(
                tr!("Telegram, Discord or email — if you'd like an answer"),
                TEXT_DIM,
            ));
        let toggle = div()
            .id("diagnostics")
            .flex()
            .items_center()
            .gap(rems_from_px(10.))
            .px(rems_from_px(6.))
            .py(rems_from_px(4.))
            .rounded(rems_from_px(6.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _: &MouseDownEvent, _, cx| view.toggle_diagnostics(cx)),
            )
            .child(switch("switch", attach))
            .child(tr!("Attach diagnostics"));
        let diagnostics = div()
            .flex()
            .flex_col()
            .gap(rems_from_px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(rems_from_px(12.))
                    .child(ease_hover("diagnostics", toggle, |row, hover| {
                        row.bg(alpha(GOLD, 0.05 * hover))
                    }))
                    .child(link(
                        "inside",
                        tr!("What's inside"),
                        cx.listener(|view, _: &MouseDownEvent, _, cx| view.show_diagnostics(cx)),
                    )),
            )
            .child(note(
                tr!(
                    "Logs, settings, unread item texts and a system summary. Your Windows name \
                     and folders are hidden."
                ),
                TEXT_DIM,
            ))
            .children(self.save_error.as_deref().map(save_failed));
        let inputs = div()
            .flex()
            .flex_col()
            .gap(rems_from_px(GAP))
            // In a row of its own: a column would stretch its frame to the whole width.
            .child(div().flex().child(segmented(
                "kind",
                labels,
                picked,
                cx.listener(move |view, index: &usize, _, cx| {
                    if let Some(&kind) = kinds.get(*index) {
                        view.pick(kind, cx);
                    }
                }),
            )))
            .child(text)
            .children(self.attached().map(|line| note(line, TEXT)))
            .child(contact)
            .child(diagnostics);

        let send = if sending {
            inert(button(
                "send",
                tr!("Sending…"),
                ButtonKind::Primary,
                face,
                |_, _, _| {},
            ))
            .into_any_element()
        } else if self.ready {
            button(
                "send",
                tr!("Send"),
                ButtonKind::Primary,
                face,
                cx.listener(|view, _: &MouseDownEvent, window, cx| view.send(window, cx)),
            )
            .into_any_element()
        } else {
            inert(button(
                "send",
                tr!("Send"),
                ButtonKind::Primary,
                face,
                |_, _, _| {},
            ))
            .into_any_element()
        };
        // Out of reach while the report is on its way, as closing is (`ReportView::close`).
        let cancel = button(
            "cancel",
            tr!("Cancel"),
            ButtonKind::Secondary,
            face,
            cx.listener(|view, _: &MouseDownEvent, window, cx| view.close("Cancel", window, cx)),
        );
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(rems_from_px(GAP))
            .p(rems_from_px(INSET))
            .child(
                div()
                    .id("form")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(if sending {
                        inert(inputs).into_any_element()
                    } else {
                        inputs.into_any_element()
                    }),
            )
            .child(note(
                tr!(
                    "Sent to oracle.pushka.biz: your text, the contact if given, the app's version \
                     and languages, and what's attached. No account needed."
                ),
                TEXT_DIM,
            ))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(rems_from_px(8.))
                    .child(if sending {
                        inert(cancel).into_any_element()
                    } else {
                        cancel.into_any_element()
                    })
                    .child(send),
            )
    }

    /// What the report attaches besides the diagnostics, in a line: the item, or the crash.
    fn attached(&self) -> Option<String> {
        match self.form.kind {
            ReportKind::Item => self
                .form
                .item
                .as_ref()
                .map(|item| tr!("Item: {name} — its text is attached", name = item.name)),
            ReportKind::Crash => self.form.crash.as_ref().map(|crash| {
                let at = &crash.at;
                let date = format!(
                    "{} {:02}:{:02}",
                    i18n::day_month(at.wDay, at.wMonth),
                    at.wHour,
                    at.wMinute
                );
                tr!(
                    "PoE2 Oracle closed unexpectedly on {date}; what it reported is attached",
                    date = date
                )
            }),
            ReportKind::Bug | ReportKind::Idea => None,
        }
    }

    fn render_sent(
        &self,
        id: u64,
        face: &'static NameFont,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let said: SharedString = if id == 0 {
            tr!("Sent — thank you!").into()
        } else {
            tr!("Sent — thank you! Report #{id}", id = id).into()
        };
        outcome(
            face,
            said,
            GOLD_LIGHT,
            button(
                "done",
                tr!("Close"),
                ButtonKind::Primary,
                face,
                cx.listener(|view, _: &MouseDownEvent, window, cx| {
                    view.close("Close after sending", window, cx)
                }),
            ),
            None,
        )
    }

    fn render_failed(
        &self,
        failure: Failure,
        face: &'static NameFont,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let save = button(
            "save",
            tr!("Save to desktop"),
            ButtonKind::Secondary,
            face,
            cx.listener(|view, _: &MouseDownEvent, _, cx| view.save_unsent(cx)),
        );
        let actions = div()
            .flex()
            .flex_wrap()
            .justify_center()
            .gap(rems_from_px(8.))
            .child(button(
                "retry",
                tr!("Try again"),
                ButtonKind::Primary,
                face,
                cx.listener(|view, _: &MouseDownEvent, _, cx| view.retry(cx)),
            ))
            .child(if self.saving {
                inert(save).into_any_element()
            } else {
                save.into_any_element()
            })
            .child(button(
                "done",
                tr!("Close"),
                ButtonKind::Secondary,
                face,
                cx.listener(|view, _: &MouseDownEvent, window, cx| {
                    view.close("Close after a failure", window, cx)
                }),
            ));
        outcome(
            face,
            tr!("Couldn't send: {reason}", reason = failure.reason()).into(),
            TEXT_WARNING,
            actions,
            self.save_error
                .as_deref()
                .map(|error| save_failed(error).into_any_element()),
        )
    }
}

impl Focusable for ReportView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ReportView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let face = fonts::interface_font();
        window.set_rem_size(px(BASE_REM_SIZE * self.app.read(cx).settings.ui_scale));
        // Each stage rises in anew.
        let (stage, body) = match &self.form.stage {
            Stage::Compose | Stage::Sending => (0, self.render_form(face, cx).into_any_element()),
            Stage::Sent(id) => (1, self.render_sent(*id, face, cx).into_any_element()),
            Stage::Failed(_, failure) => {
                (2, self.render_failed(*failure, face, cx).into_any_element())
            }
        };
        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                view.escape(event, window, cx);
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG_PANEL))
            .text_color(rgb(TEXT))
            .text_size(rems_from_px(14.))
            .line_height(relative(1.4))
            .child(self.render_title_bar(face, cx))
            .child(appear(
                ("stage", stage as u64),
                div().flex().flex_col().flex_1().min_h_0().child(body),
            ))
            .child(game_frame())
    }
}

/// The window's title, as the taskbar and Alt+Tab show it.
pub fn window_title() -> &'static str {
    tr!("PoE2 Oracle — report")
}

fn kind_label(kind: ReportKind) -> &'static str {
    match kind {
        ReportKind::Bug => tr!("Problem"),
        ReportKind::Idea => tr!("Idea"),
        ReportKind::Item => tr!("Item"),
        ReportKind::Crash => tr!("Crash"),
    }
}

/// What the empty text box asks, by the kind.
fn placeholder(kind: ReportKind) -> fn() -> &'static str {
    match kind {
        ReportKind::Bug => || tr!("What happened, and how can it be repeated?"),
        ReportKind::Idea => || tr!("What would you like the app to do?"),
        ReportKind::Item => || tr!("What's wrong with this item's price or reading?"),
        ReportKind::Crash => || tr!("What were you doing when it closed? (optional)"),
    }
}

/// A contact left longer than the service takes is cut to fit, in the field for the player to
/// see.
fn cut_contact(field: &Entity<TextField>, cx: &mut App) {
    let contact = field.read(cx).text();
    if contact.chars().count() > MAX_CONTACT_CHARS {
        let cut: String = contact.chars().take(MAX_CONTACT_CHARS).collect();
        field.update(cx, |field, cx| field.set_text(cut, cx));
    }
}

/// A line under a control: what it does, or what just happened, in its colour.
fn note(text: impl Into<SharedString>, color: u32) -> impl IntoElement {
    div()
        .text_size(rems_from_px(12.))
        .text_color(rgb(color))
        .child(text.into())
}

/// Why a save to the desktop failed.
fn save_failed(error: &str) -> impl IntoElement {
    note(tr!("Couldn't save: {error}", error = error), TEXT_WARNING)
}

/// `element` shown but out of reach: dimmed, with nothing in it answering the pointer.
fn inert(element: impl IntoElement) -> impl IntoElement {
    div()
        .relative()
        .opacity(INERT_OPACITY)
        .child(element)
        .child(div().absolute().inset_0().occlude())
}

/// The service's answer in place of the form: an ornament, what it said in `color`, what can be
/// done next, and why a save failed.
fn outcome(
    face: &'static NameFont,
    said: SharedString,
    color: u32,
    actions: impl IntoElement,
    save_error: Option<AnyElement>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(rems_from_px(18.))
        .p(rems_from_px(INSET))
        .child(diamond(10., GOLD))
        .child(
            heading(face)
                .w_full()
                .text_center()
                .text_size(rems_from_px(17.))
                .text_color(rgb(color))
                .child(said),
        )
        .child(actions)
        .children(save_error)
}
