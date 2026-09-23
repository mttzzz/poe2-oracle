//! Where the price-check panel goes on screen, ported from EE2's own layout math so the panel
//! sits exactly where an EE2 user expects it:
//!
//! - `renderer/src/web/overlay/OverlayWindow.vue`: PoE's side panel (inventory on the right, stash
//!   on the left) is 986px wide at 1600px game height, i.e. `game_height * 986 / 1600`.
//! - `renderer/src/web/price-check/PriceCheckWindow.vue`: the price window spans the full game
//!   height and is glued to the inventory panel's left edge when the hotkey was pressed over the
//!   right half of the game, to the stash panel's right edge otherwise.
//!
//! EE2's window is `28.75rem` wide; this one is `32rem`: Russian stat lines run noticeably longer
//! than the English ones EE2's width was sized for.
//!
//! (EE2 also closes an untouched panel when the cursor drifts off the item; this app deliberately
//! does not -- the player asked for Esc to be the only way the panel closes, see
//! `platform::esc_hook`.)
//!
//! Everything here is physical pixels. `scale` is the monitor's DPI scale (1.0 at 96 DPI) times
//! the player's UI scale, so the `rem`-based width lands at the same visual size as EE2's CSS
//! pixels and grows with the panel's text. Pure math, deliberately not Windows-gated, so the
//! formula is covered by the native CI test pass.

/// EE2's default `fontSize` (`renderer/src/web/Config.ts`): the `rem` base below.
const EE2_FONT_SIZE_PX: f64 = 16.0;
const PANEL_WIDTH_REM: f64 = 32.0;
const SIDEBAR_WIDTH_PER_GAME_HEIGHT: f64 = 986.0 / 1600.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The panel's rect for a check triggered with the cursor at `cursor_x`, given the game window's
/// client area `game`. A panel wider than the room beside the side panel is narrowed to fit, so
/// it never covers the inventory or stash, nor leaves the game.
pub fn panel_rect(game: PhysicalRect, cursor_x: i32, scale: f64) -> PhysicalRect {
    let sidebar = (f64::from(game.height) * SIDEBAR_WIDTH_PER_GAME_HEIGHT).round() as i32;
    let width = ((PANEL_WIDTH_REM * EE2_FONT_SIZE_PX * scale).round() as i32)
        .min(game.width - sidebar)
        .max(0);
    let x = if cursor_x > game.x + game.width / 2 {
        game.x + game.width - sidebar - width
    } else {
        game.x + sidebar
    };
    PhysicalRect {
        x,
        y: game.y,
        width,
        height: game.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test machine's real setup: fullscreen 3840x2160 at 200% scaling.
    const GAME_4K: PhysicalRect = PhysicalRect {
        x: 0,
        y: 0,
        width: 3840,
        height: 2160,
    };

    #[test]
    fn inventory_side_panel_ends_where_the_inventory_panel_starts() {
        let panel = panel_rect(GAME_4K, 2958, 2.0);
        let inventory_left_edge = 3840 - (2160.0_f64 * 986.0 / 1600.0).round() as i32;
        assert_eq!(panel.x + panel.width, inventory_left_edge);
        assert_eq!(panel_rect(GAME_4K, 2958, 1.0).width * 2, panel.width);
        assert_eq!((panel.y, panel.height), (0, 2160));
    }

    #[test]
    fn stash_side_panel_starts_where_the_stash_panel_ends() {
        let panel = panel_rect(GAME_4K, 500, 2.0);
        assert_eq!(panel.x, (2160.0_f64 * 986.0 / 1600.0).round() as i32);
    }

    #[test]
    fn windowed_game_offset_is_respected() {
        let game = PhysicalRect {
            x: 100,
            y: 50,
            width: 1600,
            height: 900,
        };
        let panel = panel_rect(game, 1500, 1.0);
        assert_eq!(panel.x + panel.width, 100 + 1600 - 555);
        assert_eq!((panel.y, panel.height), (50, 900));
    }

    /// A 4:3 game at 150% UI scale has no room for the full-width panel beside the side panel.
    #[test]
    fn panel_too_wide_for_the_game_is_narrowed_beside_the_side_panel() {
        let game = PhysicalRect {
            x: 0,
            y: 0,
            width: 1024,
            height: 768,
        };
        let sidebar = (768.0_f64 * 986.0 / 1600.0).round() as i32;
        let inventory_side = panel_rect(game, 900, 1.5);
        assert_eq!(
            (inventory_side.x, inventory_side.x + inventory_side.width),
            (0, 1024 - sidebar)
        );
        let stash_side = panel_rect(game, 100, 1.5);
        assert_eq!(
            (stash_side.x, stash_side.x + stash_side.width),
            (sidebar, 1024)
        );
    }
}
