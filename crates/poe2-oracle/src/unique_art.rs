//! Which of its base's uniques an unidentified unique item is, by its art. The game's text of an
//! unidentified unique names only its base, which several uniques share (seven Diamonds, the rings
//! and amulets most of all); the art the game draws for it is the unique's own, and no two uniques
//! of a base share one. The price check reads the screen around the cursor at the moment of the
//! check (`platform::item_art`) and holds each candidate's art, as the trade site serves it, against
//! the item there: [`score`] for each, then [`verdict`] on whether the best one is sure.
//!
//! The match is a masked normalized cross-correlation of the art's opaque pixels, mean-removed per
//! colour, with the screen scaled down to the art's 47 pixels a cell ([`ART_CELL`]): brightness and
//! contrast don't move it, and the cell's own background, which the art leaves bare, plays no part.
//! The item's place is unknown -- only that the cursor is over it -- so every place it can be is
//! tried, first at half the scale and then around the best of those at full scale, and so are cells
//! a little smaller and larger than the inventory's (a stash tab's, a quad stash tab's half-size
//! ones).
//!
//! Measured on 22 uniques from a player's inventory, read off his 4K screen and scaled to 1440p and
//! 1080p: the unique's own art scores 0.70 to 0.92, the base's other uniques 0.56 at most, and the
//! art the item reference table had wrong for one ring (its base's) 0.38 against all of them --
//! hence [`SURE_SCORE`] and [`SURE_MARGIN`].

use image::RgbaImage;
use image::imageops::{self, FilterType};

/// The edge of an inventory cell in the trade site's item art (`"scale": 1`).
pub const ART_CELL: f32 = 47.0;
/// The best score the item's own art reaches at least, read off a screen.
pub const SURE_SCORE: f32 = 0.6;
/// How far ahead of the rest the best art must be.
pub const SURE_MARGIN: f32 = 0.15;
/// An art pixel at least this opaque is the item's; anything thinner is the cell behind it.
const OPAQUE: u8 = 191;
/// The cell sizes tried, against the inventory's: around it (a stash tab's are the same), and around
/// a quad stash tab's half.
const CELL_FACTORS: [f32; 5] = [0.94, 1.0, 1.06, 0.5, 0.53];
/// How far around the half-scale match's place the full-scale one is looked for, in pixels.
const REFINE: i32 = 2;

/// A candidate's art, ready to match: at full and at half scale.
pub struct Reference {
    full: Template,
    half: Template,
}

impl Reference {
    /// From the trade site's art of the unique. `None` for art with too few opaque pixels to tell
    /// anything apart.
    pub fn new(art: &RgbaImage) -> Option<Reference> {
        let half = imageops::resize(
            art,
            (art.width() / 2).max(1),
            (art.height() / 2).max(1),
            FilterType::Triangle,
        );
        Some(Reference {
            full: Template::new(art)?,
            half: Template::new(&half)?,
        })
    }
}

/// An art's opaque pixels, each colour less the art's mean of it.
struct Template {
    width: usize,
    height: usize,
    /// `(x, y, colour)` of each opaque pixel.
    pixels: Vec<(usize, usize, [f32; 3])>,
    /// The root of the sum of the colours' squares.
    norm: f32,
}

impl Template {
    fn new(art: &RgbaImage) -> Option<Template> {
        let opaque: Vec<(usize, usize, [f32; 3])> = art
            .enumerate_pixels()
            .filter(|(_, _, pixel)| pixel.0[3] >= OPAQUE)
            .map(|(x, y, pixel)| {
                let [r, g, b, _] = pixel.0;
                (
                    x as usize,
                    y as usize,
                    [r, g, b].map(|c| f32::from(c) / 255.0),
                )
            })
            .collect();
        if opaque.len() < 16 {
            return None;
        }
        let count = opaque.len() as f32;
        let mut mean = [0.0f32; 3];
        for (_, _, colour) in &opaque {
            for (sum, value) in mean.iter_mut().zip(colour) {
                *sum += value;
            }
        }
        let mean = mean.map(|sum| sum / count);
        let pixels: Vec<(usize, usize, [f32; 3])> = opaque
            .into_iter()
            .map(|(x, y, colour)| (x, y, [0, 1, 2].map(|c| colour[c] - mean[c])))
            .collect();
        let norm = pixels
            .iter()
            .flat_map(|(_, _, colour)| colour)
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        (norm > 0.0).then_some(Template {
            width: art.width() as usize,
            height: art.height() as usize,
            pixels,
            norm,
        })
    }
}

/// The screen around the cursor at the moment of a check.
pub struct Look {
    pub pixels: RgbaImage,
    /// Where the cursor was, in `pixels`.
    pub cursor: (f32, f32),
    /// An inventory cell's edge on that screen, in pixels.
    pub cell: f32,
}

impl Look {
    /// The look at the art's scale for each cell size tried ([`CELL_FACTORS`]): made once a check
    /// and matched against every candidate's art ([`score`]).
    pub fn scaled(&self) -> Scaled {
        let levels = CELL_FACTORS
            .iter()
            .map(|factor| {
                let scale = ART_CELL / (self.cell * factor);
                let width = ((self.pixels.width() as f32 * scale).round() as u32).max(1);
                let height = ((self.pixels.height() as f32 * scale).round() as u32).max(1);
                let full = imageops::resize(&self.pixels, width, height, FilterType::Triangle);
                let half = imageops::resize(
                    &full,
                    (width / 2).max(1),
                    (height / 2).max(1),
                    FilterType::Triangle,
                );
                Level {
                    full: Plane::of(&full),
                    half: Plane::of(&half),
                    cursor: (self.cursor.0 * scale, self.cursor.1 * scale),
                }
            })
            .collect();
        Scaled { levels }
    }
}

/// A [`Look`] at the art's scale, for each cell size tried.
pub struct Scaled {
    levels: Vec<Level>,
}

/// The look at one cell size: at the art's scale and at half of it, and where the cursor is in
/// the first.
struct Level {
    full: Plane,
    half: Plane,
    cursor: (f32, f32),
}

/// A picture's colours, row after row.
struct Plane {
    width: usize,
    height: usize,
    colours: Vec<[u8; 3]>,
}

impl Plane {
    fn of(image: &RgbaImage) -> Plane {
        Plane {
            width: image.width() as usize,
            height: image.height() as usize,
            colours: image
                .pixels()
                .map(|pixel| {
                    let [r, g, b, _] = pixel.0;
                    [r, g, b]
                })
                .collect(),
        }
    }

    /// The correlation of `template` with this plane at `(x, y)`, its top left.
    fn correlation(&self, template: &Template, x: usize, y: usize) -> f32 {
        let mut product = 0.0f32;
        let mut sums = [0.0f32; 3];
        let mut squares = 0.0f32;
        for &(dx, dy, centred) in &template.pixels {
            let colour = self.colours[(y + dy) * self.width + x + dx].map(|c| f32::from(c) / 255.0);
            for c in 0..3 {
                product += colour[c] * centred[c];
                sums[c] += colour[c];
                squares += colour[c] * colour[c];
            }
        }
        let count = template.pixels.len() as f32;
        let variance = squares - sums.iter().map(|sum| sum * sum / count).sum::<f32>();
        if variance <= 1e-9 {
            return 0.0;
        }
        product / (template.norm * variance.sqrt())
    }

    /// The best correlation of `template` over the places `xs` by `ys` (top lefts) that fit.
    fn best(
        &self,
        template: &Template,
        xs: std::ops::RangeInclusive<i32>,
        ys: std::ops::RangeInclusive<i32>,
    ) -> Option<(f32, i32, i32)> {
        let max_x = self.width as i32 - template.width as i32;
        let max_y = self.height as i32 - template.height as i32;
        let mut best: Option<(f32, i32, i32)> = None;
        for y in (*ys.start()).max(0)..=(*ys.end()).min(max_y) {
            for x in (*xs.start()).max(0)..=(*xs.end()).min(max_x) {
                let score = self.correlation(template, x as usize, y as usize);
                if best.is_none_or(|(top, _, _)| score > top) {
                    best = Some((score, x, y));
                }
            }
        }
        best
    }
}

/// How well `reference` matches the item under the cursor in `look`: its best correlation, -1 to
/// 1, over every place and cell size the item can have there.
pub fn score(look: &Scaled, reference: &Reference) -> f32 {
    let mut best = -1.0f32;
    for level in &look.levels {
        // Every top left that keeps the cursor on the art, at half scale.
        let (tw, th) = (reference.half.width as f32, reference.half.height as f32);
        let (cx, cy) = (level.cursor.0 / 2.0, level.cursor.1 / 2.0);
        let Some((_, x, y)) = level.half.best(
            &reference.half,
            (cx - tw).floor() as i32..=cx.ceil() as i32,
            (cy - th).floor() as i32..=cy.ceil() as i32,
        ) else {
            continue;
        };
        if let Some((score, _, _)) = level.full.best(
            &reference.full,
            x * 2 - REFINE..=x * 2 + REFINE,
            y * 2 - REFINE..=y * 2 + REFINE,
        ) {
            best = best.max(score);
        }
    }
    best
}

/// What the scores of a base's uniques (`None` for one without art) say.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    /// The item is this one, by far.
    Sure(usize),
    /// Too close to call, or nothing matched well: the player picks, most likely first.
    Unsure,
}

/// Whether the best score is sure: high enough, and ahead of every other by [`SURE_MARGIN`]. A
/// unique without art can't be ruled out by the others' scores, only by a sure one's: the item
/// looks like that art, and no two uniques of a base share one.
pub fn verdict(scores: &[Option<f32>]) -> Verdict {
    let mut ranked: Vec<(usize, f32)> = scores
        .iter()
        .enumerate()
        .filter_map(|(at, score)| score.map(|score| (at, score)))
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    match ranked[..] {
        [(at, best), (_, second), ..] if best >= SURE_SCORE && best - second >= SURE_MARGIN => {
            Verdict::Sure(at)
        }
        [(at, best)] if best >= SURE_SCORE => Verdict::Sure(at),
        _ => Verdict::Unsure,
    }
}

/// The candidates by score, best first: the order the player is asked in. Those without art go
/// last, in their own order.
pub fn ranking(scores: &[Option<f32>]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| match (scores[a], scores[b]) {
        (Some(a), Some(b)) => b.total_cmp(&a),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique off the owner's 4K screen (2026-10-12, his inventory, 105 px cells), the cursor at
    /// the picture's middle, and the uniques its base has.
    const ITEMS: [(&str, &str, &[&str]); 4] = [
        (
            "call-of-the-brotherhood",
            "Call of the Brotherhood",
            &["Call of the Brotherhood", "Levinstone", "The Burrower"],
        ),
        (
            "meginords-girdle",
            "Meginord's Girdle",
            &["Meginord's Girdle", "Midnight Braid"],
        ),
        (
            "blackheart",
            "Blackheart",
            &["Blackheart", "Icefang Orbit", "Prized Pain", "Venopuncture"],
        ),
        ("igniferis", "Igniferis", &["Idol of Uldurn", "Igniferis"]),
    ];

    fn fixture(name: &str) -> RgbaImage {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/unique_art")
            .join(name);
        image::open(&path)
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()))
            .into_rgba8()
    }

    #[test]
    fn a_real_unique_is_told_by_its_art_at_4k_1440p_and_1080p() {
        for (shot, unique, candidates) in ITEMS {
            let screen = fixture(&format!("{shot}.png"));
            let references: Vec<Reference> = candidates
                .iter()
                .map(|name| Reference::new(&fixture(&format!("art/{name}.png"))).expect("art"))
                .collect();
            // 2160, 1440 and 1080 pixels high: the game scales its inventory with the height.
            for scale in [1.0f32, 2.0 / 3.0, 0.5] {
                let pixels = imageops::resize(
                    &screen,
                    (screen.width() as f32 * scale).round() as u32,
                    (screen.height() as f32 * scale).round() as u32,
                    FilterType::Lanczos3,
                );
                let look = Look {
                    cursor: (pixels.width() as f32 / 2.0, pixels.height() as f32 / 2.0),
                    pixels,
                    cell: 105.0 * scale,
                }
                .scaled();
                let scores: Vec<Option<f32>> = references
                    .iter()
                    .map(|reference| Some(score(&look, reference)))
                    .collect();
                let own = candidates.iter().position(|name| *name == unique).unwrap();
                assert_eq!(
                    verdict(&scores),
                    Verdict::Sure(own),
                    "{unique} at {scale}: {scores:?}"
                );
            }
        }
    }

    #[test]
    fn only_a_clear_and_good_enough_best_is_sure() {
        assert_eq!(
            verdict(&[Some(0.9), Some(0.5), Some(0.3)]),
            Verdict::Sure(0)
        );
        assert_eq!(verdict(&[Some(0.5), Some(0.88)]), Verdict::Sure(1));
        // Ahead, but not by enough: two arts alike.
        assert_eq!(verdict(&[Some(0.8), Some(0.7)]), Verdict::Unsure);
        // Far ahead of the rest, but nothing matched well: art the table has wrong, or a screen
        // with something over the item.
        assert_eq!(verdict(&[Some(0.4), Some(0.1)]), Verdict::Unsure);
        // A unique without art can't be ruled out by a poor match of the others...
        assert_eq!(verdict(&[Some(0.45), None]), Verdict::Unsure);
        // ...but by a good one: the item looks like that art, and no two uniques share one.
        assert_eq!(verdict(&[None, Some(0.85), None]), Verdict::Sure(1));
        assert_eq!(verdict(&[None, None]), Verdict::Unsure);
    }

    #[test]
    fn the_player_is_asked_best_first_and_art_less_uniques_last() {
        assert_eq!(
            ranking(&[Some(0.3), None, Some(0.8), Some(0.5), None]),
            [2, 3, 0, 1, 4]
        );
    }
}
