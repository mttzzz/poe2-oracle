//! The currency formatting the panel's parts share; numbers go through `crate::i18n`.

use gpui::{IntoElement, div, img, prelude::*, rgb};

use crate::price_check::PriceCheckApp;
use crate::ui::theme::{TEXT_DIM, rems_from_px};

pub(crate) fn currency_img(url: Option<&str>, size: f32) -> Option<impl IntoElement> {
    url.map(|url| {
        img(url.to_owned())
            .w(rems_from_px(size))
            .h(rems_from_px(size))
            .flex_none()
    })
}

/// `amount` followed by `currency`'s icon, `size` rems square -- `0,026 [divine]` -- the way
/// every price in the panel reads: currencies go by their icons, never by words like "div" or
/// "chaos", so a price reads the same in the title bar, the market card and the listings. The
/// currency's name stands in only where the catalog has no icon for it.
pub(super) fn amount_in(
    state: &PriceCheckApp,
    amount: String,
    currency: &str,
    size: f32,
) -> impl IntoElement {
    let icon = currency_img(state.currency_icon(currency), size);
    let name = icon
        .is_none()
        .then(|| state.currency_name(currency).unwrap_or(currency).to_owned());
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(rems_from_px(3.))
        .child(amount)
        .children(icon)
        .children(name.map(|name| div().text_color(rgb(TEXT_DIM)).child(name)))
}

/// A roll as the game's item text writes it, with a point in either client language: up to two
/// decimals, trailing zeros dropped -- `29`, `10.4`, `1.25`.
pub(super) fn format_value(value: f64) -> String {
    let fixed = format!("{value:.2}");
    fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
}
