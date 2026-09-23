//! The settings window in the new style (plan-review.md, grill 20, B1-B2): the approved six
//! sidebar sections; every change applies at once, so there is no Отмена or Сохранить and the
//! window closes by × or Esc; the one erasing action asks first. The values are the plan's
//! defaults and a sample player's; the controls answer clicks here only and save nothing.

use std::time::Instant;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Context, FocusHandle, Focusable, KeyDownEvent,
    MouseButton, MouseDownEvent, SharedString, Window, WindowControlArea, div, linear_color_stop,
    linear_gradient, prelude::*, px, relative, rgb,
};
use trade_client::TradeSite;

use crate::ui::fonts::{self, NameFont};
use crate::ui::style::{
    ButtonKind, SCRIM_OPACITY, TRANSITION, alpha, appear, button, card, diamond, ease, ease_hover,
    ease_state, field, game_frame, heading, icon_button, keycaps, menu, modal_shadow,
    ornament_rule, recorder, section_heading, segmented, select, stepper, switch, switch_in,
    title_button, title_gradient,
};
use crate::ui::theme::{
    BG_CARD, BG_PANEL, BG_SIDEBAR, BORDER_CARD, BORDER_GOLD, GOLD, GOLD_LIGHT, PRICE_RISE, TEXT,
    TEXT_DIM, TEXT_MUTED, blend,
};

use super::LEAGUES;

const TITLE_HEIGHT: f32 = 40.;
const SIDEBAR_WIDTH: f32 = 216.;
/// The sidebar's list: where it starts, its rows and the gap between them.
const NAV_TOP: f32 = 18.;
const NAV_ITEM_HEIGHT: f32 = 38.;
const NAV_GAP: f32 = 2.;
/// Inset of a section's content from the sidebar and the window's right edge.
const CONTENT_INSET: f32 = 36.;
/// How much gold the top of a section's background takes.
const CONTENT_GLOW: f32 = 0.035;
/// The sample player's marks on waystone modifiers.
const WAYSTONE_MARKS: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    General,
    PriceCheck,
    QuickActions,
    XpOverlay,
    Account,
    Help,
}

impl Section {
    const ALL: [Section; 6] = [
        Section::General,
        Section::PriceCheck,
        Section::QuickActions,
        Section::XpOverlay,
        Section::Account,
        Section::Help,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::General => "Общие",
            Section::PriceCheck => "Проверка цены",
            Section::QuickActions => "Быстрые действия",
            Section::XpOverlay => "Оверлей опыта",
            Section::Account => "Аккаунт",
            Section::Help => "Помощь",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Section::General => "Лига, языки, масштаб и запуск вместе с Windows",
            Section::PriceCheck => "Горячая клавиша, продавцы и таблица результатов",
            Section::QuickActions => "Клавиши, которые печатают в игру команды чата и поиск",
            Section::XpOverlay => "Скорость набора опыта и таймер карты поверх игры",
            Section::Account => "Вход на pathofexile.com: приватные лиги и слежение за поиском",
            Section::Help => "Обучение, отчёт об ошибке и сведения о программе",
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    /// Where the sidebar's marker sits for this section.
    fn marker_top(self) -> f32 {
        NAV_TOP + self.index() as f32 * (NAV_ITEM_HEIGHT + NAV_GAP)
    }
}

/// A quick action as the sample player set it up.
struct Action {
    /// 0 for a chat command, 1 for a stash search.
    kind: usize,
    text: Option<&'static str>,
    keys: Option<&'static [&'static str]>,
}

pub(super) struct SettingsMockup {
    focus_handle: FocusHandle,
    section: Section,
    /// The sidebar marker's slide between sections: from where to where, since when, and how
    /// many so far (each restarts its animation).
    marker_from: f32,
    marker_to: f32,
    marker_moved: Option<Instant>,
    marker_slides: usize,
    league: usize,
    league_menu: bool,
    client_language: usize,
    interface_language: usize,
    scale_percent: u16,
    autostart: bool,
    check_updates: bool,
    recording_hotkey: bool,
    sellers: usize,
    seller_column: bool,
    waystone_marks: usize,
    confirming_reset: bool,
    actions: Vec<Action>,
    xp_overlay: bool,
    xp_percent: bool,
    xp_map_timer: bool,
    signed_in: bool,
}

impl SettingsMockup {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        let top = Section::General.marker_top();
        SettingsMockup {
            focus_handle: cx.focus_handle(),
            section: Section::General,
            marker_from: top,
            marker_to: top,
            marker_moved: None,
            marker_slides: 0,
            league: 0,
            league_menu: false,
            client_language: 0,
            interface_language: 0,
            scale_percent: 100,
            autostart: true,
            check_updates: true,
            recording_hotkey: false,
            // Only instant buyout (grill 6, B9).
            sellers: 0,
            seller_column: true,
            waystone_marks: WAYSTONE_MARKS,
            confirming_reset: false,
            actions: vec![
                Action {
                    kind: 0,
                    text: Some("/hideout"),
                    keys: Some(&["F5"]),
                },
                Action {
                    kind: 0,
                    text: Some("@last спасибо, удачи!"),
                    keys: Some(&["F6"]),
                },
                Action {
                    kind: 1,
                    text: Some("\"ур. предмета: 8[2-9]\""),
                    keys: Some(&["Ctrl", "1"]),
                },
            ],
            xp_overlay: true,
            xp_percent: false,
            xp_map_timer: true,
            signed_in: true,
        }
    }

    /// The marker's position `now`, partway along its slide.
    fn marker_at(&self, now: Instant) -> f32 {
        match self.marker_moved {
            Some(moved) => {
                let progress = (now.saturating_duration_since(moved).as_secs_f32()
                    / TRANSITION.as_secs_f32())
                .min(1.);
                self.marker_from + (self.marker_to - self.marker_from) * ease(progress)
            }
            None => self.marker_to,
        }
    }

    fn show(&mut self, section: Section, cx: &mut Context<Self>) {
        if section == self.section {
            return;
        }
        let now = Instant::now();
        self.marker_from = self.marker_at(now);
        self.marker_to = section.marker_top();
        self.marker_moved = Some(now);
        self.marker_slides += 1;
        self.section = section;
        self.league_menu = false;
        self.recording_hotkey = false;
        cx.notify();
    }

    /// Stores the index a segmented choice or menu picked, through `set`.
    fn pick(
        cx: &Context<Self>,
        set: fn(&mut Self, usize),
    ) -> impl Fn(&usize, &mut Window, &mut App) + 'static {
        cx.listener(move |view, index: &usize, _, cx| {
            set(view, *index);
            cx.notify();
        })
    }

    fn render_title_bar(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(TITLE_HEIGHT))
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
                    .gap(px(10.))
                    .pl(px(20.))
                    .window_control_area(WindowControlArea::Drag)
                    .child(diamond(8., GOLD))
                    .child(
                        heading(face)
                            .text_size(px(15.))
                            .text_color(rgb(GOLD_LIGHT))
                            .child("PoE2 Oracle"),
                    )
                    .child(div().text_color(rgb(TEXT_MUTED)).child("·"))
                    .child(
                        heading(face)
                            .text_size(px(15.))
                            .text_color(rgb(TEXT))
                            .child("Настройки"),
                    ),
            )
            .child(title_button(
                "close",
                "×",
                46.,
                true,
                cx.listener(|_, _: &MouseDownEvent, window, _| window.remove_window()),
            ))
    }

    fn render_sidebar(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        let (from, to) = (self.marker_from, self.marker_to);
        let marker = div()
            .absolute()
            .left(px(10.))
            .right(px(10.))
            .h(px(NAV_ITEM_HEIGHT))
            .rounded(px(6.))
            .bg(linear_gradient(
                90.,
                linear_color_stop(alpha(GOLD, 0.16), 0.),
                linear_color_stop(alpha(GOLD, 0.02), 1.),
            ))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(10.))
                    .bottom(px(10.))
                    .w(px(2.))
                    .rounded_full()
                    .bg(rgb(GOLD)),
            )
            .with_animation(
                ("marker", self.marker_slides),
                Animation::new(TRANSITION).with_easing(ease),
                move |marker, t| marker.top(px(from + (to - from) * t)),
            );
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .bg(rgb(BG_SIDEBAR))
            .border_r_1()
            .border_color(rgb(BORDER_CARD))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(NAV_GAP))
                    .px(px(10.))
                    .pt(px(NAV_TOP))
                    .child(marker)
                    .children(Section::ALL.map(|section| self.render_nav_item(section, face, cx))),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(px(26.))
                    .pb(px(18.))
                    .child(
                        heading(face)
                            .text_size(px(13.))
                            .text_color(rgb(GOLD))
                            .child("PoE2 Oracle"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(TEXT_MUTED))
                            .child("версия 0.1.0"),
                    ),
            )
    }

    /// A section in the sidebar, named in the heading face.
    fn render_nav_item(
        &self,
        section: Section,
        face: &'static NameFont,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let current = section == self.section;
        let item = div()
            .id(("nav", section.index()))
            .relative()
            .flex()
            .items_center()
            .h(px(NAV_ITEM_HEIGHT))
            .px(px(18.))
            .rounded(px(6.))
            .font_family(face.family)
            .font_weight(face.weight)
            .text_size(px(15.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| view.show(section, cx)),
            );
        ease_hover(("nav", section.index()), item, move |item, hover| {
            item.bg(alpha(GOLD, 0.05 * hover)).child(ease_state(
                "label",
                current,
                div(),
                move |label, on| {
                    label
                        .text_color(rgb(blend(blend(TEXT_DIM, TEXT, hover), GOLD_LIGHT, on)))
                        .child(section.title())
                },
            ))
        })
    }

    fn render_content(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        let section = self.section;
        let body: AnyElement = match section {
            Section::General => self.render_general(face, cx).into_any_element(),
            Section::PriceCheck => self.render_price_check(face, cx).into_any_element(),
            Section::QuickActions => self.render_quick_actions(face, cx).into_any_element(),
            Section::XpOverlay => self.render_xp_overlay(face, cx).into_any_element(),
            Section::Account => self.render_account(face, cx).into_any_element(),
            Section::Help => self.render_help(face, cx).into_any_element(),
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            // A faint warmth under the title bar, as if lit from it, gone a third of the way
            // down.
            .bg(linear_gradient(
                180.,
                linear_color_stop(rgb(blend(BG_PANEL, GOLD, CONTENT_GLOW)), 0.),
                linear_color_stop(rgb(BG_PANEL), 0.3),
            ))
            .child(switch_in(
                ("section", section.index()),
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_none()
                            .gap(px(4.))
                            .px(px(CONTENT_INSET))
                            .pt(px(26.))
                            .pb(px(16.))
                            .child(
                                heading(face)
                                    .text_size(px(24.))
                                    .text_color(rgb(GOLD_LIGHT))
                                    .child(section.title()),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(rgb(TEXT_DIM))
                                    .child(section.summary()),
                            )
                            .child(div().pt(px(12.)).child(ornament_rule(BORDER_GOLD))),
                    )
                    .child(
                        div()
                            .id(("section-body", section.index()))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(CONTENT_INSET))
                            .pt(px(4.))
                            .pb(px(28.))
                            .child(body),
                    ),
            ))
    }

    fn render_general(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        let league = div()
            .flex()
            .flex_col()
            .child(select(
                "league",
                LEAGUES[self.league],
                false,
                cx.listener(|view, _: &MouseDownEvent, _, cx| {
                    view.league_menu = !view.league_menu;
                    cx.notify();
                }),
            ))
            .when(self.league_menu, |this| {
                this.child(menu(
                    "league-menu",
                    Vec::from(LEAGUES.map(SharedString::new_static)),
                    self.league,
                    260.,
                    Self::pick(cx, |view, index| {
                        view.league = index;
                        view.league_menu = false;
                    }),
                    cx.listener(|view, _: &MouseDownEvent, _, cx| {
                        view.league_menu = false;
                        cx.notify();
                    }),
                ))
            });
        let scale = self.scale_percent;
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(group(
                face,
                "Лига и язык",
                [
                    setting_row(
                        "Лига",
                        Some("Где искать цены. Авто — текущая лига сайта торговли"),
                        league,
                    ),
                    setting_row(
                        "Язык клиента игры",
                        Some("На каком языке игра копирует предметы. Авто — по тексту предмета"),
                        segmented(
                            "client-language",
                            &["Авто", "Русский", "English"],
                            self.client_language,
                            Self::pick(cx, |view, index| view.client_language = index),
                        ),
                    ),
                    setting_row(
                        "Язык интерфейса",
                        Some(
                            "Авто — как у клиента игры, а до первого запуска игры — как у Windows",
                        ),
                        segmented(
                            "interface-language",
                            &["Авто · Русский", "Русский", "English"],
                            self.interface_language,
                            Self::pick(cx, |view, index| view.interface_language = index),
                        ),
                    ),
                ],
            ))
            .child(group(
                face,
                "Окно",
                [setting_row(
                    "Масштаб интерфейса",
                    Some("Размер текста и элементов панели цены и оверлеев"),
                    stepper(
                        "scale",
                        format!("{scale} %"),
                        (scale > 80, scale < 150),
                        cx.listener(|view, up: &bool, _, cx| {
                            view.scale_percent = if *up {
                                (view.scale_percent + 5).min(150)
                            } else {
                                view.scale_percent.saturating_sub(5).max(80)
                            };
                            cx.notify();
                        }),
                    ),
                )],
            ))
            .child(group(
                face,
                "Система",
                [
                    toggle_row(
                        "autostart",
                        "Запускать вместе с Windows",
                        None,
                        self.autostart,
                        |view| &mut view.autostart,
                        cx,
                    ),
                    toggle_row(
                        "check-updates",
                        "Проверять обновления",
                        Some("Новая версия появится в меню значка у часов"),
                        self.check_updates,
                        |view| &mut view.check_updates,
                        cx,
                    ),
                ],
            ))
    }

    fn render_price_check(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        let hotkey: AnyElement = if self.recording_hotkey {
            div()
                .text_color(rgb(GOLD_LIGHT))
                .child("Нажмите сочетание…")
                .into_any_element()
        } else {
            keycaps(&["Ctrl", "E"]).into_any_element()
        };
        let marks = self.waystone_marks;
        let marks_note: SharedString = if marks == 0 {
            "Пометок нет. Ставятся на панели цены у модификаторов путевого камня".into()
        } else {
            format!("{marks} пометок: опасные, на внимание, нужные — ставятся на панели цены")
                .into()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(group(
                face,
                "Горячая клавиша",
                [setting_row(
                    "Проверка цены",
                    Some("Наведите курсор на предмет в игре и нажмите это сочетание"),
                    recorder(
                        "price-check-hotkey",
                        hotkey,
                        self.recording_hotkey,
                        cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            view.recording_hotkey = !view.recording_hotkey;
                            cx.notify();
                        }),
                    ),
                )],
            ))
            .child(group(
                face,
                "Поиск",
                [
                    setting_row(
                        "Продавцы по умолчанию",
                        Some("Мгновенный выкуп не ждёт продавца в сети: покупка проходит в игре"),
                        segmented(
                            "sellers",
                            &["Мгновенный выкуп", "Выкуп и онлайн", "Онлайн", "Все"],
                            self.sellers,
                            Self::pick(cx, |view, index| view.sellers = index),
                        ),
                    ),
                    toggle_row(
                        "seller-column",
                        "Колонка продавца",
                        Some("Имя продавца в таблице результатов"),
                        self.seller_column,
                        |view| &mut view.seller_column,
                        cx,
                    ),
                ],
            ))
            .child(group(
                face,
                "Путевые камни",
                [setting_row(
                    "Пометки модификаторов",
                    Some(marks_note),
                    div().when(marks > 0, |this| {
                        this.child(button(
                            "reset-marks",
                            "Сбросить…",
                            ButtonKind::Danger,
                            face,
                            cx.listener(|view, _: &MouseDownEvent, _, cx| {
                                view.confirming_reset = true;
                                cx.notify();
                            }),
                        ))
                    }),
                )],
            ))
    }

    fn render_quick_actions(
        &self,
        face: &'static NameFont,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let rows = self.actions.iter().enumerate().map(|(index, action)| {
            let text_placeholder = if action.kind == 0 {
                "/hideout, /exit, @last спасибо…"
            } else {
                "Строка поиска, например с poe2.re"
            };
            let keys: AnyElement = match action.keys {
                Some(keys) => keycaps(keys).into_any_element(),
                None => div()
                    .text_color(rgb(TEXT_MUTED))
                    .child("без клавиши")
                    .into_any_element(),
            };
            div()
                .id(("action", index))
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(12.))
                .py(px(10.))
                .child(segmented(
                    "kind",
                    &["Чат", "Тайник"],
                    action.kind,
                    cx.listener(move |view, kind: &usize, _, cx| {
                        view.actions[index].kind = *kind;
                        cx.notify();
                    }),
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(field("text", action.text, text_placeholder)),
                )
                .child(recorder(
                    "keys",
                    keys,
                    false,
                    |_: &MouseDownEvent, _: &mut Window, _: &mut App| {},
                ))
                .child(icon_button(
                    "remove",
                    "×",
                    true,
                    cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                        view.actions.remove(index);
                        cx.notify();
                    }),
                ))
                .into_any_element()
        });
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .text_size(px(13.))
                    .child(div().text_color(rgb(TEXT_DIM)).child(
                        "Чат: команда уходит в чат игры — /hideout, /exit, @last спасибо \
                         (ответ тому, кто писал последним). Тайник: строка поиска в открытом \
                         тайнике или у торговца, например с poe2.re.",
                    ))
                    .child(div().text_size(px(12.)).text_color(rgb(TEXT_MUTED)).child(
                        "Опасные команды — /destroy, /clear_ignore_list — не отправляются.",
                    )),
            )
            .child(group(face, "Действия", rows.collect::<Vec<_>>()))
            .child(div().flex().child(button(
                "add-action",
                "+ Добавить действие",
                ButtonKind::Secondary,
                face,
                cx.listener(|view, _: &MouseDownEvent, _, cx| {
                    view.actions.push(Action {
                        kind: 0,
                        text: None,
                        keys: None,
                    });
                    cx.notify();
                }),
            )))
    }

    fn render_xp_overlay(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        group(
            face,
            "Строка опыта",
            [
                toggle_row(
                    "xp-overlay",
                    "Показывать оверлей опыта",
                    Some("Скорость набора опыта и время до следующего уровня"),
                    self.xp_overlay,
                    |view| &mut view.xp_overlay,
                    cx,
                ),
                toggle_row(
                    "xp-percent",
                    "Процент уровня",
                    Some("Сколько уровня уже набрано"),
                    self.xp_percent,
                    |view| &mut view.xp_percent,
                    cx,
                ),
                toggle_row(
                    "xp-map-timer",
                    "Таймер карты",
                    Some("Время в текущей карте и опыт за неё; в убежище — последняя карта"),
                    self.xp_map_timer,
                    |view| &mut view.xp_map_timer,
                    cx,
                ),
            ],
        )
    }

    fn render_account(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        let status = if self.signed_in {
            setting_row(
                "Вы вошли как mttzzz#1362",
                Some("Сессия хранится только в диспетчере учётных данных Windows"),
                button(
                    "sign-out",
                    "Выйти",
                    ButtonKind::Secondary,
                    face,
                    cx.listener(|view, _: &MouseDownEvent, _, cx| {
                        view.signed_in = false;
                        cx.notify();
                    }),
                ),
            )
        } else {
            setting_row(
                "Вход не выполнен",
                Some("Откроется страница входа pathofexile.com — можно войти и через Steam"),
                button(
                    "sign-in",
                    "Войти",
                    ButtonKind::Primary,
                    face,
                    cx.listener(|view, _: &MouseDownEvent, _, cx| {
                        view.signed_in = true;
                        cx.notify();
                    }),
                ),
            )
        };
        let watching: SharedString = if self.signed_in {
            "2 из 20".into()
        } else {
            "нужен вход".into()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(group(face, "pathofexile.com", [status]))
            .child(group(
                face,
                "Слежение за поиском",
                [setting_row(
                    "Следить",
                    Some(
                        "Кнопка «Следить» на панели цены: новые лоты по поиску приходят \
                         карточками поверх игры. Не больше 20 поисков сразу — правило сайта",
                    ),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .when(self.signed_in, |this| {
                            this.child(div().size(px(7.)).rounded_full().bg(rgb(PRICE_RISE)))
                        })
                        .text_color(rgb(TEXT_DIM))
                        .child(watching),
                )],
            ))
            .child(group(
                face,
                "Приватная лига",
                [setting_row(
                    "Название лиги",
                    Some(
                        "Как на сайте торговли, со скобками. Поиск в приватной лиге работает \
                         только со входом",
                    ),
                    div()
                        .w(px(250.))
                        .child(field("private-league", None, "My League (PL12345)")),
                )],
            ))
    }

    fn render_help(&self, face: &'static NameFont, _cx: &Context<Self>) -> impl IntoElement {
        let quiet = |_: &MouseDownEvent, _: &mut Window, _: &mut App| {};
        group(
            face,
            "Помощь",
            [
                setting_row(
                    "Обучение",
                    Some("Пять коротких шагов: проверка цены, фильтры, профили и результаты"),
                    button("tour", "Пройти заново", ButtonKind::Secondary, face, quiet),
                ),
                setting_row(
                    "Сообщить об ошибке",
                    Some(
                        "Форма на GitHub. Отчёт — логи и настройки в одном архиве, имя \
                         пользователя Windows в нём скрыто",
                    ),
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(button(
                            "report",
                            "Сообщить ↗",
                            ButtonKind::Secondary,
                            face,
                            quiet,
                        ))
                        .child(button(
                            "diagnostics",
                            "Собрать отчёт",
                            ButtonKind::Secondary,
                            face,
                            quiet,
                        )),
                ),
                setting_row(
                    "Папка логов",
                    Some("Журнал работы программы за последние запуски"),
                    button("logs", "Открыть", ButtonKind::Secondary, face, quiet),
                ),
                setting_row(
                    "О программе",
                    Some("PoE2 Oracle 0.1.0 · лицензия MIT или Apache-2.0"),
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(button(
                            "licenses",
                            "Лицензии",
                            ButtonKind::Secondary,
                            face,
                            quiet,
                        ))
                        .child(button(
                            "github",
                            "GitHub ↗",
                            ButtonKind::Secondary,
                            face,
                            quiet,
                        )),
                ),
            ],
        )
    }

    /// Resetting the waystone marks asks first (grill 20, B2): a dialog over a dimmed window.
    fn render_confirm(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        let close = cx.listener(|view, _: &MouseDownEvent, _, cx| {
            view.confirming_reset = false;
            cx.notify();
        });
        div()
            .id("scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(alpha(0x000000, SCRIM_OPACITY))
            .occlude()
            .on_mouse_down(MouseButton::Left, close)
            .child(appear(
                "confirm",
                div()
                    .id("confirm")
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .w(px(420.))
                    .px(px(24.))
                    .pt(px(22.))
                    .pb(px(20.))
                    .bg(rgb(BG_CARD))
                    .shadow(modal_shadow())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        heading(face)
                            .text_size(px(18.))
                            .text_color(rgb(GOLD_LIGHT))
                            .child("Сбросить пометки?"),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(TEXT_DIM))
                            .child(format!(
                                "Все {} пометок модификаторов путевых камней будут удалены. \
                             Отменить это нельзя.",
                                self.waystone_marks
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .pt(px(10.))
                            .child(button(
                                "keep",
                                "Отмена",
                                ButtonKind::Secondary,
                                face,
                                cx.listener(|view, _: &MouseDownEvent, _, cx| {
                                    view.confirming_reset = false;
                                    cx.notify();
                                }),
                            ))
                            .child(button(
                                "reset",
                                "Сбросить",
                                ButtonKind::Danger,
                                face,
                                cx.listener(|view, _: &MouseDownEvent, _, cx| {
                                    view.waystone_marks = 0;
                                    view.confirming_reset = false;
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(game_frame()),
            ))
    }
}

impl Focusable for SettingsMockup {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsMockup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let face = fonts::name_font(TradeSite::Russian);
        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                // Esc closes what is open, innermost first, then the window.
                if event.keystroke.key != "escape" {
                    if view.recording_hotkey {
                        view.recording_hotkey = false;
                        cx.notify();
                    }
                    return;
                }
                if view.confirming_reset {
                    view.confirming_reset = false;
                } else if view.league_menu {
                    view.league_menu = false;
                } else if view.recording_hotkey {
                    view.recording_hotkey = false;
                } else {
                    window.remove_window();
                    return;
                }
                cx.notify();
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG_PANEL))
            .text_color(rgb(TEXT))
            .text_size(px(14.))
            .line_height(relative(1.4))
            .child(self.render_title_bar(face, cx))
            .child(appear(
                "open",
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(face, cx))
                    .child(self.render_content(face, cx)),
            ))
            .when(self.confirming_reset, |this| {
                this.child(self.render_confirm(face, cx))
            })
            .child(game_frame())
    }
}

/// A group of rows under its heading.
fn group(
    face: &'static NameFont,
    title: &'static str,
    rows: impl IntoIterator<Item = AnyElement>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(section_heading(face, title))
        .child(card(rows))
}

/// A setting: its name and, under it, what it does on the left; its control on the right.
fn setting_row(
    label: &'static str,
    description: Option<impl Into<SharedString>>,
    control: impl IntoElement,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(24.))
        .px(px(14.))
        .py(px(12.))
        .child(label_block(label, description))
        .child(div().flex_none().child(control))
        .into_any_element()
}

fn label_block(label: &'static str, description: Option<impl Into<SharedString>>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .flex_1()
        .min_w_0()
        .child(div().text_color(rgb(TEXT)).child(label))
        .children(description.map(|description| {
            div()
                .text_size(px(12.))
                .text_color(rgb(TEXT_DIM))
                .child(description.into())
        }))
}

/// A setting that is on or off: the whole row flips it, lighting up under the pointer.
fn toggle_row(
    key: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    on: bool,
    flag: fn(&mut SettingsMockup) -> &mut bool,
    cx: &Context<SettingsMockup>,
) -> AnyElement {
    let row = div()
        .id(key)
        .flex()
        .items_center()
        .gap(px(24.))
        .px(px(14.))
        .py(px(12.))
        .rounded(px(6.))
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                let value = flag(view);
                *value = !*value;
                cx.notify();
            }),
        )
        .child(label_block(label, description))
        .child(switch("switch", on));
    ease_hover(key, row, |row, hover| row.bg(alpha(GOLD, 0.05 * hover))).into_any_element()
}
