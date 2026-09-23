//! The settings window: a copy of [`Settings`] edited in place and handed to `on_save` by
//! "Сохранить" -- nothing applies before that. It reads as part of the price-check panel
//! (`ui::panel`): the same near-black panel, gold section headers, chips and buttons. Its last
//! section writes the diagnostics report (`crate::diagnostics`), which applies nothing either.
//!
//! GPUI has no stock text input or dropdown, so the controls are built here: chips that pick one
//! of a few choices, switches, -/+ steppers, and hotkey recorders that capture the next
//! combination pressed while they have focus -- plus `ui::text_field` for the quick actions' text.
//!
//! The view draws its own title bar (open the window with `TitlebarOptions::appears_transparent`)
//! and closes its window once it has reported: `on_save` after "Сохранить", `on_cancel` after
//! "Отмена", its × or Esc -- and after any other close (Alt+F4, the taskbar), noticed when the
//! view is released.

use std::path::PathBuf;

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, Keystroke,
    Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent, Render, SharedString, Window,
    WindowControlArea, div, prelude::*, px, relative, rgb,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_0, VK_9, VK_SHIFT};

use crate::diagnostics;
use crate::settings::{
    self, ClientLanguage, Hotkey, HotkeyProblem, KeyName, LeagueChoice, ListingStatusChoice,
    QuickAction, QuickActionKind, Settings,
};
use crate::ui::text_field::TextField;
use crate::ui::theme::{
    BG_BUTTON, BG_BUTTON_HOVER, BG_CLOSE_HOVER, BG_CONTROL, BG_PANEL, BG_TITLE, BORDER,
    BORDER_GOLD, CONTENT_PADDING, GOLD, TEXT, TEXT_DIM, TEXT_MUTED, TEXT_WARNING,
};

/// What one click of a stepper moves the tolerance and the scale by, in percent.
const STEP_PERCENT: u16 = 5;

/// Gets the edited settings when the player saves.
type SaveHandler = Box<dyn Fn(Settings, &mut App)>;
/// Told that the window closed without saving.
type CancelHandler = Box<dyn Fn(&mut App)>;
/// The app's side of the diagnostics report, as of the click.
type ReportSummary = Box<dyn Fn(&App) -> String>;

/// The diagnostics report's progress, shown under its buttons.
enum ReportState {
    Idle,
    Writing,
    Written(PathBuf),
    Failed(String),
}

/// What the window says above the settings: a welcome on the first launch, and whatever in the
/// player's setup keeps checks from working (`diagnostics::setup_problems`).
pub struct Intro {
    pub welcome: bool,
    pub problems: Vec<String>,
}

/// Which hotkey recorder a key-down is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Recorder {
    PriceCheck,
    /// The quick action at this index.
    Action(usize),
}

pub struct SettingsView {
    /// The copy being edited, handed to `on_save` as it stands.
    settings: Settings,
    /// The trade site's leagues, current first.
    leagues: Vec<String>,
    intro: Intro,
    on_save: SaveHandler,
    on_cancel: CancelHandler,
    /// The window's own focus: Esc cancels from here. A click anywhere lands here (GPUI focuses
    /// the innermost focusable element under the mouse), ending a capture; so does a finished one.
    focus_handle: FocusHandle,
    /// Focused while the price-check hotkey recorder captures.
    recorder_focus: FocusHandle,
    /// The quick actions' text fields and hotkey recorders, in `settings.quick_actions`' order;
    /// the fields' text goes into the settings on save.
    action_fields: Vec<Entity<TextField>>,
    action_recorders: Vec<FocusHandle>,
    /// The modifiers held during a capture, shown until the key comes.
    held: Modifiers,
    /// Why the combination last pressed into the recorder was refused.
    recorder_error: Option<&'static str>,
    /// `on_save` or `on_cancel` has run, so the release must not report again.
    reported: bool,
    report_summary: ReportSummary,
    report: ReportState,
}

impl SettingsView {
    pub fn new(
        settings: Settings,
        leagues: Vec<String>,
        intro: Intro,
        on_save: impl Fn(Settings, &mut App) + 'static,
        on_cancel: impl Fn(&mut App) + 'static,
        report_summary: impl Fn(&App) -> String + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        // A window closed without the buttons (Alt+F4, the taskbar) is still a cancel.
        cx.on_release(|view, cx| {
            if !view.reported {
                (view.on_cancel)(cx);
            }
        })
        .detach();
        let action_fields = settings
            .quick_actions
            .iter()
            .map(|action| action_field(action, cx))
            .collect();
        let action_recorders = settings
            .quick_actions
            .iter()
            .map(|_| cx.focus_handle())
            .collect();
        SettingsView {
            settings,
            leagues,
            intro,
            on_save: Box::new(on_save),
            on_cancel: Box::new(on_cancel),
            focus_handle: cx.focus_handle(),
            recorder_focus: cx.focus_handle(),
            action_fields,
            action_recorders,
            held: Modifiers::default(),
            recorder_error: None,
            reported: false,
            report_summary: Box::new(report_summary),
            report: ReportState::Idle,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reported = true;
        for (action, field) in self
            .settings
            .quick_actions
            .iter_mut()
            .zip(&self.action_fields)
        {
            action.text = field.read(cx).text().trim().to_owned();
        }
        // An action left without text would do nothing: it isn't kept.
        self.settings
            .quick_actions
            .retain(|action| !action.text.is_empty());
        (self.on_save)(self.settings.clone(), cx);
        window.remove_window();
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reported = true;
        (self.on_cancel)(cx);
        window.remove_window();
    }

    /// Writes the diagnostics report off the main thread, then shows it in Explorer.
    fn write_report(&mut self, cx: &mut Context<Self>) {
        if matches!(self.report, ReportState::Writing) {
            return;
        }
        let summary = (self.report_summary)(cx);
        self.report = ReportState::Writing;
        cx.notify();
        cx.spawn(async move |view, cx| {
            let written = cx
                .background_executor()
                .spawn(async move { diagnostics::write_report(&summary) })
                .await;
            let report = match written {
                Ok(path) => {
                    log::info!("diagnostics report written to {}", path.display());
                    diagnostics::reveal(&path);
                    ReportState::Written(path)
                }
                Err(err) => {
                    log::warn!("writing the diagnostics report failed: {err:#}");
                    ReportState::Failed(format!("{err:#}"))
                }
            };
            view.update(cx, |view, cx| {
                view.report = report;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The recorder was clicked; the same click focuses it (GPUI moves focus to a focusable
    /// element on mouse-down), which is what makes it capture.
    fn start_capture(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.held = window.modifiers();
        self.recorder_error = None;
        cx.notify();
    }

    fn stop_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn capture_key(
        &mut self,
        recorder: Recorder,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Every key is the recorder's while it captures: Esc must not also cancel the window, nor
        // Alt+key reach the system menu.
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        let keystroke = &event.keystroke;
        if !keystroke.modifiers.modified() {
            match (keystroke.key.as_str(), recorder) {
                ("escape", _) => {
                    self.stop_capture(window, cx);
                    return;
                }
                // A quick action may go without a hotkey; the price check may not.
                ("backspace" | "delete", Recorder::Action(index)) => {
                    if let Some(action) = self.settings.quick_actions.get_mut(index) {
                        action.hotkey = None;
                    }
                    self.stop_capture(window, cx);
                    return;
                }
                _ => {}
            }
        }
        match recorded_hotkey(keystroke).and_then(|hotkey| self.unused(hotkey, recorder)) {
            Ok(hotkey) => {
                match recorder {
                    Recorder::PriceCheck => self.settings.hotkey = hotkey,
                    Recorder::Action(index) => {
                        if let Some(action) = self.settings.quick_actions.get_mut(index) {
                            action.hotkey = Some(hotkey);
                        }
                    }
                }
                self.stop_capture(window, cx);
            }
            Err(error) => {
                self.recorder_error = Some(error);
                cx.notify();
            }
        }
    }

    /// `hotkey`, unless a recorder other than `recorder` already has it: one combination does
    /// one thing.
    fn unused(&self, hotkey: Hotkey, recorder: Recorder) -> Result<Hotkey, &'static str> {
        if recorder != Recorder::PriceCheck && self.settings.hotkey == hotkey {
            return Err("Это сочетание уже у проверки цены");
        }
        let taken = self
            .settings
            .quick_actions
            .iter()
            .enumerate()
            .any(|(index, action)| {
                recorder != Recorder::Action(index) && action.hotkey == Some(hotkey)
            });
        if taken {
            return Err("Это сочетание уже у другого быстрого действия");
        }
        Ok(hotkey)
    }

    fn add_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.quick_actions.len() >= settings::MAX_QUICK_ACTIONS {
            return;
        }
        let action = QuickAction {
            kind: QuickActionKind::ChatCommand,
            text: String::new(),
            hotkey: None,
        };
        let field = action_field(&action, cx);
        // Ready for typing the command at once.
        window.focus(&field.focus_handle(cx), cx);
        self.settings.quick_actions.push(action);
        self.action_fields.push(field);
        self.action_recorders.push(cx.focus_handle());
        cx.notify();
    }

    fn remove_action(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.settings.quick_actions.len() {
            self.settings.quick_actions.remove(index);
            self.action_fields.remove(index);
            self.action_recorders.remove(index);
        }
        cx.notify();
    }

    /// Switches the action at `index` between a chat command and a stash search; its field's
    /// placeholder follows.
    fn set_action_kind(&mut self, index: usize, kind: QuickActionKind, cx: &mut Context<Self>) {
        if let Some(action) = self.settings.quick_actions.get_mut(index) {
            action.kind = kind;
        }
        if let Some(field) = self.action_fields.get(index) {
            field.update(cx, |field, cx| {
                field.set_placeholder(action_placeholder(kind));
                cx.notify();
            });
        }
        cx.notify();
    }

    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(30.))
            .bg(rgb(BG_TITLE))
            .border_b_1()
            .border_color(rgb(BORDER_GOLD))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .pl(px(CONTENT_PADDING))
                    .window_control_area(WindowControlArea::Drag)
                    .text_xs()
                    .text_color(rgb(TEXT_DIM))
                    .child("PoE2 Oracle · Настройки"),
            )
            .child(
                div()
                    .w(px(38.))
                    .h_full()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(TEXT_DIM))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(BG_CLOSE_HOVER)).text_color(rgb(TEXT)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _event: &MouseDownEvent, window, cx| {
                            view.cancel(window, cx);
                        }),
                    )
                    .child("×"),
            )
    }

    /// The welcome (first launch only) and the setup problems, in a gold-edged box above the
    /// settings; nothing when there is neither.
    fn render_intro(&self) -> Option<impl IntoElement> {
        let Intro { welcome, problems } = &self.intro;
        if !welcome && problems.is_empty() {
            return None;
        }
        let hotkey = self.settings.hotkey;
        let welcome_lines = [
            "PoE2 Oracle работает в фоне: его значок — у часов, иногда под стрелкой «Показать \
             скрытые значки». Эти настройки открываются из меню значка и шестерёнкой на панели."
                .to_owned(),
            format!(
                "В игре наведите курсор на предмет и нажмите {hotkey} — рядом с инвентарём \
                 откроется панель с ценой. Esc закрывает её."
            ),
        ];
        Some(
            div()
                .mt(px(12.))
                .p(px(10.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .rounded_xs()
                .border_1()
                .border_color(rgb(BORDER_GOLD))
                .bg(rgb(BG_CONTROL))
                .when(*welcome, |this| {
                    this.child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(GOLD))
                            .child("Добро пожаловать!"),
                    )
                    .children(
                        welcome_lines
                            .map(|line| div().text_xs().text_color(rgb(TEXT_DIM)).child(line)),
                    )
                })
                .children(problems.iter().map(|problem| {
                    div()
                        .text_xs()
                        .text_color(rgb(TEXT_WARNING))
                        .child(format!("⚠ {problem}"))
                })),
        )
    }

    /// "Авто" (naming the league it stands for), every league the trade site lists, and a picked
    /// league it no longer lists -- still shown, since it's what the file holds.
    fn render_league(&self, cx: &Context<Self>) -> impl IntoElement {
        let auto_label: SharedString = match self.leagues.first() {
            Some(current) => format!("Авто · {current}").into(),
            None => "Авто".into(),
        };
        let unlisted = match &self.settings.league {
            LeagueChoice::Named(league) if !self.leagues.contains(league) => Some(league),
            _ => None,
        };
        let options = std::iter::once((LeagueChoice::Auto, auto_label))
            .chain(
                self.leagues
                    .iter()
                    .chain(unlisted)
                    .map(|league| (LeagueChoice::Named(league.clone()), league.clone().into())),
            )
            .collect();
        let (note, note_color) = if self.leagues.is_empty() {
            ("Список лиг с сайта торговли не загрузился", TEXT_MUTED)
        } else if unlisted.is_some() {
            (
                "Выбранной лиги больше нет на сайте торговли — поиск пойдёт в текущей",
                TEXT_WARNING,
            )
        } else {
            (
                "Авто — текущая лига, первая в списке сайта торговли",
                TEXT_MUTED,
            )
        };
        section("Лига")
            .child(choice_chips(
                options,
                &self.settings.league,
                |settings, league| settings.league = league,
                cx,
            ))
            .child(note_line(note, note_color))
    }

    fn render_language(&self, cx: &Context<Self>) -> impl IntoElement {
        let options = vec![
            (ClientLanguage::Auto, "Авто".into()),
            (ClientLanguage::Russian, "Русский".into()),
            (ClientLanguage::English, "English".into()),
        ];
        section("Язык клиента")
            .child(choice_chips(
                options,
                &self.settings.client_language,
                |settings, language| settings.client_language = language,
                cx,
            ))
            .child(note_line(
                "Авто — по тексту скопированного предмета",
                TEXT_MUTED,
            ))
    }

    /// A hotkey recorder: shows `current` (or `empty` without one) until clicked, then captures
    /// the next combination pressed, showing the modifiers held meanwhile.
    fn recorder(
        &self,
        recorder: Recorder,
        current: Option<Hotkey>,
        empty: &'static str,
        width: f32,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let focus = match recorder {
            Recorder::PriceCheck => &self.recorder_focus,
            Recorder::Action(index) => &self.action_recorders[index],
        };
        let capturing = focus.is_focused(window);
        let shown: SharedString = if capturing {
            let held: String =
                settings::modifier_names(self.held.control, self.held.shift, self.held.alt)
                    .map(|name| format!("{name}+"))
                    .collect();
            if held.is_empty() {
                "Нажмите сочетание…".into()
            } else {
                format!("{held}…").into()
            }
        } else {
            current.map_or(empty.into(), |hotkey| hotkey.to_string().into())
        };
        div()
            .track_focus(focus)
            .w(px(width))
            .h(px(26.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_xs()
            .bg(rgb(BG_CONTROL))
            .border_1()
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .map(|this| {
                if capturing {
                    this.border_color(rgb(GOLD)).text_color(rgb(GOLD))
                } else {
                    this.border_color(rgb(BORDER))
                        .text_color(rgb(if current.is_some() { TEXT } else { TEXT_MUTED }))
                        .hover(|style| style.border_color(rgb(TEXT_DIM)))
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _event: &MouseDownEvent, window, cx| {
                    view.start_capture(window, cx);
                }),
            )
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                view.capture_key(recorder, event, window, cx);
            }))
            .on_modifiers_changed(cx.listener(
                |view, event: &ModifiersChangedEvent, _window, cx| {
                    view.held = event.modifiers;
                    cx.notify();
                },
            ))
            .child(shown)
    }

    fn render_hotkey(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let capturing = self.recorder_focus.is_focused(window);
        let (note, note_color) = match self.recorder_error {
            Some(error) if capturing => (error, TEXT_WARNING),
            _ if capturing => (
                "Ctrl или Alt с буквой или цифрой, либо F1–F12 · Esc — оставить как было",
                TEXT_MUTED,
            ),
            _ => ("Щёлкните по полю и нажмите новое сочетание", TEXT_MUTED),
        };
        section("Горячая клавиша")
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .py(px(4.))
                    .child(labelled("Проверка цены", None))
                    .child(self.recorder(
                        Recorder::PriceCheck,
                        Some(self.settings.hotkey),
                        "",
                        160.,
                        window,
                        cx,
                    )),
            )
            .child(note_line(note, note_color))
    }

    /// Hotkeys that type into the game: a chat command, or a stash search.
    fn render_quick_actions(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let capturing = self
            .action_recorders
            .iter()
            .any(|focus| focus.is_focused(window));
        let note = match self.recorder_error {
            Some(error) if capturing => Some((error, TEXT_WARNING)),
            _ if capturing => Some((
                "F1–F12, либо Ctrl или Alt с буквой или цифрой · Backspace — без клавиши · Esc — \
                 оставить как было",
                TEXT_MUTED,
            )),
            _ => None,
        };
        let can_add = self.settings.quick_actions.len() < settings::MAX_QUICK_ACTIONS;
        section("Быстрые действия")
            .child(note_line(
                "Клавиши, которые печатают в игру. Чат: /hideout, /exit, @last спасибо — ответ \
                 тому, кто писал последним. Тайник: строка поиска в открытом тайнике или у \
                 торговца, например с poe2.re.",
                TEXT_MUTED,
            ))
            .children(
                self.settings
                    .quick_actions
                    .iter()
                    .enumerate()
                    .map(|(index, action)| self.render_action(index, action, window, cx)),
            )
            .children(note.map(|(text, color)| note_line(text, color)))
            .when(can_add, |this| {
                this.child(
                    div()
                        .flex()
                        .child(button("+ Добавить действие", false).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _event: &MouseDownEvent, window, cx| {
                                view.add_action(window, cx);
                            }),
                        )),
                )
            })
    }

    /// One quick action: its kind, hotkey and a × on top, the text below.
    fn render_action(
        &self,
        index: usize,
        action: &QuickAction,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let kinds = [
            (QuickActionKind::ChatCommand, "Чат"),
            (QuickActionKind::StashSearch, "Тайник"),
        ];
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(8.))
            .rounded_xs()
            .border_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .children(kinds.into_iter().map(|(kind, label)| {
                        let selected = action.kind == kind;
                        chip(label.into(), selected).when(!selected, |this| {
                            this.on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                                    view.set_action_kind(index, kind, cx);
                                }),
                            )
                        })
                    }))
                    .child(div().flex_1())
                    .child(self.recorder(
                        Recorder::Action(index),
                        action.hotkey,
                        "без клавиши",
                        110.,
                        window,
                        cx,
                    ))
                    .child(
                        div()
                            .w(px(22.))
                            .h(px(22.))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .rounded_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(BG_CLOSE_HOVER)).text_color(rgb(TEXT)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                                    view.remove_action(index, cx);
                                }),
                            )
                            .child("×"),
                    ),
            )
            .child(div().flex().child(self.action_fields[index].clone()))
    }

    fn render_search(&self, cx: &Context<Self>) -> impl IntoElement {
        let tolerance = self.settings.search_tolerance_percent;
        let statuses = vec![
            (ListingStatusChoice::Available, "выкуп и онлайн".into()),
            (
                ListingStatusChoice::Securable,
                "только мгновенный выкуп".into(),
            ),
            (ListingStatusChoice::Online, "только онлайн".into()),
            (ListingStatusChoice::Any, "все, включая офлайн".into()),
        ];
        section("Поиск")
            .child(stepper_row(
                labelled(
                    "Допуск значений",
                    Some("Насколько значения свойств в поиске могут отличаться от ваших"),
                ),
                format!("±{tolerance}%"),
                (
                    tolerance > 0,
                    tolerance < settings::MAX_SEARCH_TOLERANCE_PERCENT,
                ),
                step_tolerance,
                cx,
            ))
            .child(div().pt(px(4.)).child("Продавцы по умолчанию"))
            .child(choice_chips(
                statuses,
                &self.settings.listing_status,
                |settings, status| settings.listing_status = status,
                cx,
            ))
            .child(toggle_row(
                labelled(
                    "Колонка продавца",
                    Some("Имя продавца в таблице результатов"),
                ),
                self.settings.show_seller_column,
                |settings| &mut settings.show_seller_column,
                cx,
            ))
    }

    fn render_appearance(&self, cx: &Context<Self>) -> impl IntoElement {
        let percent = scale_percent(self.settings.ui_scale);
        section("Внешний вид").child(stepper_row(
            labelled(
                "Масштаб интерфейса",
                Some("Размер текста и элементов оверлея"),
            ),
            format!("{percent}%"),
            (
                percent > scale_percent(settings::MIN_UI_SCALE),
                percent < scale_percent(settings::MAX_UI_SCALE),
            ),
            step_scale,
            cx,
        ))
    }

    fn render_xp_overlay(&self, cx: &Context<Self>) -> impl IntoElement {
        let windows = settings::XP_RATE_WINDOWS
            .into_iter()
            .map(|minutes| (minutes, format!("{minutes} мин").into()))
            .collect();
        section("Оверлей опыта")
            .child(toggle_row(
                labelled(
                    "Показывать оверлей опыта",
                    Some("Скорость набора опыта и время до следующего уровня"),
                ),
                self.settings.xp_overlay,
                |settings| &mut settings.xp_overlay,
                cx,
            ))
            .child(toggle_row(
                labelled("Процент уровня", Some("Сколько уровня уже набрано")),
                self.settings.xp_show_percent,
                |settings| &mut settings.xp_show_percent,
                cx,
            ))
            .child(toggle_row(
                labelled(
                    "Таймер карты",
                    Some("Время в текущей карте, опыт за неё и среднее время карты за сессию"),
                ),
                self.settings.xp_map_timer,
                |settings| &mut settings.xp_map_timer,
                cx,
            ))
            .child(div().pt(px(4.)).child("Сглаживание скорости"))
            .child(choice_chips(
                windows,
                &self.settings.xp_rate_window_minutes,
                |settings, minutes| settings.xp_rate_window_minutes = minutes,
                cx,
            ))
            .child(note_line(
                "Скорость считается в основном за последние минуты: короче — быстрее видна смена \
                 фарма, длиннее — ровнее",
                TEXT_MUTED,
            ))
    }

    fn render_trade(&self, cx: &Context<Self>) -> impl IntoElement {
        section("Торговля")
            .child(toggle_row(
                labelled(
                    "Запросы покупателей",
                    Some(
                        "Кто хочет купить ваш предмет, за сколько и где он лежит — с кнопками \
                         ответа в игру",
                    ),
                ),
                self.settings.trade_requests,
                |settings| &mut settings.trade_requests,
                cx,
            ))
            .child(toggle_row(
                labelled("Звук при новом запросе", None),
                self.settings.trade_sound,
                |settings| &mut settings.trade_sound,
                cx,
            ))
    }

    fn render_system(&self, cx: &Context<Self>) -> impl IntoElement {
        section("Система")
            .child(toggle_row(
                labelled("Запускать вместе с Windows", None),
                self.settings.autostart,
                |settings| &mut settings.autostart,
                cx,
            ))
            .child(toggle_row(
                labelled("Проверять обновления", None),
                self.settings.check_updates,
                |settings| &mut settings.check_updates,
                cx,
            ))
    }

    /// The report a bug needs, written on request; it applies nothing, so it needs no saving.
    fn render_diagnostics(&self, cx: &Context<Self>) -> impl IntoElement {
        let status: Option<(SharedString, u32)> = match &self.report {
            ReportState::Idle => None,
            ReportState::Writing => Some(("Собираю отчёт…".into(), TEXT_MUTED)),
            ReportState::Written(path) => Some((
                format!(
                    "Сохранён: {} — приложите его к сообщению об ошибке",
                    path.display()
                )
                .into(),
                TEXT_DIM,
            )),
            ReportState::Failed(error) => Some((
                format!("Не удалось собрать отчёт: {error}").into(),
                TEXT_WARNING,
            )),
        };
        section("Диагностика")
            .child(labelled(
                "Отчёт для разработчика",
                Some(
                    "Логи, настройки и нераспознанные предметы в одном архиве на рабочем столе; \
                     имя пользователя Windows в нём скрыто",
                ),
            ))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(button("Собрать отчёт", false).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                            view.write_report(cx);
                        }),
                    ))
                    .child(button("Папка логов", false).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_view, _event: &MouseDownEvent, _window, _cx| {
                            diagnostics::open_logs_folder();
                        }),
                    )),
            )
            .children(
                status.map(|(text, color)| div().text_xs().text_color(rgb(color)).child(text)),
            )
    }

    fn render_footer(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .justify_end()
            .gap(px(8.))
            .px(px(CONTENT_PADDING))
            .py(px(10.))
            .bg(rgb(BG_TITLE))
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(button("Отмена", false).on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _event: &MouseDownEvent, window, cx| {
                    view.cancel(window, cx);
                }),
            ))
            .child(button("Сохранить", true).on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _event: &MouseDownEvent, window, cx| {
                    view.save(window, cx);
                }),
            ))
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    view.cancel(window, cx);
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG_PANEL))
            .text_color(rgb(TEXT))
            .text_sm()
            .line_height(relative(1.35))
            .child(self.render_title_bar(cx))
            .child(
                div()
                    .id("settings-scroll")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(CONTENT_PADDING))
                    .pb(px(CONTENT_PADDING))
                    .children(self.render_intro())
                    .child(self.render_league(cx))
                    .child(self.render_language(cx))
                    .child(self.render_hotkey(window, cx))
                    .child(self.render_quick_actions(window, cx))
                    .child(self.render_search(cx))
                    .child(self.render_appearance(cx))
                    .child(self.render_xp_overlay(cx))
                    .child(self.render_trade(cx))
                    .child(self.render_system(cx))
                    .child(self.render_diagnostics(cx)),
            )
            .child(self.render_footer(cx))
    }
}

/// The hotkey a key-down in the recorder makes, or why it can't be one. Letters arrive as `a`-`z`
/// whatever the keyboard layout: gpui_windows names keys through
/// `MapVirtualKeyW(MAPVK_VK_TO_CHAR)`, which returns 'A'-'Z' for those keys "regardless of current
/// keyboard layout" (Microsoft's `MapVirtualKeyW` reference) -- so Ctrl+E pressed with the Russian
/// layout on is still the physical E key, the one `RegisterHotKey` will get.
fn recorded_hotkey(keystroke: &Keystroke) -> Result<Hotkey, &'static str> {
    let modifiers = keystroke.modifiers;
    if modifiers.platform {
        return Err("Сочетания с клавишей Win не поддерживаются");
    }
    let (key, shift) = match KeyName::from_label(&keystroke.key) {
        Some(key) => (key, modifiers.shift),
        None => (
            shifted_digit().ok_or("Подходят только буквы A–Z, цифры 0–9 и F1–F12")?,
            true,
        ),
    };
    let hotkey = Hotkey {
        ctrl: modifiers.control,
        shift,
        alt: modifiers.alt,
        key,
    };
    hotkey
        .check()
        .map(|()| hotkey)
        .map_err(|problem| match problem {
            HotkeyProblem::NeedsModifier => {
                "Буквам и цифрам нужен Ctrl или Alt — иначе клавиша перестанет печататься"
            }
            HotkeyProblem::CopyCombo => "Ctrl+C занято: этим сочетанием программа копирует предмет",
            HotkeyProblem::CloseCombo => "Alt+F4 закрывает окна, в том числе игру",
        })
}

/// The digit key held with Shift during the key-down being handled. gpui_windows reports
/// Shift+digit as the character it types, with Shift cleared -- "!" for Shift+1 on the US and
/// Russian layouts (`keyboard.rs`, `get_keystroke_key`) -- so the digit is read back from this
/// thread's key state, which `GetKeyState` keeps in step with the message being handled.
fn shifted_digit() -> Option<KeyName> {
    // SAFETY: `GetKeyState` only reads the calling thread's keyboard state; negative means down.
    let down = |vk: u16| unsafe { GetKeyState(i32::from(vk)) } < 0;
    if !down(VK_SHIFT.0) {
        return None;
    }
    let mut digits = (VK_0.0..=VK_9.0).filter(|&vk| down(vk));
    let digit = digits.next()?;
    // Two digits down at once name no single key.
    if digits.next().is_some() {
        return None;
    }
    KeyName::from_virtual_key(digit)
}

fn step_tolerance(settings: &mut Settings, up: bool) {
    let stepped = step_percent(
        settings.search_tolerance_percent.into(),
        up,
        0,
        settings::MAX_SEARCH_TOLERANCE_PERCENT.into(),
    );
    // Within 0..=MAX_SEARCH_TOLERANCE_PERCENT, which is a u8.
    settings.search_tolerance_percent = stepped as u8;
}

fn step_scale(settings: &mut Settings, up: bool) {
    let stepped = step_percent(
        scale_percent(settings.ui_scale),
        up,
        scale_percent(settings::MIN_UI_SCALE),
        scale_percent(settings::MAX_UI_SCALE),
    );
    settings.ui_scale = f32::from(stepped) / 100.;
}

fn scale_percent(scale: f32) -> u16 {
    (scale * 100.).round() as u16
}

/// The next multiple of [`STEP_PERCENT`] up or down from `value`, kept within `min..=max`: an
/// off-step value (a hand-edited 12) lands on the step either side of it (15 or 10).
fn step_percent(value: u16, up: bool, min: u16, max: u16) -> u16 {
    let stepped = if up {
        (value / STEP_PERCENT + 1) * STEP_PERCENT
    } else {
        value.saturating_sub(1) / STEP_PERCENT * STEP_PERCENT
    };
    stepped.clamp(min, max)
}

/// A gold section header, as the panel's, with the section's rows below it.
fn section(title: &'static str) -> gpui::Div {
    div().flex().flex_col().gap(px(6.)).child(
        div()
            .pt(px(12.))
            .pb(px(3.))
            .border_b_1()
            .border_color(rgb(BORDER_GOLD))
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(GOLD))
            .child(title.to_uppercase()),
    )
}

/// A setting's name, and under it what it does.
fn labelled(label: &'static str, description: Option<&'static str>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .child(label)
        .children(description.map(|description| {
            div()
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(description)
        }))
}

fn note_line(text: &'static str, color: u32) -> impl IntoElement {
    div().text_xs().text_color(rgb(color)).child(text)
}

/// One chip per choice, the current one outlined in gold; clicking another picks it.
fn choice_chips<T: Clone + PartialEq + 'static>(
    options: Vec<(T, SharedString)>,
    current: &T,
    select: fn(&mut Settings, T),
    cx: &Context<SettingsView>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_wrap()
        .gap(px(6.))
        .children(options.into_iter().map(|(value, label)| {
            let selected = value == *current;
            chip(label, selected).when(!selected, |this| {
                this.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                        select(&mut view.settings, value.clone());
                        cx.notify();
                    }),
                )
            })
        }))
}

/// A choice: the selected one outlined in gold, the others plain, lighting up under the mouse.
fn chip(label: SharedString, selected: bool) -> gpui::Div {
    div()
        .flex_none()
        .px(px(8.))
        .py(px(2.))
        .rounded_xs()
        .border_1()
        .text_xs()
        .map(|this| {
            if selected {
                this.bg(rgb(BG_BUTTON))
                    .border_color(rgb(GOLD))
                    .text_color(rgb(GOLD))
            } else {
                this.bg(rgb(BG_CONTROL))
                    .border_color(rgb(BG_CONTROL))
                    .text_color(rgb(TEXT_DIM))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(BG_BUTTON_HOVER)).text_color(rgb(TEXT)))
            }
        })
        .child(label)
}

/// The text field of a quick action.
fn action_field(action: &QuickAction, cx: &mut Context<SettingsView>) -> Entity<TextField> {
    let text = action.text.clone();
    let placeholder = action_placeholder(action.kind);
    cx.new(|cx| TextField::new(text, placeholder, cx))
}

fn action_placeholder(kind: QuickActionKind) -> &'static str {
    match kind {
        QuickActionKind::ChatCommand => "/hideout, /exit, @last спасибо…",
        QuickActionKind::StashSearch => "Строка поиска, например с poe2.re",
    }
}

/// A setting that is on or off; the whole row flips it.
fn toggle_row(
    label: gpui::Div,
    on: bool,
    field: fn(&mut Settings) -> &mut bool,
    cx: &Context<SettingsView>,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .py(px(4.))
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                let value = field(&mut view.settings);
                *value = !*value;
                cx.notify();
            }),
        )
        .child(label)
        .child(switch(on))
}

fn switch(on: bool) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .w(px(30.))
        .h(px(16.))
        .px(px(2.))
        .rounded_full()
        .border_1()
        .map(|this| {
            if on {
                this.justify_end().bg(rgb(GOLD)).border_color(rgb(GOLD))
            } else {
                this.justify_start()
                    .bg(rgb(BG_CONTROL))
                    .border_color(rgb(BORDER))
            }
        })
        .child(
            div()
                .size(px(10.))
                .rounded_full()
                .bg(rgb(if on { BG_PANEL } else { TEXT_DIM })),
        )
}

/// A number with − and + either side; `(can_decrease, can_increase)` dims a button at its end of
/// the range.
fn stepper_row(
    label: gpui::Div,
    value: String,
    (can_decrease, can_increase): (bool, bool),
    step: fn(&mut Settings, bool),
    cx: &Context<SettingsView>,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .py(px(4.))
        .child(label)
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .child(step_button("−", false, can_decrease, step, cx))
                .child(div().w(px(56.)).text_center().child(value))
                .child(step_button("+", true, can_increase, step, cx)),
        )
}

fn step_button(
    symbol: &'static str,
    up: bool,
    enabled: bool,
    step: fn(&mut Settings, bool),
    cx: &Context<SettingsView>,
) -> impl IntoElement {
    div()
        .w(px(22.))
        .h(px(22.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_xs()
        .bg(rgb(BG_CONTROL))
        .border_1()
        .border_color(rgb(BORDER))
        .map(|this| {
            if enabled {
                this.text_color(rgb(GOLD))
                    .cursor_pointer()
                    .hover(|style| {
                        style
                            .bg(rgb(BG_BUTTON_HOVER))
                            .border_color(rgb(BORDER_GOLD))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                            step(&mut view.settings, up);
                            cx.notify();
                        }),
                    )
            } else {
                this.text_color(rgb(TEXT_MUTED))
            }
        })
        .child(symbol)
}

/// A button: `primary` in gold like the panel's search button ("Сохранить"), the rest plain.
fn button(label: &'static str, primary: bool) -> gpui::Div {
    div()
        .h(px(30.))
        .px(px(16.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_xs()
        .border_1()
        .cursor_pointer()
        .map(|this| {
            if primary {
                this.bg(rgb(BG_BUTTON))
                    .border_color(rgb(GOLD))
                    .text_color(rgb(GOLD))
                    .font_weight(FontWeight::SEMIBOLD)
                    .hover(|style| style.bg(rgb(BG_BUTTON_HOVER)))
            } else {
                this.bg(rgb(BG_CONTROL))
                    .border_color(rgb(BORDER))
                    .text_color(rgb(TEXT_DIM))
                    .hover(|style| style.text_color(rgb(TEXT)).border_color(rgb(TEXT_DIM)))
            }
        })
        .child(label)
}
