//! The palette and spacing the price-check panel (`ui::panel`), the settings window and the XP
//! overlay all draw with, so the three read as one app.

use gpui::{Rems, rems};

/// GPUI's rem size at 100% UI scale. The overlay's rem size is this times `Settings::ui_scale`
/// (`app::PriceCheckRoot`), so everything the panel sizes in rems follows the player's scale.
pub(crate) const BASE_REM_SIZE: f32 = 16.;

/// A length laid out in pixels at 100% UI scale, as rems: it grows and shrinks with the player's
/// UI scale the way text does.
pub(crate) fn rems_from_px(px: f32) -> Rems {
    rems(px / BASE_REM_SIZE)
}

/// `over` laid on `base` at `amount` (0 is `base`, 1 is `over`): a tint of an opaque colour, for
/// the rarity-coloured nameplate.
pub(crate) fn blend(base: u32, over: u32, amount: f32) -> u32 {
    let channel = |shift: u32| {
        let from = ((base >> shift) & 0xff) as f32;
        let to = ((over >> shift) & 0xff) as f32;
        ((from + (to - from) * amount).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// How much of an item name's colour the top of its header takes, fading to none at the bottom,
/// the way the game's tooltip banners are tinted: rarity reads before the name does.
pub(crate) const BANNER_TINT: f32 = 0.16;
/// How much of it the line under the header takes.
pub(crate) const BANNER_EDGE: f32 = 0.45;

pub(crate) const BG_PANEL: u32 = 0x0e0e10;
pub(crate) const BG_TITLE: u32 = 0x060607;
pub(crate) const BG_NAMEPLATE: u32 = 0x17130d;
pub(crate) const BG_CONTROL: u32 = 0x1e1e22;
pub(crate) const BG_ROW_STRIPE: u32 = 0x17171a;
pub(crate) const BG_BUTTON: u32 = 0x1f1811;
pub(crate) const BG_BUTTON_HOVER: u32 = 0x33261a;
pub(crate) const BG_CLOSE_HOVER: u32 = 0x8b2a1e;
pub(crate) const BORDER: u32 = 0x2b2b30;
pub(crate) const BORDER_GOLD: u32 = 0x6b5022;
pub(crate) const GOLD: u32 = 0xd0913b;
pub(crate) const TEXT: u32 = 0xdcdcdc;
pub(crate) const TEXT_DIM: u32 = 0x9d9da3;
pub(crate) const TEXT_MUTED: u32 = 0x66666c;
pub(crate) const TEXT_WARNING: u32 = 0xe0664f;
/// The game's colour for rolled values in item tooltips.
pub(crate) const TEXT_VALUE: u32 = 0x8888ff;
pub(crate) const TIER_TOP: u32 = 0xecc94b;
/// poe.ninja's colours for a price that rose / fell over the week.
pub(crate) const PRICE_RISE: u32 = 0x4fc97f;
pub(crate) const PRICE_FALL: u32 = 0xe06c6c;
/// The game's name colours for currency and gems, which have no rarity of their own.
pub(crate) const CURRENCY_NAME: u32 = 0xaa9e82;
pub(crate) const GEM_NAME: u32 = 0x1ba29b;

// The game's rarity colours, as in EE2's `tailwind.config.js`.
pub(crate) const RARITY_NORMAL: u32 = 0xc8c8c8;
pub(crate) const RARITY_MAGIC: u32 = 0x8888ff;
pub(crate) const RARITY_RARE: u32 = 0xffff77;
pub(crate) const RARITY_UNIQUE: u32 = 0xaf6025;

// The game's item tooltip beyond its name colours, as the PoE wiki's `c` template and Path of
// Building draw it.
/// The tooltip's black.
pub(crate) const BG_ITEM_CARD: u32 = 0x000000;
/// Unmet requirements, and the Corrupted and Unidentified lines.
pub(crate) const GAME_RED: u32 = 0xd20000;
/// Damage values by kind; physical damage is plain white.
pub(crate) const DAMAGE_FIRE: u32 = 0x960000;
pub(crate) const DAMAGE_COLD: u32 = 0x366492;
pub(crate) const DAMAGE_LIGHTNING: u32 = 0xffd700;
pub(crate) const DAMAGE_CHAOS: u32 = 0xd02090;
/// Enchanted, rune and crafted mods.
pub(crate) const MOD_ENCHANTED: u32 = 0xb4b4ff;
/// Fractured mods, and the Fractured Item and Sanctified lines.
pub(crate) const MOD_FRACTURED: u32 = 0xa29162;
/// Desecrated mods. The game draws a revealed one like any other mod; this is the green of the
/// Abyss's unrevealed runes, muted to read on the tooltip's black, so they stand out as the
/// panel's own "очернённый" badge makes them.
pub(crate) const MOD_DESECRATED: u32 = 0x6fae8c;

/// Horizontal inset of everything below the panel's nameplate and the settings window's title
/// bar.
pub(crate) const CONTENT_PADDING: f32 = 12.;
