//! Draws the game-styled ornaments as the app's windows show them, for review: the parts and
//! sprites `poe2_oracle::ui::ornament` gives `ui::style` to paint, composed as GPUI's renderer
//! composes them -- a menu and its frame's corner, the settings window's corner over its title
//! bar, the diamonds, a rule and a group heading's rule -- at 100, 125, 150 and 200 %, and at the
//! test machine's own 200 % with the UI at 120 %, over the backgrounds they sit on, each with an
//! 8x nearest-neighbour zoom. `corners-8x.png` lays every zoomed corner side by side.
//!
//! ```text
//! cargo run -p poe2-oracle --example ornaments -- .tmp/ornaments
//! ```

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use image::{Rgba, RgbaImage};
use poe2_oracle::ui::ornament::{
    self, DeviceRect, FrameMetrics, Picture, Placed, RULE_DIAMOND, Scale,
};
use poe2_oracle::ui::theme::{
    BG_CARD, BG_MENU, BG_PANEL, BORDER_GOLD, GOLD, GOLD_LIGHT, TITLE_BOTTOM, TITLE_TOP, blend,
};

/// Display scale and UI scale: Windows' usual steps with the UI at 100 %, then the test
/// machine's 4K screen at 200 % with the UI at 120 %.
const SCALES: [(f32, f32); 5] = [(1., 1.), (1.25, 1.), (1.5, 1.), (2., 1.), (2., 1.2)];
const ZOOM: u32 = 8;
/// Room around a drawing, px.
const MARGIN: f32 = 12.;

fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("usage: ornaments <directory>")?,
    );
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut corners = Vec::new();
    for (device, unit) in SCALES {
        let scale = Scale { device, unit };
        let name = |what: &str| format!("{what}@{}.png", label(scale));
        let arm = FrameMetrics::new(scale).arm;
        let margin = scale.length(MARGIN);
        // A corner's zoom: its sprite and a few pixels of the lines running on.
        let corner = |image: &RgbaImage, at: i32| zoom(&crop(image, at, at, arm + 6, arm + 6));

        let menu = menu(scale).to_image();
        let menu_corner = corner(&menu, margin);
        save(&dir, &name("menu"), &menu)?;
        save(&dir, &name("menu-corner-8x"), &menu_corner)?;

        let window = window(scale).to_image();
        let window_corner = corner(&window, 0);
        save(&dir, &name("window"), &window)?;
        save(&dir, &name("window-corner-8x"), &window_corner)?;

        let diamonds = diamonds(scale).to_image();
        save(&dir, &name("diamonds"), &diamonds)?;
        save(&dir, &name("diamonds-8x"), &zoom(&diamonds))?;

        let rules = rules(scale).to_image();
        save(&dir, &name("rules"), &rules)?;
        save(&dir, &name("rules-8x"), &zoom(&rules))?;
        corners.push((menu_corner, window_corner));
    }
    save(&dir, "corners-8x.png", &side_by_side(&corners))?;
    Ok(())
}

/// A menu over the settings window: its list 240 by 102 px in its frame, the current choice's
/// diamond in its first row.
fn menu(scale: Scale) -> Picture {
    let margin = scale.length(MARGIN);
    let list = DeviceRect::new(
        margin,
        margin,
        margin + scale.length(240.),
        margin + scale.length(102.),
    );
    let mut picture = Picture::new(list.right + margin, list.bottom + margin, BG_PANEL);
    picture.fill(list, BG_MENU, u8::MAX);
    // The first row: the frame's keep-out, then the row's padding, its diamond's box and 28 px of
    // height.
    let row_top = list.top + scale.length(ornament::FRAME_CLEAR);
    let left = list.left + scale.length(ornament::FRAME_CLEAR + 10.);
    let marker = ornament::diamond(
        DeviceRect::new(
            left,
            row_top,
            left + scale.length(7.),
            row_top + scale.length(28.),
        ),
        7.,
        scale,
        GOLD,
        false,
    );
    sprite(&mut picture, &marker);
    frame(&mut picture, list, scale);
    picture
}

/// The settings window's top left corner: its title bar -- the bronze fading into black, a gold
/// line under it, its diamond centred between the frame's inner line and that line -- over the
/// window, the frame on the window's edge.
fn window(scale: Scale) -> Picture {
    let mut picture = Picture::new(scale.length(170.), scale.length(90.), BG_PANEL);
    let title = scale.length(40.);
    for y in 0..title {
        let along = (y as f32 + 0.5) / title as f32;
        let row = DeviceRect::new(0, y, picture.width(), y + 1);
        picture.fill(row, blend(TITLE_TOP, TITLE_BOTTOM, along), u8::MAX);
    }
    let border = DeviceRect::new(0, title - scale.hairline(), picture.width(), title);
    picture.fill(border, BORDER_GOLD, u8::MAX);
    let (left, band) = (scale.length(20.), scale.length(ornament::FRAME_GAP + 1.));
    let mark = ornament::diamond(
        DeviceRect::new(left, band, left + scale.length(8.), border.top),
        8.,
        scale,
        GOLD,
        false,
    );
    sprite(&mut picture, &mark);
    frame(
        &mut picture,
        DeviceRect::new(0, 0, scale.length(720.), scale.length(540.)),
        scale,
    );
    picture
}

/// The free diamonds, over the window and over a card: a group's 6 px, a menu's 7, a title
/// bar's 8, the report window's 10, and the tour's steps -- passed, current and to come.
fn diamonds(scale: Scale) -> Picture {
    let sizes = [
        (6., GOLD),
        (7., GOLD),
        (8., GOLD),
        (10., GOLD),
        (6., GOLD),
        (8., GOLD_LIGHT),
        (6., BORDER_GOLD),
    ];
    let height = scale.length(20.);
    let width = scale.length(12. + sizes.iter().map(|(size, _)| size + 10.).sum::<f32>());
    let mut picture = Picture::new(width, 2 * height, BG_PANEL);
    picture.fill(
        DeviceRect::new(0, height, width, 2 * height),
        BG_CARD,
        u8::MAX,
    );
    for top in [0, height] {
        let mut left = scale.length(12.);
        for (size, color) in sizes {
            let right = left + scale.length(size);
            let rect = DeviceRect::new(left, top, right, top + height);
            sprite(
                &mut picture,
                &ornament::diamond(rect, size, scale, color, false),
            );
            left = right + scale.length(10.);
        }
    }
    picture
}

/// A rule 280 px long, and under it a group heading's diamond and rule, a gap where its title
/// goes.
fn rules(scale: Scale) -> Picture {
    let margin = scale.length(MARGIN);
    let width = margin + scale.length(280.);
    let rule_box = DeviceRect::new(margin, margin, width, margin + scale.length(RULE_DIAMOND));
    let heading_top = rule_box.bottom + margin;
    let heading = DeviceRect::new(margin, heading_top, width, heading_top + scale.length(17.));
    let mut picture = Picture::new(width + margin, heading.bottom + margin, BG_PANEL);
    for part in ornament::rule(rule_box, scale, BORDER_GOLD, GOLD)
        .iter()
        .flatten()
    {
        sprite(&mut picture, part);
    }
    let mark_right = heading.left + scale.length(6.);
    let mark = DeviceRect::new(heading.left, heading.top, mark_right, heading.bottom);
    sprite(
        &mut picture,
        &ornament::diamond(mark, 6., scale, GOLD, true),
    );
    let line_left = mark_right + scale.length(7. + 90. + 7.);
    let line = DeviceRect::new(line_left, heading.top, heading.right, heading.bottom);
    if let Some(line) = ornament::fading_line(line, scale, BORDER_GOLD, false) {
        sprite(&mut picture, &line);
    }
    picture
}

fn frame(picture: &mut Picture, rect: DeviceRect, scale: Scale) {
    let parts = ornament::frame(rect, FrameMetrics::new(scale));
    for line in &parts.lines {
        picture.line(line);
    }
    for corner in &parts.corners {
        sprite(picture, corner);
    }
}

fn sprite(picture: &mut Picture, placed: &Placed) {
    picture.sprite(placed, &ornament::rasterize(&placed.sprite));
}

/// A scale's part of a file name: `125`, or `200-ui120` with the UI scaled.
fn label(scale: Scale) -> String {
    let percent = |value: f32| (value * 100.).round() as u32;
    if scale.unit == 1. {
        percent(scale.device).to_string()
    } else {
        format!("{}-ui{}", percent(scale.device), percent(scale.unit))
    }
}

fn crop(image: &RgbaImage, x: i32, y: i32, width: i32, height: i32) -> RgbaImage {
    RgbaImage::from_fn(width as u32, height as u32, |cx, cy| {
        *image.get_pixel(x as u32 + cx, y as u32 + cy)
    })
}

fn zoom(image: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(image.width() * ZOOM, image.height() * ZOOM, |x, y| {
        *image.get_pixel(x / ZOOM, y / ZOOM)
    })
}

/// The zoomed corners in two rows, the menus' over the windows', scale by scale, on grey.
fn side_by_side(corners: &[(RgbaImage, RgbaImage)]) -> RgbaImage {
    let gap = 2 * ZOOM;
    let width = corners
        .iter()
        .map(|(menu, _)| menu.width() + gap)
        .sum::<u32>()
        + gap;
    let row = corners
        .iter()
        .map(|(menu, _)| menu.height())
        .max()
        .unwrap_or(0);
    let mut sheet = RgbaImage::from_pixel(width, 2 * row + 3 * gap, Rgba([48, 48, 52, 255]));
    let mut left = gap;
    for (menu, window) in corners {
        for (image, top) in [(menu, gap), (window, 2 * gap + row)] {
            for (x, y, pixel) in image.enumerate_pixels() {
                sheet.put_pixel(left + x, top + y, *pixel);
            }
        }
        left += menu.width() + gap;
    }
    sheet
}

fn save(dir: &Path, name: &str, image: &RgbaImage) -> anyhow::Result<()> {
    let path = dir.join(name);
    image
        .save(&path)
        .with_context(|| format!("writing {}", path.display()))?;
    println!("{}", path.display());
    Ok(())
}
