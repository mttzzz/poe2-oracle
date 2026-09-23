//! The item's header: its art, its name in the game's own colour and face with links to its
//! poe2db and wiki pages, and EE2's chip row under it. The header is tinted with the name's colour
//! the way the game's tooltip banners are: rarity reads before the name does.

use gpui::{
    AnyElement, Context, IntoElement, MouseButton, MouseDownEvent, ObjectFit, div, img,
    linear_color_stop, linear_gradient, prelude::*, rgb,
};

use poe2_domain::{ItemRarity, ParsedItem};
use trade_client::TradeSite;

use crate::item_refs;
use crate::price_check::PriceCheckApp;
use crate::ui::fonts;
use crate::ui::hint as hints;
use crate::ui::theme::{
    BG_BUTTON_HOVER, BG_CONTROL, BG_NAMEPLATE, BORDER_GOLD, CONTENT_PADDING, CURRENCY_NAME,
    GEM_NAME, GOLD, RARITY_MAGIC, RARITY_NORMAL, RARITY_RARE, RARITY_UNIQUE, TEXT, TEXT_DIM,
    TEXT_VALUE, TEXT_WARNING, blend, rems_from_px,
};

use super::results::render_link;

/// Edge of the item art beside the name.
const ART_SIZE: f32 = 48.;

/// How much of the name's colour the top of the header takes, fading to none at its bottom.
const BANNER_TINT: f32 = 0.16;
/// How much of it the line under the header takes.
const BANNER_EDGE: f32 = 0.45;

/// The item's name -- and, for rares and uniques, its base type -- in the game's own name colour
/// and in the stand-in for its tooltip face on the item's client language (see `fonts`), like its
/// tooltip header; beside it the item's art, and under it links to the item's poe2db and wiki
/// pages (`item_refs`), when the item database knows it.
pub(super) fn render_nameplate(item: &ParsedItem, site: TradeSite) -> impl IntoElement {
    let name_font = fonts::name_font(site);
    let refs = item_refs::refs_for(item);
    let art = refs.and_then(|found| found.icon_url());
    let color = name_color(item);
    let names = div()
        .flex()
        .flex_col()
        .items_center()
        .text_color(rgb(color))
        .font_family(name_font.family)
        .font_weight(name_font.weight)
        .child(div().text_size(rems_from_px(20.)).child(item.name.clone()))
        .children(
            item.base_type
                .clone()
                .map(|base| div().text_size(rems_from_px(17.)).child(base)),
        );
    let links = refs.map(|found| {
        div()
            .flex()
            .gap(rems_from_px(12.))
            .mt(rems_from_px(3.))
            .child(render_link(
                "poe2db ↗",
                found.poe2db_url(site == TradeSite::Russian),
            ))
            .child(render_link("вики ↗", found.wiki_url()))
    });
    div()
        .flex()
        .items_center()
        .gap(rems_from_px(10.))
        .px(rems_from_px(CONTENT_PADDING))
        .py(rems_from_px(8.))
        .bg(linear_gradient(
            180.,
            linear_color_stop(rgb(blend(BG_NAMEPLATE, color, BANNER_TINT)), 0.),
            linear_color_stop(rgb(BG_NAMEPLATE), 1.),
        ))
        .border_b_1()
        .border_color(rgb(blend(BG_NAMEPLATE, color, BANNER_EDGE)))
        .text_center()
        // The art sits on the left, as on the trade site; an empty column of its width on the
        // right keeps the name centred.
        .children(art.clone().map(|url| {
            img(url)
                .w(rems_from_px(ART_SIZE))
                .h(rems_from_px(ART_SIZE))
                .flex_none()
                .object_fit(ObjectFit::Contain)
        }))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .flex_1()
                .min_w_0()
                .child(names)
                .children(links),
        )
        .when(art.is_some(), |this| {
            this.child(
                div()
                    .w(rems_from_px(ART_SIZE))
                    .h(rems_from_px(ART_SIZE))
                    .flex_none(),
            )
        })
}

fn name_color(item: &ParsedItem) -> u32 {
    let category = item.category.as_ref().map(|category| category.id.as_str());
    match item.rarity {
        Some(ItemRarity::Magic) => RARITY_MAGIC,
        Some(ItemRarity::Rare) => RARITY_RARE,
        Some(ItemRarity::Unique) => RARITY_UNIQUE,
        Some(ItemRarity::Normal) => RARITY_NORMAL,
        None if category.is_some_and(|id| id.starts_with("currency")) => CURRENCY_NAME,
        None if category.is_some_and(|id| id.starts_with("gem")) => GEM_NAME,
        None => RARITY_NORMAL,
    }
}

/// Item class (or the base type the search goes by), level, requirement, sockets, quality,
/// corruption, and how many of the listed searchable stats are selected -- EE2's chip row. A click
/// on the count checks them all, or all off (`PriceCheckApp::toggle_all_filters`).
pub(super) fn render_chips(
    state: &PriceCheckApp,
    item: &ParsedItem,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let (selected, searchable) = state
        .filters
        .iter()
        .filter(|filter| filter.searchable() && !filter.hidden)
        .fold((0, 0), |(selected, total), filter| {
            (selected + usize::from(filter.enabled), total + 1)
        });
    let sockets = item
        .sockets
        .map(|sockets| sockets.current)
        .or_else(|| item.gem_sockets.map(|sockets| sockets.number));
    let required_level = item
        .requirements
        .map(|requirements| requirements.level)
        .filter(|&level| level > 0);
    let class = item
        .category
        .as_ref()
        .map(|category| category.display_name.clone());
    let item_type: Option<AnyElement> = match &state.scope {
        Some(choice) => {
            let (label, value, hint) = match &choice.current().base_type {
                Some(base_type) => (
                    "База:",
                    base_type.clone(),
                    "Поиск только по этой базе. Нажмите, чтобы искать среди всех вещей класса.",
                ),
                None => (
                    "Класс:",
                    class.unwrap_or_default(),
                    "Поиск среди всех вещей класса. Нажмите, чтобы искать только по базе этой вещи.",
                ),
            };
            Some(
                toggle_chip(Some(label), value, hint, PriceCheckApp::toggle_scope, cx)
                    .into_any_element(),
            )
        }
        None => class.map(|class| chip(None, class, TEXT).into_any_element()),
    };
    let corruption = state.uncorrupted_only.map(|only| {
        let (value, hint) = if only {
            (
                "Без осквернённых",
                "Осквернённые лоты не учитываются: их нельзя изменить, и у них бывают свойства, \
                 которых нет у этой вещи. Нажмите, чтобы учитывать и их.",
            )
        } else {
            (
                "И осквернённые",
                "Учитываются и осквернённые лоты. Нажмите, чтобы их исключить.",
            )
        };
        toggle_chip(
            None,
            value.to_owned(),
            hint,
            PriceCheckApp::toggle_uncorrupted_only,
            cx,
        )
    });

    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(rems_from_px(6.))
        .py(rems_from_px(8.))
        .children(item_type)
        .children(
            item.item_level
                .map(|level| chip(Some("Ур. предмета:"), level.to_string(), TEXT)),
        )
        .children(required_level.map(|level| chip(Some("Требуется ур.:"), level.to_string(), TEXT)))
        .children(sockets.map(|count| chip(Some("Гнёзда:"), count.to_string(), TEXT)))
        .children(
            item.quality
                .map(|quality| chip(Some("Качество:"), format!("+{quality}%"), TEXT_VALUE)),
        )
        .children(
            item.is_corrupted
                .then(|| chip(None, "Осквернено".to_owned(), TEXT_WARNING)),
        )
        .children(
            item.stack_size
                .map(|(count, _)| chip(Some("В стопке:"), count.to_string(), TEXT)),
        )
        .children(corruption)
        .children((searchable > 0).then(|| {
            toggle_chip(
                Some("Св-ва:"),
                format!("{selected} из {searchable}"),
                "Сколько свойств выбрано для поиска. Нажмите, чтобы отметить все или снять все.",
                PriceCheckApp::toggle_all_filters,
                cx,
            )
        }))
}

fn chip(label: Option<&'static str>, value: String, value_color: u32) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(rems_from_px(4.))
        .px(rems_from_px(8.))
        .py(rems_from_px(2.))
        .rounded_xs()
        .bg(rgb(BG_CONTROL))
        .text_xs()
        .children(label.map(|label| div().text_color(rgb(TEXT_DIM)).child(label)))
        .child(div().text_color(rgb(value_color)).child(value))
}

/// A chip a click turns to its other state for the next search -- the item-type chip, when the
/// search can go by the item's class or its base type (`PriceCheckApp::toggle_scope`), the
/// corrupted-listings one (`PriceCheckApp::toggle_uncorrupted_only`), the stats count
/// (`PriceCheckApp::toggle_all_filters`) -- saying on hover what it does (`hint`).
fn toggle_chip(
    label: Option<&'static str>,
    value: String,
    hint: &'static str,
    toggle: fn(&mut PriceCheckApp, &mut Context<PriceCheckApp>),
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    div()
        .id(hint)
        .flex()
        .flex_none()
        .items_center()
        .gap(rems_from_px(4.))
        .px(rems_from_px(8.))
        .py(rems_from_px(2.))
        .rounded_xs()
        .bg(rgb(BG_CONTROL))
        .border_1()
        .border_color(rgb(BORDER_GOLD))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(BG_BUTTON_HOVER)))
        .text_xs()
        .tooltip(hints::hint(hint))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| toggle(view, cx)),
        )
        .children(label.map(|label| div().text_color(rgb(TEXT_DIM)).child(label)))
        .child(div().text_color(rgb(TEXT)).child(value))
        .child(div().text_color(rgb(GOLD)).child("↔"))
}
