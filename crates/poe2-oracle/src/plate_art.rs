//! The XP overlay's plates as pixels (`ui::xp_overlay` shows them): each rail's plate and its
//! ends, drawn pixel by pixel at the game's own resolution in the HUD's own materials, and exactly
//! the pixels its windows show -- as rects of one colour ([`ArtSlice::fills`]), which a window
//! paints as quads: an image would cost each window a texture page of its own in GPUI's sprite
//! atlas, 4 MiB at the least, for a few kilobytes of pixels.
//!
//! A plate is its rail's straight run (`overlay_layout::hud_rails`) -- the cap molding along its
//! top, a face, a seam where it sits on the rail -- with an end at each side:
//! - at its globe it runs on over the world to the globe's frame (`overlay_layout::meet_globe`),
//!   the cap and the face as they are;
//! - at its other end, over the world past the rail's end cap, an ogee: the cap molding rolls
//!   over the corner in a quarter-round and sweeps down in a concave curve onto the top of the
//!   game's volute tip -- one smooth S, every band of the molding following it, no corner
//!   anywhere: a square corner reads as foreign among the HUD's curves.
//!
//! Colours, measured on the test machine's 4K game on 2026-09-24 as the median of five captures:
//! the cap is the rails' own cap molding, row by row; the face the warm dark of the game's key
//! plates (`Mouse 4`, `F`, `1`, `2` under the skills and flasks). Our earlier flat bands had a
//! purple-blue cast the game's greys don't.
//!
//! Every length is in pixels of a 2160-row game, as `overlay_layout`'s are, scaled to the game's
//! height. A pixel's colour is the average of what it covers -- the rows exact at 4K, pairs of
//! them averaged at 1080 -- and the ogee's pixels are supersampled 4x4, smooth inside; its outer
//! edge is whole pixels, as a window region is, with the world past it.

use crate::overlay_layout::{Globe, HudRails, PhysicalRect, meet_globe};

/// The cap molding along a plate's top edge, one entry per row of a 2160-row game (half a HUD
/// px), top down: the rails' own cap, their rows 1860-1869 -- the light bead and its fall, the
/// shade, the groove, the rise, the second highlight and its fall, the shade and the dark.
pub const CAP: [u32; 10] = [
    0x787169, 0x736c64, 0x56514c, 0x302e2c, 0x201f1e, 0x58554f, 0x887f76, 0x746d66, 0x44423d,
    0x2a2a2a,
];
/// A plate's face under the cap, top and bottom: the game's key plates' #1f1e1d, a shade lighter
/// at the top.
pub const FACE_TOP: u32 = 0x211f1e;
pub const FACE_BOTTOM: u32 = 0x1b1a19;
/// The seam where a plate sits on its rail: its last row.
pub const SEAM: u32 = 0x0b0b0a;

const REFERENCE_HEIGHT: f64 = 2160.0;
/// A plate's rows at a 2160-row game (`overlay_layout`'s `PLATE_HEIGHT`), the cap's and the
/// seam's.
const PLATE_ROWS: f64 = 40.0;
const SEAM_ROWS: f64 = 1.0;
/// Samples per pixel along each axis where the ogee curves.
const SUPERSAMPLE: usize = 4;
/// The ogee's quarter-round over the corner: 7 HUD px, so the molding's innermost band still
/// turns in a curve (radius 4) rather than a corner.
const OGEE_RADIUS: f64 = 14.0;

/// A plate's inner end, where its rail stops and the game's volute curls down past the rail's
/// end cap.
struct Ogee {
    /// The top of the volute's tip the ogee sweeps down to: how far out from the plate's end,
    /// and how far down from its top edge.
    tip: (f64, f64),
    /// Below the plate (its rows 40, 41, ...): how far out from the plate's end a row may be
    /// covered -- past the rail's end cap, and on the crown's row past the volute's crown. Mapped
    /// pixel by pixel on dark-world captures (the world black there, the HUD not): the first
    /// column from which everything out to the tip is the world, or the HUD's anti-aliased rim.
    below: &'static [f64],
}

/// Past the flask rail's right end: its end cap reaches 15 px out, the crown 23 on row 1864, the
/// tip rises to row 1865 32 px out.
const FLASK_OGEE: Ogee = Ogee {
    tip: (32.0, 46.0),
    below: &[15.0, 15.0, 15.0, 14.0, 14.0, 23.0],
};
/// Past the skill rail's left end: the end cap 9 px out, the crown 14, the tip 22.
const SKILL_OGEE: Ogee = Ogee {
    tip: (22.0, 46.0),
    below: &[9.0, 9.0, 9.0, 10.0, 10.0, 14.0],
};

/// Which side of the plate an end is on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

/// A plate and its ends as pixels, on screen.
pub struct PlateArt {
    /// The screen rect the pixels span.
    pub bounds: PhysicalRect,
    /// Each pixel's colour, `0xRRGGBB`, row by row, `bounds.width` a row; `None` where the plate
    /// doesn't cover it.
    pixels: Vec<Option<u32>>,
}

/// A window's part of a plate: its pixels, and the rects of them that show.
pub struct ArtSlice {
    /// The pixels, relative to the slice's top left corner, as few rects of one colour as a row's
    /// runs of equal pixels make, each merged with the same run on the rows below it: a window
    /// paints each as a quad, the ogee's a few hundred at 4K, the straight plate's one a row.
    pub fills: Vec<Fill>,
    /// The parts that show, the same way: rows of equal runs merged, whatever their colours.
    pub shown: Vec<PhysicalRect>,
}

/// Pixels of one colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fill {
    pub rect: PhysicalRect,
    /// `0xRRGGBB`, opaque.
    pub colour: u32,
}

impl PlateArt {
    /// The flask rail's plate: the life globe on its left, the ogee on its right.
    pub fn flask(rails: &HudRails, game_height: i32) -> PlateArt {
        build(
            rails.flask,
            game_height,
            Globe::Life,
            &FLASK_OGEE,
            Side::Right,
        )
    }

    /// The skill rail's plate: the ogee on its left, the mana globe on its right.
    pub fn skill(rails: &HudRails, game_height: i32) -> PlateArt {
        build(
            rails.skill,
            game_height,
            Globe::Mana,
            &SKILL_OGEE,
            Side::Left,
        )
    }

    /// The pixels in screen rect `rect`.
    pub fn slice(&self, rect: PhysicalRect) -> ArtSlice {
        let width = rect.width.max(0);
        let height = rect.height.max(0);
        let mut fills = Vec::new();
        // The runs of the row above, their fills still growing down.
        let mut open: Vec<Fill> = Vec::new();
        let mut rows: Vec<Vec<(i32, i32)>> = Vec::with_capacity(height as usize);
        for row in 0..height {
            let mut runs: Vec<(i32, i32, u32)> = Vec::new();
            for col in 0..width {
                let Some(colour) = self.pixel(rect.x + col, rect.y + row) else {
                    continue;
                };
                match runs.last_mut() {
                    Some((_, to, last)) if *to == col && *last == colour => *to += 1,
                    _ => runs.push((col, col + 1, colour)),
                }
            }
            let mut continued = Vec::with_capacity(runs.len());
            for &(from, to, colour) in &runs {
                let above = open.iter().position(|fill| {
                    fill.colour == colour && fill.rect.x == from && fill.rect.width == to - from
                });
                continued.push(match above {
                    Some(index) => {
                        let mut fill = open.swap_remove(index);
                        fill.rect.height += 1;
                        fill
                    }
                    None => Fill {
                        rect: PhysicalRect {
                            x: from,
                            y: row,
                            width: to - from,
                            height: 1,
                        },
                        colour,
                    },
                });
            }
            fills.append(&mut open);
            open = continued;
            // What shows: the covered runs, whatever their colours.
            let mut covered: Vec<(i32, i32)> = Vec::new();
            for &(from, to, _) in &runs {
                match covered.last_mut() {
                    Some((_, end)) if *end == from => *end = to,
                    _ => covered.push((from, to)),
                }
            }
            rows.push(covered);
        }
        fills.append(&mut open);
        let mut shown = Vec::new();
        let mut first = 0;
        for row in 1..=rows.len() {
            if row < rows.len() && rows[row] == rows[first] {
                continue;
            }
            for &(from, to) in &rows[first] {
                shown.push(PhysicalRect {
                    x: from,
                    y: first as i32,
                    width: to - from,
                    height: (row - first) as i32,
                });
            }
            first = row;
        }
        ArtSlice { fills, shown }
    }

    /// The colour of the pixel at `x`, `y` on screen, if the plate covers it.
    fn pixel(&self, x: i32, y: i32) -> Option<u32> {
        self.index(x, y).and_then(|i| self.pixels[i])
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let b = self.bounds;
        (x >= b.x && x < b.x + b.width && y >= b.y && y < b.y + b.height)
            .then(|| ((y - b.y) * b.width + (x - b.x)) as usize)
    }
}

fn build(plate: PhysicalRect, game_height: i32, globe: Globe, ogee: &Ogee, side: Side) -> PlateArt {
    // Device px per 2160-row px: along the plate by the game's height, down it by the plate's own
    // rows, so the seam lands on its last row whatever rounding made it.
    let k = f64::from(game_height) / REFERENCE_HEIGHT;
    let kv = f64::from(plate.height) / PLATE_ROWS;
    let gap = meet_globe(plate, game_height, globe);
    let ogee_reach = (ogee.tip.0 * k).ceil() as i32 + 1;
    let ogee_rows = (ogee.tip.1 * kv).ceil() as i32;
    let (left, right) = match side {
        Side::Left => (plate.x - ogee_reach, gap.window.x + gap.window.width),
        Side::Right => (gap.window.x, plate.x + plate.width + ogee_reach),
    };
    let bounds = PhysicalRect {
        x: left,
        y: plate.y,
        width: right - left,
        height: plate.height.max(ogee_rows),
    };
    let outline = Outline::new(ogee.tip);
    let rows: Vec<[f64; 3]> = (0..plate.height)
        .map(|row| row_colour(f64::from(row) / kv, f64::from(row + 1) / kv))
        .collect();
    let mut pixels = vec![None; (bounds.width * bounds.height) as usize];
    for y in bounds.y..bounds.y + bounds.height {
        let rel_y = y - plate.y;
        for x in bounds.x..bounds.x + bounds.width {
            let rel_x = x - plate.x;
            let in_plate_rows = rel_y < plate.height;
            let straight = in_plate_rows && rel_x >= 0 && rel_x < plate.width;
            let in_gap = in_plate_rows && {
                let (gx, gy) = (x - gap.window.x, y - gap.window.y);
                gap.shown[1..]
                    .iter()
                    .any(|r| gx >= r.x && gx < r.x + r.width && gy >= r.y && gy < r.y + r.height)
            };
            let colour = if straight || in_gap {
                Some(rows[rel_y as usize])
            } else {
                // Columns out past the plate's end on the ogee's side, 0 the first.
                let out = match side {
                    Side::Right => rel_x - plate.width,
                    Side::Left => -rel_x - 1,
                };
                (out >= 0)
                    .then(|| ogee_pixel(&outline, ogee, out, rel_y, k, kv))
                    .flatten()
            };
            if let Some(colour) = colour {
                let [r, g, b] = colour.map(|c| c.round().clamp(0.0, 255.0) as u32);
                pixels[((y - bounds.y) * bounds.width + (x - bounds.x)) as usize] =
                    Some((r << 16) | (g << 8) | b);
            }
        }
    }
    PlateArt { bounds, pixels }
}

/// The colour of an ogee pixel `out` columns past the plate's end and `row` rows below its top,
/// if the ogee covers it: its centre inside the outline and where the HUD leaves the world.
fn ogee_pixel(
    outline: &Outline,
    ogee: &Ogee,
    out: i32,
    row: i32,
    k: f64,
    kv: f64,
) -> Option<[f64; 3]> {
    let u = (f64::from(out) + 0.5) / k;
    let v = (f64::from(row) + 0.5) / kv;
    if !outline.inside(u, v) || !allowed(ogee, u, v) {
        return None;
    }
    let mut sum = [0.0; 3];
    for sy in 0..SUPERSAMPLE {
        for sx in 0..SUPERSAMPLE {
            let su = (f64::from(out) + (sx as f64 + 0.5) / SUPERSAMPLE as f64) / k;
            let sv = (f64::from(row) + (sy as f64 + 0.5) / SUPERSAMPLE as f64) / kv;
            // A sample past the outline in a pixel that shows takes the outline's own band.
            let colour = if outline.inside(su, sv) {
                band_colour(outline.distance(su, sv), sv)
            } else {
                rgb(CAP[0])
            };
            for (s, c) in sum.iter_mut().zip(colour) {
                *s += c;
            }
        }
    }
    let n = (SUPERSAMPLE * SUPERSAMPLE) as f64;
    Some(sum.map(|s| s / n))
}

/// Whether the ogee may cover the point `u` out and `v` down: anywhere over the plate's rows,
/// below them only past the HUD.
fn allowed(ogee: &Ogee, u: f64, v: f64) -> bool {
    if v < PLATE_ROWS {
        return true;
    }
    ogee.below
        .get((v - PLATE_ROWS) as usize)
        .is_some_and(|&from| u >= from)
}

/// The colour `depth` px (2160-row) inside the outline at `v` down: the cap's row, or the face.
fn band_colour(depth: f64, v: f64) -> [f64; 3] {
    match CAP.get(depth as usize) {
        Some(&colour) if depth >= 0.0 => rgb(colour),
        _ => face(v),
    }
}

/// The straight plate's colour over rows `from`..`to` (2160-row px down from its top): the cap,
/// the face, the seam, averaged over the span.
fn row_colour(from: f64, to: f64) -> [f64; 3] {
    const STEPS: usize = 16;
    let mut sum = [0.0; 3];
    for step in 0..STEPS {
        let v = from + (to - from) * (step as f64 + 0.5) / STEPS as f64;
        let colour = if v >= PLATE_ROWS - SEAM_ROWS {
            rgb(SEAM)
        } else {
            band_colour(v, v)
        };
        for (s, c) in sum.iter_mut().zip(colour) {
            *s += c;
        }
    }
    sum.map(|s| s / STEPS as f64)
}

/// The face at `v` down: lighter at the top.
fn face(v: f64) -> [f64; 3] {
    let t = (v / PLATE_ROWS).clamp(0.0, 1.0);
    let (top, bottom) = (rgb(FACE_TOP), rgb(FACE_BOTTOM));
    [0, 1, 2].map(|i| top[i] + (bottom[i] - top[i]) * t)
}

fn rgb(colour: u32) -> [f64; 3] {
    [
        f64::from((colour >> 16) & 0xff),
        f64::from((colour >> 8) & 0xff),
        f64::from(colour & 0xff),
    ]
}

/// The ogee's outline, `u` out from the plate's end and `v` down from its top edge (2160-row
/// px): the top edge up to the end, a quarter-round of `OGEE_RADIUS` over the corner, then a
/// concave quarter-ellipse down to the tip -- level where it meets the top edge, upright where
/// the two curves meet, level again on the tip.
struct Outline {
    tip: (f64, f64),
    /// The concave part as a polyline, dense enough to measure distances by.
    cove: Vec<(f64, f64)>,
}

impl Outline {
    fn new(tip: (f64, f64)) -> Outline {
        const SEGMENTS: usize = 96;
        let (a, b) = (tip.0 - OGEE_RADIUS, tip.1 - OGEE_RADIUS);
        let cove = (0..=SEGMENTS)
            .map(|i| {
                let t = i as f64 * std::f64::consts::FRAC_PI_2 / SEGMENTS as f64;
                (tip.0 - a * t.cos(), OGEE_RADIUS + b * t.sin())
            })
            .collect();
        Outline { tip, cove }
    }

    fn inside(&self, u: f64, v: f64) -> bool {
        let r = OGEE_RADIUS;
        if !(0.0..=self.tip.1).contains(&v) {
            return false;
        }
        if u <= 0.0 {
            return true;
        }
        if v <= r {
            return u * u + (v - r) * (v - r) <= r * r;
        }
        let (a, b) = (self.tip.0 - r, self.tip.1 - r);
        let s = (v - r) / b;
        u <= self.tip.0 - a * (1.0 - s * s).max(0.0).sqrt()
    }

    /// How far (u, v) is from the outline.
    fn distance(&self, u: f64, v: f64) -> f64 {
        let r = OGEE_RADIUS;
        // The top edge, up to the plate's end.
        let edge = if u <= 0.0 { v.abs() } else { u.hypot(v) };
        // The quarter-round, centred (0, r), from straight up to straight out.
        let (du, dv) = (u, v - r);
        let round = if du >= 0.0 && dv <= 0.0 {
            (du.hypot(dv) - r).abs()
        } else {
            u.hypot(v).min((u - r).hypot(v - r))
        };
        let cove = self
            .cove
            .windows(2)
            .map(|pair| segment_distance((u, v), pair[0], pair[1]))
            .fold(f64::INFINITY, f64::min);
        edge.min(round).min(cove)
    }
}

fn segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0)
    };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay_layout::hud_rails;

    /// The test machine's game: 3840x2160.
    const GAME_4K: PhysicalRect = PhysicalRect {
        x: 0,
        y: 0,
        width: 3840,
        height: 2160,
    };

    fn hex(colour: [f64; 3]) -> u32 {
        let [r, g, b] = colour.map(|c| c.round() as u32);
        (r << 16) | (g << 8) | b
    }

    #[test]
    fn at_4k_the_plate_wears_the_rails_cap_row_for_row_then_the_face_and_the_seam() {
        let rails = hud_rails(GAME_4K);
        let art = PlateArt::flask(&rails, 2160);
        let x = rails.flask.x + 200;
        for (row, &colour) in CAP.iter().enumerate() {
            assert_eq!(art.pixel(x, 1819 + row as i32), Some(colour), "row {row}");
        }
        assert_eq!(art.pixel(x, 1858), Some(SEAM));
        assert_eq!(art.pixel(x, 1829), Some(hex(face(10.5))));
        // Past the plate, over the world between it and the life globe, the same rows.
        assert_eq!(art.pixel(rails.flask.x - 30, 1819), Some(CAP[0]));
    }

    #[test]
    fn the_ogee_rolls_over_the_corner_and_lands_on_the_volute_tip_without_touching_the_hud() {
        let rails = hud_rails(GAME_4K);
        let art = PlateArt::flask(&rails, 2160);
        let end = rails.flask.x + rails.flask.width; // the first column past the plate
        // How far out each row of the ogee reaches.
        let reach = |y: i32| {
            (0..40)
                .rev()
                .find(|&d| art.pixel(end + d, y).is_some())
                .map_or(0, |d| d + 1)
        };
        // No square corner: the top row already rounds off a little past the end, and every row
        // down reaches as far or farther, out to the tip's column on the tip's row.
        assert!(
            (1..=5).contains(&reach(1819)),
            "top row reach {}",
            reach(1819)
        );
        let reaches: Vec<i32> = (1819..1865).map(reach).collect();
        assert!(reaches.windows(2).all(|w| w[1] >= w[0]), "{reaches:?}");
        assert!((24..=32).contains(&reach(1864)), "{}", reach(1864));
        // Below the plate, the rail's end cap and the volute's crown stay the game's.
        for (row, from) in FLASK_OGEE.below.iter().enumerate() {
            let y = 1859 + row as i32;
            let first = (0..40).find(|&d| art.pixel(end + d, y).is_some());
            assert!(
                first.is_none_or(|d| f64::from(d) >= *from),
                "row {y}: {first:?}"
            );
        }
        // The outer edge is the cap's light bead; deeper in, the face.
        let luma = |c: u32| rgb(c)[0] * 0.299 + rgb(c)[1] * 0.587 + rgb(c)[2] * 0.114;
        assert!(luma(art.pixel(end + reach(1840) - 1, 1840).unwrap()) > 90.0);
        assert_eq!(art.pixel(end, 1850), Some(hex(face(31.5))));
        // The skill plate's ogee faces the other way: on the crown's row, from past the crown to
        // short of the tip.
        let skill = PlateArt::skill(&rails, 2160);
        let past = |d: i32| skill.pixel(rails.skill.x - 1 - d, 1864).is_some();
        assert!(!past(13) && past(14) && past(20) && !past(21));
        assert!(
            skill
                .pixel(rails.skill.x + rails.skill.width + 3, 1819)
                .is_some(),
            "the mana gap"
        );
    }

    #[test]
    fn at_1080_rows_take_the_average_of_the_rows_they_cover() {
        let game = PhysicalRect {
            width: 1920,
            height: 1080,
            ..GAME_4K
        };
        let rails = hud_rails(game);
        let art = PlateArt::flask(&rails, 1080);
        let x = rails.flask.x + 100;
        let average = |a: u32, b: u32| {
            let (a, b) = (rgb(a), rgb(b));
            let [r, g, bl] = [0, 1, 2].map(|i| ((a[i] + b[i]) / 2.0).round() as u32);
            (r << 16) | (g << 8) | bl
        };
        assert_eq!(art.pixel(x, rails.flask.y), Some(average(CAP[0], CAP[1])));
        assert_eq!(
            art.pixel(x, rails.flask.y + 4),
            Some(average(CAP[8], CAP[9]))
        );
    }

    #[test]
    fn a_slice_paints_each_pixel_the_art_covers_once_in_its_colour_and_shows_just_those() {
        for (width, height) in [(3840, 2160), (2560, 1440), (1920, 1080), (1280, 720)] {
            let rails = hud_rails(PhysicalRect {
                width,
                height,
                ..GAME_4K
            });
            for art in [
                PlateArt::flask(&rails, height),
                PlateArt::skill(&rails, height),
            ] {
                // The whole art, and two parts of it the way the windows cut theirs: one ending
                // mid-plate and short of the ogee's last rows, the other the rest.
                let whole = art.bounds;
                let cut = whole.x + whole.width / 2;
                let parts = [
                    whole,
                    PhysicalRect {
                        width: cut - whole.x,
                        height: whole.height - 3,
                        ..whole
                    },
                    PhysicalRect {
                        x: cut,
                        width: whole.x + whole.width - cut,
                        ..whole
                    },
                ];
                for rect in parts {
                    let slice = art.slice(rect);
                    let at = |x: i32, y: i32| (y * rect.width + x) as usize;
                    let mut painted = vec![None; at(0, rect.height)];
                    for fill in &slice.fills {
                        let r = fill.rect;
                        for y in r.y..r.y + r.height {
                            for x in r.x..r.x + r.width {
                                assert_eq!(painted[at(x, y)], None, "{width}x{height} {x},{y}");
                                painted[at(x, y)] = Some(fill.colour);
                            }
                        }
                    }
                    let mut shown = vec![false; at(0, rect.height)];
                    for r in &slice.shown {
                        for y in r.y..r.y + r.height {
                            for x in r.x..r.x + r.width {
                                assert!(!shown[at(x, y)], "{width}x{height} {x},{y}");
                                shown[at(x, y)] = true;
                            }
                        }
                    }
                    for y in 0..rect.height {
                        for x in 0..rect.width {
                            let pixel = art.pixel(rect.x + x, rect.y + y);
                            assert_eq!(painted[at(x, y)], pixel, "{width}x{height} {x},{y}");
                            assert_eq!(shown[at(x, y)], pixel.is_some());
                        }
                    }
                }
            }
        }
    }
}
