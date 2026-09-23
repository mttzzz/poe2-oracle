//! The palette and spacing the price-check panel (`ui::panel`), the settings window and the
//! overlays all draw with, so they read as one app -- but for the XP overlay's plates, inlaid in
//! the game's HUD, which take the HUD's own colours (the `HUD_*` ones).

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
pub(crate) const BG_BUTTON_HOVER: u32 = 0x33261a;
pub(crate) const BG_CLOSE_HOVER: u32 = 0x8b2a1e;
pub(crate) const BORDER_GOLD: u32 = 0x6b5022;
pub(crate) const GOLD: u32 = 0xd0913b;
pub(crate) const TEXT: u32 = 0xdcdcdc;
pub(crate) const TEXT_DIM: u32 = 0x9d9da3;
pub(crate) const TEXT_MUTED: u32 = 0x66666c;
pub(crate) const TEXT_WARNING: u32 = 0xe0664f;
/// The game's colour for rolled values in item tooltips.
pub(crate) const TEXT_VALUE: u32 = 0x8888ff;
pub(crate) const TIER_TOP: u32 = 0xecc94b;
/// Colours of a price that rose / fell over the week.
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

// EE2's badges and markers (its `tailwind.config.js` palette), which the panel keeps so a player
// coming from EE2 reads them at a glance.
/// Text on a filled light badge: the top-tier badge, the fractured badge.
pub(crate) const BADGE_INK: u32 = 0x000000;
/// The source badges of rune and crafted mods (`.tag-rune`, `.tag-crafted`).
pub(crate) const BADGE_RUNE_BG: u32 = 0x3182ce;
pub(crate) const BADGE_RUNE_TEXT: u32 = 0xebf8ff;
/// The fractured source badge (`.tag-fractured`).
pub(crate) const BADGE_FRACTURED_BG: u32 = 0xf6e05e;
/// The enchant source badge (`.tag-enchant`).
pub(crate) const BADGE_ENCHANT_BG: u32 = 0x805ad5;
pub(crate) const BADGE_ENCHANT_TEXT: u32 = 0xfaf5ff;
/// The desecrated source badge (`.tag-desecrated`).
pub(crate) const BADGE_DESECRATED_BG: u32 = 0x22543d;
pub(crate) const BADGE_DESECRATED_TEXT: u32 = 0xf0fff4;
/// A seller's status dot: online, away, offline.
pub(crate) const STATUS_ONLINE: u32 = 0xf687b3;
pub(crate) const STATUS_AFK: u32 = 0xed8936;
pub(crate) const STATUS_OFFLINE: u32 = 0xe53e3e;
/// The player's waystone mod marks: danger, warning, wanted.
pub(crate) const MARK_DANGER: u32 = 0xe53e3e;
pub(crate) const MARK_WARNING: u32 = 0xed8936;
pub(crate) const MARK_WANTED: u32 = 0x48bb78;

/// Horizontal inset of everything below the panel's nameplate and the settings window's title
/// bar.
pub(crate) const CONTENT_PADDING: f32 = 12.;

// The game-styled look `ui::style` draws, as the owner approved it on the style mockup.
/// Gold lifted for text on black: headings, the current section, a hovered control's label.
pub(crate) const GOLD_LIGHT: u32 = 0xebc27a;
/// A title bar's gradient, top to bottom: the game's bronze fading into black.
pub(crate) const TITLE_TOP: u32 = 0x1c160f;
pub(crate) const TITLE_BOTTOM: u32 = 0x09090a;
/// The settings window's sidebar, a step below its content.
pub(crate) const BG_SIDEBAR: u32 = 0x0a0a0c;
/// A card grouping rows, a step above the window; its edge, and the hairline between its rows.
pub(crate) const BG_CARD: u32 = 0x141417;
pub(crate) const BORDER_CARD: u32 = 0x25252b;
pub(crate) const BORDER_ROW: u32 = 0x1d1d22;
/// A field, select or segmented choice at rest, and its edge.
pub(crate) const BG_FIELD: u32 = 0x0b0b0d;
pub(crate) const BORDER_FIELD: u32 = 0x36363d;
/// A keycap's top, fading down into `BG_FIELD`.
pub(crate) const KEY_TOP: u32 = 0x202026;
/// A menu's or tooltip's fill.
pub(crate) const BG_MENU: u32 = 0x0c0c0e;
/// A primary button's bronze plate, top and bottom.
pub(crate) const PLATE_TOP: u32 = 0x3d2c16;
pub(crate) const PLATE_BOTTOM: u32 = 0x1f170c;
/// A destructive button's edge; its label is `TEXT_WARNING`.
pub(crate) const BORDER_DANGER: u32 = 0x5e2c22;

// The game's own HUD, sampled live 2026-09-23 on the test machine's 4K game, for the plates the
// XP overlay inlays in its rails (`ui::xp_overlay`).
/// A recessed slot's near-black, top and bottom: the plate of the menu button by the flasks.
pub(crate) const HUD_SLOT_TOP: u32 = 0x100f0e;
pub(crate) const HUD_SLOT_BOTTOM: u32 = 0x1d1b17;
/// The slot's rim: in shade along its top, under the rail's lip, and lit bronze along its bottom,
/// like the rims of the HUD's buttons.
pub(crate) const HUD_RIM_SHADE: u32 = 0x060607;
pub(crate) const HUD_RIM_LIGHT: u32 = 0x544832;
/// The HUD's text: the charm counts' cream, a muted step of it for words, and the stash's gold.
pub(crate) const HUD_TEXT: u32 = 0xe4dab8;
pub(crate) const HUD_LABEL: u32 = 0x8f8772;
pub(crate) const HUD_GOLD: u32 = 0xc4aa57;
