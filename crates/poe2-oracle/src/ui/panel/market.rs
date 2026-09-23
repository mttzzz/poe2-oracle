//! A Currency Exchange item's market card, priced from poe.ninja rather than from listings.

use gpui::{
    FontWeight, IntoElement, PathBuilder, canvas, div, linear_color_stop, linear_gradient, point,
    prelude::*, rgb,
};

use poe2_domain::ParsedItem;
use trade_client::ninja::{Market, MarketPrice};
use trade_client::rates::PriceUnit;

use crate::price_check::PriceCheckApp;
use crate::ui::theme::{
    BG_NAMEPLATE, BORDER, BORDER_GOLD, CONTENT_PADDING, PRICE_FALL, PRICE_RISE, TEXT_DIM,
    TEXT_MUTED, rems_from_px,
};

use super::format::{amount_in, currency_img, format_compact, format_ru};
use super::results::render_link;

/// A Currency Exchange item's market, the way poe.ninja shows it (the player's reference for
/// exchange prices): the value in the unit that reads best with its icon, the other units, how
/// many a divine buys when it's cheap, the week's chart and change, the hourly volume, the most
/// traded pair, and what the copied stack is worth.
pub(super) fn render_market_card(
    state: &PriceCheckApp,
    item: &ParsedItem,
    market: &Market,
    price: &MarketPrice,
) -> impl IntoElement {
    let divines = price.divine_value;
    let (value, unit) = value_not_in_itself(market, price, divines);
    // The other core currencies it's worth, each with its icon: "= 0,77 [ex] · 1,2 [chaos]".
    let mut equivalents = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(rems_from_px(4.))
        .text_xs()
        .text_color(rgb(TEXT_DIM))
        .child("=");
    for (index, (amount, id)) in [
        (divines, PriceUnit::Divine.trade_id()),
        (
            divines * market.exalted_per_divine,
            PriceUnit::Exalted.trade_id(),
        ),
        (divines * market.chaos_per_divine, "chaos"),
    ]
    .into_iter()
    .filter(|&(_, id)| id != unit.trade_id() && id != price.id)
    .enumerate()
    {
        if index > 0 {
            equivalents = equivalents.child("·");
        }
        equivalents = equivalents.child(amount_in(state, format_ru(amount), id, 14.));
    }
    let change_color = match price.change_7d {
        Some(change) if change < 0.0 => PRICE_FALL,
        _ => PRICE_RISE,
    };
    let stack = item
        .stack_size
        .map(|(count, _)| count)
        .filter(|&count| count > 1);

    div()
        .flex()
        .flex_col()
        .mt(rems_from_px(10.))
        .rounded_xs()
        .border_1()
        .border_color(rgb(BORDER_GOLD))
        .bg(rgb(BG_NAMEPLATE))
        .child(
            div()
                .flex()
                .items_center()
                .gap(rems_from_px(12.))
                .p(rems_from_px(CONTENT_PADDING))
                .children(currency_img(state.currency_icon(&price.id), 44.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(rems_from_px(2.))
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(rems_from_px(6.))
                                .text_size(rems_from_px(24.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("≈ {}", format_ru(value)))
                                .children(currency_img(state.currency_icon(unit.trade_id()), 24.)),
                        )
                        .child(equivalents)
                        .when(divines < 1.0, |this| {
                            this.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(rems_from_px(4.))
                                    .text_xs()
                                    .text_color(rgb(TEXT_DIM))
                                    .child(amount_in(state, "1".to_owned(), "divine", 14.))
                                    .child("=")
                                    .child(amount_in(
                                        state,
                                        format_compact(1.0 / divines),
                                        &price.id,
                                        14.,
                                    )),
                            )
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(rems_from_px(4.))
                .px(rems_from_px(CONTENT_PADDING))
                .py(rems_from_px(8.))
                .border_t_1()
                .border_color(rgb(BORDER))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .text_xs()
                        .child(div().text_color(rgb(TEXT_DIM)).child("За 7 дней"))
                        .child(
                            div()
                                .text_color(rgb(change_color))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(match price.change_7d {
                                    Some(change) => format!("{change:+.0} %"),
                                    None => "мало данных".to_owned(),
                                }),
                        ),
                )
                .child(render_sparkline(&price.sparkline, change_color)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(rems_from_px(4.))
                .px(rems_from_px(CONTENT_PADDING))
                .py(rems_from_px(8.))
                .border_t_1()
                .border_color(rgb(BORDER))
                .text_xs()
                .child(market_line(
                    "Оборот в час",
                    div()
                        .flex()
                        .items_center()
                        .gap(rems_from_px(3.))
                        .child(format_compact(price.volume_divine))
                        .children(currency_img(state.currency_icon("divine"), 14.)),
                ))
                .children(
                    most_traded_pair(state, price)
                        .map(|pair| market_line("Чаще всего меняют", pair)),
                )
                .children(stack.map(|count| {
                    let (total, total_unit) =
                        value_not_in_itself(market, price, divines * f64::from(count));
                    market_line(
                        "Ваша стопка",
                        div()
                            .flex()
                            .items_center()
                            .gap(rems_from_px(4.))
                            .child(format!("{count} шт. ≈"))
                            .child(amount_in(
                                state,
                                format_ru(total),
                                total_unit.trade_id(),
                                14.,
                            )),
                    )
                })),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .px(rems_from_px(CONTENT_PADDING))
                .py(rems_from_px(6.))
                .border_t_1()
                .border_color(rgb(BORDER))
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(format!(
                    "{} · валютная биржа, обновление раз в час",
                    ninja_category_ru(&price.category)
                ))
                .child(render_link("poe.ninja ↗", price.details_url.clone())),
        )
}

/// `divines` in the unit that reads best for it -- except the item's own unit: a core currency is
/// never priced in itself, so a Divine Orb reads in exalted and an Exalted Orb in divines.
fn value_not_in_itself(market: &Market, price: &MarketPrice, divines: f64) -> (f64, PriceUnit) {
    match market.in_display_unit(divines) {
        (_, PriceUnit::Divine) if price.id == PriceUnit::Divine.trade_id() => {
            (divines * market.exalted_per_divine, PriceUnit::Exalted)
        }
        (_, PriceUnit::Exalted) if price.id == PriceUnit::Exalted.trade_id() => {
            (divines, PriceUnit::Divine)
        }
        shown => shown,
    }
}

fn market_line(label: &'static str, value: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .justify_between()
        .items_center()
        .child(div().text_color(rgb(TEXT_DIM)).child(label))
        .child(value)
}

/// poe.ninja's "Most Popular" column: the core currency the item trades against most and the
/// rate, written the way that reads best -- "1 [div] ⇆ 164 [item]" for a cheap item, "4,1k [div]
/// ⇆ 1 [item]" for a dear one. The rate always means units of the item per unit of that currency.
fn most_traded_pair(state: &PriceCheckApp, price: &MarketPrice) -> Option<impl IntoElement> {
    let currency = price.most_traded_with.as_deref()?;
    let rate = price
        .most_traded_rate
        .filter(|rate| rate.is_finite() && *rate > 0.0)?;
    let (currency_amount, item_amount) = if rate >= 1.0 {
        (1.0, rate)
    } else {
        (1.0 / rate, 1.0)
    };
    Some(
        div()
            .flex()
            .items_center()
            .gap(rems_from_px(3.))
            .child(format_compact(currency_amount))
            .children(currency_img(state.currency_icon(currency), 14.))
            .child(format!("⇆ {}", format_compact(item_amount)))
            .children(currency_img(state.currency_icon(&price.id), 14.)),
    )
}

/// poe.ninja's sidebar page titles, in Russian.
fn ninja_category_ru(category: &str) -> &str {
    match category {
        "Currency" => "Валюта",
        "Fragments" => "Фрагменты",
        "Abyssal Bones" => "Кости Бездны",
        "Uncut Gems" => "Неогранённые камни",
        "Lineage Gems" => "Династические камни",
        "Essences" => "Сущности",
        "Soul Cores" => "Ядра душ",
        "Idols" => "Идолы",
        "Runes" => "Руны",
        "Omens" => "Предзнаменования",
        "Expedition" => "Экспедиция",
        "Liquid Emotions" => "Жидкие эмоции",
        "Catalysts" => "Катализаторы",
        "Verisium" => "Веризий",
        other => other,
    }
}

/// poe.ninja's "Last 7 days" chart: the day-by-day change (percent against 7 days ago) as a line
/// over a fading area, in the colour of the week's change -- drawn the way poe.ninja draws it:
/// points evenly spaced by day, the line broken where a day had no trades, and a y axis that
/// always spans at least -5..+5 % so a flat week reads flat.
fn render_sparkline(points: &[Option<f64>], color: u32) -> impl IntoElement {
    let count = points.len();
    let points = points.to_vec();
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let (low, high) = points
                .iter()
                .flatten()
                .fold((-5.0_f64, 5.0_f64), |(low, high), &value| {
                    (low.min(value), high.max(value))
                });
            // Painted in pixels, scaled with the rest of the panel.
            let rem_size = window.rem_size();
            let inset = rems_from_px(2.).to_pixels(rem_size);
            let width = bounds.size.width - inset * 2.;
            let height = bounds.size.height - inset * 2.;
            let at = |index: usize, value: f64| {
                point(
                    bounds.left()
                        + inset
                        + width * (index as f32 / count.saturating_sub(1).max(1) as f32),
                    bounds.top() + inset + height * ((high - value) / (high - low)) as f32,
                )
            };

            // Each run of consecutive trading days is its own line.
            let runs = points
                .iter()
                .enumerate()
                .collect::<Vec<_>>()
                .split(|(_, value)| value.is_none())
                .map(|run| {
                    run.iter()
                        .filter_map(|&(index, value)| value.map(|value| at(index, value)))
                        .collect::<Vec<_>>()
                })
                .filter(|run| run.len() >= 2)
                .collect::<Vec<_>>();
            for run in &runs {
                let (first, last) = (run[0], run[run.len() - 1]);
                let mut area = PathBuilder::fill();
                area.move_to(point(first.x, bounds.bottom()));
                for &vertex in run {
                    area.line_to(vertex);
                }
                area.line_to(point(last.x, bounds.bottom()));
                area.close();
                if let Ok(path) = area.build() {
                    window.paint_path(
                        path,
                        linear_gradient(
                            180.,
                            linear_color_stop(rgb(color).alpha(0.35), 0.),
                            linear_color_stop(rgb(color).alpha(0.), 1.),
                        ),
                    );
                }
                let mut line = PathBuilder::stroke(rems_from_px(1.5).to_pixels(rem_size));
                line.move_to(first);
                for &vertex in &run[1..] {
                    line.line_to(vertex);
                }
                if let Ok(path) = line.build() {
                    window.paint_path(path, rgb(color));
                }
            }
        },
    )
    .w_full()
    .h(rems_from_px(48.))
}
