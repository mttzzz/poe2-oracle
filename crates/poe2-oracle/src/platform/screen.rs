//! Reading the screen: a rectangle of the composed desktop -- the game and whatever is over it --
//! copied with GDI, for the XP bar's fill (`xp_bar`) and an unidentified unique's art
//! (`item_art`).

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};

use crate::overlay_layout::PhysicalRect;

/// Copies `rect` of the screen into a 32-bit top-down DIB section and hands its BGRA bytes to
/// `read` before the section is freed.
pub fn read<R>(rect: PhysicalRect, read: impl FnOnce(&[u8]) -> Option<R>) -> Option<R> {
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
