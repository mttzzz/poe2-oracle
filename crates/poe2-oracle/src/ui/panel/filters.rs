//! The stat filter rows, in the game tooltip's own sections under their diamond headings: each
//! row's text (the stat with its rolled value, tier and source badge), controls (checkbox, min/max
//! inputs) and, for a mod the tier table knows, its roll slider; and the toggle that unfolds the
//! rows kept out of sight.

use std::ops::Range;

use gpui::{
    AnyElement, BorderStyle, Bounds, Context, CursorStyle, DispatchPhase, FocusHandle, FontWeight,
    HighlightStyle, HitboxBehavior, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, StyledText, Window, canvas, div, fill, point, prelude::*,
    px, quad, rgb, size,
};

use poe2_domain::{ModGeneration, ParsedItem};
use stat_filters::{FilterTag, SearchFilter, TierInfo};
use trade_client::TradeSite;

use crate::platform::win32::Win32Overlay;
use crate::price_check::{FilterRowUi, PriceCheckApp};
use crate::roll_slider::{Handle, Slider};
use crate::tr;
use crate::ui::fonts;
use crate::ui::hint as hints;
use crate::ui::style::{
    alpha, checkbox, ease_hover, ease_state, ease_value, game_hint, glow, link, section_heading,
};
use crate::ui::theme::{
    BADGE_DESECRATED_BG, BADGE_DESECRATED_TEXT, BADGE_ENCHANT_BG, BADGE_ENCHANT_TEXT,
    BADGE_FRACTURED_BG, BADGE_INK, BADGE_RUNE_BG, BADGE_RUNE_TEXT, BG_FIELD, BG_PANEL,
    BORDER_FIELD, BORDER_GOLD, GOLD, GOLD_LIGHT, TEXT, TEXT_DIM, TEXT_MUTED, TEXT_VALUE, TIER_TOP,
    blend, rems_from_px,
};

use super::format::format_value;
use super::waystone::{is_waystone, mark_color, render_mark_button, waystone_mark_of};

/// Indent of a filter row's second line: the checkbox plus the gap after it.
const CHECK_COLUMN: f32 = CHECKBOX + ROW_GAP;
/// The checkbox's edge (`style::checkbox`).
const CHECKBOX: f32 = 15.;
const ROW_GAP: f32 = 8.;
const BOUND_INPUT_WIDTH: f32 = 52.;
const BOUND_INPUT_HEIGHT: f32 = 24.;
/// A roll slider: its height, its track's thickness, the thumb's edge and the height of the notch
/// at the item's own roll.
const SLIDER_HEIGHT: f32 = 12.;
const SLIDER_TRACK: f32 = 3.;
const SLIDER_THUMB: f32 = 10.;
const SLIDER_NOTCH: f32 = 9.;

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
        (Section::Properties, tr!("Item properties").to_owned()),
        (Section::Implicits, tr!("Implicit modifiers").to_owned()),
        (
            Section::Prefixes,
            tr!(
                "Prefixes · {count}",
                count = generation_count(ModGeneration::Prefix)
            ),
        ),
        (
            Section::Suffixes,
            tr!(
                "Suffixes · {count}",
                count = generation_count(ModGeneration::Suffix)
            ),
        ),
        (
            Section::Other,
            if has_affixes {
                tr!("Other modifiers")
            } else {
                tr!("Modifiers")
            }
            .to_owned(),
        ),
        (Section::Totals, tr!("Totals (pseudo)").to_owned()),
    ];
    let folded = state
        .filters
        .iter()
        .filter(|filter| is_folded(filter))
        .count();

    div()
        .flex()
        .flex_col()
        .gap(rems_from_px(4.))
        .children(sections.into_iter().filter_map(|(section, title)| {
            let rows: Vec<AnyElement> = state
                .filters
                .iter()
                .enumerate()
                .filter(|(_, filter)| {
                    section_of(filter) == section && (state.show_hidden || !is_folded(filter))
                })
                .map(|(row, _)| render_filter_row(state, row, waystone, window, cx))
                .collect();
            (!rows.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap(rems_from_px(4.))
                    .child(
                        div()
                            .pt(rems_from_px(10.))
                            .pb(rems_from_px(2.))
                            .child(section_heading(fonts::interface_font(), &title)),
                    )
                    .children(rows)
            })
        }))
        .when(folded > 0, |this| {
            this.child(render_hidden_toggle(folded, state.show_hidden, cx))
        })
}

/// Unfolds/folds the rows `is_folded` keeps out of sight -- PoE Overlay II's "show N hidden
/// mods", EE2's "Hidden" toggle.
fn render_hidden_toggle(
    count: usize,
    shown: bool,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let label = if shown {
        tr!("▴ Hide totals and minor rows ({count})", count = count)
    } else {
        tr!("▾ Totals and minor rows: {count} more", count = count)
    };
    div()
        .flex()
        .justify_center()
        .py(rems_from_px(5.))
        .child(link(
            "hidden-rows",
            label,
            cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                view.toggle_show_hidden(cx);
            }),
        ))
}

/// Row `row` of the panel's filters: checkbox, tier and text, min/max inputs on the right -- a
/// press anywhere else on the row toggles it, as in EE2, and the row lights up under the pointer
/// -- and -- only for the kinds worth flagging (fractured, desecrated, crafted, rune, enchant, a
/// weighted sum) -- a badge underneath; the section already says prefix/suffix/implicit. The text
/// dims while the row is out of the search. The tier badge says on hover where the tier sits
/// among its family's (`tier_hint`), and a mod the tier table knows gets its roll slider under the
/// text (`render_roll_slider`). A `waystone`'s own modifiers also carry the player's mark, in the
/// mark's colour.
fn render_filter_row(
    state: &PriceCheckApp,
    row: usize,
    waystone: bool,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> AnyElement {
    let (filter, ui) = (&state.filters[row], &state.filter_ui[row]);
    let site = state.trade_site();
    let mark = waystone.then(|| waystone_mark_of(state, filter)).flatten();
    // A row the search can't use is shown for information only, without the controls that
    // would suggest otherwise.
    let searchable = filter.searchable();
    let marked_color = mark.as_ref().and_then(|(_, mark)| *mark).map(mark_color);
    let text: AnyElement = if filter.tag == FilterTag::EmptyAffix {
        div()
            .flex_1()
            .min_w_0()
            .italic()
            .text_color(rgb(TEXT_DIM))
            .child(free_slot_text(filter))
            .into_any_element()
    } else {
        ease_state(
            "text",
            filter.enabled,
            div().flex_1().min_w_0().child(stat_line(filter, site)),
            move |line, on| {
                line.text_color(rgb(match marked_color {
                    Some(color) => color,
                    None if searchable => blend(TEXT_DIM, TEXT, on),
                    None => TEXT_DIM,
                }))
            },
        )
        .into_any_element()
    };
    let badge = source_badge(filter.tag);
    let slider = searchable.then(|| Slider::of(filter)).flatten();

    let line = div()
        .flex()
        .items_start()
        .gap(rems_from_px(ROW_GAP))
        .child(
            div()
                .flex_none()
                .w(rems_from_px(CHECKBOX))
                .pt(rems_from_px(2.))
                .when(searchable, |this| {
                    this.child(checkbox("check", filter.enabled))
                }),
        )
        .children(filter.tier.map(|tier| {
            div()
                .id("tier")
                .flex_none()
                .pt(rems_from_px(2.))
                .when_some(filter.tier_info.as_ref(), |this, info| {
                    this.tooltip(game_hint(fonts::interface_font(), None, tier_hint(info)))
                })
                .child(render_tier(tier))
        }))
        .child(text)
        .children(mark.map(|(key, mark)| render_mark_button(key, mark, cx)))
        .when(searchable && filter.roll.is_some(), |this| {
            this.child(render_bounds(row, ui, window, cx))
        });

    let element = div()
        .id(("filter", row))
        .flex()
        .flex_col()
        .gap(rems_from_px(6.))
        .px(rems_from_px(6.))
        .py(rems_from_px(7.))
        .rounded(rems_from_px(4.))
        .when(searchable, |this| {
            this.cursor_pointer().on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                    view.toggle_filter(row, cx);
                }),
            )
        })
        .child(line)
        .children(slider.map(|slider| {
            let dragging = state.roll_drag == Some(row);
            render_roll_slider(row, filter, ui, slider, dragging, cx)
        }))
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
                        .when(filter.weighted_sum, |this| this.child(weighted_sum_badge()))
                        .when(!searchable, |this| {
                            this.child(
                                div()
                                    .text_size(rems_from_px(12.))
                                    .text_color(rgb(TEXT_MUTED))
                                    .child(tr!("not part of the search")),
                            )
                        }),
                )
            },
        );
    if searchable {
        ease_hover(("filter", row), element, |element, hover| {
            element.bg(alpha(GOLD, 0.04 * hover))
        })
        .into_any_element()
    } else {
        element.into_any_element()
    }
}

/// The tier badge's hint, from the tier table: «Тир 3 из 9» (`Tier 3 of 9`), the bottom of the
/// tier's rolls, every tier's rolls, the item level the tier needs, and the best tier the item's
/// level lets roll.
fn tier_hint(info: &TierInfo) -> Vec<(gpui::SharedString, u32)> {
    let mut lines = Vec::with_capacity(5);
    lines.push((
        tr!(
            "Tier {tier} of {count}",
            tier = info.current,
            count = info.count
        )
        .into(),
        GOLD_LIGHT,
    ));
    if let Some(floor) = info.tier_floor {
        lines.push((
            tr!("This tier: from {value}", value = format_value(floor)).into(),
            TEXT,
        ));
    }
    if let Some((low, high)) = info.range {
        lines.push((
            tr!(
                "All tiers: {low}–{high}",
                low = format_value(low),
                high = format_value(high)
            )
            .into(),
            TEXT_DIM,
        ));
    }
    lines.push((
        tr!("Requires item level {level}", level = info.min_level).into(),
        TEXT_DIM,
    ));
    lines.push((
        tr!(
            "Best tier at this item's level: T{tier}",
            tier = info.best_available
        )
        .into(),
        TEXT_DIM,
    ));
    lines
}

/// A weighted sum's badge: its value adds up the same stat from several of the item's mods, as
/// the trade site sums them for the search -- a total, like the site's own pseudo rows.
fn weighted_sum_badge() -> impl IntoElement {
    div()
        .id("weighted-sum")
        .flex_none()
        .px(rems_from_px(4.))
        .rounded(rems_from_px(3.))
        .border_1()
        .border_color(rgb(BORDER_GOLD))
        .text_size(rems_from_px(11.))
        .line_height(rems_from_px(14.))
        .text_color(rgb(GOLD))
        .tooltip(hints::hint(tr!(
            "Sum: the value adds up this stat from every modifier of the item that has it, and \
             the trade site sums listings the same way."
        )))
        .child(tr!("sum"))
}

/// A mod row's roll slider (`roll_slider::Slider`): a track from the
/// lowest roll of the row's family on this kind of item to the highest, a bright notch at the
/// item's own roll, and a round thumb at the row's search bound -- its minimum box, or its
/// maximum where a lower roll is better -- with the part of the track the search admits lit in
/// gold. The tier table gives only the family's range and the item's own tier's floor, not every
/// tier's, so the track isn't cut into tiers. An unchecked row's slider dims, its lit part and the
/// thumb's glow fade out, and the thumb stays faint: the bound can still be set there.
/// Pressing the track puts the thumb there and dragging moves it
/// (`PriceCheckApp::begin_roll_drag`); the bound box follows, and typing in the box -- or a
/// profile, «Минимум тира» -- slides it there. Like a typed bound, it takes effect with the next
/// search.
fn render_roll_slider(
    row: usize,
    filter: &SearchFilter,
    ui: &FilterRowUi,
    slider: Slider,
    dragging: bool,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let bound = match slider.handle {
        Handle::Min => &ui.min_text,
        Handle::Max => &ui.max_text,
    };
    let thumb = slider.handle_fraction(bound.parse::<f64>().ok()) as f32;
    let notch = filter
        .roll
        .as_ref()
        .map(|roll| slider.fraction(roll.value) as f32);
    let view = cx.entity();
    let (low, high) = (format_value(slider.low), format_value(slider.high));
    let hint = match slider.handle {
        Handle::Min => tr!(
            "The ends are this stat's lowest ({low}) and highest ({high}) rolls across all tiers, \
             the bright mark is this item's roll, the circle is the search's minimum.",
            low = low,
            high = high
        ),
        Handle::Max => tr!(
            "The ends are this stat's lowest ({low}) and highest ({high}) rolls across all tiers, \
             the bright mark is this item's roll, the circle is the search's maximum: lower is \
             better here.",
            low = low,
            high = high
        ),
    };
    div()
        .id(("roll-slider", row))
        .pl(rems_from_px(CHECK_COLUMN))
        .tooltip(hints::hint(hint))
        .child(ease_state(
            "checked",
            filter.enabled,
            div(),
            move |holder, on| {
                let view = view.clone();
                holder.opacity(0.45 + 0.55 * on).child(ease_value(
                    "thumb",
                    thumb,
                    dragging,
                    div(),
                    move |holder, thumb| {
                        holder.child(slider_track(
                            row,
                            slider.handle,
                            thumb,
                            notch,
                            on,
                            view.clone(),
                        ))
                    },
                ))
            },
        ))
}

/// The slider's painted track and its mouse handling: `thumb` and `notch` are fractions of the
/// track, `on` how checked the row is, 0 to 1: the lit part and the thumb's glow fade with it.
fn slider_track(
    row: usize,
    handle: Handle,
    thumb: f32,
    notch: Option<f32>,
    on: f32,
    view: gpui::Entity<PriceCheckApp>,
) -> impl IntoElement {
    canvas(
        |bounds, window, _cx| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _cx| {
            let rem_size = window.rem_size();
            let thumb_size = rems_from_px(SLIDER_THUMB).to_pixels(rem_size);
            // The thumb's centre travels between the track's ends, so it never overhangs them.
            let left = bounds.left() + thumb_size / 2.;
            let width = (bounds.size.width - thumb_size).max(px(1.));
            let x_at = move |fraction: f32| left + width * fraction;
            let fraction_at = move |x: Pixels| f64::from((x - left) / width);
            let middle = bounds.center().y;
            let track = rems_from_px(SLIDER_TRACK).to_pixels(rem_size);
            let bar = |from: f32, to: f32| {
                Bounds::new(
                    point(x_at(from), middle - track / 2.),
                    size(x_at(to) - x_at(from), track),
                )
            };
            window.paint_quad(fill(bar(0., 1.), rgb(BORDER_FIELD)).corner_radii(track / 2.));
            let admitted = match handle {
                Handle::Min => (thumb, 1.),
                Handle::Max => (0., thumb),
            };
            if on > 0. {
                window.paint_quad(
                    fill(bar(admitted.0, admitted.1), alpha(GOLD, 0.55 * on))
                        .corner_radii(track / 2.),
                );
            }
            if let Some(notch) = notch {
                let height = rems_from_px(SLIDER_NOTCH).to_pixels(rem_size);
                window.paint_quad(
                    fill(
                        Bounds::new(
                            point(x_at(notch) - px(1.), middle - height / 2.),
                            size(px(2.), height),
                        ),
                        rgb(GOLD_LIGHT),
                    )
                    .corner_radii(px(1.)),
                );
            }
            let at = Bounds::new(
                point(x_at(thumb) - thumb_size / 2., middle - thumb_size / 2.),
                size(thumb_size, thumb_size),
            );
            window.paint_drop_shadows(at, (thumb_size / 2.).into(), &glow(GOLD, 0.7 * on));
            window.paint_quad(quad(
                at,
                thumb_size / 2.,
                rgb(BG_PANEL),
                px(1.),
                alpha(GOLD_LIGHT, 0.4 + 0.6 * on),
                BorderStyle::Solid,
            ));

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
    .w_full()
    .h(rems_from_px(SLIDER_HEIGHT))
}

/// A free affix slot row -- searchable as "at least this many empty prefixes/suffixes", the trade
/// site's `# Empty Prefix Modifiers`.
fn free_slot_text(filter: &SearchFilter) -> String {
    let count = filter.roll.as_ref().map_or(0.0, |roll| roll.value) as u32;
    match (filter.generation, count) {
        (Some(ModGeneration::Prefix), 1) => tr!("Empty prefix").to_owned(),
        (Some(ModGeneration::Prefix), count) => tr!("Empty prefixes: {count}", count = count),
        (_, 1) => tr!("Empty suffix").to_owned(),
        (_, count) => tr!("Empty suffixes: {count}", count = count),
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
/// read in the game's own Russian wording on the Russian site. They follow the item's language,
/// not the interface's: they are the item's own property lines, as its tooltip words them.
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

/// EE2's coloured source badges (`FilterModifier.vue`'s `.tag-*` classes; the trade site's names
/// of the stat groups) for the kinds that change what an item is worth; `None` for the plain ones
/// the section header already names.
fn source_badge(tag: FilterTag) -> Option<impl IntoElement> {
    let (label, bg, fg) = match tag {
        FilterTag::Rune => (tr!("augment"), BADGE_RUNE_BG, BADGE_RUNE_TEXT),
        FilterTag::Crafted => (tr!("crafted"), BADGE_RUNE_BG, BADGE_RUNE_TEXT),
        FilterTag::Fractured => (tr!("fractured"), BADGE_FRACTURED_BG, BADGE_INK),
        FilterTag::Enchant => (tr!("enchant"), BADGE_ENCHANT_BG, BADGE_ENCHANT_TEXT),
        FilterTag::Desecrated => (
            tr!("desecrated"),
            BADGE_DESECRATED_BG,
            BADGE_DESECRATED_TEXT,
        ),
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
            .rounded(rems_from_px(3.))
            .bg(rgb(bg))
            .text_size(rems_from_px(11.))
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

/// One min/max box: shows `text` (or the min/max placeholder while empty), its edge warming
/// to gold under the pointer so it reads as editable, takes focus on click so
/// `PriceCheckApp::handle_filter_key` receives the keystrokes, and while focused keeps the gold
/// edge and shows a caret after the value -- highlighted as selected right after the click
/// (`fresh`), when typing replaces it. A press on it never toggles its row. A click never
/// activates the panel (see `app`'s overlay setup), so the box activates it: the keys must come
/// here, not to the game.
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
    let (key, placeholder) = if is_min {
        ("min", tr!("min"))
    } else {
        ("max", tr!("max"))
    };

    let element = div()
        .id(key)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(rems_from_px(BOUND_INPUT_WIDTH))
        .h(rems_from_px(BOUND_INPUT_HEIGHT))
        .bg(rgb(BG_FIELD))
        .border_1()
        .map(|this| {
            if is_min {
                this.rounded_l(rems_from_px(4.))
            } else {
                this.rounded_r(rems_from_px(4.))
            }
        })
        .text_size(rems_from_px(13.))
        .cursor_text()
        .track_focus(focus_handle)
        .on_key_down(cx.listener(move |view, event: &KeyDownEvent, _window, cx| {
            view.handle_filter_key(row, is_min, event, cx);
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _event: &MouseDownEvent, window, cx| {
                // Only the box: the row's own press toggles the filter.
                cx.stop_propagation();
                if let Ok(overlay) = Win32Overlay::from_window(window) {
                    cx.spawn(async move |_, _| overlay.activate()).detach();
                }
                focus_for_click.focus(window, cx);
                view.begin_bound_edit(row, is_min, cx);
            }),
        )
        .map(|this| {
            if text.is_empty() && !focused {
                this.text_size(rems_from_px(12.))
                    .text_color(rgb(TEXT_MUTED))
                    .child(placeholder)
            } else {
                this.text_color(rgb(TEXT))
                    .child(
                        div()
                            .px(rems_from_px(1.))
                            .when(fresh && !text.is_empty(), |this| this.bg(alpha(GOLD, 0.3)))
                            .child(text.to_owned()),
                    )
                    .when(focused, |this| {
                        this.child(div().w(px(1.)).h(rems_from_px(14.)).bg(rgb(GOLD)))
                    })
            }
        });
    ease_hover(key, element, move |element, hover| {
        let lit = if focused { 1. } else { 0.7 * hover };
        element
            .border_color(rgb(blend(BORDER_FIELD, GOLD, lit)))
            .shadow(glow(GOLD, if focused { 0.5 } else { 0.4 * hover }))
    })
}
