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
//! `read_fill` refuses; the tracker keeps its last reading until the bar can be read again. So
//! does the pointer on the bar, which shows the game's tooltip and makes the bar read wrong
//! ([`bar_hovered`]). The game's own inventory and stash panels are drawn inside the game window
//! and leave the bar uncovered at 16:9 (verified live 2026-09-22 with the inventory open).
//!
//! No `CAPTUREBLT`: it only adds layered windows to the copy (the game's isn't one) and is known
//! to make the mouse cursor flicker on each blit, which here would be every two seconds over the
//! game.
//!
//! A blit waits for the desktop's next composition and costs the app about a millisecond of CPU
//! each. While the game is in front `platform::lip_watch` reads the lips and the bar off the
//! duplicated desktop instead, and the sampler asks here only where the game is
//! ([`sample`]'s `read_pixels`); out of the front it asks for the bar only now and then
//! (`xp_tracker::XpTracker::unattended_look_due`), since no experience comes in then.
//!
//! With `POE2_ORACLE_XP_DEBUG=1`, the looks -- here and in `platform::lip_watch` -- keep the rows
//! of the bar they read last ([`keep_rows`]), and a reading the tracker finds suspect has them
//! saved as a PNG ([`snapshot`]): what was over the bar, for the next misread to be seen.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::SystemInformation::GetSystemTime;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, IsIconic, WindowFromPoint};

use crate::overlay_layout::{PhysicalRect, hud_rails, rail_lip, rail_seen};
use crate::paths;
use crate::platform::game_window;
use crate::xp_tracker::{BarLook, Suspect, XpBarGeometry, read_fill};

/// One look at the game.
#[derive(Debug, Clone)]
pub struct BarSample {
    /// The game's client area on screen, physical pixels: the overlay's plates go in its HUD
    /// (`overlay_layout::hud_rails`).
    pub client: PhysicalRect,
    /// The game window's DPI scale (1.0 at 96 DPI), for sizing the overlay's windows.
    pub dpi_scale: f64,
    /// The look at the bar: what it read, or `Skipped` when it wasn't asked for ([`sample`]).
    pub bar: BarLook,
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

/// Looks at the game once: where it is, and with `read_pixels` its rails too, and its bar if
/// `bar` -- told whether the game is in front -- asks for it. `None` while there is no game
/// window or it is minimized. Blocking GDI work when reading pixels (a readback of the composed
/// screen), so call it off the UI thread.
pub fn sample(read_pixels: bool, bar: impl FnOnce(bool) -> bool) -> Option<BarSample> {
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
            bar: BarLook::Skipped,
            rails: None,
        });
    }
    let bar = if bar(game_window::in_front(hwnd)) {
        XpBarGeometry::for_client(client)
            .filter(|geometry| {
                !bar_hovered(geometry, Instant::now()) && shows_the_game(hwnd, geometry.capture)
            })
            .and_then(|geometry| {
                read_screen(geometry.capture, |bgra| {
                    let fill = read_fill(&geometry, bgra);
                    keep_rows(&geometry, bgra, fill);
                    fill
                })
            })
            .map_or(BarLook::Unreadable, BarLook::Read)
    } else {
        BarLook::Skipped
    };
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
        bar,
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

/// How long after the pointer leaves the experience bar its readings are still set aside
/// ([`bar_hovered`]): the game's tooltip fades, and whatever it did to the bar with it. The
/// sampler takes a reading every two seconds, and three missed in a row still count as play.
const HOVER_GRACE: Duration = Duration::from_secs(1);

/// When a look at the bar last found the pointer on it ([`bar_hovered`]): the lip watcher's
/// looks and the sampler's alike.
static HOVERED_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether a look at the bar of `geometry` at `now` sets its reading aside, as covered: the
/// pointer is on the bar (`XpBarGeometry::hovered`: physical pixels, as `GetCursorPos` gives them
/// to this per-monitor-aware process), or left it less than `HOVER_GRACE` ago. There the game
/// shows the bar's tooltip, and the bar reads wrong but whole -- live on 2026-09-28, 94.75 % for
/// the 40 s the owner rested the pointer there, where it showed 59.33 %.
pub(crate) fn bar_hovered(geometry: &XpBarGeometry, now: Instant) -> bool {
    let on_bar = game_window::cursor_pos().filter(|&pointer| geometry.hovered(pointer));
    let mut hovered_at = HOVERED_AT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let lately = |at: &Instant| now.saturating_duration_since(*at) < HOVER_GRACE;
    if let Some(pointer) = on_bar {
        if !hovered_at.as_ref().is_some_and(lately) {
            log::debug!(
                target: "poe2_oracle::xp_tracker",
                "xp: the pointer is on the bar at {pointer:?}: its readings set aside"
            );
        }
        *hovered_at = Some(hovered_at.map_or(now, |at| at.max(now)));
    }
    hovered_at.as_ref().is_some_and(lately)
}

/// Whether the app runs with `POE2_ORACLE_XP_DEBUG=1`: the looks keep the rows of the bar they
/// read last, for a [`snapshot`] of a suspect reading. Read once.
static XP_DEBUG: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("POE2_ORACLE_XP_DEBUG").is_some_and(|value| value == "1"));

/// How many snapshots [`snapshot`] leaves, the newest.
const SNAPSHOTS_KEPT: usize = 30;

/// The rows of the bar a look read, BGRA, top to bottom, and what they read as.
#[derive(Clone)]
pub(crate) struct BarRows {
    reading: f64,
    width: u32,
    height: u32,
    bgra: Vec<u8>,
}

/// The rows of the latest look that read the bar, while debugging ([`XP_DEBUG`]).
static LAST_ROWS: Mutex<Option<BarRows>> = Mutex::new(None);

/// Keeps `bgra`, the capture of `geometry` that read as `fill`, as the rows of the latest look
/// that read the bar -- only while debugging ([`XP_DEBUG`]).
pub(crate) fn keep_rows(geometry: &XpBarGeometry, bgra: &[u8], fill: Option<f64>) {
    let Some(reading) = fill.filter(|_| *XP_DEBUG) else {
        return;
    };
    let mut last = LAST_ROWS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let rows = last.get_or_insert_with(|| BarRows {
        reading,
        width: 0,
        height: 0,
        bgra: Vec::new(),
    });
    rows.reading = reading;
    rows.width = geometry.capture.width.unsigned_abs();
    rows.height = geometry.capture.height.unsigned_abs();
    rows.bgra.clear();
    rows.bgra.extend_from_slice(bgra);
}

/// The rows of the latest look that read the bar, kept while debugging ([`keep_rows`]).
pub(crate) fn last_rows() -> Option<BarRows> {
    LAST_ROWS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Saves `rows` -- the look behind `suspect`, the change the tracker starts holding -- as a PNG in
/// `paths::xp_debug_dir()`, named by the time (UTC, as the log's) and the values, and leaves the
/// newest `SNAPSHOTS_KEPT` there. Blocking file work.
pub(crate) fn snapshot(rows: &BarRows, suspect: Suspect) {
    let dir = paths::xp_debug_dir();
    let time = unsafe { GetSystemTime() };
    let path = dir.join(format!(
        "{:04}-{:02}-{:02}T{:02}-{:02}-{:02}.{:03}Z_{:.4}_to_{:.4}_read_{:.4}.png",
        time.wYear,
        time.wMonth,
        time.wDay,
        time.wHour,
        time.wMinute,
        time.wSecond,
        time.wMilliseconds,
        suspect.from,
        suspect.to,
        rows.reading
    ));
    let (pixels, _) = rows.bgra.as_chunks::<4>();
    let rgb: Vec<u8> = pixels.iter().flat_map(|&[b, g, r, _]| [r, g, b]).collect();
    let saved = fs::create_dir_all(&dir)
        .map_err(image::ImageError::from)
        .and_then(|()| {
            image::save_buffer_with_format(
                &path,
                &rgb,
                rows.width,
                rows.height,
                image::ColorType::Rgb8,
                image::ImageFormat::Png,
            )
        });
    match saved {
        Ok(()) => log::info!("xp: the bar's pixels saved to {}", path.display()),
        Err(err) => {
            log::warn!(
                "xp: saving the bar's pixels to {} failed: {err}",
                path.display()
            );
            return;
        }
    }
    prune(&dir);
}

/// Deletes all but the newest `SNAPSHOTS_KEPT` snapshots in `dir`, whose names start with their
/// time.
fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut snapshots: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "png"))
        .collect();
    snapshots.sort();
    let stale = snapshots.len().saturating_sub(SNAPSHOTS_KEPT);
    for path in &snapshots[..stale] {
        if let Err(err) = fs::remove_file(path) {
            log::warn!("deleting {} failed: {err}", path.display());
        }
    }
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
