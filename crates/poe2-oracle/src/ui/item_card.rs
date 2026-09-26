//! A listed item drawn the way the game's own tooltip draws it: a header banner tinted with the
//! name's colour, the name in the tooltip's face and the item's art beside it, then the tooltip's
//! sections in the game's order -- properties, requirements, sockets, item level, enchants, runes,
//! implicits, granted skills, the explicit mods, flavour and description, the flags, the seller's
//! note -- every value and mod in the game's colours, mods with their tier on the left, the item
//! level they need on the right and their roll ranges written in as the game's advanced (Alt)
//! tooltip writes them. As in that tooltip, the explicit mods run prefixes first, then suffixes.
//!
//! Every line's text and colour runs are worked out once, in [`ItemCard::new`], so drawing the card
//! each frame it shows only builds elements. The card is as wide as the game's tooltip and wraps
//! its lines, never truncating them. Like every tooltip of the app (`style::game_hint`) it sits in
//! the game's double gold frame over the tooltip shadow, and fades in.

use std::ops::Range;

use gpui::{
    AnyElement, HighlightStyle, IntoElement, ObjectFit, SharedString, SharedUri, StyledText,
    Window, div, img, linear_color_stop, linear_gradient, prelude::*, relative, rgb,
};
use trade_client::{ItemFrame, ListedItem, ListedMod, ModKind, TradeSite, ValueColor};

use crate::tr;
use crate::ui::fonts::{self, NameFont};
use crate::ui::ornament::FRAME_CLEAR;
use crate::ui::panel::format::currency_img;
use crate::ui::style::{game_frame, switch_in, tooltip_shadow};
use crate::ui::theme::{
    BANNER_EDGE, BANNER_TINT, BG_ITEM_CARD, CURRENCY_NAME, DAMAGE_CHAOS, DAMAGE_COLD, DAMAGE_FIRE,
    DAMAGE_LIGHTNING, GAME_RED, GEM_NAME, MOD_DESECRATED, MOD_ENCHANTED, MOD_FRACTURED, PRICE_RISE,
    RARITY_MAGIC, RARITY_NORMAL, RARITY_RARE, RARITY_UNIQUE, TEXT, TEXT_DIM, TEXT_MUTED,
    TEXT_VALUE, TEXT_WARNING, blend, rems_from_px,
};

/// The game's tooltip width at 100% UI scale.
const CARD_WIDTH: f32 = 420.;
const BODY_TEXT: f32 = 13.;
const TITLE_TEXT: f32 = 16.;
/// Edge of the item art beside the name.
const ART_SIZE: f32 = 44.;
/// A mod line's tier and item-level columns: as wide as each other, so its text stays centred.
const SIDE_COLUMN: f32 = 34.;
const SIDE_TEXT: f32 = 11.;
/// The currency icon in the seller's price note.
const NOTE_ICON: f32 = 14.;
/// The room around the header's and the body's content, px -- the frame's keep-out on the card's
/// edges, this much where the two meet at the header's rule.
const PADDING: f32 = 6.;

/// How a mod stands against the search that found the listing.
pub(crate) enum ModMark {
    /// The search doesn't ask for its stat.
    None,
    /// Asked for and rolled within the search's bounds: `✓`.
    Met,
    /// Asked for and rolled outside them: `✗`, and what the search needs (`needs at least 40`).
    Short(String),
}

/// A listing's price, for the seller's price note to show as the panel shows every price: its
/// amount and its currency's icon.
#[derive(Clone)]
pub(crate) struct CardPrice {
    /// The amount as the panel writes amounts (`i18n::number`).
    pub amount: SharedString,
    pub icon: Option<SharedString>,
    /// Stands in for the icon where the catalog has none, as in `panel::format::amount_in`.
    pub currency_name: SharedString,
}

/// A listed item laid out as the game's tooltip lays it out.
pub(crate) struct ItemCard {
    name_font: &'static NameFont,
    /// The name's colour, which tints the header, the border and the section rules too.
    color: u32,
    icon: Option<SharedUri>,
    /// The name, then the type line; just the type line for an item it names on its own.
    title: Vec<SharedString>,
    sections: Vec<Vec<CardLine>>,
}

/// One line of the card.
enum CardLine {
    /// Centred across the card.
    Text(LineText),
    /// A mod: centred between its tier (left) and the item level it needs (right).
    Mod {
        tier: SharedString,
        level: SharedString,
        text: LineText,
    },
    /// The seller's price note: its kind (`~b/o`), then the listing's price.
    Note {
        label: SharedString,
        price: CardPrice,
    },
}

struct LineText {
    text: SharedString,
    /// The colour of everything outside `runs`.
    color: u32,
    italic: bool,
    /// Byte ranges in colours of their own: values, roll ranges, match marks.
    runs: Vec<(Range<usize>, HighlightStyle)>,
}

impl ItemCard {
    /// `item`, listed on `site` (whose client language picks the name's face, `ui::fonts`), laid
    /// out as its tooltip. `mark` says how each mod stands against the search; `price` is the
    /// listing's, which the seller's price note (`~b/o 1 exalted`) shows with its currency's icon.
    pub(crate) fn new(
        item: &ListedItem,
        site: TradeSite,
        price: Option<CardPrice>,
        mark: impl Fn(&ListedMod) -> ModMark,
    ) -> ItemCard {
        let mut sections = Vec::new();
        let mut section = |lines: Vec<CardLine>| {
            if !lines.is_empty() {
                sections.push(lines);
            }
        };

        // The labels carry the space after their colon, as the game's tooltip lines do.
        section(
            item.properties
                .iter()
                .map(|property| valued_line("", property.text()))
                .collect(),
        );
        if !item.requirements.is_empty() {
            section(vec![valued_line(
                tr!("Requires: "),
                item.requirements_text(),
            )]);
        }
        if !item.sockets.is_empty() {
            section(vec![labelled(
                tr!("Sockets: "),
                &sockets_text(&item.sockets),
                TEXT,
            )]);
        }
        // Gems and currency list item level 0: nothing to show.
        if let Some(level) = item.item_level.filter(|&level| level > 0) {
            section(vec![labelled(
                tr!("Item Level: "),
                &level.to_string(),
                TEXT,
            )]);
        }
        let mods = |kinds: &[ModKind]| -> Vec<CardLine> {
            item.mods
                .iter()
                .filter(|listed| kinds.contains(&listed.kind))
                .map(|listed| mod_line(listed, mark(listed)))
                .collect()
        };
        section(mods(&[ModKind::Enchant]));
        section(mods(&[ModKind::Rune]));
        section(mods(&[ModKind::Implicit]));
        section(
            item.granted_skills
                .iter()
                .map(|skill| valued_line("", skill.text()))
                .collect(),
        );
        // The site lists explicit mods in the tooltip's stat order, prefixes and suffixes mixed;
        // the advanced tooltip, whose tiers the card shows, lists the prefixes first. Each side
        // keeps the site's order, and a mod the site gives no side for comes last.
        let mut explicit: Vec<&ListedMod> = item
            .mods
            .iter()
            .filter(|listed| {
                matches!(
                    listed.kind,
                    ModKind::Fractured | ModKind::Explicit | ModKind::Desecrated | ModKind::Crafted
                )
            })
            .collect();
        explicit.sort_by_key(
            |listed| match listed.tier.as_deref().map(|tier| tier.as_bytes()) {
                Some([b'P', ..]) => 0,
                Some([b'S', ..]) => 1,
                _ => 2,
            },
        );
        section(
            explicit
                .into_iter()
                .map(|listed| mod_line(listed, mark(listed)))
                .collect(),
        );
        if let Some(flavour) = &item.flavour {
            section(vec![plain(flavour.clone(), RARITY_UNIQUE, true)]);
        }
        if let Some(description) = &item.description {
            section(vec![plain(description.clone(), TEXT_DIM, true)]);
        }
        if item.unidentified {
            let text = match item.unidentified_tier {
                Some(tier) => tr!("Unidentified (Tier {tier})", tier = tier),
                None => tr!("Unidentified").to_owned(),
            };
            section(vec![plain(text, GAME_RED, false)]);
        }
        let flags = [
            (item.corrupted, tr!("Corrupted"), GAME_RED),
            (item.mirrored, tr!("Mirrored"), TEXT_VALUE),
            (item.sanctified, tr!("Sanctified"), MOD_FRACTURED),
            (item.fractured, tr!("Fractured Item"), MOD_FRACTURED),
        ];
        for (set, flag, color) in flags {
            if set {
                section(vec![plain(flag.to_owned(), color, false)]);
            }
        }
        if let Some(note) = item.note.as_deref().map(str::trim)
            && !note.is_empty()
        {
            // A price note (`~b/o 1 exalted`) is the listing's price: its currency goes by its
            // icon, never by its trade id.
            let kind = note
                .split_whitespace()
                .next()
                .filter(|kind| kind.starts_with('~'));
            let line = match (kind, price) {
                (Some(kind), Some(price)) => CardLine::Note {
                    label: tr!("Note: {kind}", kind = kind).into(),
                    price,
                },
                _ => labelled(tr!("Note: "), note, TEXT),
            };
            section(vec![line]);
        }

        let color = name_color(item.frame);
        ItemCard {
            name_font: fonts::name_font(site),
            color,
            icon: item.icon.clone().map(SharedUri::from),
            title: [&item.name, &item.type_line]
                .into_iter()
                .filter(|line| !line.is_empty())
                .map(|line| SharedString::from(line.clone()))
                .collect(),
            sections,
        }
    }

    /// Lines of the caller's own under the card's, each `(text, colour)`: what it makes of the
    /// listing.
    pub(crate) fn with_notes(mut self, notes: Vec<(String, u32)>) -> ItemCard {
        if !notes.is_empty() {
            self.sections.push(
                notes
                    .into_iter()
                    .map(|(text, color)| plain(text, color, false))
                    .collect(),
            );
        }
        self
    }
}

/// The card, as wide as the game's tooltip -- narrower only in a window narrower than that, which
/// a tooltip can't leave.
pub(crate) fn render_item_card(card: &ItemCard, window: &Window) -> impl IntoElement {
    let width = rems_from_px(CARD_WIDTH)
        .to_pixels(window.rem_size())
        .min(window.viewport_size().width);
    switch_in(
        "card",
        div()
            .relative()
            .w(width)
            .flex()
            .flex_col()
            .bg(rgb(BG_ITEM_CARD))
            .shadow(tooltip_shadow())
            .text_size(rems_from_px(BODY_TEXT))
            .line_height(relative(1.3))
            .text_color(rgb(TEXT))
            .child(render_header(card))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px(rems_from_px(FRAME_CLEAR))
                    .pt(rems_from_px(PADDING))
                    .pb(rems_from_px(FRAME_CLEAR))
                    .children(card.sections.iter().enumerate().map(|(index, lines)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap(rems_from_px(1.))
                            .when(index > 0, |this| this.child(render_rule(card.color)))
                            .children(lines.iter().map(render_line))
                    })),
            )
            .child(game_frame()),
    )
}

/// The name -- a rare's or unique's over its base -- centred in the tooltip's face and the name's
/// colour on a banner tinted with it, the art on the left as on the trade site; an empty column
/// of its width on the right keeps the name centred.
fn render_header(card: &ItemCard) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(rems_from_px(8.))
        .px(rems_from_px(FRAME_CLEAR))
        .pt(rems_from_px(FRAME_CLEAR))
        .pb(rems_from_px(PADDING))
        .bg(linear_gradient(
            180.,
            linear_color_stop(rgb(blend(BG_ITEM_CARD, card.color, BANNER_TINT)), 0.),
            linear_color_stop(rgb(BG_ITEM_CARD), 1.),
        ))
        .border_b_1()
        .border_color(rgb(blend(BG_ITEM_CARD, card.color, BANNER_EDGE)))
        .children(card.icon.clone().map(|url| {
            img(url)
                .w(rems_from_px(ART_SIZE))
                .h(rems_from_px(ART_SIZE))
                .flex_none()
                .object_fit(ObjectFit::Contain)
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .text_center()
                .text_size(rems_from_px(TITLE_TEXT))
                .text_color(rgb(card.color))
                .font_family(card.name_font.family)
                .font_weight(card.name_font.weight)
                .children(card.title.iter().map(|line| div().child(line.clone()))),
        )
        .when(card.icon.is_some(), |this| {
            this.child(
                div()
                    .w(rems_from_px(ART_SIZE))
                    .h(rems_from_px(ART_SIZE))
                    .flex_none(),
            )
        })
}

/// The rule between two sections: a line in the name's colour fading out toward both ends.
fn render_rule(color: u32) -> impl IntoElement {
    let line = rgb(blend(BG_ITEM_CARD, color, BANNER_EDGE));
    let clear = rgb(BG_ITEM_CARD);
    div()
        .flex()
        .h(rems_from_px(1.))
        .my(rems_from_px(4.))
        .child(div().flex_1().bg(linear_gradient(
            90.,
            linear_color_stop(clear, 0.),
            linear_color_stop(line, 1.),
        )))
        .child(div().flex_1().bg(linear_gradient(
            90.,
            linear_color_stop(line, 0.),
            linear_color_stop(clear, 1.),
        )))
}

fn render_line(line: &CardLine) -> AnyElement {
    match line {
        CardLine::Text(text) => render_text(text).text_center().into_any_element(),
        CardLine::Mod { tier, level, text } => div()
            .flex()
            .gap(rems_from_px(4.))
            .child(side_column().child(tier.clone()))
            .child(render_text(text).flex_1().min_w_0().text_center())
            .child(side_column().text_right().child(level.clone()))
            .into_any_element(),
        CardLine::Note { label, price } => div()
            .flex()
            .flex_wrap()
            .justify_center()
            .items_center()
            .gap(rems_from_px(3.))
            .text_color(rgb(TEXT_DIM))
            .child(label.clone())
            .child(div().text_color(rgb(TEXT)).child(price.amount.clone()))
            .children(currency_img(price.icon.as_deref(), NOTE_ICON))
            .when(price.icon.is_none(), |this| {
                this.child(price.currency_name.clone())
            })
            .into_any_element(),
    }
}

fn render_text(line: &LineText) -> gpui::Div {
    div()
        .text_color(rgb(line.color))
        .when(line.italic, |this| this.italic())
        .child(StyledText::new(line.text.clone()).with_highlights(line.runs.iter().cloned()))
}

fn side_column() -> gpui::Div {
    div()
        .w(rems_from_px(SIDE_COLUMN))
        .flex_none()
        .pt(rems_from_px(1.))
        .text_size(rems_from_px(SIDE_TEXT))
        .text_color(rgb(TEXT_MUTED))
}

/// `text` in `color` alone: a flag, the flavour text, a note.
fn plain(text: String, color: u32, italic: bool) -> CardLine {
    CardLine::Text(LineText {
        text: text.into(),
        color,
        italic,
        runs: Vec::new(),
    })
}

/// `label` in the tooltip's label grey, then `value` in `color`: `Item Level: 75`.
fn labelled(label: &str, value: &str, color: u32) -> CardLine {
    valued(
        format!("{label}{value}"),
        [(label.len()..label.len() + value.len(), color)],
    )
}

/// A property line (`trade_client::ItemProperty::text`) after `label`: its words in the label
/// grey, each value in the game's colour for it.
fn valued_line(label: &str, (text, values): (String, Vec<(Range<usize>, ValueColor)>)) -> CardLine {
    let shift = label.len();
    valued(
        format!("{label}{text}"),
        values
            .into_iter()
            .map(|(span, color)| (span.start + shift..span.end + shift, value_color(color))),
    )
}

/// `text` in the label grey but for its `values`, each `(where, colour)`, in order.
fn valued(text: String, values: impl IntoIterator<Item = (Range<usize>, u32)>) -> CardLine {
    CardLine::Text(LineText {
        text: text.into(),
        color: TEXT_DIM,
        italic: false,
        runs: values
            .into_iter()
            .map(|(span, color)| (span, highlight(color)))
            .collect(),
    })
}

/// A mod in its kind's colour, roll ranges written in, behind the search's mark for it: a green
/// `✓` where it rolled within the search's bounds, a `✗` and what the search needs where it
/// didn't.
fn mod_line(listed: &ListedMod, mark: ModMark) -> CardLine {
    let (ranged, ranges) = listed.text_with_ranges();
    let (prefix, need) = match mark {
        ModMark::None => ("", None),
        ModMark::Met => ("✓ ", None),
        ModMark::Short(need) => ("✗ ", Some(need)),
    };
    let mut text = String::with_capacity(prefix.len() + ranged.len() + 24);
    let mut runs = Vec::with_capacity(ranges.len() + 2);
    if !prefix.is_empty() {
        text.push_str(prefix);
        let color = if need.is_some() {
            TEXT_WARNING
        } else {
            PRICE_RISE
        };
        runs.push((0..prefix.len(), highlight(color)));
    }
    let shift = text.len();
    text.push_str(&ranged);
    runs.extend(
        ranges
            .into_iter()
            .map(|span| (span.start + shift..span.end + shift, highlight(TEXT_DIM))),
    );
    if let Some(need) = need {
        let start = text.len();
        text.push_str(" (");
        text.push_str(&need);
        text.push(')');
        runs.push((start..text.len(), highlight(TEXT_WARNING)));
    }
    let text = LineText {
        text: text.into(),
        color: mod_color(listed.kind),
        italic: false,
        runs,
    };
    if listed.tier.is_none() && listed.level.is_none() {
        return CardLine::Text(text);
    }
    CardLine::Mod {
        tier: listed.tier.clone().unwrap_or_default().into(),
        level: listed
            .level
            .map(|level| tr!("lvl {level}", level = level))
            .unwrap_or_default()
            .into(),
        text,
    }
}

/// `3` for empty sockets; `2 — Greater Iron Rune, empty` once something sits in one.
fn sockets_text(sockets: &[Option<String>]) -> String {
    let count = sockets.len().to_string();
    if sockets.iter().all(Option::is_none) {
        return count;
    }
    let filled: Vec<&str> = sockets
        .iter()
        .map(|socket| socket.as_deref().unwrap_or(tr!("empty")))
        .collect();
    format!("{count} — {}", filled.join(", "))
}

fn highlight(color: u32) -> HighlightStyle {
    HighlightStyle {
        color: Some(rgb(color).into()),
        ..HighlightStyle::default()
    }
}

/// The game's name colours: gear's rarity, or the kind of item that has none.
fn name_color(frame: ItemFrame) -> u32 {
    match frame {
        ItemFrame::Normal => RARITY_NORMAL,
        ItemFrame::Magic => RARITY_MAGIC,
        ItemFrame::Rare => RARITY_RARE,
        ItemFrame::Unique => RARITY_UNIQUE,
        ItemFrame::Gem => GEM_NAME,
        ItemFrame::Currency => CURRENCY_NAME,
    }
}

/// The game's colours for a property's values; physical damage is plain white.
fn value_color(color: ValueColor) -> u32 {
    match color {
        ValueColor::Default | ValueColor::Physical => TEXT,
        ValueColor::Augmented => TEXT_VALUE,
        ValueColor::Unmet => GAME_RED,
        ValueColor::Fire => DAMAGE_FIRE,
        ValueColor::Cold => DAMAGE_COLD,
        ValueColor::Lightning => DAMAGE_LIGHTNING,
        ValueColor::Chaos => DAMAGE_CHAOS,
    }
}

/// The game's colours for mods: implicits and explicits in its mod blue, enchants, runes and
/// crafted mods lighter, fractured ones gold -- and desecrated ones in `MOD_DESECRATED`.
fn mod_color(kind: ModKind) -> u32 {
    match kind {
        ModKind::Implicit | ModKind::Explicit => TEXT_VALUE,
        ModKind::Enchant | ModKind::Rune | ModKind::Crafted => MOD_ENCHANTED,
        ModKind::Fractured => MOD_FRACTURED,
        ModKind::Desecrated => MOD_DESECRATED,
    }
}
