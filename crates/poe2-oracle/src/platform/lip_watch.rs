//! Watches the lips of the HUD's rails for the XP overlay frame by frame, so that a plate steps
//! aside the moment the game draws anything over its rail -- a tooltip, a panel, the chat -- and
//! comes back the moment it's gone; and reads the experience bar off the same frames.
//!
//! `platform::xp_bar` looks at the lips every two seconds with a GDI blit of the screen, and a
//! blit waits for the desktop's next composition: 11 ms a lip, 27 ms both, up to 145 ms (measured
//! 2026-09-24 on the test machine's 4K game) -- far too slow to look many times a second. Here the
//! desktop comes through DXGI desktop duplication instead: Windows hands over each frame it
//! composes as a texture, the lips' few rows are copied out of it on the GPU, and only those rows
//! are read back -- the bar's too, every `BAR_EVERY`, so that the sampler needs no blit at all
//! while this watches. The watching runs on a thread of its own while the game is in front and
//! not minimised, and a moment after (`platform::lip_schedule`, which paces the looks too). The
//! thread learns of the foreground from Windows' reports, handed on by the overlay
//! ([`LipWatch::foreground`]), and asks nothing while it doesn't watch: it sleeps till told
//! otherwise. Once it stops, the duplication and its Direct3D device are let go -- their video
//! memory and the graphics driver's threads with them -- and [`LipReport::Idle`] leaves the
//! plates and the bar to the sampler's slower look.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_UNSPECIFIED,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIFactory1,
    IDXGIOutput1, IDXGIOutput5, IDXGIOutputDuplication, IDXGIResource,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::IsIconic;
use windows::core::Interface;

use crate::overlay_layout::{PhysicalRect, hud_rails, rail_lip, rail_seen};
use crate::platform::game_window::{self, Foreground};
use crate::platform::lip_schedule::{WhenToWatch, look_wait};
use crate::platform::xp_bar::{RailsSeen, shows_the_game};
use crate::xp_tracker::{XpBarGeometry, read_fill};

/// How long it waits for a frame before looking at its orders again: a still desktop sends none.
const FRAME_WAIT_MS: u32 = 100;
/// How often a look reads the experience bar too: the sampler takes a reading every two seconds,
/// and a reading this old is as good as its own.
const BAR_EVERY: Duration = Duration::from_millis(500);

/// What the watcher saw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LipReport {
    /// The latest looks: whether each rail's lip shows, as `overlay_layout::rail_seen` reads it,
    /// and the fill the bar showed when last read (`xp_tracker::read_fill`), `None` when it was
    /// covered or unreadable. Sent at the first look and whenever either changes.
    Seen { rails: RailsSeen, fill: Option<f64> },
    /// Not watching -- the game isn't in front, there is no game to watch, or the desktop can't
    /// be duplicated -- so the sampler's look decides.
    Idle,
}

/// The watching thread's handle: it watches the rails of the game it was last given, while the
/// game is in front.
pub struct LipWatch {
    orders: Sender<Order>,
}

/// What the watching thread is told.
enum Order {
    /// The client area of the game to watch the rails of, or none.
    Game(Option<PhysicalRect>),
    /// A new foreground window, of this kind.
    Foreground(Foreground),
}

impl LipWatch {
    /// Starts the watching thread, which reports on the returned channel and ends once the
    /// handle is dropped.
    pub fn start() -> Result<(LipWatch, async_channel::Receiver<LipReport>)> {
        let (orders, received) = mpsc::channel();
        let (reports, reported) = async_channel::unbounded();
        std::thread::Builder::new()
            .name("lip-watch".into())
            .spawn(move || watch(&received, &reports))
            .context("starting the lip watcher")?;
        Ok((LipWatch { orders }, reported))
    }

    /// Watches the rails of a game whose client area is `game`, or nothing.
    pub fn watch(&self, game: Option<PhysicalRect>) {
        let _ = self.orders.send(Order::Game(game));
    }

    /// Takes Windows' report of a new foreground window (`game_window::watch_foreground`): the
    /// watching follows the game to the front and away from it.
    pub fn foreground(&self, foreground: Foreground) {
        let _ = self.orders.send(Order::Foreground(foreground));
    }
}

fn watch(orders: &Receiver<Order>, reports: &async_channel::Sender<LipReport>) {
    let mut when = WhenToWatch::default();
    let mut game = None;
    // The game's window, found anew with each order and each settle: whether it's in front, and
    // whether it shows the bar.
    let mut window: Option<HWND> = None;
    let mut duplication: Option<Duplication> = None;
    // What was reported last: `None` for `Idle`, and before the first report.
    let mut reported: Option<(RailsSeen, Option<f64>)> = None;
    // The bar's fill as last read, while watching.
    let mut fill: Option<f64> = None;
    let mut last_error = String::new();
    // The first duplication of the run is logged, as a sign in the diagnostics report that the
    // fast look works; each later one, on every return to the game, only at debug.
    let mut announced = false;
    loop {
        let now = Instant::now();
        if when.settle_due(now) {
            window = game_window::game_window();
            when.asked(window.is_some_and(game_window::in_front), now);
        }
        let order = if when.watching(now) {
            match orders.try_recv() {
                Ok(order) => Some(order),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        } else {
            // Nothing to look at: the duplication and its device go, and the thread sleeps till
            // an order comes, or the watching may start by itself.
            if duplication.take().is_some() {
                log::debug!("lip watch: the duplication let go");
            }
            fill = None;
            if reported.take().is_some() {
                let _ = reports.try_send(LipReport::Idle);
            }
            let received = match when.idle_wait(now) {
                Some(wait) => orders.recv_timeout(wait),
                None => orders.recv().map_err(RecvTimeoutError::from),
            };
            match received {
                Ok(order) => Some(order),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        };
        if let Some(order) = order {
            let now = Instant::now();
            window = game_window::game_window();
            match order {
                Order::Game(target) => {
                    game = target;
                    when.given(game.is_some());
                    when.asked(window.is_some_and(game_window::in_front), now);
                }
                // The report names the new foreground window, which `GetForegroundWindow` may not
                // yet while it's delivered: the settle asks that.
                Order::Foreground(foreground) => {
                    let front = foreground == Foreground::Game
                        && window.is_some_and(|window| !unsafe { IsIconic(window) }.as_bool());
                    when.reported(front, now);
                }
            }
            continue;
        }
        let Some(client) = game else {
            continue;
        };
        let lips = {
            let rails = hud_rails(client);
            [rails.flask, rails.skill].map(|plate| rail_lip(plate, client.height))
        };
        let bar = XpBarGeometry::for_client(client);
        let rects = || lips.iter().chain(bar.as_ref().map(|bar| &bar.capture));
        if duplication.as_ref().is_none_or(|open| !open.shows(rects())) {
            duplication = None;
            // A game across two monitors: what's read must all be on the one duplicated, else
            // the next round would open it again, and again.
            let opened = Duplication::open(client).and_then(|opened| {
                if !opened.shows(rects()) {
                    bail!("the rails and the bar aren't all on the game's monitor");
                }
                Ok(opened)
            });
            match opened {
                Ok(opened) => {
                    let monitor = opened.monitor;
                    if announced {
                        log::debug!("lip watch: duplicating the monitor at {monitor:?}");
                    } else {
                        log::info!("lip watch: watching the rails frame by frame on {monitor:?}");
                        announced = true;
                    }
                    last_error.clear();
                    duplication = Some(opened);
                }
                Err(err) => {
                    let error = format!("{err:#}");
                    if error != last_error {
                        log::warn!("lip watch: {error}");
                        last_error = error;
                    }
                    when.failed(Instant::now());
                    continue;
                }
            }
        }
        let Some(open) = duplication.as_mut() else {
            continue;
        };
        match open.next_look(&lips, bar.as_ref(), window) {
            Ok(Some(look)) => {
                if let Some(read) = look.fill {
                    fill = read;
                }
                if reported != Some((look.rails, fill)) {
                    reported = Some((look.rails, fill));
                    let _ = reports.try_send(LipReport::Seen {
                        rails: look.rails,
                        fill,
                    });
                }
            }
            Ok(None) => {}
            // Lost to a mode change, the secure desktop or another program's fullscreen: open
            // it again on the next round.
            Err(err) => {
                log::debug!("lip watch: {err:#}");
                duplication = None;
            }
        }
    }
}

/// The duplication of the monitor the game is on, and what reading its lips and bar takes: a
/// Direct3D device of its own, let go with it.
struct Duplication {
    /// The monitor's area of the desktop, physical pixels.
    monitor: PhysicalRect,
    duplication: IDXGIOutputDuplication,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    /// The texture the lips and the bar are copied into to be read -- the flask lip's rows, the
    /// skill lip's under them, the bar's under those -- and its size.
    staging: Option<(ID3D11Texture2D, u32, u32)>,
    /// When the last frame was taken: looks are paced from it.
    last_look: Option<Instant>,
    /// When the bar was last read.
    bar_read: Option<Instant>,
    /// One lip's or the bar's rows, reused from look to look.
    rows: Vec<u8>,
}

/// What one look read.
struct Look {
    rails: RailsSeen,
    /// The bar's fill if the look read the bar: `Some(None)` when it was covered or unreadable.
    fill: Option<Option<f64>>,
}

impl Duplication {
    /// Duplicates the monitor under the middle of the game's client area.
    fn open(client: PhysicalRect) -> Result<Duplication> {
        let (x, y) = (client.x + client.width / 2, client.y + client.height / 2);
        let factory: IDXGIFactory1 =
            unsafe { CreateDXGIFactory1() }.context("CreateDXGIFactory1")?;
        let mut adapter_index = 0;
        while let Ok(adapter) = unsafe { factory.EnumAdapters1(adapter_index) } {
            adapter_index += 1;
            let mut output_index = 0;
            while let Ok(output) = unsafe { adapter.EnumOutputs(output_index) } {
                output_index += 1;
                let desc = unsafe { output.GetDesc() }.context("IDXGIOutput::GetDesc")?;
                let area = desc.DesktopCoordinates;
                let monitor = PhysicalRect {
                    x: area.left,
                    y: area.top,
                    width: area.right - area.left,
                    height: area.bottom - area.top,
                };
                let under = x >= monitor.x
                    && x < monitor.x + monitor.width
                    && y >= monitor.y
                    && y < monitor.y + monitor.height;
                if !desc.AttachedToDesktop.as_bool() || !under {
                    continue;
                }
                if ![DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_UNSPECIFIED]
                    .contains(&desc.Rotation)
                {
                    bail!("the game's monitor is rotated");
                }
                let mut device = None;
                let mut context = None;
                unsafe {
                    D3D11CreateDevice(
                        &adapter,
                        D3D_DRIVER_TYPE_UNKNOWN,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_FLAG(0),
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut device),
                        None,
                        Some(&mut context),
                    )
                }
                .context("D3D11CreateDevice")?;
                let (Some(device), Some(context)) = (device, context) else {
                    bail!("D3D11CreateDevice returned no device");
                };
                // 8-bit BGRA whatever the desktop is, HDR included; outputs before Windows 10
                // 1703 duplicate the legacy way.
                let duplication = match output.cast::<IDXGIOutput5>() {
                    Ok(output) => unsafe {
                        output.DuplicateOutput1(&device, 0, &[DXGI_FORMAT_B8G8R8A8_UNORM])
                    },
                    Err(_) => unsafe { output.cast::<IDXGIOutput1>()?.DuplicateOutput(&device) },
                }
                .context("duplicating the game's monitor")?;
                return Ok(Duplication {
                    monitor,
                    duplication,
                    context,
                    device,
                    staging: None,
                    last_look: None,
                    bar_read: None,
                    rows: Vec::new(),
                });
            }
        }
        bail!("no monitor shows the game")
    }

    /// Whether the monitor shows every one of `rects` whole.
    fn shows<'a>(&self, mut rects: impl Iterator<Item = &'a PhysicalRect>) -> bool {
        let m = self.monitor;
        rects.all(|rect| {
            rect.x >= m.x
                && rect.y >= m.y
                && rect.x + rect.width <= m.x + m.width
                && rect.y + rect.height <= m.y + m.height
        })
    }

    /// Waits for the next frame -- no sooner than the next look is due (`wait_for_look`) -- and
    /// reads `lips` off it, and `bar` if it's due (`BAR_EVERY`) and the game itself shows it on
    /// `game`'s window: `None` if no frame came in time, or only the pointer moved.
    fn next_look(
        &mut self,
        lips: &[PhysicalRect; 2],
        bar: Option<&XpBarGeometry>,
        game: Option<HWND>,
    ) -> Result<Option<Look>> {
        if let Some(last) = self.last_look {
            wait_for_look(last);
        }
        let bar_due = self.bar_read.is_none_or(|at| at.elapsed() >= BAR_EVERY);
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource = None;
        match unsafe {
            self.duplication
                .AcquireNextFrame(FRAME_WAIT_MS, &mut info, &mut resource)
        } {
            Err(err) if err.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            other => other.context("AcquireNextFrame")?,
        }
        self.last_look = Some(Instant::now());
        // A present time of zero: only the pointer moved.
        let copied = if info.LastPresentTime == 0 {
            Ok(false)
        } else {
            self.copy_rows(resource, lips, bar.map(|bar| &bar.capture), bar_due)
        };
        // Windows holds the next frame back until this one is released. The copies are queued
        // on the GPU before the release, so they read this frame.
        unsafe { self.duplication.ReleaseFrame() }.context("ReleaseFrame")?;
        if !copied? {
            return Ok(None);
        }
        // The bar counts only where the game itself shows it: a window over it -- the price
        // panel spans its middle -- would read as a wrong fill (see `xp_bar::shows_the_game`).
        let bar = bar.filter(|_| bar_due).map(|geometry| {
            game.is_some_and(|game| shows_the_game(game, geometry.capture))
                .then_some(geometry)
        });
        if bar.is_some() {
            self.bar_read = self.last_look;
        }
        self.read_rows(lips, bar).map(Some)
    }

    /// Queues the copy of `lips`, and of `bar` if `copy_bar`, out of the frame into the staging
    /// texture -- which has room for the bar either way; whether it did.
    fn copy_rows(
        &mut self,
        resource: Option<IDXGIResource>,
        lips: &[PhysicalRect; 2],
        bar: Option<&PhysicalRect>,
        copy_bar: bool,
    ) -> Result<bool> {
        let Some(resource) = resource else {
            return Ok(false);
        };
        let frame: ID3D11Texture2D = resource.cast().context("the frame as a texture")?;
        let rects = || lips.iter().chain(bar);
        let width = rects().map(|rect| rect.width).max().unwrap_or(0) as u32;
        let height = rects().map(|rect| rect.height).sum::<i32>() as u32;
        if self
            .staging
            .as_ref()
            .is_none_or(|&(_, w, h)| (w, h) != (width, height))
        {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut texture = None;
            unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut texture)) }
                .context("CreateTexture2D")?;
            let Some(texture) = texture else {
                bail!("CreateTexture2D returned no texture");
            };
            self.staging = Some((texture, width, height));
        }
        let Some((staging, _, _)) = self.staging.as_ref() else {
            return Ok(false);
        };
        let mut top = 0;
        for (index, rect) in rects().enumerate() {
            // The lips first, then the bar.
            if index < lips.len() || copy_bar {
                let x = (rect.x - self.monitor.x) as u32;
                let y = (rect.y - self.monitor.y) as u32;
                let area = D3D11_BOX {
                    left: x,
                    top: y,
                    front: 0,
                    right: x + rect.width as u32,
                    bottom: y + rect.height as u32,
                    back: 1,
                };
                unsafe {
                    self.context.CopySubresourceRegion(
                        staging,
                        0,
                        0,
                        top,
                        0,
                        &frame,
                        0,
                        Some(&area),
                    );
                }
            }
            top += rect.height as u32;
        }
        Ok(true)
    }

    /// Reads the copied lips back, once the GPU has copied them, and the bar if `bar` says it
    /// was copied (`Some`) and shows the game (`Some(Some)`).
    fn read_rows(
        &mut self,
        lips: &[PhysicalRect; 2],
        bar: Option<Option<&XpBarGeometry>>,
    ) -> Result<Look> {
        let Some((staging, _, _)) = self.staging.as_ref() else {
            bail!("no staging texture");
        };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .context("Map")?;
        let data = mapped.pData.cast::<u8>().cast_const();
        let pitch = mapped.RowPitch as usize;
        let mut seen = [false; 2];
        let mut top = 0;
        for (lip, seen) in lips.iter().zip(&mut seen) {
            let (width, height) = (lip.width as usize, lip.height as usize);
            // SAFETY: the mapped texture is `pitch` bytes a row, as wide as the widest of the
            // lips and the bar, and holds all their rows.
            unsafe { gather(&mut self.rows, data, pitch, top, width, height) };
            *seen = rail_seen(&self.rows, width);
            top += height;
        }
        let fill = bar.map(|shown| {
            shown.and_then(|geometry| {
                let capture = geometry.capture;
                // SAFETY: as for the lips; the bar's rows are the last ones.
                unsafe {
                    gather(
                        &mut self.rows,
                        data,
                        pitch,
                        top,
                        capture.width as usize,
                        capture.height as usize,
                    );
                }
                read_fill(geometry, &self.rows)
            })
        });
        unsafe { self.context.Unmap(staging, 0) };
        Ok(Look {
            rails: RailsSeen {
                flask: seen[0],
                skill: seen[1],
            },
            fill,
        })
    }
}

impl Drop for Duplication {
    /// Direct3D destroys what's let go only once nothing on its context holds it: cleared and
    /// flushed first, the device goes with the last reference -- its video memory, the frame's,
    /// and the graphics driver's threads that served it -- not some time later.
    fn drop(&mut self) {
        unsafe {
            self.context.ClearState();
            self.context.Flush();
        }
    }
}

/// Puts `height` rows of `width` pixels, from row `top` down, of a mapped texture's `data` into
/// `rows`, tightly packed.
///
/// # Safety
///
/// `data` points at a mapped texture of at least `top + height` rows, `pitch` bytes each, each at
/// least `width` 4-byte pixels wide.
unsafe fn gather(
    rows: &mut Vec<u8>,
    data: *const u8,
    pitch: usize,
    top: usize,
    width: usize,
    height: usize,
) {
    rows.clear();
    for row in top..top + height {
        rows.extend_from_slice(unsafe {
            std::slice::from_raw_parts(data.add(row * pitch), width * 4)
        });
    }
}

/// Waits until the next look after the one at `last` is due (`lip_schedule::look_wait`): soon
/// while the player moves the mouse or presses keys, anywhere in the session -- a tooltip comes
/// or goes only then -- and later once they've kept still, unless they move meanwhile.
fn wait_for_look(last: Instant) {
    loop {
        let wait = look_wait(last.elapsed(), still_for());
        if wait.is_zero() {
            return;
        }
        std::thread::sleep(wait);
    }
}

/// How long the player has kept still: no mouse or key input anywhere in the session. Zero if
/// Windows can't say, as if they had just moved.
fn still_for() -> Duration {
    let mut last = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if !unsafe { GetLastInputInfo(&mut last) }.as_bool() {
        return Duration::ZERO;
    }
    // Both on the 32-bit millisecond tick, which wraps every 49.7 days.
    let still = unsafe { GetTickCount() }.wrapping_sub(last.dwTime);
    Duration::from_millis(u64::from(still))
}
