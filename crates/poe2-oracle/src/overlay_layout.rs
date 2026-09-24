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
// rails are the game's own gauges -- rage and stun fill them -- so the XP overlay's plates sit on
// top of them, their bottom on the rail's highlight, along each rail's straight run between its
// end caps. PoE2 scales its HUD with the game's height (as `xp_tracker::XpBarGeometry` assumes),
// so every length is in pixels of a 2160-row game; each panel hangs from its globe in a bottom
// corner, so each run is measured from its own side's edge -- which only 16:9 has confirmed. A
// plate shows only where its rail's lip is seen on screen ([`rail_seen`]): the game's other HUD
// layouts (its centred HUD options), a loading screen, a full-screen panel or another window
// leave the plate off rather than floating over whatever is there.

const HUD_REFERENCE_HEIGHT: f64 = 2160.0;
/// A rail's top row, 1859, as a distance from the game's bottom edge: where a plate ends.
const RAIL_TOP: f64 = 301.0;
/// A plate's height: the rail's own cap molding over a face for one line of words.
const PLATE_HEIGHT: f64 = 40.0;
/// Its rows at a 2160-row game.
const PLATE_ROWS: usize = PLATE_HEIGHT as usize;
/// The flask rail's straight run, x 467-927: its ends' distances from the game's left edge.
const FLASK_RAIL_RUN: (f64, f64) = (467.0, 927.0);
/// The skill rail's straight run, x 2905-3374: its ends' distances from the game's right edge.
const SKILL_RAIL_RUN: (f64, f64) = (935.0, 466.0);
/// Rows of a rail's lip read at a 2160-row game: its highlight, just under a plate.
const LIP_ROWS: f64 = 4.0;
/// What reads as the lip in a column: its brightest row at least this light and this grey, and
/// this much lighter than the row under it, the rail's dark groove.
const LIP_MIN_LUMA: f64 = 85.0;
const LIP_MAX_CHROMA: u8 = 40;
const LIP_MIN_STEP: f64 = 40.0;
/// The share of columns that must read as the lip. Measured 2026-09-23 on the 4K test machine
/// and on its capture scaled to 720-1440 rows: the rails 0.91-1.0, strips of the game's world at
/// most 0.15.
const LIP_MIN_SHARE: f64 = 0.6;

/// Where the XP overlay's plates go in a game whose client area is `game`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudRails {
    /// On the flask panel's rail, left of the experience bar.
    pub flask: PhysicalRect,
    /// On the skill panel's rail, right of it.
    pub skill: PhysicalRect,
}

/// The plates' rects in the game's client area `game`.
pub fn hud_rails(game: PhysicalRect) -> HudRails {
    let scale = f64::from(game.height) / HUD_REFERENCE_HEIGHT;
    let bottom = f64::from(game.y + game.height);
    let rail = (bottom - RAIL_TOP * scale).round() as i32;
    let top = (bottom - (RAIL_TOP + PLATE_HEIGHT) * scale).round() as i32;
    let run = |from: f64, to: f64| {
        let x = from.round() as i32;
        PhysicalRect {
            x,
            y: top,
            width: to.round() as i32 - x,
            height: rail - top,
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

// --- Where a plate meets its globe --------------------------------------------------------------
//
// A rail's straight run ends a little short of its globe: between a plate's outer end and the
// globe's frame -- its rim, then an ornament's curl, a knob and a notch under it -- the game's
// world shows through, a gap as tall as the plate and 49 pixels wide at its top. Mapped pixel by
// pixel 2026-09-24 on the 4K test machine: a pixel is the world where the capture of a map with a
// black world shows black and captures over bright ground don't (the frame is the HUD's art, the
// same in every capture), and the gap is what a fill from the plate's end reaches through such
// pixels within the plate's rows, then the frame's anti-aliased rim one pixel further. A plate's
// window covers exactly that ([`meet_globe`]), so the plate runs on to the frame: no gap left, no
// frame covered. The two sides are the HUD's mirror images but for a pixel of anti-aliasing here
// and there, so each has its own map.

/// The gap left of the flask plate, row by row from the plate's top at a 2160-row game: its runs
/// as distances out from the plate's end, `[from, to)` pixels.
const LIFE_GLOBE_GAP: [&[(u8, u8)]; PLATE_ROWS] = [
    &[(0, 49)],
    &[(0, 48)],
    &[(0, 47)],
    &[(0, 47)],
    &[(0, 46)],
    &[(0, 45)],
    &[(0, 45)],
    &[(0, 44)],
    &[(0, 40), (42, 43)],
    &[(0, 38)],
    &[(0, 36)],
    &[(0, 34)],
    &[(0, 32)],
    &[(0, 31)],
    &[(0, 30)],
    &[(0, 29)],
    &[(0, 28)],
    &[(0, 27)],
    &[(0, 27)],
    &[(0, 26)],
    &[(0, 25)],
    &[(0, 25)],
    &[(0, 24)],
    &[(0, 24)],
    &[(0, 16), (23, 24)],
    &[(0, 14)],
    &[(0, 13)],
    &[(0, 12)],
    &[(0, 10)],
    &[(0, 8)],
    &[(0, 5)],
    &[(0, 6)],
    &[(0, 7)],
    &[(0, 10)],
    &[(2, 12)],
    &[(2, 12)],
    &[(3, 12)],
    &[(4, 12)],
    &[],
    &[],
];

/// The gap right of the skill plate, the same way.
const MANA_GLOBE_GAP: [&[(u8, u8)]; PLATE_ROWS] = [
    &[(0, 49)],
    &[(0, 48)],
    &[(0, 47)],
    &[(0, 46)],
    &[(0, 46)],
    &[(0, 45)],
    &[(0, 44)],
    &[(0, 43)],
    &[(0, 39), (42, 43)],
    &[(0, 37)],
    &[(0, 35)],
    &[(0, 33)],
    &[(0, 32)],
    &[(0, 31)],
    &[(0, 29)],
    &[(0, 28)],
    &[(0, 28)],
    &[(0, 27)],
    &[(0, 26)],
    &[(0, 26)],
    &[(0, 25)],
    &[(0, 24)],
    &[(0, 24)],
    &[(0, 24)],
    &[(0, 15)],
    &[(0, 13)],
    &[(0, 12)],
    &[(0, 11)],
    &[(0, 10)],
    &[(0, 8)],
    &[(0, 5)],
    &[(0, 6)],
    &[(0, 7)],
    &[(0, 10)],
    &[(2, 12)],
    &[(2, 12)],
    &[(3, 12)],
    &[(4, 11)],
    &[],
    &[],
];

/// The globe a plate's outer end meets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Globe {
    /// The life globe, left of the flask plate.
    Life,
    /// The mana globe, right of the skill plate.
    Mana,
}

/// A plate run on to its globe's frame: its window, and what of the window shows -- rects
/// relative to the window: the plate's own, then the gap between its outer end and the frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlateShape {
    pub window: PhysicalRect,
    /// Where the plate itself starts in the window: past the gap, for the life globe's.
    pub plate_x: i32,
    pub shown: Vec<PhysicalRect>,
}

/// `plate` -- one of [`hud_rails`]'s in a game `game_height` rows high, or a part of one that
/// keeps its outer end -- run on to `globe`'s frame. Each of the plate's rows covers the gap of
/// every 2160-row plate's row it spans, its runs widened to whole pixels, rather than leave a
/// sliver of the world at another height; runs of equal rows make one rect.
pub fn meet_globe(plate: PhysicalRect, game_height: i32, globe: Globe) -> PlateShape {
    let scale = f64::from(game_height) / HUD_REFERENCE_HEIGHT;
    let gap = match globe {
        Globe::Life => &LIFE_GLOBE_GAP,
        Globe::Mana => &MANA_GLOBE_GAP,
    };
    let rows: Vec<Vec<(i32, i32)>> = (0..plate.height)
        .map(|row| {
            let first = ((f64::from(row) / scale) as usize).min(PLATE_ROWS - 1);
            let last = ((f64::from(row + 1) / scale).ceil() as usize)
                .saturating_sub(1)
                .clamp(first, PLATE_ROWS - 1);
            let mut runs: Vec<(i32, i32)> = gap[first..=last]
                .iter()
                .flat_map(|runs| runs.iter())
                .map(|&(from, to)| {
                    (
                        (f64::from(from) * scale).floor() as i32,
                        (f64::from(to) * scale).ceil() as i32,
                    )
                })
                .collect();
            runs.sort_unstable();
            runs.dedup_by(|next, kept| {
                let touches = next.0 <= kept.1;
                if touches {
                    kept.1 = kept.1.max(next.1);
                }
                touches
            });
            runs
        })
        .collect();
    let reach = rows.iter().flatten().map(|&(_, to)| to).max().unwrap_or(0);
    let (window, plate_x) = match globe {
        Globe::Life => (
            PhysicalRect {
                x: plate.x - reach,
                width: plate.width + reach,
                ..plate
            },
            reach,
        ),
        Globe::Mana => (
            PhysicalRect {
                width: plate.width + reach,
                ..plate
            },
            0,
        ),
    };
    let mut shown = vec![PhysicalRect {
        x: plate_x,
        y: 0,
        width: plate.width,
        height: plate.height,
    }];
    let mut first = 0;
    for row in 1..=rows.len() {
        if row < rows.len() && rows[row] == rows[first] {
            continue;
        }
        for &(from, to) in &rows[first] {
            shown.push(PhysicalRect {
                x: match globe {
                    Globe::Life => plate_x - to,
                    Globe::Mana => plate.width + from,
                },
                y: first as i32,
                width: to - from,
                height: (row - first) as i32,
            });
        }
        first = row;
    }
    PlateShape {
        window,
        plate_x,
        shown,
    }
}

/// The strip read to tell whether `plate`'s rail is on screen ([`rail_seen`]): the lip's rows
/// right under the plate -- four at 2160 rows, at least two -- and the row under them.
pub fn rail_lip(plate: PhysicalRect, game_height: i32) -> PhysicalRect {
    let rows = ((LIP_ROWS * f64::from(game_height) / HUD_REFERENCE_HEIGHT).ceil() as i32).max(2);
    PhysicalRect {
        x: plate.x,
        y: plate.y + plate.height,
        width: plate.width,
        height: rows + 1,
    }
}

/// Whether `bgra` -- [`rail_lip`]'s strip, 32-bit BGRA rows top to bottom, `width` pixels each --
/// shows the rail: in most columns the lip's brightest row is the molding's light grey, well
/// above the row under the lip. The game's world, or a window over the rail, reads as neither.
pub fn rail_seen(bgra: &[u8], width: usize) -> bool {
    let rows = bgra.len() / 4 / width.max(1);
    if width == 0 || rows < 2 {
        return false;
    }
    let pixel = |x: usize, y: usize| {
        let at = (y * width + x) * 4;
        [bgra[at + 2], bgra[at + 1], bgra[at]]
    };
    let luma =
        |[r, g, b]: [u8; 3]| 0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b);
    let seen = (0..width)
        .filter(|&x| {
            let Some(highlight) = (0..rows - 1)
                .map(|y| pixel(x, y))
                .max_by(|a, b| luma(*a).total_cmp(&luma(*b)))
            else {
                return false;
            };
            let chroma =
                highlight.iter().max().unwrap_or(&0) - highlight.iter().min().unwrap_or(&0);
            luma(highlight) >= LIP_MIN_LUMA
                && chroma <= LIP_MAX_CHROMA
                && luma(highlight) - luma(pixel(x, rows - 1)) >= LIP_MIN_STEP
        })
        .count();
    seen as f64 >= LIP_MIN_SHARE * width as f64
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

    /// A live capture's raw RGB rows, as the BGRA a screen read hands over.
    fn bgra(rgb: &[u8]) -> Vec<u8> {
        let (pixels, _) = rgb.as_chunks::<3>();
        pixels
            .iter()
            .flat_map(|&[r, g, b]| [b, g, r, 255])
            .collect()
    }

    #[test]
    fn a_rail_is_seen_by_its_lip_and_nothing_else_passes_for_it() {
        // The bare flask and skill rails, and the flask rail of the same capture scaled to 1080
        // rows.
        for (capture, width) in [
            (
                &include_bytes!("../tests/fixtures/rail_lip_4k_flask_bare.rgb")[..],
                460,
            ),
            (
                &include_bytes!("../tests/fixtures/rail_lip_4k_skill_bare.rgb")[..],
                469,
            ),
            (
                &include_bytes!("../tests/fixtures/rail_lip_1080p_flask_bare.rgb")[..],
                230,
            ),
        ] {
            assert!(rail_seen(&bgra(capture), width));
        }
        // The hideout's floor where a rail would be in another HUD layout.
        let world = bgra(include_bytes!("../tests/fixtures/rail_lip_4k_world.rgb"));
        assert!(!rail_seen(&world, 460));
        // Light grey all the way down -- stone, fog -- has no lip over a darker row.
        assert!(!rail_seen(&[150, 150, 150, 255].repeat(460 * 5), 460));
    }

    #[test]
    fn the_lip_strip_sits_just_under_the_plate_and_keeps_two_rows_when_small() {
        let plate = hud_rails(GAME_4K).flask;
        assert_eq!(
            rail_lip(plate, 2160),
            PhysicalRect {
                x: 467,
                y: 1859,
                width: 460,
                height: 5,
            }
        );
        let small = PhysicalRect {
            x: 0,
            y: 0,
            width: 1280,
            height: 720,
        };
        assert_eq!(rail_lip(hud_rails(small).flask, 720).height, 3);
    }

    #[test]
    fn plates_sit_on_the_rails_they_were_measured_on() {
        let rails = hud_rails(GAME_4K);
        assert_eq!(
            rails.flask,
            PhysicalRect {
                x: 467,
                y: 1819,
                width: 460,
                height: 40,
            }
        );
        assert_eq!(
            rails.skill,
            PhysicalRect {
                x: 2905,
                y: 1819,
                width: 469,
                height: 40,
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
        assert_eq!((windowed.flask.y, windowed.flask.height), (960, 20));
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
    fn a_plate_runs_on_to_its_globe_over_the_gap_and_not_the_frame() {
        let rails = hud_rails(GAME_4K);
        let shows = |shape: &PlateShape, x: i32, y: i32| {
            shape
                .shown
                .iter()
                .any(|r| r.x <= x && x < r.x + r.width && r.y <= y && y < r.y + r.height)
        };
        // Left of the flask plate the gap is 49 pixels wide at its top row, one less a row down,
        // where the life globe's rim comes in.
        let life = meet_globe(rails.flask, 2160, Globe::Life);
        assert_eq!(
            life.window,
            PhysicalRect {
                x: 418,
                width: rails.flask.width + 49,
                ..rails.flask
            }
        );
        assert_eq!(life.plate_x, 49);
        assert!(shows(&life, 0, 0) && !shows(&life, 0, 1));
        // Row 24: the knob 16 pixels out stays the game's, its anti-aliased rim beyond is covered.
        assert!(shows(&life, 49 - 16, 24) && !shows(&life, 49 - 17, 24));
        assert!(shows(&life, 49 - 24, 24));
        // Over the rail's end cap, the plate alone.
        assert!(!shows(&life, 48, 38) && shows(&life, 49, 38));
        // The skill plate reaches right, to the mana globe.
        let mana = meet_globe(rails.skill, 2160, Globe::Mana);
        let width = rails.skill.width;
        assert_eq!(
            mana.window,
            PhysicalRect {
                width: width + 49,
                ..rails.skill
            }
        );
        assert!(shows(&mana, width + 48, 0) && !shows(&mana, width + 49, 0));
        // A 1080-row game: each row covers the two it stands for, whole pixels out.
        let small = hud_rails(PhysicalRect {
            width: 1920,
            height: 1080,
            ..GAME_4K
        });
        let life = meet_globe(small.flask, 1080, Globe::Life);
        assert_eq!(life.plate_x, 25);
        assert!(shows(&life, 0, 0));
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
