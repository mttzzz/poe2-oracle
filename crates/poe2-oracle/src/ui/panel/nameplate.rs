//! The item's header: its art, its name in the game's own colour and face, links to its poe2db
//! and wiki pages and to Craft of Exile, an ornament rule under it all -- and EE2's chip row under
//! the header. The header is tinted with the name's colour the way the game's tooltip banners
//! are: rarity reads before the name does.

use gpui::{
    AnyElement, Context, IntoElement, MouseDownEvent, ObjectFit, SharedString, div, img,
    linear_color_stop, linear_gradient, prelude::*, rgb,
};

use item_parser::ItemLanguage;
use poe2_domain::{ItemRarity, ParsedItem};
use trade_client::{RarityFilter, TradeSite};

use crate::craft_link;
use crate::item_refs;
use crate::price_check::PriceCheckApp;
use crate::ui::fonts;
use crate::ui::hint as hints;
use crate::ui::style::{self, chip, heading, link, ornament_rule};
use crate::ui::theme::{
    BANNER_EDGE, BANNER_TINT, BG_NAMEPLATE, CONTENT_PADDING, CURRENCY_NAME, GEM_NAME, RARITY_MAGIC,
    RARITY_NORMAL, RARITY_RARE, RARITY_UNIQUE, TEXT, TEXT_VALUE, TEXT_WARNING, blend, rems_from_px,
};

use super::results::render_link;

/// Edge of the item art beside the name.
const ART_SIZE: f32 = 48.;

/// The item's name -- and, for rares and uniques, its base type -- in the game's own name colour
/// and in the stand-in for its tooltip face on the item's client language (see `fonts`), like its
/// tooltip header; beside it the item's art, and under it links to the item's poe2db and wiki
/// pages (`item_refs`), when the item database knows it, to the item in Craft of Exile
/// (`craft_link`), when the site crafts it, and to the form that reports the item read or priced
/// wrong (`PriceCheckApp::report_item`).
pub(super) fn render_nameplate(
    item: &ParsedItem,
    site: TradeSite,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let name_font = fonts::name_font(site);
    let refs = item_refs::refs_for(item);
    let art = refs.and_then(|found| found.icon_url());
    let color = name_color(item);
    let language = match site {
        TradeSite::Russian => ItemLanguage::Russian,
        TradeSite::International => ItemLanguage::English,
    };
    let names = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .items_center()
        .text_center()
        .text_color(rgb(color))
        .child(
            heading(name_font)
                .text_size(rems_from_px(21.))
                .child(item.name.clone()),
        )
        .children(
            item.base_type
                .clone()
                .map(|base| heading(name_font).text_size(rems_from_px(16.)).child(base)),
        );
    // Under the whole header, not the name's column: four links outgrow it.
    let links = div()
        .flex()
        .flex_wrap()
        .justify_center()
        .items_center()
        .gap_x(rems_from_px(14.))
        .gap_y(rems_from_px(2.))
        .px(rems_from_px(CONTENT_PADDING))
        .pb(rems_from_px(8.))
        .children(
            refs.map(|found| render_link("poe2db ↗", found.poe2db_url(site == TradeSite::Russian))),
        )
        .children(refs.map(|found| render_link("вики ↗", found.wiki_url())))
        .children(
            craft_link::url(item, craft_link::site_language(language))
                .map(|url| render_link("Craft of Exile ↗", url)),
        )
        .child(
            div()
                .id("report-item")
                .flex_none()
                .tooltip(hints::hint(
                    "Предмет разобран или оценён неверно? Откроет на GitHub форму с текстом \
                     предмета: останется описать, что не так.",
                ))
                .child(link(
                    "link",
                    "сообщить об ошибке ↗",
                    cx.listener(|view, _event: &MouseDownEvent, _window, cx| {
                        view.report_item(cx);
                    }),
                )),
        );
    div()
        .flex()
        .flex_col()
        .flex_none()
        .bg(linear_gradient(
            180.,
            linear_color_stop(rgb(blend(BG_NAMEPLATE, color, BANNER_TINT)), 0.),
            linear_color_stop(rgb(BG_NAMEPLATE), 1.),
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap(rems_from_px(10.))
                .px(rems_from_px(CONTENT_PADDING))
                .pt(rems_from_px(12.))
                .pb(rems_from_px(4.))
                // The art sits on the left, as on the trade site; an empty column of its width on
                // the right keeps the name centred.
                .children(art.clone().map(|url| {
                    img(url)
                        .flex_none()
                        .size(rems_from_px(ART_SIZE))
                        .object_fit(ObjectFit::Contain)
                }))
                .child(names)
                .when(art.is_some(), |this| {
                    this.child(div().flex_none().size(rems_from_px(ART_SIZE)))
                }),
        )
        .child(links)
        .child(
            div()
                .px(rems_from_px(16.))
                .pb(rems_from_px(2.))
                .child(ornament_rule(blend(BG_NAMEPLATE, color, BANNER_EDGE))),
        )
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
                toggle_chip(
                    "scope",
                    Some(label),
                    value,
                    hint,
                    PriceCheckApp::toggle_scope,
                    cx,
                )
                .into_any_element(),
            )
        }
        None => class.map(|class| chip(None, class, TEXT).into_any_element()),
    };
    // The item's own state, matched by default as PoE Overlay II does: "Можно изменить" there.
    let corruption = state.corruption.map(|choice| {
        let (value, hint) = match (choice.value, choice.on) {
            (false, true) => (
                "Можно изменить",
                "Только лоты, которые ещё можно изменить: осквернённые не учитываются — их \
                 нельзя улучшить, и у них бывают свойства, которых нет у этой вещи. Нажмите, \
                 чтобы учитывать и их.",
            ),
            (true, true) => (
                "Только осквернённые",
                "Эта вещь осквернена: учитываются только осквернённые лоты, у которых скверна \
                 так же изменила свойства. Нажмите, чтобы учитывать и неосквернённые.",
            ),
            (_, false) => (
                "И осквернённые, и нет",
                "Скверна не учитывается. Нажмите, чтобы искать только лоты в том же состоянии, \
                 что и эта вещь.",
            ),
        };
        toggle_chip(
            "corruption",
            None,
            value,
            hint,
            PriceCheckApp::toggle_corruption,
            cx,
        )
    });
    let identification = state.identification.map(|choice| {
        let (value, hint) = if choice.on {
            (
                "Неопознанные",
                "Эта вещь не опознана: учитываются только неопознанные лоты — опознанные \
                 продаются за свои свойства. Нажмите, чтобы учитывать и опознанные.",
            )
        } else {
            (
                "И опознанные",
                "Учитываются и опознанные лоты. Нажмите, чтобы искать только неопознанные.",
            )
        };
        toggle_chip(
            "identification",
            None,
            value,
            hint,
            PriceCheckApp::toggle_identification,
            cx,
        )
    });
    let rarity = state.rarity.map(|choice| {
        let (value, hint) = match choice.current() {
            RarityFilter::Magic => (
                "волшебные",
                "Поиск только среди волшебных вещей. Нажмите, чтобы искать среди всех, \
                 кроме уникальных.",
            ),
            RarityFilter::Rare => (
                "редкие",
                "Поиск только среди редких вещей. Нажмите, чтобы искать среди всех, \
                 кроме уникальных.",
            ),
            RarityFilter::Normal => (
                "обычные",
                "Поиск только среди обычных вещей. Нажмите, чтобы искать среди всех, \
                 кроме уникальных.",
            ),
            RarityFilter::NonUnique | RarityFilter::Unique => (
                "все, кроме уникальных",
                "Поиск среди вещей любой редкости, кроме уникальных. Нажмите, чтобы искать \
                 только среди вещей той же редкости, что и эта.",
            ),
        };
        toggle_chip(
            "rarity",
            Some("Редкость:"),
            value,
            hint,
            PriceCheckApp::toggle_rarity,
            cx,
        )
    });

    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(rems_from_px(6.))
        .pt(rems_from_px(10.))
        .pb(rems_from_px(6.))
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
                .then(|| chip(None, "Осквернено", TEXT_WARNING)),
        )
        .children(
            item.stack_size
                .map(|(count, _)| chip(Some("В стопке:"), count.to_string(), TEXT)),
        )
        .children(rarity)
        .children(identification)
        .children(corruption)
        .children((searchable > 0).then(|| {
            toggle_chip(
                "stats",
                Some("Св-ва:"),
                format!("{selected} из {searchable}"),
                "Сколько свойств выбрано для поиска. Нажмите, чтобы отметить все или снять все.",
                PriceCheckApp::toggle_all_filters,
                cx,
            )
        }))
}

/// A chip a click turns to its other state for the next search -- the item-type chip, when the
/// search can go by the item's class or its base type (`PriceCheckApp::toggle_scope`), the
/// rarity one (`PriceCheckApp::toggle_rarity`), the corruption and identification ones
/// (`PriceCheckApp::toggle_corruption`, `toggle_identification`), the stats count
/// (`PriceCheckApp::toggle_all_filters`) -- saying on hover what it does (`hint`).
fn toggle_chip(
    key: &'static str,
    label: Option<&'static str>,
    value: impl Into<SharedString>,
    hint: &'static str,
    toggle: fn(&mut PriceCheckApp, &mut Context<PriceCheckApp>),
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    div()
        .id(key)
        .flex_none()
        .tooltip(hints::hint(hint))
        .child(style::toggle_chip(
            "chip",
            label,
            value,
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| toggle(view, cx)),
        ))
}
