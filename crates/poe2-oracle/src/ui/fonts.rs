//! The item-name typefaces. The game draws a tooltip's name in Fontin SmallCaps on an English
//! client and in Friz Quadrata on a Russian one. Neither can ship with this app: Fontin's free
//! license excludes embedding in software without exljbris' paid extended license
//! (<https://www.exljbris.com/extended_license.html>), and Friz Quadrata is a commercial ITC face.
//! The nameplate uses the closest SIL Open Font License faces instead, bundled with their license
//! texts under `assets/fonts/`: Alegreya SC Medium for Fontin SmallCaps (a small-caps serif of the
//! same weight) and Philosopher Bold for Friz Quadrata (the same sturdy flared letterforms, with
//! full Cyrillic).

use std::borrow::Cow;

use gpui::{App, FontWeight};
use trade_client::TradeSite;

/// A nameplate typeface: its family and the one weight bundled for it.
pub struct NameFont {
    pub family: &'static str,
    pub weight: FontWeight,
}

const RUSSIAN_NAME: NameFont = NameFont {
    family: "Philosopher",
    weight: FontWeight::BOLD,
};

const ENGLISH_NAME: NameFont = NameFont {
    family: "Alegreya SC",
    weight: FontWeight::MEDIUM,
};

/// The nameplate typeface for an item copied from `site`'s client language, as in the game.
pub fn name_font(site: TradeSite) -> &'static NameFont {
    match site {
        TradeSite::Russian => &RUSSIAN_NAME,
        TradeSite::International => &ENGLISH_NAME,
    }
}

/// Makes the bundled faces available to every window. A family that failed to register falls
/// back to GPUI's default stack (Segoe UI), so a failure costs only the look.
pub fn register(cx: &App) -> anyhow::Result<()> {
    cx.text_system().add_fonts(vec![
        Cow::Borrowed(include_bytes!("../../assets/fonts/Philosopher-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/AlegreyaSC-Medium.ttf")),
    ])
}
