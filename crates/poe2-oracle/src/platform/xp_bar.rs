//! Reads PoE2's experience bar off the screen for the XP overlay: finds the game window through
//! `game_window`, copies the bar's few rows with GDI, and leaves the reading to
//! `crate::xp_tracker::read_fill`. It looks at the HUD's rails the overlay's plates stand on
//! the same way: a strip of each rail's lip, which `overlay_layout::rail_seen` tells from anything
//! else.
//!
//! Only the bar's own rect is copied -- `XpBarGeometry::capture`, inside the game's client area
//! by construction: 1536x10 pixels on the 4K test machine. The copy is a plain `SRCCOPY` blit
//! from the screen DC, i.e. the composed desktop: whatever the player sees there. So the bar is
//! read only while the screen shows the game at points along it ([`shows_the_game`]): a window
//! over it -- our price panel, which can span its middle, the tour's dim, another program --
//! makes the sample unreadable, like a screen without the HUD (loading screen, passive tree) that
//! `read_fill` refuses; the tracker keeps its last reading until the bar can be read again. The
//! game's own inventory and stash panels are drawn inside the game window and leave the bar
//! uncovered at 16:9 (verified live 2026-09-22 with the inventory open).
//!
//! No `CAPTUREBLT`: it only adds layered windows to the copy (the game's isn't one) and is known
//! to make the mouse cursor flicker on each blit, which here would be every two seconds over the
//! game.
//!
//! A blit waits for the desktop's next composition and costs the app about a millisecond of CPU
//! each. While the player is at the game `platform::lip_watch` reads the lips and the bar off
//! the duplicated desktop instead, and the sampler asks here only where the game is
//! ([`sample`]'s `read_pixels`).

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, IsIconic, WindowFromPoint};

use crate::overlay_layout::{PhysicalRect, hud_rails, rail_lip, rail_seen};
use crate::platform::game_window;
use crate::xp_tracker::{XpBarGeometry, read_fill};

/// One look at the game.
#[derive(Debug, Clone)]
pub struct BarSample {
    /// The game's client area on screen, physical pixels: the overlay's plates go in its HUD
    /// (`overlay_layout::hud_rails`).
    pub client: PhysicalRect,
    /// The game window's DPI scale (1.0 at 96 DPI), for sizing the overlay's windows.
    pub dpi_scale: f64,
    /// The fraction of the level the bar shows; `None` when it isn't readable, or wasn't read.
    pub fill: Option<f64>,
    /// Whether each plate's rail is on screen where the plate goes; `None` when not looked at.
    pub rails: Option<RailsSeen>,
}

/// Whether the flask and the skill panel's rails were seen where `overlay_layout::hud_rails`
/// puts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RailsSeen {
    pub flask: bool,
    pub skill: bool,
}

/// Looks at the game once: where it is, and with `read_pixels` its bar and rails too. `None`
/// while there is no game window or it is minimized. Blocking GDI work when reading pixels (a
/// readback of the composed screen), so call it off the UI thread.
pub fn sample(read_pixels: bool) -> Option<BarSample> {
    let hwnd = game_window::game_window()?;
    if unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }
    let client = game_window::client_rect_on_screen(hwnd)?;
    let dpi_scale = game_window::dpi_to_scale(unsafe { GetDpiForWindow(hwnd) });
    if !read_pixels {
        return Some(BarSample {
            client,
            dpi_scale,
            fill: None,
            rails: None,
        });
    }
    let geometry = XpBarGeometry::for_client(client);
    let fill = geometry
        .as_ref()
        .filter(|geometry| shows_the_game(hwnd, geometry.capture))
        .and_then(|geometry| read_screen(geometry.capture, |bgra| read_fill(geometry, bgra)));
    let plates = hud_rails(client);
    // Like the bar: only pixels the game itself shows there count -- another program's light
    // line over a darker one would pass for a lip.
    let seen = |plate: PhysicalRect| {
        let lip = rail_lip(plate, client.height);
        let width = usize::try_from(lip.width).unwrap_or(0);
        shows_the_game(hwnd, lip)
            && read_screen(lip, |bgra| Some(rail_seen(bgra, width))).unwrap_or(false)
    };
    Some(BarSample {
        client,
        dpi_scale,
        fill,
        rails: Some(RailsSeen {
            flask: seen(plates.flask),
            skill: seen(plates.skill),
        }),
    })
}

/// Whether the screen shows the game itself at points along the middle row of `rect`: the bar's
/// capture, or a rail's lip (whose middle row is always the lip's, never the plate's under it).
/// Anything over it -- the price panel, which can span the bar's middle, the tour's dim, another
/// program, the desktop after an Alt+Tab -- is what a blit copies, and a cover that happens to
/// pass `read_fill`'s checks reads as a wrong fill: a drop the tracker takes for a loss, then a
/// "gain" when the cover goes. Windows that let clicks through are passed over by
/// `WindowFromPoint`, as by the mouse.
pub(crate) fn shows_the_game(game: HWND, rect: PhysicalRect) -> bool {
    const POINTS: i32 = 9;
    let y = rect.y + rect.height / 2;
    (0..POINTS).all(|i| {
        let x = rect.x + (rect.width - 1) * i / (POINTS - 1);
        let at = unsafe { WindowFromPoint(POINT { x, y }) };
        !at.is_invalid() && unsafe { GetAncestor(at, GA_ROOT) } == game
    })
}

/// Copies `rect` of the screen into a 32-bit top-down DIB section and hands its BGRA bytes to
/// `read` before the section is freed.
fn read_screen<R>(rect: PhysicalRect, read: impl FnOnce(&[u8]) -> Option<R>) -> Option<R> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: rect.width,
            // Negative: rows top to bottom, as on screen.
            biHeight: -rect.height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let memory = CreateCompatibleDC(Some(screen));
        let mut bits = std::ptr::null_mut();
        let result = if memory.is_invalid() {
            None
        } else {
            match CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bitmap) => {
                    let previous = SelectObject(memory, bitmap.into());
                    let copied = BitBlt(
                        memory,
                        0,
                        0,
                        rect.width,
                        rect.height,
                        Some(screen),
                        rect.x,
                        rect.y,
                        SRCCOPY,
                    )
                    .is_ok();
                    // GDI may still have the blit queued; the section's bits are only current
                    // after a flush.
                    let _ = GdiFlush();
                    let len = rect.width as usize * rect.height as usize * 4;
                    let result = if copied {
                        read(std::slice::from_raw_parts(bits.cast::<u8>(), len))
                    } else {
                        None
                    };
                    SelectObject(memory, previous);
                    let _ = DeleteObject(bitmap.into());
                    result
                }
                Err(_) => None,
            }
        };
        if !memory.is_invalid() {
            let _ = DeleteDC(memory);
        }
        ReleaseDC(None, screen);
        result
    }
}
