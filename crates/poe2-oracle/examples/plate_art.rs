//! Draws the XP overlay's plates (`poe2_oracle::plate_art`) into the site's figures of the HUD
//! (site/ui/xp.js): each plate's pixels where the app puts them on the test machine's 4K game, over
//! the figure's part of the game and transparent elsewhere, so the site lays them on its capture of
//! the HUD exactly as the app lays them on the game.
//!
//! ```text
//! cargo run -p poe2-oracle --example plate_art -- site/ui/img
//! ```

use std::path::PathBuf;

use anyhow::Context as _;
use image::Rgba;
use poe2_oracle::overlay_layout::{PhysicalRect, hud_rails};
use poe2_oracle::plate_art::PlateArt;

/// The test machine's game, the one the site's HUD captures are of.
const GAME: PhysicalRect = PhysicalRect {
    x: 0,
    y: 0,
    width: 3840,
    height: 2160,
};

/// A figure's part of the game (site/ui/xp.css `.oui-xp-figure`: 420x220 HUD px, a 2160-row
/// game's pixels halved), from row 1720 -- the flask panel's from column 300, the skill panel's
/// from 2740.
const FIGURE_TOP: i32 = 1720;
const FIGURE_WIDTH: i32 = 840;
const FIGURE_HEIGHT: i32 = 440;

fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("usage: plate_art <directory>")?,
    );
    let rails = hud_rails(GAME);
    let plates = [
        ("flask", PlateArt::flask(&rails, GAME.height), 300),
        ("skill", PlateArt::skill(&rails, GAME.height), 2740),
    ];
    for (rail, art, left) in plates {
        let figure = PhysicalRect {
            x: left,
            y: FIGURE_TOP,
            width: FIGURE_WIDTH,
            height: FIGURE_HEIGHT,
        };
        let mut image = art.slice(figure).image;
        // A slice is BGRA, as GPUI takes it; a PNG is RGBA.
        for Rgba([b, _, r, _]) in image.pixels_mut() {
            std::mem::swap(b, r);
        }
        let path = dir.join(format!("plate-{rail}.png"));
        image
            .save(&path)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}
