//! The screen around the cursor at the moment of a check, for telling an unidentified unique by its
//! art (`crate::unique_art`): one copy of the game's pixels there, matched in memory for that check
//! and dropped -- never kept, never sent.

use image::RgbaImage;

use crate::overlay_layout::PhysicalRect;
use crate::platform::{game_window, screen};
use crate::unique_art::Look;

/// An inventory cell's edge over the height of the game's picture: 105 pixels at 2160 (measured on
/// the owner's 4K screen, 2026-10-12). The game scales its inventory and stash with the height.
const CELL_PER_HEIGHT: f32 = 105.0 / 2160.0;
/// How many cells the copy reaches out from the cursor each way: the cursor is somewhere over the
/// item, and the biggest items are four cells long.
const REACH_CELLS: f32 = 4.5;

/// The game's pixels around `cursor` (physical screen pixels), as the screen shows them now.
/// `None` without a game window to look in.
pub fn look_around(cursor: (i32, i32)) -> Option<Look> {
    let client = game_window::client_rect_on_screen(game_window::game_window()?)?;
    let cell = client.height as f32 * CELL_PER_HEIGHT;
    let reach = (REACH_CELLS * cell).ceil() as i32;
    // Within the game's picture: the item is in it.
    let left = (cursor.0 - reach).max(client.x);
    let top = (cursor.1 - reach).max(client.y);
    let right = (cursor.0 + reach).min(client.x + client.width);
    let bottom = (cursor.1 + reach).min(client.y + client.height);
    if right <= left || bottom <= top {
        return None;
    }
    let rect = PhysicalRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    };
    let pixels = screen::read(rect, |bgra| {
        let mut rgba = bgra.to_vec();
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            pixel[3] = u8::MAX;
        }
        RgbaImage::from_raw(rect.width as u32, rect.height as u32, rgba)
    })?;
    Some(Look {
        pixels,
        cursor: ((cursor.0 - left) as f32, (cursor.1 - top) as f32),
        cell,
    })
}
