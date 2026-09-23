//! The stat filter rows, in the game tooltip's own sections: each row's text (the stat with its
//! rolled value, tier and source badge), controls (checkbox, min/max inputs) and, for a mod the
//! tier table knows, its roll slider; and the toggle that unfolds the rows kept out of sight.

use std::ops::Range;

use gpui::{
    AnyElement, Bounds, Context, CursorStyle, DispatchPhase, FocusHandle, FontWeight,
    HighlightStyle, HitboxBehavior, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, StyledText, Window, canvas, div, fill, point, prelude::*,
    px, rgb, size,
};

use poe2_domain::{ModGeneration, ParsedItem};
use stat_filters::{FilterTag, SearchFilter, TierInfo};
use trade_client::TradeSite;

use crate::platform::win32::Win32Overlay;
use crate::price_check::{FilterRowUi, PriceCheckApp};
use crate::roll_slider::{Handle, Slider};
use crate::settings::WaystoneMark;
use crate::ui::hint as hints;
use crate::ui::theme::{
    BADGE_DESECRATED_BG, BADGE_DESECRATED_TEXT, BADGE_ENCHANT_BG, BADGE_ENCHANT_TEXT,
    BADGE_FRACTURED_BG, BADGE_INK, BADGE_RUNE_BG, BADGE_RUNE_TEXT, BG_BUTTON_HOVER, BG_CONTROL,
    BG_PANEL, BORDER, BORDER_GOLD, GOLD, TEXT, TEXT_DIM, TEXT_MUTED, TEXT_VALUE, TIER_TOP,
    rems_from_px,
};

use super::format::format_value;
use super::waystone::{is_waystone, mark_color, render_mark_button, waystone_mark_of};

/// Indent of a filter row's second line: the checkbox plus the gap after it.
const CHECK_COLUMN: f32 = CHECKBOX + 6.;
const CHECKBOX: f32 = 14.;
const BOUND_INPUT_WIDTH: f32 = 52.;
const BOUND_INPUT_HEIGHT: f32 = 22.;
/// A roll slider's track: its height, its handle's width and height, its rail's thickness and
/// the width of the tick at the item's own roll.
const SLIDER_HEIGHT: f32 = 14.;
const SLIDER_HANDLE_WIDTH: f32 = 8.;
const SLIDER_HANDLE_HEIGHT: f32 = 10.;
const SLIDER_RAIL: f32 = 2.;
const SLIDER_MARK: f32 = 2.;

/// The panel's sections, in the game tooltip's own order: base properties, implicits, the prefix
/// and suffix slots (tiers inline, free slots included), anything else (enchants, runes, a
/// unique's fixed mods), then the pseudo totals -- folded, since they repeat what's above.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Properties,
    Implicits,
    Prefixes,
    Suffixes,
    Other,
    Totals,
}

fn section_of(filter: &SearchFilter) -> Section {
    match (filter.tag, filter.generation) {
        (FilterTag::Property, _) => Section::Properties,
        (FilterTag::Implicit, _) => Section::Implicits,
        (FilterTag::Pseudo, _) => Section::Totals,
        (_, Some(ModGeneration::Prefix)) => Section::Prefixes,
        (_, Some(ModGeneration::Suffix)) => Section::Suffixes,
        _ => Section::Other,
    }
}

/// Whether a row starts folded away: the pseudo totals the search doesn't use, and the rows EE2
/// hides outright (minor DPS shares). Everything the search does use stays in sight.
fn is_folded(filter: &SearchFilter) -> bool {
    match section_of(filter) {
        Section::Totals => !filter.enabled,
        Section::Properties => filter.hidden,
        _ => false,
    }
}

pub(super) fn render_sections(
    state: &PriceCheckApp,
    item: &ParsedItem,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let site = state.trade_site();
    let waystone = is_waystone(item);
    let generation_count = |generation| {
        item.mods
            .iter()
            .filter(|modifier| modifier.info.generation == Some(generation))
            .count()
    };
    let has_affixes = state
        .filters
        .iter()
        .any(|filter| filter.generation.is_some());
    let sections = [
        (Section::Properties, "Свойства предмета".to_owned()),
        (Section::Implicits, "Собственные свойства".to_owned()),
        (
            Section::Prefixes,
            format!("Префиксы · {}", generation_count(ModGeneration::Prefix)),
        ),
        (
            Section::Suffixes,
            format!("Суффиксы · {}", generation_count(ModGeneration::Suffix)),
        ),
        (
            Section::Other,
            if has_affixes {
                "Прочие свойства"
            } else {
                "Свойства"
            }
            .to_owned(),
        ),
        (Section::Totals, "Суммарные (псевдо)".to_owned()),
    ];
    let folded = state
        .filters
        .iter()
        .filter(|filter| is_folded(filter))
        .count();

    div()
        .flex()
        .flex_col()
        .children(sections.into_iter().filter_map(|(section, title)| {
            let rows: Vec<AnyElement> = state
                .filters
                .iter()
                .zip(&state.filter_ui)
                .enumerate()
                .filter(|(_, (filter, _))| {
                    section_of(filter) == section && (state.show_hidden || !is_folded(filter))
                })
                .map(|(row, (filter, ui))| {
                    let mark = waystone.then(|| waystone_mark_of(state, filter)).flatten();
                    render_filter_row(row, filter, ui, site, mark, window, cx).into_any_element()
                })
                .collect();
            (!rows.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .child(section_header(title))
                    .children(rows)
            })
        }))
        .when(folded > 0, |this| {
            this.child(render_hidden_toggle(folded, state.show_hidden, cx))
        })
}

fn section_header(title: String) -> impl IntoElement {
    div()
        .pt(rems_from_px(10.))
        .pb(rems_from_px(3.))
        .border_b_1()
        .border_color(rgb(BORDER_GOLD))
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(GOLD))
        .child(title.to_uppercase())
}

/// Unfolds/folds the rows `is_folded` keeps out of sight -- PoE Overlay II's "show N hidden
/// mods", EE2's "Hidden" toggle.
fn render_hidden_toggle(
    count: usize,
    shown: bool,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let label = if shown {
        format!("▴ Свернуть суммарные и второстепенные ({count})")
    } else {
        format!("▾ Суммарные и второстепенные: ещё {count}")
    };
    div()
        .flex()
        .justify_center()
        .py(rems_from_px(5.))
        .text_xs()
        .text_color(rgb(TEXT_DIM))
        .cursor_pointer()
        .hover(|style| style.text_color(rgb(GOLD)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                view.toggle_show_hidden(cx);
            }),
        )
        .child(label)
}

/// One stat: checkbox, tier and text (all toggle the filter, as in EE2), min/max inputs on the
/// right, and -- only for the kinds worth flagging (fractured, desecrated, crafted, rune,
/// enchant, a weighted sum) -- a badge underneath; the section already says
/// prefix/suffix/implicit. The tier badge says on hover where the tier sits among its family's
/// (`tier_hint`), and a mod the tier table knows gets its roll slider under the text
/// (`render_roll_slider`). A waystone's own modifiers also carry the player's mark (`mark`: its
/// key and current mark), in the mark's colour.
fn render_filter_row(
    row: usize,
    filter: &SearchFilter,
    ui: &FilterRowUi,
    site: TradeSite,
    mark: Option<(String, Option<WaystoneMark>)>,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    // A row the search can't use is shown for information only, without the controls that
    // would suggest otherwise.
    let searchable = filter.searchable();
    let marked_color = mark.as_ref().and_then(|(_, mark)| *mark).map(mark_color);
    let text: AnyElement = if filter.tag == FilterTag::EmptyAffix {
        div()
            .italic()
            .text_color(rgb(TEXT_DIM))
            .child(free_slot_text(filter))
            .into_any_element()
    } else {
        div()
            .text_color(rgb(marked_color.unwrap_or(if searchable {
                TEXT
            } else {
                TEXT_DIM
            })))
            .child(stat_line(filter, site))
            .into_any_element()
    };
    let badge = source_badge(filter.tag);
    let slider = searchable.then(|| Slider::of(filter)).flatten();

    let toggle_area = div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_start()
        .gap(rems_from_px(6.))
        .when(searchable, |this| {
            this.cursor_pointer().on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                    view.toggle_filter(row, cx);
                }),
            )
        })
        .child(
            div()
                .flex_none()
                .w(rems_from_px(CHECKBOX))
                .pt(rems_from_px(3.))
                .when(searchable, |this| {
                    this.child(render_checkbox(filter.enabled))
                }),
        )
        .children(filter.tier.map(|tier| {
            div()
                .id(("tier", row))
                .flex_none()
                .pt(rems_from_px(2.))
                .when_some(filter.tier_info.as_ref(), |this, info| {
                    this.tooltip(hints::hint(tier_hint(info)))
                })
                .child(render_tier(tier))
        }))
        .child(div().flex_1().min_w_0().child(text))
        .children(mark.map(|(key, mark)| render_mark_button(key, mark, cx)));

    div()
        .flex()
        .flex_col()
        .gap(rems_from_px(2.))
        .py(rems_from_px(5.))
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .flex()
                .items_start()
                .gap(rems_from_px(8.))
                .child(toggle_area)
                .when(searchable && filter.roll.is_some(), |this| {
                    this.child(render_bounds(row, ui, window, cx))
                }),
        )
        .children(slider.map(|slider| render_roll_slider(row, filter, ui, slider, cx)))
        .when(
            badge.is_some() || filter.weighted_sum || !searchable,
            |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(rems_from_px(6.))
                        .pl(rems_from_px(CHECK_COLUMN))
                        .children(badge)
                        .when(filter.weighted_sum, |this| {
                            this.child(weighted_sum_badge(row))
                        })
                        .when(!searchable, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child("не участвует в поиске"),
                            )
                        }),
                )
            },
        )
}

/// The tier badge's hint, from the tier table: «T3 из 9 · с 68 ур. предмета · лучший доступный
/// T2» -- the tier among its family's on this kind of item, the item level it needs, and the best
/// one the item's level lets roll.
fn tier_hint(info: &TierInfo) -> String {
    format!(
        "T{} из {} · с {} ур. предмета · лучший доступный T{}",
        info.current, info.count, info.min_level, info.best_available
    )
}

/// A weighted sum's badge: its value adds up the same stat from several of the item's mods, as
/// the trade site sums them for the search -- a total, like the site's own pseudo rows.
fn weighted_sum_badge(row: usize) -> impl IntoElement {
    div()
        .id(("weighted-sum", row))
        .flex_none()
        .px(rems_from_px(4.))
        .rounded_xs()
        .border_1()
        .border_color(rgb(BORDER_GOLD))
        .text_xs()
        .line_height(rems_from_px(13.))
        .text_color(rgb(GOLD))
        .tooltip(hints::hint(
            "Сумма: значение сложено из всех модификаторов вещи с этим свойством, и сайт \
             торговли ищет по такой же сумме у лотов.",
        ))
        .child("сумма")
}

/// A mod row's roll slider (`roll_slider::Slider`): the lowest roll of the row's family on this
/// kind of item at the left end, the highest at the right, a blue tick at the item's own roll,
/// and a handle at the row's search bound -- its minimum box, or its maximum where a lower roll is
/// better -- with the part of the track the search admits lit in gold. Pressing the track puts
/// the handle there and dragging moves it (`PriceCheckApp::begin_roll_drag`); the bound box
/// follows, and typing in the box moves the handle. Like a typed bound, it takes effect with the
/// next search.
fn render_roll_slider(
    row: usize,
    filter: &SearchFilter,
    ui: &FilterRowUi,
    slider: Slider,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let bound = match slider.handle {
        Handle::Min => &ui.min_text,
        Handle::Max => &ui.max_text,
    };
    let handle = slider.handle_fraction(bound.parse::<f64>().ok());
    let mark = filter.roll.as_ref().map(|roll| slider.fraction(roll.value));
    let enabled = filter.enabled;
    let view = cx.entity();
    let track = canvas(
        |bounds, window, _cx| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _cx| {
            let rem_size = window.rem_size();
            let handle_width = rems_from_px(SLIDER_HANDLE_WIDTH).to_pixels(rem_size);
            // The handle's centre travels between the track's ends, so it never overhangs them.
            let left = bounds.left() + handle_width / 2.;
            let width = (bounds.size.width - handle_width).max(px(1.));
            let x_at = move |fraction: f64| left + width * fraction as f32;
            let fraction_at = move |x: Pixels| f64::from((x - left) / width);
            let middle = bounds.center().y;
            let bar = |from: f64, to: f64, color: u32| {
                let thickness = rems_from_px(SLIDER_RAIL).to_pixels(rem_size);
                fill(
                    Bounds::new(
                        point(x_at(from), middle - thickness / 2.),
                        size(x_at(to) - x_at(from), thickness),
                    ),
                    rgb(color),
                )
            };
            window.paint_quad(bar(0., 1., BORDER));
            let admitted = match slider.handle {
                Handle::Min => (handle, 1.),
                Handle::Max => (0., handle),
            };
            window.paint_quad(bar(
                admitted.0,
                admitted.1,
                if enabled { BORDER_GOLD } else { TEXT_MUTED },
            ));
            let handle_height = rems_from_px(SLIDER_HANDLE_HEIGHT).to_pixels(rem_size);
            window.paint_quad(
                fill(
                    Bounds::new(
                        point(
                            x_at(handle) - handle_width / 2.,
                            middle - handle_height / 2.,
                        ),
                        size(handle_width, handle_height),
                    ),
                    rgb(if enabled { GOLD } else { TEXT_DIM }),
                )
                .corner_radii(rems_from_px(2.).to_pixels(rem_size)),
            );
            // Over the handle, so the item's own roll shows even where the handle sits on it.
            if let Some(mark) = mark {
                let mark_width = rems_from_px(SLIDER_MARK).to_pixels(rem_size);
                window.paint_quad(fill(
                    Bounds::new(
                        point(x_at(mark) - mark_width / 2., bounds.top()),
                        size(mark_width, bounds.size.height),
                    ),
                    rgb(TEXT_VALUE),
                ));
            }

            window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
            window.on_mouse_event({
                let view = view.clone();
                move |event: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && hitbox.is_hovered(window)
                    {
                        let fraction = fraction_at(event.position.x);
                        view.update(cx, |state, cx| state.begin_roll_drag(row, fraction, cx));
                        cx.stop_propagation();
                    }
                }
            });
            window.on_mouse_event({
                let view = view.clone();
                move |event: &MouseMoveEvent, phase, _window, cx| {
                    if phase != DispatchPhase::Capture || view.read(cx).roll_drag != Some(row) {
                        return;
                    }
                    // A panel the game keeps in front of can't capture the mouse (only the
                    // foreground window can): a release outside it never arrives, and the next
                    // move without the button ends the drag instead.
                    if event.pressed_button == Some(MouseButton::Left) {
                        let fraction = fraction_at(event.position.x);
                        view.update(cx, |state, cx| state.slide_roll(row, fraction, cx));
                    } else {
                        view.update(cx, |state, cx| state.end_roll_drag(cx));
                    }
                }
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _window, cx| {
                if phase == DispatchPhase::Capture
                    && event.button == MouseButton::Left
                    && view.read(cx).roll_drag == Some(row)
                {
                    view.update(cx, |state, cx| state.end_roll_drag(cx));
                }
            });
        },
    )
    .flex_1()
    .h(rems_from_px(SLIDER_HEIGHT));
    div()
        .id(("roll-slider", row))
        .flex()
        .items_center()
        .gap(rems_from_px(6.))
        .pl(rems_from_px(CHECK_COLUMN))
        .text_xs()
        .text_color(rgb(TEXT_MUTED))
        .tooltip(hints::hint(match slider.handle {
            Handle::Min => {
                "Края — самое низкое и самое высокое значение этого свойства во всех тирах, \
                 голубая метка — значение этой вещи, бегунок — минимум поиска."
            }
            Handle::Max => {
                "Края — самое низкое и самое высокое значение этого свойства во всех тирах, \
                 голубая метка — значение этой вещи, бегунок — максимум поиска: здесь чем \
                 меньше, тем лучше."
            }
        }))
        .child(div().flex_none().child(format_value(slider.low)))
        .child(track)
        .child(div().flex_none().child(format_value(slider.high)))
}

/// A free affix slot row -- searchable as "at least this many empty prefixes/suffixes".
fn free_slot_text(filter: &SearchFilter) -> String {
    let count = filter.roll.as_ref().map_or(0.0, |roll| roll.value) as u32;
    match (filter.generation, count) {
        (Some(ModGeneration::Prefix), 1) => "Свободный префикс".to_owned(),
        (Some(ModGeneration::Prefix), count) => format!("Свободных префиксов: {count}"),
        (_, 1) => "Свободный суффикс".to_owned(),
        (_, count) => format!("Свободных суффиксов: {count}"),
    }
}

/// The stat text with its rolled value written in, and where the value sits -- EE2's
/// `ItemModifierText`: the template's `#` (with any `+`/`-` right before it, the value carries
/// its own sign) becomes the value. A template with several (`Adds # to #`, rolled at the average
/// of its numbers) gets `: ≈value` appended; one with none already spells out its fixed numbers.
pub(super) fn stat_text(filter: &SearchFilter, site: TradeSite) -> (String, Option<Range<usize>>) {
    let template = display_template(filter, site);
    let Some(roll) = &filter.roll else {
        return (template.to_owned(), None);
    };
    let value = format_value(roll.value);
    let pieces: Vec<&str> = template.split('#').collect();
    let mut text = String::with_capacity(template.len() + value.len() + 4);
    let value_range = match pieces.len() {
        1 => return (template.to_owned(), None),
        2 => {
            text.push_str(pieces[0].trim_end_matches(['+', '-']));
            let start = text.len();
            text.push_str(&value);
            let range = start..text.len();
            text.push_str(pieces[1]);
            range
        }
        _ => {
            text.push_str(template);
            text.push_str(": ≈");
            let start = text.len();
            text.push_str(&value);
            start..text.len()
        }
    };
    (text, Some(value_range))
}

/// `stat_text` with the value highlighted.
fn stat_line(filter: &SearchFilter, site: TradeSite) -> StyledText {
    let (text, value_range) = stat_text(filter, site);
    let highlight = HighlightStyle {
        color: Some(rgb(TEXT_VALUE).into()),
        font_weight: Some(FontWeight::BOLD),
        ..Default::default()
    };
    match value_range {
        Some(range) => StyledText::new(text).with_highlights([(range, highlight)]),
        None => StyledText::new(text),
    }
}

/// `filter.display_text`, except the base-property rows `stat_filters` labels in English: those
/// read in the game's own Russian wording on the Russian site.
fn display_template(filter: &SearchFilter, site: TradeSite) -> &str {
    if filter.tag != FilterTag::Property || site != TradeSite::Russian {
        return &filter.display_text;
    }
    // The game's own Russian wording (`item-parser`'s client strings), and PoE Overlay II's
    // "УВС" for damage per second.
    match filter.display_text.as_str() {
        "Item Level: #" => "Уровень предмета: #",
        "Sockets: #" => "Гнёзда: #",
        "Quality: #%" => "Качество: #%",
        "Armour: #" => "Броня: #",
        "Evasion Rating: #" => "Уклонение: #",
        "Energy Shield: #" => "Энергетический щит: #",
        "Block: #%" => "Шанс блока: #%",
        "Runic Ward: #" => "Рунический барьер: #",
        "Total DPS: #" => "УВС: #",
        "Elemental DPS: #" => "Стихийный УВС: #",
        "Physical DPS: #" => "Физический УВС: #",
        "Attacks per Second: #" => "Атак в секунду: #",
        "Critical Hit Chance: #%" => "Шанс крит. попадания: #%",
        "Reload Time: #" => "Время перезарядки: #",
        "Spirit: #" => "Дух: #",
        "Gem Sockets: #" => "Гнёзда: #",
        "Gem Level: #" => "Уровень камня: #",
        "Area Level: #" => "Уровень области: #",
        "Waystone Tier: #" => "Уровень путевого камня: #",
        "Revives Available: #" => "Доступно возрождений: #",
        "Monster Pack Size: #%" => "Размер групп монстров: #%",
        "Magic Monsters: #%" => "Волшебные монстры: #%",
        "Rare Monsters: #%" => "Редкие монстры: #%",
        "Waystone Drop Chance: #%" => "Шанс выпадения путевого камня: #%",
        "Item Rarity: #%" => "Редкость предметов: #%",
        "Gold Found: #%" => "Найденное золото: #%",
        "Monster Rarity: #%" => "Редкость монстров: #%",
        "Monster Effectiveness: #%" => "Эффективность монстров: #%",
        other => other,
    }
}

fn render_checkbox(checked: bool) -> impl IntoElement {
    div()
        .w(rems_from_px(CHECKBOX))
        .h(rems_from_px(CHECKBOX))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_xs()
        .border_1()
        .text_size(rems_from_px(11.))
        .line_height(rems_from_px(CHECKBOX))
        .font_weight(FontWeight::BOLD)
        .map(|this| {
            if checked {
                this.bg(rgb(GOLD))
                    .border_color(rgb(GOLD))
                    .text_color(rgb(BG_PANEL))
                    .child("✓")
            } else {
                this.border_color(rgb(TEXT_MUTED))
            }
        })
}

/// EE2's coloured source badges (`FilterModifier.vue`'s `.tag-*` classes, Russian locale
/// wording) for the kinds that change what an item is worth; `None` for the plain ones the
/// section header already names.
fn source_badge(tag: FilterTag) -> Option<impl IntoElement> {
    let (label, bg, fg) = match tag {
        FilterTag::Rune => ("усилитель", BADGE_RUNE_BG, BADGE_RUNE_TEXT),
        FilterTag::Crafted => ("мастер", BADGE_RUNE_BG, BADGE_RUNE_TEXT),
        FilterTag::Fractured => ("расколотый", BADGE_FRACTURED_BG, BADGE_INK),
        FilterTag::Enchant => ("зачарование", BADGE_ENCHANT_BG, BADGE_ENCHANT_TEXT),
        FilterTag::Desecrated => ("очернённый", BADGE_DESECRATED_BG, BADGE_DESECRATED_TEXT),
        FilterTag::Explicit
        | FilterTag::Implicit
        | FilterTag::Property
        | FilterTag::Pseudo
        | FilterTag::EmptyAffix => return None,
    };
    Some(
        div()
            .flex_none()
            .px(rems_from_px(4.))
            .rounded_xs()
            .bg(rgb(bg))
            .text_xs()
            .line_height(rems_from_px(15.))
            .text_color(rgb(fg))
            .child(label),
    )
}

/// EE2's tier badge (`FilterModifierTiers.vue`): filled for the top tier, outlined for the
/// second, neutral below that.
fn render_tier(tier: u32) -> impl IntoElement {
    div()
        .flex_none()
        .px(rems_from_px(4.))
        .rounded_xs()
        .border_1()
        .text_xs()
        .line_height(rems_from_px(13.))
        .map(|this| match tier {
            1 => this
                .bg(rgb(TIER_TOP))
                .border_color(rgb(TIER_TOP))
                .text_color(rgb(BADGE_INK)),
            2 => this.border_color(rgb(TIER_TOP)).text_color(rgb(TIER_TOP)),
            _ => this.border_color(rgb(BORDER)).text_color(rgb(TEXT_DIM)),
        })
        .child(format!("T{tier}"))
}

fn render_bounds(
    row: usize,
    ui: &FilterRowUi,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .gap(px(1.))
        .child(render_bound_input(
            row,
            true,
            &ui.min_focus,
            &ui.min_text,
            ui.min_fresh,
            window,
            cx,
        ))
        .child(render_bound_input(
            row,
            false,
            &ui.max_focus,
            &ui.max_text,
            ui.max_fresh,
            window,
            cx,
        ))
}

/// One min/max box: shows `text` (or the "мин"/"макс" placeholder while empty), outlines itself
/// on hover so it reads as editable, takes focus on click so `PriceCheckApp::handle_filter_key`
/// receives the keystrokes, and while focused shows a caret after the value -- highlighted as
/// selected right after the click (`fresh`), when typing replaces it. A click never activates
/// the panel (see `app`'s overlay setup), so the box activates it: the keys must come here, not
/// to the game.
fn render_bound_input(
    row: usize,
    is_min: bool,
    focus_handle: &FocusHandle,
    text: &str,
    fresh: bool,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let focused = focus_handle.is_focused(window);
    let focus_for_click = focus_handle.clone();
    let placeholder = if is_min { "мин" } else { "макс" };

    div()
        .w(rems_from_px(BOUND_INPUT_WIDTH))
        .h(rems_from_px(BOUND_INPUT_HEIGHT))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .bg(rgb(BG_CONTROL))
        .border_1()
        .border_color(rgb(if focused { GOLD } else { BORDER }))
        .when(!focused, |this| {
            this.hover(|style| style.border_color(rgb(TEXT_DIM)))
        })
        .map(|this| {
            if is_min {
                this.rounded_l_xs()
            } else {
                this.rounded_r_xs()
            }
        })
        .cursor_text()
        .track_focus(focus_handle)
        .on_key_down(cx.listener(move |view, event: &KeyDownEvent, _window, cx| {
            view.handle_filter_key(row, is_min, event, cx);
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _event: &MouseDownEvent, window, cx| {
                if let Ok(overlay) = Win32Overlay::from_window(window) {
                    cx.spawn(async move |_, _| overlay.activate()).detach();
                }
                focus_for_click.focus(window, cx);
                view.begin_bound_edit(row, is_min, cx);
            }),
        )
        .map(|this| {
            if text.is_empty() && !focused {
                this.text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .child(placeholder)
            } else {
                this.text_color(rgb(TEXT))
                    .child(
                        div()
                            .px(rems_from_px(1.))
                            .when(fresh && !text.is_empty(), |this| {
                                this.bg(rgb(BG_BUTTON_HOVER))
                            })
                            .child(text.to_owned()),
                    )
                    .when(focused, |this| {
                        this.child(div().w(px(1.)).h(rems_from_px(14.)).bg(rgb(GOLD)))
                    })
            }
        })
}
