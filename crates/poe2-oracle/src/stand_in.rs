//! The developer's stand-in item, in a build made with `--features stand-in` and never in a
//! release (`packaging/build-release.ps1` doesn't name the feature): for trying the app in the
//! game on an item the developer doesn't have, made from a real trade listing by `item-synth`
//! (CONTRIBUTING.md, "Items made from trade listings").
//!
//! With `item.txt` in `%LOCALAPPDATA%\poe2-oracle\data\stand-in`, a price check takes that text as
//! the game's copy of the item under the cursor and presses no copy keys; the rest is real -- the
//! game, the cursor's place, the panel, the searches. With `art.png` and `place.txt` beside it
//! (`x y width height` on one line, physical screen pixels), the art is drawn over the game at
//! that place, as the game draws an item in an inventory cell: what a check that reads the item's
//! art off the screen sees there. Removing a file ends what it does; the art follows its files
//! within a second.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, Result, bail};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
    RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow, TranslateMessage, ULW_ALPHA,
    UpdateLayeredWindow, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

fn dir() -> PathBuf {
    crate::paths::data_dir().join("stand-in")
}

/// The stand-in item's text, when there is one.
pub fn item_text() -> Option<String> {
    let path = dir().join("item.txt");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            log::info!("stand-in: {} instead of the game's copy", path.display());
            Some(text)
        }
        Err(_) => None,
    }
}

/// Starts drawing the stand-in's art while its files are there: a thread with a window of its own.
pub fn start() {
    let spawned = std::thread::Builder::new()
        .name("stand-in art".to_owned())
        .spawn(|| {
            if let Err(err) = draw_art() {
                log::warn!("stand-in: the art window failed: {err:#}");
            }
        });
    if let Err(err) = spawned {
        log::warn!("stand-in: no art thread: {err}");
    }
}

/// What the art window shows: the art's pixels, premultiplied BGRA rows at the place's size, and
/// where.
struct Art {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    bgra: Vec<u8>,
}

fn read_art() -> Result<Art> {
    let dir = dir();
    let place = std::fs::read_to_string(dir.join("place.txt")).context("place.txt")?;
    let numbers: Vec<i32> = place
        .split_whitespace()
        .map(|number| number.parse().context("place.txt holds x y width height"))
        .collect::<Result<_>>()?;
    let [x, y, width, height] = numbers[..] else {
        bail!("place.txt holds x y width height");
    };
    if width <= 0 || height <= 0 {
        bail!("place.txt's width and height must be above 0");
    }
    let art = image::open(dir.join("art.png"))
        .context("art.png")?
        .into_rgba8();
    let art = image::imageops::resize(
        &art,
        width as u32,
        height as u32,
        image::imageops::FilterType::Triangle,
    );
    let mut bgra = Vec::with_capacity(art.len());
    for pixel in art.pixels() {
        let [r, g, b, a] = pixel.0;
        let premultiplied = |channel: u8| (u16::from(channel) * u16::from(a) / 255) as u8;
        bgra.extend([premultiplied(b), premultiplied(g), premultiplied(r), a]);
    }
    Ok(Art {
        x,
        y,
        width,
        height,
        bgra,
    })
}

/// When the art's files last changed, or `None` while either is missing.
fn art_stamp() -> Option<(SystemTime, SystemTime)> {
    let dir = dir();
    let modified = |name: &str| std::fs::metadata(dir.join(name)).ok()?.modified().ok();
    Some((modified("art.png")?, modified("place.txt")?))
}

fn draw_art() -> Result<()> {
    let module = unsafe { GetModuleHandleW(PCWSTR::null()) }.context("GetModuleHandleW")?;
    let class_name = w!("PoE2OracleStandInArt");
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(art_proc),
        hInstance: module.into(),
        lpszClassName: class_name,
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        bail!("RegisterClassExW failed");
    }
    // Layered and click-through, over everything, never activated: the game keeps the input, and
    // the cursor over the art is over the game's cell below it.
    let window = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .context("CreateWindowExW")?;

    let mut shown: Option<(SystemTime, SystemTime)> = None;
    loop {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let stamp = art_stamp();
        if stamp != shown {
            match stamp.map(|_| read_art()) {
                Some(Ok(art)) => match show(window, &art) {
                    Ok(()) => log::info!(
                        "stand-in: art at {} {} {}x{}",
                        art.x,
                        art.y,
                        art.width,
                        art.height
                    ),
                    Err(err) => log::warn!("stand-in: can't draw the art: {err:#}"),
                },
                Some(Err(err)) => {
                    let _ = unsafe { ShowWindow(window, SW_HIDE) };
                    log::warn!("stand-in: no art: {err:#}");
                }
                None => {
                    let _ = unsafe { ShowWindow(window, SW_HIDE) };
                }
            }
            shown = stamp;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn show(window: HWND, art: &Art) -> Result<()> {
    let screen = unsafe { GetDC(None) };
    let memory = unsafe { CreateCompatibleDC(Some(screen)) };
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: art.width,
            // Negative: rows top to bottom, as the pixels are.
            biHeight: -art.height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let result = (|| -> Result<()> {
        let bitmap =
            unsafe { CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0) }
                .context("CreateDIBSection")?;
        unsafe {
            std::ptr::copy_nonoverlapping(art.bgra.as_ptr(), bits.cast::<u8>(), art.bgra.len());
        }
        let previous = unsafe { SelectObject(memory, bitmap.into()) };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let place = POINT { x: art.x, y: art.y };
        let size = SIZE {
            cx: art.width,
            cy: art.height,
        };
        let origin = POINT { x: 0, y: 0 };
        let updated = unsafe {
            UpdateLayeredWindow(
                window,
                Some(screen),
                Some(&raw const place),
                Some(&raw const size),
                Some(memory),
                Some(&raw const origin),
                COLORREF(0),
                Some(&raw const blend),
                ULW_ALPHA,
            )
        };
        unsafe {
            SelectObject(memory, previous);
            let _ = DeleteObject(bitmap.into());
        }
        updated.context("UpdateLayeredWindow")?;
        let _ = unsafe { ShowWindow(window, SW_SHOWNOACTIVATE) };
        Ok(())
    })();
    unsafe {
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
    }
    result
}

unsafe extern "system" fn art_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
