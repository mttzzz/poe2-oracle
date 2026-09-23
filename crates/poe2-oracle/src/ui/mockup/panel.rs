//! One price panel in the new style, on a real item: the rare ring «Крутящий ободок» of
//! `item-parser/tests/fixtures/ru_live_krutyaschiy_obodok.txt`, its tier ladders from
//! `stat-filters`' RePoE table, with sample instant-buyout listings. It has the league chip in
//! the title bar (grill 23), the profile chip and «Минимум тира» (grill 6, B5-B6), the roll slider
//! under each stat, and the results table. It appears as the real one will, rising in over
//! 0.15 s; F5 plays that again. Nothing searches: the controls answer clicks here only.

use gpui::{
    Animation, AnimationExt as _, App, Context, FocusHandle, Focusable, FontWeight, HighlightStyle,
    KeyDownEvent, MouseButton, MouseDownEvent, SharedString, StyledText, Window, WindowControlArea,
    div, img, linear_color_stop, linear_gradient, prelude::*, px, relative, rgb,
};
use trade_client::TradeSite;

use crate::item_refs::{self, RefKind};
use crate::ui::fonts::{self, NameFont};
use crate::ui::style::{
    ButtonKind, TRANSITION, alpha, appear, button, checkbox, chip, ease, ease_hover, ease_state,
    game_frame, game_hint, glow, heading, inner_glow, link, menu, ornament_rule, plate,
    section_heading, select, switch, title_button, title_gradient, toggle_chip,
};
use crate::ui::theme::{
    BADGE_INK, BANNER_EDGE, BANNER_TINT, BG_FIELD, BG_NAMEPLATE, BG_PANEL, BORDER_FIELD,
    BORDER_GOLD, BORDER_ROW, GOLD, GOLD_LIGHT, RARITY_RARE, TEXT, TEXT_DIM, TEXT_MUTED, TEXT_VALUE,
    TIER_TOP, blend, rems_from_px,
};

use super::LEAGUES;

const TITLE_HEIGHT: f32 = 32.;
/// Indent of a stat row's slider: the checkbox and the gap after it.
const CHECK_COLUMN: f32 = 23.;
/// The roll slider: its track's thickness and the whole slider's height, px.
const SLIDER_TRACK: f32 = 3.;
const SLIDER_HEIGHT: f32 = 12.;
const ART_SIZE: f32 = 48.;
const PRICE_COLUMN: f32 = 150.;
const LEVEL_COLUMN: f32 = 30.;
const LISTED_COLUMN: f32 = 100.;

/// The search profiles (grill 6, B5).
const PROFILES: [&str; 4] = [
    "Быстрая цена",
    "Точное совпадение",
    "Широкий −10 %",
    "База для крафта",
];

const DIVINE_ICON: &str = "https://web.poecdn.com/gen/image/WzI1LDE0LHsiZiI6IjJESXRlbXMvQ3VycmVuY3kvQ3VycmVuY3lNb2RWYWx1ZXMiLCJ3IjoxLCJoIjoxLCJzY2FsZSI6MSwicmVhbG0iOiJwb2UyIn1d/af4036c345/CurrencyModValues.png";
const EXALTED_ICON: &str = "https://web.poecdn.com/gen/image/WzI1LDE0LHsiZiI6IjJESXRlbXMvQ3VycmVuY3kvQ3VycmVuY3lBZGRNb2RUb1JhcmUiLCJ3IjoxLCJoIjoxLCJzY2FsZSI6MSwicmVhbG0iOiJwb2UyIn1d/4ef5836606/CurrencyAddModToRare.png";
const CHAOS_ICON: &str = "https://web.poecdn.com/gen/image/WzI1LDE0LHsiZiI6IjJESXRlbXMvQ3VycmVuY3kvQ3VycmVuY3lSZXJvbGxSYXJlIiwidyI6MSwiaCI6MSwic2NhbGUiOjEsInJlYWxtIjoicG9lMiJ9XQ/20cd0159b7/CurrencyRerollRare.png";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Implicit,
    Prefix,
    Suffix,
}

/// One of the ring's stats: its line around the rolled value, its tier among how many, and the
/// ladder of every tier's range, lowest first.
struct Stat {
    kind: Kind,
    before: &'static str,
    roll: f32,
    after: &'static str,
    tier: Option<(u32, u32)>,
    ladder: &'static [(f32, f32)],
    /// The item level its tier needs.
    tier_level: u32,
}

impl Stat {
    fn range(&self) -> (f32, f32) {
        (self.ladder[0].0, self.ladder[self.ladder.len() - 1].1)
    }

    /// The range of its own tier.
    fn tier_range(&self) -> (f32, f32) {
        match self.tier {
            Some((tier, count)) => self.ladder[(count - tier) as usize],
            None => self.range(),
        }
    }

    /// Where `value` sits along the ladder, 0 to 1.
    fn fraction(&self, value: f32) -> f32 {
        let (low, high) = self.range();
        ((value - low) / (high - low)).clamp(0., 1.)
    }
}

static STATS: [Stat; 5] = [
    Stat {
        kind: Kind::Implicit,
        before: "+",
        roll: 8.,
        after: "% к сопротивлению хаосу",
        tier: None,
        ladder: &[(7., 13.)],
        tier_level: 1,
    },
    Stat {
        kind: Kind::Prefix,
        before: "",
        roll: 26.,
        after: "% увеличение урона от молнии",
        tier: Some((2, 6)),
        ladder: &[
            (3., 7.),
            (8., 12.),
            (13., 17.),
            (18., 22.),
            (23., 26.),
            (27., 30.),
        ],
        tier_level: 65,
    },
    Stat {
        kind: Kind::Suffix,
        before: "",
        roll: 17.,
        after: "% повышение скорости сотворения чар",
        tier: Some((3, 5)),
        ladder: &[(9., 12.), (13., 15.), (16., 18.), (19., 21.), (22., 24.)],
        tier_level: 35,
    },
    Stat {
        kind: Kind::Suffix,
        before: "+",
        roll: 7.,
        after: " к силе",
        tier: Some((8, 8)),
        ladder: &[
            (5., 8.),
            (9., 12.),
            (13., 16.),
            (17., 20.),
            (21., 24.),
            (25., 27.),
            (28., 30.),
            (31., 33.),
        ],
        tier_level: 1,
    },
    Stat {
        kind: Kind::Suffix,
        before: "+",
        roll: 6.,
        after: "% к сопротивлению холоду",
        tier: Some((8, 8)),
        ladder: &[
            (6., 10.),
            (11., 15.),
            (16., 20.),
            (21., 25.),
            (26., 30.),
            (31., 35.),
            (36., 40.),
            (41., 45.),
        ],
        tier_level: 1,
    },
];

/// What «Быстрая цена» picks for this ring.
const QUICK_PRICE: [bool; 5] = [false, true, true, false, true];

#[derive(Clone, Copy)]
enum Currency {
    Exalted,
    Divine,
    Chaos,
}

impl Currency {
    fn icon(self) -> &'static str {
        match self {
            Currency::Exalted => EXALTED_ICON,
            Currency::Divine => DIVINE_ICON,
            Currency::Chaos => CHAOS_ICON,
        }
    }
}

/// A sample listing: price, item level, seller, age -- and EE2's markers: listed that many times
/// at this price, or priced by its stash tab's name.
struct Listing {
    amount: &'static str,
    currency: Currency,
    /// What it's worth in exalts, for a price in another currency.
    in_exalted: Option<&'static str>,
    level: u32,
    seller: &'static str,
    listed: &'static str,
    repeats: Option<u32>,
    tab_price: bool,
}

static LISTINGS: [Listing; 10] = [
    listing("3", Currency::Exalted, 79, "Arkhon#4412", "4 мин. назад"),
    listing("4", Currency::Exalted, 81, "Льдинка#0932", "12 мин. назад"),
    Listing {
        repeats: Some(3),
        ..listing("5", Currency::Exalted, 76, "Veyra#2291", "25 мин. назад")
    },
    listing("5", Currency::Exalted, 82, "Tamerlan#7715", "1 ч. назад"),
    listing("8", Currency::Exalted, 79, "krokodil#1840", "2 ч. назад"),
    listing("12", Currency::Exalted, 80, "Sonne#0027", "5 ч. назад"),
    Listing {
        tab_price: true,
        ..listing("20", Currency::Exalted, 77, "MapDevice#3131", "14 ч. назад")
    },
    listing("35", Currency::Exalted, 83, "Хранитель#5530", "1 дн. назад"),
    Listing {
        in_exalted: Some("63"),
        ..listing("1", Currency::Chaos, 81, "Kadabra#0451", "2 дн. назад")
    },
    listing("1", Currency::Divine, 84, "Graviton#3303", "3 дн. назад"),
];

const fn listing(
    amount: &'static str,
    currency: Currency,
    level: u32,
    seller: &'static str,
    listed: &'static str,
) -> Listing {
    Listing {
        amount,
        currency,
        in_exalted: None,
        level,
        seller,
        listed,
        repeats: None,
        tab_price: false,
    }
}

pub(super) struct PanelMockup {
    focus_handle: FocusHandle,
    /// Times the panel appeared: F5 plays the appearance again.
    appearances: usize,
    league: usize,
    league_menu: bool,
    profile: usize,
    profile_menu: bool,
    checked: [bool; 5],
    free_prefixes: bool,
    /// Each stat's search minimum, where its thumb set off from when it last moved, and how many
    /// moves so far (each restarts the thumbs' slide).
    minimums: [f32; 5],
    thumbs_from: [f32; 5],
    thumb_moves: usize,
    watching: bool,
}

impl PanelMockup {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        let own = STATS.each_ref().map(|stat| stat.roll);
        PanelMockup {
            focus_handle: cx.focus_handle(),
            appearances: 0,
            league: 0,
            league_menu: false,
            profile: 0,
            profile_menu: false,
            checked: QUICK_PRICE,
            free_prefixes: false,
            minimums: own,
            thumbs_from: own,
            thumb_moves: 0,
            watching: false,
        }
    }

    /// Moves every thumb to `minimums`, sliding from where they are.
    fn set_minimums(&mut self, minimums: [f32; 5]) {
        self.thumbs_from = self.minimums;
        self.minimums = minimums;
        self.thumb_moves += 1;
    }

    /// A profile's picks and minimums (grill 6, B5): «Быстрая цена» and «Точное совпадение» from
    /// the item's own rolls, «Широкий −10 %» a tenth lower, «База для крафта» the implicit only.
    fn apply_profile(&mut self, profile: usize) {
        self.profile = profile;
        let own = STATS.each_ref().map(|stat| stat.roll);
        match profile {
            0 => {
                self.checked = QUICK_PRICE;
                self.set_minimums(own);
            }
            1 => {
                self.checked = [true; 5];
                self.set_minimums(own);
            }
            2 => self.set_minimums(own.map(|roll| (roll * 0.9).floor())),
            _ => {
                self.checked = [true, false, false, false, false];
                self.set_minimums(own);
            }
        }
    }

    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(rems_from_px(TITLE_HEIGHT))
            .pl(rems_from_px(8.))
            .bg(title_gradient())
            .border_b_1()
            .border_color(rgb(BORDER_GOLD))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(select(
                        "league",
                        LEAGUES[self.league],
                        true,
                        cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            view.league_menu = !view.league_menu;
                            view.profile_menu = false;
                            cx.notify();
                        }),
                    ))
                    .when(self.league_menu, |this| {
                        this.child(menu(
                            "league-menu",
                            Vec::from(LEAGUES.map(SharedString::new_static)),
                            self.league,
                            250.,
                            cx.listener(|view, index: &usize, _, cx| {
                                view.league = *index;
                                view.league_menu = false;
                                cx.notify();
                            }),
                            cx.listener(|view, _: &MouseDownEvent, _, cx| {
                                view.league_menu = false;
                                cx.notify();
                            }),
                        ))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(rems_from_px(3.))
                    .pr(rems_from_px(6.))
                    .text_size(rems_from_px(12.))
                    .text_color(rgb(TEXT_DIM))
                    .child("1")
                    .child(currency_icon(Currency::Divine, 16.))
                    .child("= 491")
                    .child(currency_icon(Currency::Exalted, 16.)),
            )
            .child(title_button(
                "settings",
                "⚙",
                34.,
                false,
                |_: &MouseDownEvent, _: &mut Window, _: &mut App| {},
            ))
            .child(title_button(
                "close",
                "×",
                34.,
                true,
                cx.listener(|_, _: &MouseDownEvent, window, _| window.remove_window()),
            ))
    }

    fn render_nameplate(&self, face: &'static NameFont) -> impl IntoElement {
        let art = item_refs::lookup(RefKind::Item, "Кольцо с аметистом");
        let quiet = |_: &MouseDownEvent, _: &mut Window, _: &mut App| {};
        // Under the whole header, not the name's column: four links outgrow it.
        let links = div()
            .flex()
            .justify_center()
            .items_center()
            .gap(rems_from_px(14.))
            .px(rems_from_px(12.))
            .pb(rems_from_px(8.))
            .child(link("poe2db", "poe2db ↗", quiet))
            .child(link("wiki", "вики ↗", quiet))
            .child(link("craft-of-exile", "Craft of Exile ↗", quiet))
            .child(link("report", "сообщить об ошибке ↗", quiet));
        div()
            .flex()
            .flex_col()
            .flex_none()
            .bg(linear_gradient(
                180.,
                linear_color_stop(rgb(blend(BG_NAMEPLATE, RARITY_RARE, BANNER_TINT)), 0.),
                linear_color_stop(rgb(BG_NAMEPLATE), 1.),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(rems_from_px(10.))
                    .px(rems_from_px(12.))
                    .pt(rems_from_px(12.))
                    .pb(rems_from_px(4.))
                    .children(art.and_then(|art| art.icon_url()).map(|url| {
                        img(url)
                            .flex_none()
                            .size(rems_from_px(ART_SIZE))
                            .object_fit(gpui::ObjectFit::Contain)
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .text_color(rgb(RARITY_RARE))
                            .child(
                                heading(face)
                                    .text_size(rems_from_px(21.))
                                    .child("Крутящий ободок"),
                            )
                            .child(
                                heading(face)
                                    .text_size(rems_from_px(16.))
                                    .child("Кольцо с аметистом"),
                            ),
                    )
                    .child(div().flex_none().size(rems_from_px(ART_SIZE))),
            )
            .child(links)
            .child(
                div()
                    .px(rems_from_px(16.))
                    .pb(rems_from_px(2.))
                    .child(ornament_rule(blend(BG_NAMEPLATE, RARITY_RARE, BANNER_EDGE))),
            )
    }

    fn render_chips(&self, cx: &Context<Self>) -> impl IntoElement {
        let selected = self.checked.iter().filter(|&&checked| checked).count()
            + usize::from(self.free_prefixes);
        let quiet = |_: &MouseDownEvent, _: &mut Window, _: &mut App| {};
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(rems_from_px(6.))
            .pt(rems_from_px(10.))
            .pb(rems_from_px(6.))
            .child(toggle_chip("scope", Some("Класс:"), "Кольца", quiet))
            .child(chip(Some("Ур. предмета:"), "79", TEXT))
            .child(chip(Some("Требуется ур.:"), "52", TEXT))
            .child(toggle_chip("rarity", Some("Редкость:"), "редкие", quiet))
            .child(toggle_chip("corruption", None, "Можно изменить", quiet))
            .child(toggle_chip(
                "stats",
                Some("Св-ва:"),
                format!("{selected} из 6"),
                cx.listener(|view, _: &MouseDownEvent, _, cx| {
                    let all = !view.checked.iter().all(|&checked| checked);
                    view.checked = [all; 5];
                    view.free_prefixes = all;
                    cx.notify();
                }),
            ))
    }

    /// The profile chip and «Минимум тира», above the stats.
    fn render_toolbar(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(rems_from_px(8.))
            .py(rems_from_px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(rems_from_px(6.))
                    .child(
                        div()
                            .text_size(rems_from_px(12.))
                            .text_color(rgb(TEXT_DIM))
                            .child("Профиль:"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(select(
                                "profile",
                                PROFILES[self.profile],
                                true,
                                cx.listener(|view, _: &MouseDownEvent, _, cx| {
                                    view.profile_menu = !view.profile_menu;
                                    view.league_menu = false;
                                    cx.notify();
                                }),
                            ))
                            .when(self.profile_menu, |this| {
                                this.child(menu(
                                    "profile-menu",
                                    Vec::from(PROFILES.map(SharedString::new_static)),
                                    self.profile,
                                    200.,
                                    cx.listener(|view, index: &usize, _, cx| {
                                        view.apply_profile(*index);
                                        view.profile_menu = false;
                                        cx.notify();
                                    }),
                                    cx.listener(|view, _: &MouseDownEvent, _, cx| {
                                        view.profile_menu = false;
                                        cx.notify();
                                    }),
                                ))
                            }),
                    ),
            )
            .child(
                div()
                    .id("tier-minimum-hint")
                    .tooltip(game_hint(
                        face,
                        Some("Минимум тира"),
                        vec![(
                            "У всех отмеченных свойств минимум поиска станет нижней \
                             границей их тира"
                                .into(),
                            TEXT,
                        )],
                    ))
                    .child(button(
                        "tier-minimum",
                        "Минимум тира",
                        ButtonKind::Secondary,
                        face,
                        cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            let minimums = std::array::from_fn(|index| {
                                if view.checked[index] {
                                    STATS[index].tier_range().0
                                } else {
                                    view.minimums[index]
                                }
                            });
                            view.set_minimums(minimums);
                            cx.notify();
                        }),
                    )),
            )
    }

    fn render_stats(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        let rows = |kind: Kind| {
            STATS
                .iter()
                .enumerate()
                .filter(move |(_, stat)| stat.kind == kind)
                .map(|(index, stat)| self.render_stat(index, stat, face, cx))
                .collect::<Vec<_>>()
        };
        div()
            .flex()
            .flex_col()
            .gap(rems_from_px(4.))
            .child(section(face, "Собственные свойства"))
            .children(rows(Kind::Implicit))
            .child(section(face, "Префиксы · 1"))
            .children(rows(Kind::Prefix))
            .child(self.render_free_prefixes(cx))
            .child(section(face, "Суффиксы · 3"))
            .children(rows(Kind::Suffix))
    }

    /// One stat: checkbox, tier and text (all toggle it), min/max on the right, and the roll
    /// slider under the text (grill 6, B6).
    fn render_stat(
        &self,
        index: usize,
        stat: &'static Stat,
        face: &'static NameFont,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let checked = self.checked[index];
        let value = format_number(stat.roll);
        let text: SharedString = format!("{}{value}{}", stat.before, stat.after).into();
        let value_range = stat.before.len()..stat.before.len() + value.len();
        let minimum = checked.then(|| format_number(self.minimums[index]));
        let row = div()
            .id(("stat", index))
            .flex()
            .flex_col()
            .gap(rems_from_px(6.))
            .px(rems_from_px(6.))
            .py(rems_from_px(7.))
            .rounded(rems_from_px(4.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                    view.checked[index] = !view.checked[index];
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(rems_from_px(8.))
                    .child(div().pt(rems_from_px(2.)).child(checkbox("check", checked)))
                    .children(stat.tier.map(|(tier, count)| {
                        let (low, high) = stat.tier_range();
                        let (all_low, all_high) = stat.range();
                        div()
                            .id("tier")
                            .flex_none()
                            .pt(rems_from_px(2.))
                            .tooltip(game_hint(
                                face,
                                None,
                                vec![
                                    (format!("Тир {tier} из {count}").into(), GOLD_LIGHT),
                                    (
                                        format!(
                                            "Этот тир: {}–{}",
                                            format_number(low),
                                            format_number(high)
                                        )
                                        .into(),
                                        TEXT,
                                    ),
                                    (
                                        format!(
                                            "Все тиры: {}–{}",
                                            format_number(all_low),
                                            format_number(all_high)
                                        )
                                        .into(),
                                        TEXT_DIM,
                                    ),
                                    (
                                        format!("Нужен уровень предмета {}", stat.tier_level)
                                            .into(),
                                        TEXT_DIM,
                                    ),
                                ],
                            ))
                            .child(tier_badge(tier))
                    }))
                    .child(ease_state(
                        "text",
                        checked,
                        div().flex_1().min_w_0(),
                        move |line, on| {
                            line.text_color(rgb(blend(TEXT_DIM, TEXT, on))).child(
                                StyledText::new(text.clone()).with_highlights([(
                                    value_range.clone(),
                                    HighlightStyle {
                                        color: Some(rgb(TEXT_VALUE).into()),
                                        font_weight: Some(FontWeight::BOLD),
                                        ..Default::default()
                                    },
                                )]),
                            )
                        },
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .gap(px(1.))
                            .child(bound_box("min", minimum, "мин"))
                            .child(bound_box("max", None, "макс")),
                    ),
            )
            .child(
                div()
                    .pl(rems_from_px(CHECK_COLUMN))
                    .child(self.roll_slider(index, stat, checked)),
            );
        ease_hover(("stat", index), row, |row, hover| {
            row.bg(alpha(GOLD, 0.04 * hover))
        })
    }

    /// The roll slider (grill 6, B6): every tier of the stat along one track, cut where each tier
    /// starts, the item's own tier a warmer stretch of it, a bright notch at its own roll, and the
    /// search minimum as a thumb with the searched range right of it filled gold. The thumb
    /// slides when the minimum moves; an unchecked stat's slider dims and drops the thumb, easing
    /// either way.
    fn roll_slider(&self, index: usize, stat: &'static Stat, checked: bool) -> impl IntoElement {
        let (from, to) = (
            stat.fraction(self.thumbs_from[index]),
            stat.fraction(self.minimums[index]),
        );
        let moves = self.thumb_moves;
        let track_top = (SLIDER_HEIGHT - SLIDER_TRACK) / 2.;
        let track = move || {
            div()
                .absolute()
                .top(rems_from_px(track_top))
                .h(rems_from_px(SLIDER_TRACK))
                .rounded_full()
        };
        let slider = div().relative().h(rems_from_px(SLIDER_HEIGHT));
        ease_state("slider", checked, slider, move |slider, on| {
            let (tier_low, tier_high) = stat.tier_range();
            slider
                .opacity(0.45 + 0.55 * on)
                .child(track().left_0().right_0().bg(rgb(BORDER_FIELD)))
                .when(stat.tier.is_some(), |this| {
                    this.child(
                        track()
                            .left(relative(stat.fraction(tier_low)))
                            .right(relative(1. - stat.fraction(tier_high)))
                            .bg(rgb(blend(BORDER_FIELD, GOLD, 0.45))),
                    )
                })
                .child(
                    track()
                        .right_0()
                        .bg(alpha(GOLD, 0.55))
                        .opacity(on)
                        .with_animation(
                            ("searched", moves),
                            Animation::new(TRANSITION).with_easing(ease),
                            move |fill, t| fill.left(relative(from + (to - from) * t)),
                        ),
                )
                .children(stat.ladder.iter().skip(1).map(|&(start, _)| {
                    div()
                        .absolute()
                        .top(rems_from_px(track_top - 1.))
                        .h(rems_from_px(SLIDER_TRACK + 2.))
                        .left(relative(stat.fraction(start)))
                        .ml(px(-1.))
                        .w(px(2.))
                        .bg(rgb(BG_PANEL))
                }))
                .child(
                    div()
                        .absolute()
                        .top(rems_from_px((SLIDER_HEIGHT - 9.) / 2.))
                        .left(relative(stat.fraction(stat.roll)))
                        .ml(px(-1.))
                        .w(px(2.))
                        .h(rems_from_px(9.))
                        .rounded_full()
                        .bg(rgb(GOLD_LIGHT)),
                )
                .child(
                    div()
                        .absolute()
                        .top(rems_from_px((SLIDER_HEIGHT - 10.) / 2.))
                        .ml(rems_from_px(-5.))
                        .size(rems_from_px(10.))
                        .rounded_full()
                        .bg(rgb(BG_PANEL))
                        .border_1()
                        .border_color(rgb(GOLD_LIGHT))
                        .shadow(glow(GOLD, 0.7 * on))
                        .opacity(on)
                        .with_animation(
                            ("thumb", moves),
                            Animation::new(TRANSITION).with_easing(ease),
                            move |thumb, t| thumb.left(relative(from + (to - from) * t)),
                        ),
                )
        })
    }

    /// The free prefix slots, searchable as "at least this many empty prefixes".
    fn render_free_prefixes(&self, cx: &Context<Self>) -> impl IntoElement {
        let row = div()
            .id("free-prefixes")
            .flex()
            .items_center()
            .gap(rems_from_px(8.))
            .px(rems_from_px(6.))
            .py(rems_from_px(7.))
            .rounded(rems_from_px(4.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _: &MouseDownEvent, _, cx| {
                    view.free_prefixes = !view.free_prefixes;
                    cx.notify();
                }),
            )
            .child(checkbox("check", self.free_prefixes))
            .child(
                div()
                    .italic()
                    .text_color(rgb(TEXT_DIM))
                    .child("Свободных префиксов: 2"),
            );
        ease_hover("free-prefixes", row, |row, hover| {
            row.bg(alpha(GOLD, 0.04 * hover))
        })
    }

    fn render_search(&self, face: &'static NameFont) -> impl IntoElement {
        let search = div()
            .id("search")
            .flex()
            .items_center()
            .justify_center()
            .mt(rems_from_px(12.))
            .h(rems_from_px(36.))
            .rounded(rems_from_px(4.))
            .border_1()
            .font_family(face.family)
            .font_weight(face.weight)
            .text_size(rems_from_px(16.))
            .text_color(rgb(GOLD_LIGHT))
            .cursor_pointer()
            .child("Поиск");
        let quiet = |_: &MouseDownEvent, _: &mut Window, _: &mut App| {};
        div()
            .flex()
            .flex_col()
            .child(ease_hover("search", search, |search, hover| {
                search
                    .bg(plate(hover))
                    .border_color(rgb(blend(GOLD, GOLD_LIGHT, hover)))
                    .shadow(glow(GOLD, hover))
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(rems_from_px(6.))
                    .mt(rems_from_px(8.))
                    .text_size(rems_from_px(12.))
                    .child(div().text_color(rgb(TEXT_DIM)).child("Продавцы:"))
                    .child(select("sellers", "мгновенный выкуп", true, quiet))
                    .child(
                        div()
                            .ml(rems_from_px(6.))
                            .text_color(rgb(TEXT_DIM))
                            .child("Цена:"),
                    )
                    .child(select(
                        "currency",
                        div()
                            .flex()
                            .items_center()
                            .gap(rems_from_px(3.))
                            .child(currency_icon(Currency::Exalted, 14.))
                            .child("или")
                            .child(currency_icon(Currency::Divine, 14.)),
                        true,
                        quiet,
                    )),
            )
    }

    fn render_results(&self, cx: &Context<Self>) -> impl IntoElement {
        let quiet = |_: &MouseDownEvent, _: &mut Window, _: &mut App| {};
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(rems_from_px(8.))
            .pb(rems_from_px(6.))
            .child(
                div()
                    .flex()
                    .gap(rems_from_px(4.))
                    .child(div().text_color(rgb(TEXT_DIM)).child("Найдено:"))
                    .child("214"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(rems_from_px(14.))
                    .child(
                        div()
                            .id("watch")
                            .flex()
                            .items_center()
                            .gap(rems_from_px(6.))
                            .cursor_pointer()
                            .text_size(rems_from_px(12.))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|view, _: &MouseDownEvent, _, cx| {
                                    view.watching = !view.watching;
                                    cx.notify();
                                }),
                            )
                            .child(switch("watch-switch", self.watching))
                            .child("Следить"),
                    )
                    .child(link("trade", "ru.pathofexile.com/trade ↗", quiet)),
            );
        let columns = table_row()
            .h(rems_from_px(22.))
            .border_b_1()
            .border_color(rgb(BORDER_GOLD))
            .text_size(rems_from_px(11.))
            .text_color(rgb(TEXT_MUTED))
            .child(price_cell().child("Цена"))
            .child(level_cell().child("Ур."))
            .child(seller_cell().child("Продавец"))
            .child(listed_cell().child("Выставлен"));
        div()
            .flex()
            .flex_col()
            .pt(rems_from_px(12.))
            .child(header)
            .child(columns)
            .children(LISTINGS.iter().enumerate().map(|(index, listing)| {
                let row = table_row()
                    .id(("listing", index))
                    .h(rems_from_px(30.))
                    .when(index + 1 < LISTINGS.len(), |this| {
                        this.border_b_1().border_color(rgb(BORDER_ROW))
                    })
                    .cursor_pointer()
                    .child(render_price(listing))
                    .child(
                        level_cell()
                            .text_color(rgb(TEXT_DIM))
                            .child(listing.level.to_string()),
                    )
                    .child(
                        seller_cell()
                            .text_size(rems_from_px(12.))
                            .text_color(rgb(TEXT_DIM))
                            .child(listing.seller),
                    )
                    .child(
                        listed_cell()
                            .text_size(rems_from_px(12.))
                            .text_color(rgb(TEXT_DIM))
                            .child(listing.listed),
                    );
                ease_hover(("listing", index), row, |row, hover| {
                    row.bg(alpha(GOLD, 0.07 * hover)).shadow(inner_glow(hover))
                })
            }))
    }
}

impl Focusable for PanelMockup {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PanelMockup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let face = fonts::name_font(TradeSite::Russian);
        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    // Plays the appearance again, as a new price check would.
                    "f5" => view.appearances += 1,
                    "escape" if view.league_menu || view.profile_menu => {
                        view.league_menu = false;
                        view.profile_menu = false;
                    }
                    "escape" => {
                        window.remove_window();
                        return;
                    }
                    _ => return,
                }
                cx.notify();
            }))
            .size_full()
            .child(appear(
                ("appear", self.appearances),
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .bg(rgb(BG_PANEL))
                    .text_color(rgb(TEXT))
                    .text_size(rems_from_px(14.))
                    .line_height(relative(1.35))
                    .child(self.render_title_bar(cx))
                    .child(
                        div()
                            .id("panel-scroll")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(self.render_nameplate(face))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .px(rems_from_px(12.))
                                    .pb(rems_from_px(14.))
                                    .child(self.render_chips(cx))
                                    .child(self.render_toolbar(face, cx))
                                    .child(self.render_stats(face, cx))
                                    .child(self.render_search(face))
                                    .child(
                                        div()
                                            .pt(rems_from_px(14.))
                                            .child(ornament_rule(BORDER_GOLD)),
                                    )
                                    .child(self.render_results(cx)),
                            ),
                    )
                    .child(game_frame()),
            ))
    }
}

/// A heading between the stat groups.
fn section(face: &NameFont, title: &str) -> impl IntoElement {
    div()
        .pt(rems_from_px(10.))
        .pb(rems_from_px(2.))
        .child(section_heading(face, title))
}

/// EE2's tier badge: filled for the top tier, outlined for the second, neutral below.
fn tier_badge(tier: u32) -> impl IntoElement {
    div()
        .flex_none()
        .px(rems_from_px(4.))
        .rounded(rems_from_px(3.))
        .border_1()
        .text_size(rems_from_px(11.))
        .line_height(rems_from_px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .map(|this| match tier {
            1 => this
                .bg(rgb(TIER_TOP))
                .border_color(rgb(TIER_TOP))
                .text_color(rgb(BADGE_INK)),
            2 => this.border_color(rgb(TIER_TOP)).text_color(rgb(TIER_TOP)),
            _ => this
                .border_color(rgb(BORDER_FIELD))
                .text_color(rgb(TEXT_DIM)),
        })
        .child(format!("T{tier}"))
}

/// A min or max box: the number, or its placeholder dimmed; its edge warms to gold under the
/// pointer. Pressing it doesn't toggle the row.
fn bound_box(
    key: &'static str,
    value: Option<String>,
    placeholder: &'static str,
) -> impl IntoElement {
    let element = div()
        .id(key)
        .flex()
        .items_center()
        .justify_center()
        .w(rems_from_px(46.))
        .h(rems_from_px(24.))
        .bg(rgb(BG_FIELD))
        .border_1()
        .map(|this| {
            if key == "min" {
                this.rounded_l(rems_from_px(4.))
            } else {
                this.rounded_r(rems_from_px(4.))
            }
        })
        .text_size(rems_from_px(13.))
        .cursor_text()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(match value {
            Some(value) => div().text_color(rgb(TEXT)).child(value),
            None => div()
                .text_size(rems_from_px(12.))
                .text_color(rgb(TEXT_MUTED))
                .child(placeholder),
        });
    ease_hover(key, element, |element, hover| {
        element
            .border_color(rgb(blend(BORDER_FIELD, GOLD, 0.7 * hover)))
            .shadow(glow(GOLD, 0.4 * hover))
    })
}

/// Amount and currency icon, EE2's markers -- `× N` for a seller listing it N times at this
/// price, `?` for a price taken from the stash tab's name -- and, for anything but exalts and
/// divines, what it's worth in exalts.
fn render_price(listing: &Listing) -> impl IntoElement {
    price_cell()
        .flex()
        .items_center()
        .gap(rems_from_px(4.))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(listing.amount),
        )
        .child(currency_icon(listing.currency, 20.))
        .children(listing.repeats.map(|repeats| {
            div()
                .flex_none()
                .px(rems_from_px(4.))
                .rounded(rems_from_px(3.))
                .bg(alpha(GOLD, 0.18))
                .text_size(rems_from_px(11.))
                .text_color(rgb(GOLD_LIGHT))
                .child(format!("× {repeats}"))
        }))
        .when(listing.tab_price, |this| {
            this.child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(TEXT_MUTED))
                    .child("?"),
            )
        })
        .children(listing.in_exalted.map(|value| {
            div()
                .flex()
                .items_center()
                .gap(rems_from_px(3.))
                .text_size(rems_from_px(12.))
                .text_color(rgb(TEXT_MUTED))
                .child(format!("≈ {value}"))
                .child(currency_icon(Currency::Exalted, 14.))
        }))
}

fn currency_icon(currency: Currency, size: f32) -> impl IntoElement {
    img(currency.icon()).flex_none().size(rems_from_px(size))
}

fn table_row() -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(rems_from_px(8.))
        .px(rems_from_px(8.))
}

fn price_cell() -> gpui::Div {
    div().w(rems_from_px(PRICE_COLUMN)).flex_none()
}

fn level_cell() -> gpui::Div {
    div().w(rems_from_px(LEVEL_COLUMN)).flex_none().text_right()
}

fn seller_cell() -> gpui::Div {
    div().flex_1().min_w_0().truncate()
}

fn listed_cell() -> gpui::Div {
    div()
        .w(rems_from_px(LISTED_COLUMN))
        .flex_none()
        .text_right()
        .truncate()
}

/// A roll or bound: whole numbers without decimals.
fn format_number(value: f32) -> String {
    if value.fract() == 0. {
        format!("{value:.0}")
    } else {
        format!("{value:.1}").replace('.', ",")
    }
}
