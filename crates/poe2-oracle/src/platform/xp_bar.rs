//! Reads PoE2's experience bar off the screen for the XP overlay: finds the game window through
//! `game_window`, copies the bar's few rows with GDI, and leaves the reading to
//! `crate::xp_tracker::read_fill`.
//!
//! Only the bar's own rect is copied -- `XpBarGeometry::capture`, inside the game's client area
//! by construction: 1536x10 pixels on the 4K test machine. The copy is a plain `SRCCOPY` blit
//! from the screen DC, i.e. the composed desktop: whatever the player sees there. A window over
//! the bar (a PoE Overlay II panel, a tooltip) or a screen without the HUD (loading screen,
//! passive tree) is therefore what gets read, and `read_fill` refuses it -- the tracker counts
//! that time as not played and the overlay hides. The game's own inventory and stash panels leave
//! the bar uncovered at 16:9 (verified live 2026-09-22 with the inventory open).
//!
//! No `CAPTUREBLT`: it only adds layered windows to the copy (the game's isn't one) and is known
//! to make the mouse cursor flicker on each blit, which here would be every two seconds over the
//! game. Whether our own layered overlay windows appear in a plain blit is not verified; if they
//! cover the bar and do appear, `read_fill` refuses the frame like any other cover.

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::IsIconic;

use crate::overlay_layout::PhysicalRect;
use crate::platform::game_window;
use crate::xp_tracker::{XpBarGeometry, read_fill};

/// One look at the game.
#[derive(Debug, Clone)]
pub struct BarSample {
    /// Where the bar is, for placing the overlay; `None` for a game window too small or too narrow
    /// to read.
    pub geometry: Option<XpBarGeometry>,
    /// The game window's DPI scale (1.0 at 96 DPI), for sizing the overlay window.
    pub dpi_scale: f64,
    /// The fraction of the level the bar shows; `None` when it isn't readable.
    pub fill: Option<f64>,
}

/// Looks at the bar once; `None` while there is no game window or it is minimized. Blocking GDI
/// work (a readback of the composed screen), so call it off the UI thread.
pub fn sample() -> Option<BarSample> {
    let hwnd = game_window::game_window()?;
    if unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }
    let client = game_window::client_rect_on_screen(hwnd)?;
    let dpi_scale = game_window::dpi_to_scale(unsafe { GetDpiForWindow(hwnd) });
    let geometry = XpBarGeometry::for_client(client);
    let fill = geometry
        .as_ref()
        .and_then(|geometry| read_screen(geometry.capture, |bgra| read_fill(geometry, bgra)));
    Some(BarSample {
        geometry,
        dpi_scale,
        fill,
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
