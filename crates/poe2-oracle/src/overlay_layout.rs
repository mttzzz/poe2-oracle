//! Where the app's windows go on the game's screen: the XP overlay's plates in the HUD's rails
//! ([`hud_rails`]), and the price-check panel, ported from EE2's own layout math so the panel
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
//! The player can drag the panel sideways by its title bar ([`dragged`]), never off the game's
//! monitor. Where it was left is kept for each side ([`PanelPositions`]) and used by the next check
//! on that side, until a double-click on the title bar forgets it.
//!
//! (EE2 also closes an untouched panel when the cursor drifts off the item; this app deliberately
//! does not -- the player asked for Esc to be the only way the panel closes, see
//! `platform::esc_hook`.)
//!
//! Everything here is physical pixels. `scale` is the monitor's DPI scale (1.0 at 96 DPI) times
//! the player's UI scale, so the `rem`-based width lands at the same visual size as EE2's CSS
//! pixels and grows with the panel's text. Pure math, deliberately not Windows-gated, so the
//! formula is covered by the native CI test pass.

use serde::{Deserialize, Serialize};

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

impl PhysicalRect {
    /// Whether the two rects share any pixel.
    pub fn intersects(&self, other: &PhysicalRect) -> bool {
        self.x < other.x + other.width
            && other.x < self.x + self.width
            && self.y < other.y + other.height
            && other.y < self.y + self.height
    }
}

// --- The HUD's rails ----------------------------------------------------------------------------
//
// Measured live 2026-09-23 on the test machine's 3840x2160 game. The panels either side of the
// experience bar -- the flasks and charms on the left, the skills on the right -- each have a top
// rail: a grey cap molding whose top highlight (rows 1859-1862) is the HUD's upper edge there,
// a scroll band under it, and a thin molding at rows 1895-1896 before the panel's ironwork. The
// XP overlay's plates are inlaid in the rails, from just under the highlight down to that
// molding, along each rail's straight run between its end caps. PoE2 scales its HUD with the
// game's height (as `xp_tracker::XpBarGeometry` assumes), so every length is in pixels of a
// 2160-row game; each panel hangs from its globe in a bottom corner, so each run is measured from
// its own side's edge -- which only 16:9 has confirmed.

const HUD_REFERENCE_HEIGHT: f64 = 2160.0;
/// A plate's top and bottom rows, as distances from the game's bottom edge: rows 1863 and 1895.
const RAIL_PLATE_ROWS: (f64, f64) = (297.0, 265.0);
/// The flask rail's straight run, x 467-927: its ends' distances from the game's left edge.
const FLASK_RAIL_RUN: (f64, f64) = (467.0, 927.0);
/// The skill rail's straight run, x 2905-3374: its ends' distances from the game's right edge.
const SKILL_RAIL_RUN: (f64, f64) = (935.0, 466.0);

/// Where the XP overlay's plates go in a game whose client area is `game`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudRails {
    /// In the flask panel's rail, left of the experience bar.
    pub flask: PhysicalRect,
    /// In the skill panel's rail, right of it.
    pub skill: PhysicalRect,
}

/// The plates' rects in the game's client area `game`.
pub fn hud_rails(game: PhysicalRect) -> HudRails {
    let scale = f64::from(game.height) / HUD_REFERENCE_HEIGHT;
    let bottom = f64::from(game.y + game.height);
    let top = (bottom - RAIL_PLATE_ROWS.0 * scale).round() as i32;
    let height = (bottom - RAIL_PLATE_ROWS.1 * scale).round() as i32 - top;
    let run = |from: f64, to: f64| {
        let x = from.round() as i32;
        PhysicalRect {
            x,
            y: top,
            width: to.round() as i32 - x,
            height,
        }
    };
    let left = f64::from(game.x);
    let right = f64::from(game.x + game.width);
    HudRails {
        flask: run(
            left + FLASK_RAIL_RUN.0 * scale,
            left + FLASK_RAIL_RUN.1 * scale,
        ),
        skill: run(
            right - SKILL_RAIL_RUN.0 * scale,
            right - SKILL_RAIL_RUN.1 * scale,
        ),
    }
}

/// Which of the game's side panels a check sits beside: the inventory, on the game's right, for a
/// check over the right half of the game; the stash, on its left, otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSide {
    Inventory,
    Stash,
}

impl PanelSide {
    /// The side of a check triggered with the cursor at `cursor_x` over the game's client area
    /// `game`.
    pub fn at(game: PhysicalRect, cursor_x: i32) -> PanelSide {
        if cursor_x > game.x + game.width / 2 {
            PanelSide::Inventory
        } else {
            PanelSide::Stash
        }
    }
}

/// The panel's automatic rect on `side` of the game window's client area `game`. A panel wider
/// than the room beside the side panel is narrowed to fit, so it never covers the inventory or
/// stash, nor leaves the game.
pub fn panel_rect(game: PhysicalRect, side: PanelSide, scale: f64) -> PhysicalRect {
    let sidebar = (f64::from(game.height) * SIDEBAR_WIDTH_PER_GAME_HEIGHT).round() as i32;
    let width = ((PANEL_WIDTH_REM * EE2_FONT_SIZE_PX * scale).round() as i32)
        .min(game.width - sidebar)
        .max(0);
    let x = match side {
        PanelSide::Inventory => game.x + game.width - sidebar - width,
        PanelSide::Stash => game.x + sidebar,
    };
    PhysicalRect {
        x,
        y: game.y,
        width,
        height: game.height,
    }
}

/// The panel dragged `dx` pixels sideways from `start`, kept whole on `monitor`. Only its x moves:
/// it keeps the game's full height.
pub fn dragged(start: PhysicalRect, dx: i32, monitor: PhysicalRect) -> PhysicalRect {
    let x = (start.x + dx)
        .min(monitor.x + monitor.width - start.width)
        .max(monitor.x);
    PhysicalRect { x, ..start }
}

/// Where the player left the panel on each side: its left edge's distance from the game's left
/// edge, as a share of the game's width -- so the place follows a moved or resized game window --
/// or `None` while that side keeps the automatic placement. Kept in the settings.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelPositions {
    pub inventory: Option<f64>,
    pub stash: Option<f64>,
}

impl PanelPositions {
    fn slot(&mut self, side: PanelSide) -> &mut Option<f64> {
        match side {
            PanelSide::Inventory => &mut self.inventory,
            PanelSide::Stash => &mut self.stash,
        }
    }

    /// Keeps `x`, the left edge the panel was dragged to over the game's client area `game`, as
    /// `side`'s place.
    pub fn remember(&mut self, side: PanelSide, game: PhysicalRect, x: i32) {
        if game.width > 0 {
            *self.slot(side) = Some(f64::from(x - game.x) / f64::from(game.width));
        }
    }

    /// Returns `side` to the automatic placement.
    pub fn forget(&mut self, side: PanelSide) {
        *self.slot(side) = None;
    }

    /// The panel's rect on `side`: the automatic one ([`panel_rect`]) at the place the player left
    /// it there -- unless that place would put any of it off `monitor`, the game's monitor (the
    /// game moved to a smaller one, or the panel grew with the UI scale), where the automatic
    /// placement stands in and the place stays kept.
    pub fn rect(
        &self,
        side: PanelSide,
        game: PhysicalRect,
        monitor: PhysicalRect,
        scale: f64,
    ) -> PhysicalRect {
        let automatic = panel_rect(game, side, scale);
        let place = match side {
            PanelSide::Inventory => self.inventory,
            PanelSide::Stash => self.stash,
        };
        let Some(share) = place else {
            return automatic;
        };
        let x = game.x + (share * f64::from(game.width)).round() as i32;
        if x < monitor.x || x + automatic.width > monitor.x + monitor.width {
            return automatic;
        }
        PhysicalRect { x, ..automatic }
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
    fn plates_sit_in_the_rails_they_were_measured_in() {
        let rails = hud_rails(GAME_4K);
        assert_eq!(
            rails.flask,
            PhysicalRect {
                x: 467,
                y: 1863,
                width: 460,
                height: 32,
            }
        );
        assert_eq!(
            rails.skill,
            PhysicalRect {
                x: 2905,
                y: 1863,
                width: 469,
                height: 32,
            }
        );
    }

    #[test]
    fn plates_scale_with_the_game_and_keep_to_their_own_edges() {
        // A 1080-row window: everything halves, from the window's own corners.
        let windowed = hud_rails(PhysicalRect {
            x: 100,
            y: 50,
            width: 1920,
            height: 1080,
        });
        assert_eq!((windowed.flask.x, windowed.flask.width), (334, 230));
        assert_eq!((windowed.flask.y, windowed.flask.height), (982, 16));
        assert_eq!((windowed.skill.x, windowed.skill.width), (1553, 234));
        // A wider game of the same height: the flask rail stays by the left edge, the skill
        // rail moves with the right one.
        let wide = hud_rails(PhysicalRect {
            width: 5120,
            ..GAME_4K
        });
        assert_eq!(wide.flask, hud_rails(GAME_4K).flask);
        assert_eq!(wide.skill.x, 5120 - 935);
    }

    #[test]
    fn rects_intersect_only_when_they_share_a_pixel() {
        let plate = hud_rails(GAME_4K).flask;
        let panel = |x| PhysicalRect {
            x,
            y: 0,
            width: 1024,
            height: 2160,
        };
        assert!(!panel(1331).intersects(&plate));
        assert!(panel(0).intersects(&plate));
        // Touching edges share no pixel.
        assert!(!panel(plate.x + plate.width).intersects(&plate));
        assert!(panel(plate.x + plate.width - 1).intersects(&plate));
    }

    #[test]
    fn inventory_side_panel_ends_where_the_inventory_panel_starts() {
        let side = PanelSide::at(GAME_4K, 2958);
        let panel = panel_rect(GAME_4K, side, 2.0);
        let inventory_left_edge = 3840 - (2160.0_f64 * 986.0 / 1600.0).round() as i32;
        assert_eq!(panel.x + panel.width, inventory_left_edge);
        assert_eq!(panel_rect(GAME_4K, side, 1.0).width * 2, panel.width);
        assert_eq!((panel.y, panel.height), (0, 2160));
    }

    #[test]
    fn stash_side_panel_starts_where_the_stash_panel_ends() {
        let panel = panel_rect(GAME_4K, PanelSide::at(GAME_4K, 500), 2.0);
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
        let panel = panel_rect(game, PanelSide::at(game, 1500), 1.0);
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
        let inventory_side = panel_rect(game, PanelSide::Inventory, 1.5);
        assert_eq!(
            (inventory_side.x, inventory_side.x + inventory_side.width),
            (0, 1024 - sidebar)
        );
        let stash_side = panel_rect(game, PanelSide::Stash, 1.5);
        assert_eq!(
            (stash_side.x, stash_side.x + stash_side.width),
            (sidebar, 1024)
        );
    }

    #[test]
    fn a_dragged_panel_opens_where_it_was_left_on_that_side_only() {
        let mut positions = PanelPositions::default();
        positions.remember(PanelSide::Inventory, GAME_4K, 1200);

        let inventory = positions.rect(PanelSide::Inventory, GAME_4K, GAME_4K, 2.0);
        let automatic = panel_rect(GAME_4K, PanelSide::Inventory, 2.0);
        assert_eq!(
            inventory,
            PhysicalRect {
                x: 1200,
                ..automatic
            }
        );
        assert_eq!(
            positions.rect(PanelSide::Stash, GAME_4K, GAME_4K, 2.0),
            panel_rect(GAME_4K, PanelSide::Stash, 2.0)
        );
    }

    /// The place is kept relative to the game window: a windowed game moved across the monitor
    /// takes its panel along.
    #[test]
    fn the_place_follows_a_moved_game_window() {
        let monitor = GAME_4K;
        let game = PhysicalRect {
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
        };
        let mut positions = PanelPositions::default();
        positions.remember(PanelSide::Stash, game, 1000);

        let moved = PhysicalRect { x: 1000, ..game };
        assert_eq!(
            positions.rect(PanelSide::Stash, moved, monitor, 2.0).x,
            2000
        );
    }

    #[test]
    fn a_drag_keeps_the_whole_panel_on_the_game_monitor() {
        // A second monitor left of the main one: its x runs negative.
        let monitor = PhysicalRect {
            x: -3840,
            ..GAME_4K
        };
        let start = panel_rect(monitor, PanelSide::Inventory, 2.0);

        assert_eq!(
            dragged(start, -300, monitor),
            PhysicalRect {
                x: start.x - 300,
                ..start
            }
        );
        assert_eq!(dragged(start, -10_000, monitor).x, -3840);
        let right = dragged(start, 10_000, monitor);
        assert_eq!(right.x + right.width, 0);
    }

    /// A place kept on a bigger screen, or before the panel grew with the UI scale, that would now
    /// put the panel partly off the monitor gives way to the automatic placement -- and is kept for
    /// when it fits again.
    #[test]
    fn a_place_that_would_leave_the_monitor_falls_back_to_the_automatic_placement() {
        let mut positions = PanelPositions::default();
        positions.remember(PanelSide::Inventory, GAME_4K, 3840 - 1024);
        assert_eq!(
            positions
                .rect(PanelSide::Inventory, GAME_4K, GAME_4K, 2.0)
                .x,
            3840 - 1024,
            "flush with the right edge still fits"
        );

        let bigger = positions.rect(PanelSide::Inventory, GAME_4K, GAME_4K, 2.5);
        assert_eq!(bigger, panel_rect(GAME_4K, PanelSide::Inventory, 2.5));
        assert!(positions.inventory.is_some());
    }

    #[test]
    fn forgetting_a_side_returns_it_to_the_automatic_placement() {
        let mut positions = PanelPositions::default();
        positions.remember(PanelSide::Inventory, GAME_4K, 1200);
        positions.remember(PanelSide::Stash, GAME_4K, 2400);

        positions.forget(PanelSide::Inventory);
        assert_eq!(
            positions.rect(PanelSide::Inventory, GAME_4K, GAME_4K, 2.0),
            panel_rect(GAME_4K, PanelSide::Inventory, 2.0)
        );
        assert_eq!(
            positions.rect(PanelSide::Stash, GAME_4K, GAME_4K, 2.0).x,
            2400
        );
    }
}
