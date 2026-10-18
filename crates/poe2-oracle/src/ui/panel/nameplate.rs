//! The item's header: its art, its name in the game's own colour and face, links to its poe2db
//! and wiki pages and to Craft of Exile, an ornament rule under it all -- and EE2's chip row under
//! the header. The header is tinted with the name's colour the way the game's tooltip banners
//! are: rarity reads before the name does.

use gpui::{
    AnyElement, Context, IntoElement, MouseDownEvent, ObjectFit, SharedString, Window, div, img,
    linear_color_stop, linear_gradient, prelude::*, rgb,
};

use poe2_domain::{ItemRarity, ParsedItem};
use trade_client::{RarityFilter, TradeSite};

use crate::craft_link;
use crate::i18n;
use crate::item_refs;
use crate::price_check::{ChipMenu, PriceCheckApp, StateFilter};
use crate::tr;
use crate::ui::fonts;
use crate::ui::hint as hints;
use crate::ui::style::{self, chip, heading, link, menu_row, ornament_rule};
use crate::ui::theme::{
    BANNER_EDGE, BANNER_TINT, BG_NAMEPLATE, CONTENT_PADDING, CURRENCY_NAME, GEM_NAME, RARITY_MAGIC,
    RARITY_NORMAL, RARITY_RARE, RARITY_UNIQUE, TEXT, TEXT_DIM, TEXT_VALUE, TEXT_WARNING, blend,
    rems_from_px,
};

use super::filters::shows_property;
use super::menu::render_menu;
use super::results::render_link;

/// Edge of the item art beside the name.
const ART_SIZE: f32 = 48.;
/// The least width of a chip-row select's menu.
const CHIP_MENU_WIDTH: f32 = 200.;

/// The item's name -- and, for rares and uniques, its base type -- in the game's own name colour
/// and in the stand-in for its tooltip face on the item's client language (see `fonts`), like its
/// tooltip header; beside it the item's art, and under it links to the item's poe2db and wiki
/// pages (`item_refs`), when the item database knows it, to the item in Craft of Exile
/// (`craft_link`, the site in the interface language), when the site crafts it, and to the report
/// window about the item read or priced wrong (`PriceCheckApp::report_item`).
pub(super) fn render_nameplate(
    item: &ParsedItem,
    site: TradeSite,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let name_font = fonts::name_font(site);
    let refs = item_refs::refs_for(item);
    let art = refs.and_then(|found| found.icon_url());
    let color = name_color(item);
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
        .children(refs.map(|found| render_link(tr!("wiki ↗"), found.wiki_url())))
        .children(
            craft_link::url(item, craft_link::site_language(i18n::lang()))
                .map(|url| render_link("Craft of Exile ↗", url)),
        )
        .child(
            div()
                .id("report-item")
                .flex_none()
                .tooltip(hints::hint(tr!(
                    "Tell the developer what's wrong with this item; its text is attached"
                )))
                .child(link(
                    "link",
                    tr!("report a problem"),
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

/// Item class (or the base type the search goes by), which unique an unidentified one is, level,
/// requirement, sockets, quality, corruption, sanctification, fracture, and how many of the
/// listed searchable stats are selected -- EE2's chip row. The level, sockets and quality stay out
/// where a property row or chip below shows the same value (`filters::shows_property`). A click on
/// the count checks them all, or all off (`PriceCheckApp::toggle_all_filters`).
pub(super) fn render_chips(
    state: &PriceCheckApp,
    item: &ParsedItem,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let (selected, searchable) = state
        .filters
        .iter()
        .filter(|filter| filter.searchable() && !filter.hidden)
        .fold((0, 0), |(selected, total), filter| {
            (selected + usize::from(filter.enabled), total + 1)
        });
    // What a property row or chip below already shows at the same value isn't repeated here.
    let shown = |trade_ids: &[&str], value: u32| {
        trade_ids
            .iter()
            .any(|id| shows_property(state, id, f64::from(value)))
    };
    let item_level = item
        .item_level
        .filter(|&level| !shown(&["type_filters.ilvl"], level));
    let sockets = item
        .sockets
        .map(|sockets| sockets.current)
        .or_else(|| item.gem_sockets.map(|sockets| sockets.number))
        .filter(|&count| {
            !shown(
                &["equipment_filters.rune_sockets", "misc_filters.gem_sockets"],
                count,
            )
        });
    let quality = item
        .quality
        .filter(|&quality| !shown(&["type_filters.quality"], quality));
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
                    tr!("Base:"),
                    base_type.clone(),
                    tr!("Searches this base only. Click to search every item of the class."),
                ),
                None => (
                    tr!("Class:"),
                    class.unwrap_or_default(),
                    tr!("Searches every item of the class. Click to search only this item's base."),
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
    // Which unique an unidentified one is, where its base has several (`PriceCheckApp::unique`):
    // told by its art, asked for when the art doesn't tell, or picked here.
    let unique = state.unique.as_ref().map(|choice| {
        let choices = choice
            .names
            .iter()
            .cloned()
            .map(SharedString::from)
            .collect();
        select_chip(
            SelectChip {
                menu: ChipMenu::Unique,
                label: Some(tr!("Unique:")),
                choices,
                current: choice.picked,
                hint: tr!(
                    "The text of an unidentified item doesn't say which unique it is — its art \
                     does, and the app tells it by the art under the cursor. Pick another one to \
                     search for its unidentified listings instead: the trade site tells them \
                     apart, and their prices differ a lot."
                ),
                choose: PriceCheckApp::choose_unique,
            },
            state,
            window,
            cx,
        )
    });
    // Which listings the search takes by corruption, and a sanctified or fractured item's by
    // sanctification or fracture: those in the item's own state by default
    // (`PriceCheckApp::corruption`, `sanctification`, `fracture`).
    let corruption = state.corruption.map(|current| {
        select_chip(
            state_chip(
                ChipMenu::Corruption,
                current,
                corruption_label,
                tr!(
                    "Which listings count by corruption. It starts on this item's own state: \
                     corrupted ones can't be improved, and corruption changes their modifiers. \
                     Picking one searches again at once."
                ),
                PriceCheckApp::choose_corruption,
            ),
            state,
            window,
            cx,
        )
    });
    let sanctification = state.sanctification.map(|current| {
        select_chip(
            state_chip(
                ChipMenu::Sanctification,
                current,
                sanctification_label,
                tr!(
                    "Which listings count by sanctification. It starts on this item's own state: \
                     sanctified ones can't be modified, and sanctifying moved their modifiers' \
                     values up or down, past their usual ranges too. Picking one searches again \
                     at once."
                ),
                PriceCheckApp::choose_sanctification,
            ),
            state,
            window,
            cx,
        )
    });
    let fracture = state.fracture.map(|current| {
        select_chip(
            state_chip(
                ChipMenu::Fracture,
                current,
                fracture_label,
                tr!(
                    "Which listings count by fracture. It starts on this item's own state: a \
                     fractured modifier can't be changed, so fractured items sell at other \
                     prices. Picking one searches again at once."
                ),
                PriceCheckApp::choose_fracture,
            ),
            state,
            window,
            cx,
        )
    });
    let identification = state.identification.map(|choice| {
        let (value, hint) = if choice.on {
            (
                tr!("Unidentified only"),
                tr!(
                    "This item is unidentified: only unidentified listings count — identified \
                     ones sell for their modifiers. Click to include identified ones."
                ),
            )
        } else {
            (
                tr!("Identified too"),
                tr!("Identified listings count too. Click to search only for unidentified ones."),
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
    // The rarity the search takes: the item's own by default (`PriceCheckApp::rarity`).
    let rarity = state.rarity.map(|choice| {
        select_chip(
            SelectChip {
                menu: ChipMenu::Rarity,
                label: Some(tr!("Rarity:")),
                choices: choice
                    .choices
                    .iter()
                    .map(|&rarity| rarity_label(rarity).into())
                    .collect(),
                current: choice
                    .choices
                    .iter()
                    .position(|&rarity| rarity == choice.current),
                hint: tr!(
                    "Which rarity the search takes. It starts on this item's own: a magic item is \
                     compared with magic ones, a rare with rares. Picking one searches again at \
                     once."
                ),
                choose: PriceCheckApp::choose_rarity,
            },
            state,
            window,
            cx,
        )
    });

    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(rems_from_px(6.))
        .gap_y(rems_from_px(4.))
        .pt(rems_from_px(8.))
        .children(item_type)
        .children(unique)
        .children(item_level.map(|level| chip(Some(tr!("Item Level:")), level.to_string(), TEXT)))
        .children(
            required_level.map(|level| chip(Some(tr!("Required Level:")), level.to_string(), TEXT)),
        )
        .children(sockets.map(|count| chip(Some(tr!("Sockets:")), count.to_string(), TEXT)))
        .children(
            quality.map(|quality| chip(Some(tr!("Quality:")), format!("+{quality}%"), TEXT_VALUE)),
        )
        .children(
            item.is_corrupted
                .then(|| chip(None, tr!("Corrupted"), TEXT_WARNING)),
        )
        .children(
            item.stack_size
                .map(|(count, _)| chip(Some(tr!("Stack Size:")), count.to_string(), TEXT)),
        )
        .children(rarity)
        .children(identification)
        .children(corruption)
        .children(sanctification)
        .children(fracture)
        .children((searchable > 0).then(|| {
            toggle_chip(
                "stats",
                Some(tr!("Stats:")),
                tr!(
                    "{selected} of {total}",
                    selected = selected,
                    total = searchable
                ),
                tr!("How many stats the search uses. Click to check them all or uncheck them all."),
                PriceCheckApp::toggle_all_filters,
                cx,
            )
        }))
}

/// A rarity as the rarity select says it.
fn rarity_label(rarity: RarityFilter) -> &'static str {
    match rarity {
        RarityFilter::Normal => tr!("Normal"),
        RarityFilter::Magic => tr!("Magic"),
        RarityFilter::Rare => tr!("Rare"),
        RarityFilter::NonUnique | RarityFilter::Unique => tr!("Any Non-Unique"),
    }
}

/// A corruption choice as the corruption select says it.
fn corruption_label(choice: StateFilter) -> &'static str {
    match choice {
        StateFilter::Without => tr!("Uncorrupted only"),
        StateFilter::With => tr!("Corrupted only"),
        StateFilter::Either => tr!("Corrupted or not"),
    }
}

/// A sanctification choice as the sanctification select says it.
fn sanctification_label(choice: StateFilter) -> &'static str {
    match choice {
        StateFilter::Without => tr!("Unsanctified only"),
        StateFilter::With => tr!("Sanctified only"),
        StateFilter::Either => tr!("Sanctified or not"),
    }
}

/// A fracture choice as the fracture select says it.
fn fracture_label(choice: StateFilter) -> &'static str {
    match choice {
        StateFilter::Without => tr!("Unfractured only"),
        StateFilter::With => tr!("Fractured only"),
        StateFilter::Either => tr!("Fractured or not"),
    }
}

/// A select of the listings the search takes by a state an item has or not, at `current`: its
/// choices (`StateFilter::CHOICES`) as `label` says them.
fn state_chip(
    menu: ChipMenu,
    current: StateFilter,
    label: fn(StateFilter) -> &'static str,
    hint: &'static str,
    choose: fn(&mut PriceCheckApp, usize, &mut Context<PriceCheckApp>),
) -> SelectChip {
    SelectChip {
        menu,
        label: None,
        choices: StateFilter::CHOICES
            .iter()
            .map(|&choice| label(choice).into())
            .collect(),
        current: StateFilter::CHOICES
            .iter()
            .position(|&choice| choice == current),
        hint,
        choose,
    }
}

/// A select of the chip row ([`select_chip`]): `label` before its current choice (`?` while it has
/// none), `hint` on hover, and its menu `menu` of `choices`, a pick of which runs `choose` with its
/// index.
struct SelectChip {
    menu: ChipMenu,
    label: Option<&'static str>,
    choices: Vec<SharedString>,
    current: Option<usize>,
    hint: &'static str,
    choose: fn(&mut PriceCheckApp, usize, &mut Context<PriceCheckApp>),
}

/// A chip-row select -- which unique an unidentified one is, the rarity, the corruption, the
/// sanctification, the fracture: its label and current choice in a compact select with a gold ▾,
/// which opens its menu below it, every choice listed and the current one marked
/// (`menu::render_menu`).
fn select_chip(
    chip: SelectChip,
    state: &PriceCheckApp,
    window: &Window,
    cx: &Context<PriceCheckApp>,
) -> impl IntoElement {
    let (key, menu_key) = match chip.menu {
        ChipMenu::Unique => ("unique", "unique-menu"),
        ChipMenu::Rarity => ("rarity", "rarity-menu"),
        ChipMenu::Corruption => ("corruption", "corruption-menu"),
        ChipMenu::Sanctification => ("sanctification", "sanctification-menu"),
        ChipMenu::Fracture => ("fracture", "fracture-menu"),
    };
    let SelectChip {
        menu,
        label,
        choices,
        current,
        hint,
        choose,
    } = chip;
    let value = current
        .and_then(|current| choices.get(current).cloned())
        .unwrap_or_else(|| "?".into());
    div()
        .id(key)
        .flex()
        .flex_col()
        .flex_none()
        // The hint would cover the menu hanging below: none while it's open.
        .when(state.chip_menu != Some(menu), |this| {
            this.tooltip(hints::hint(hint))
        })
        .child(style::select(
            "select",
            div()
                .flex()
                .gap(rems_from_px(4.))
                .children(label.map(|label| div().text_color(rgb(TEXT_DIM)).child(label)))
                .child(value),
            true,
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                view.set_chip_menu(Some(menu), cx);
            }),
        ))
        .when(state.chip_menu == Some(menu), |this| {
            let rows = choices.into_iter().enumerate().map(|(at, choice)| {
                menu_row(
                    at,
                    Some(at) == current,
                    div().min_w_0().truncate().child(choice),
                    cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                        choose(view, at, cx);
                    }),
                )
            });
            this.child(render_menu(
                menu_key,
                rows,
                CHIP_MENU_WIDTH,
                |view, cx| view.set_chip_menu(None, cx),
                window,
                cx,
            ))
        })
}

/// A chip a click turns to its other state for the next search -- the item-type chip, when the
/// search can go by the item's class or its base type (`PriceCheckApp::toggle_scope`), the
/// identification one (`PriceCheckApp::toggle_identification`), the stats count
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
