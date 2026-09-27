//! Watches the lips of the HUD's rails for the XP overlay frame by frame, so that a plate steps
//! aside the moment the game draws anything over its rail -- a tooltip, a panel, the chat -- and
//! comes back the moment it's gone; and reads the experience bar off the same frames.
//!
//! `platform::xp_bar` looks at the lips every two seconds with a GDI blit of the screen, and a
//! blit waits for the desktop's next composition: 11 ms a lip, 27 ms both, up to 145 ms (measured
//! 2026-09-24 on the test machine's 4K game) -- far too slow to look many times a second. Here the
//! desktop comes through DXGI desktop duplication instead: Windows hands over each frame it
//! composes as a texture, the lips' few rows are copied out of it on the GPU, and only those rows
//! are read back -- the bar's too, every `lip_schedule::BAR_EVERY`, so that the sampler needs no
//! blit at all while this watches. The watching runs on a thread of its own while the game is in
//! front and not minimised, and a moment after (`platform::lip_schedule`, which paces the looks
//! too). The thread learns of the foreground from Windows' reports, handed on by the overlay
//! ([`LipWatch::foreground`]), and asks nothing while it doesn't watch: it sleeps till told
//! otherwise. Once it stops, the duplication and its Direct3D device are let go -- their video
//! memory with them -- and [`LipReport::Idle`] leaves the plates and the bar to the sampler's
//! slower look.
//!
//! A look waits for nothing but a new duplication's first frame. Measured 2026-09-27 on the test
//! machine (the build of 93296e4, the game in front, idle), this thread took 1.45 ms of CPU a
//! look at four looks a second, and four context switches a look: three steps of the input poll,
//! and one wait in the look itself -- `Map` of the staging texture right after the copy was
//! queued, which waited inside the graphics driver, on this thread, for the GPU to get to the copy
//! behind the game's own work. So a look takes the latest frame without waiting for one
//! (`AcquireNextFrame` with no timeout: none new since the last look, or one where only the
//! pointer moved, and nothing on screen changed), queues the copy of the rows into one of
//! [`STAGING`] staging textures, sends it to the GPU and lets the frame go at once; the copy is
//! read back `lip_schedule::READ_AFTER` later with `D3D11_MAP_FLAG_DO_NOT_WAIT`, and tried again
//! later while the GPU hasn't made it. Held till the next look instead, the frame would spare
//! Windows copying each composed frame into it, but that look would first wait for a composition:
//! Lightpack found half its looks lost so (psieg/Lightpack#373).
//!
//! Between looks the thread sleeps on a high-resolution timer, on its orders, and -- while the
//! player keeps still (`lip_schedule::still`) -- on their input: raw input from the mouse and the
//! keyboard, sent to a message-only window of the thread's own (`RIDEV_INPUTSINK`) only while it
//! waits for it, and asked for no more at the first report, so that a moving mouse's hundreds of
//! reports a second never reach it. It adds nothing to the game's input: Windows posts raw input
//! to each program that asked for it, the game's own untouched -- unlike a low-level mouse hook,
//! which every mouse report of the session would go through first. Windows keeps one raw input
//! target per program and kind of device, and nothing else in the app asks for one (GPUI at
//! b54cc1d doesn't). Where the window can't be made, or raw input doesn't come for input Windows
//! saw (`lip_schedule::input_missed` -- a game run as administrator may send none to a program
//! that isn't), a still wait asks Windows when the last input was every
//! `lip_schedule::INPUT_POLL` instead.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HANDLE, HMODULE, HWND, LPARAM, LRESULT,
    WAIT_FAILED, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_FLAG_DO_NOT_WAIT,
    D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_UNSPECIFIED,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_ERROR_WAS_STILL_DRAWING,
    DXGI_OUTDUPL_FRAME_INFO, IDXGIFactory1, IDXGIOutput1, IDXGIOutput5, IDXGIOutputDuplication,
    IDXGIResource,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateEventW, CreateWaitableTimerExW, INFINITE,
    SetEvent, SetWaitableTimer, TIMER_ALL_ACCESS,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::Input::{
    RAWINPUTDEVICE, RAWINPUTDEVICE_FLAGS, RIDEV_INPUTSINK, RIDEV_REMOVE, RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetQueueStatus, HWND_MESSAGE,
    IsIconic, MSG, MWMO_INPUTAVAILABLE, MWMO_NONE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, QS_RAWINPUT, QUEUE_STATUS_FLAGS, RegisterClassExW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WNDCLASSEXW,
};
use windows::core::{Interface, PCWSTR, w};

use crate::overlay_layout::{PhysicalRect, hud_rails, rail_lip, rail_seen};
use crate::platform::game_window::{self, Foreground};
use crate::platform::lip_schedule::{self, WhenToWatch};
use crate::platform::xp_bar::{RailsSeen, shows_the_game};
use crate::xp_tracker::{XpBarGeometry, read_fill};

/// How many looks' copies may wait for the GPU at once: a look that finds as many waiting is let
/// go, and the next comes once one is read.
const STAGING: usize = 3;

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
    /// Its orders; taken as the handle is dropped, which ends the thread.
    orders: Option<Sender<Order>>,
    /// The event that wakes the thread for an order.
    wake: Arc<Handle>,
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
        let event = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
            .context("the lip watcher's event")?;
        let wake = Arc::new(Handle(event));
        let (orders, received) = mpsc::channel();
        let (reports, reported) = async_channel::unbounded();
        let woken = Arc::clone(&wake);
        std::thread::Builder::new()
            .name("lip-watch".into())
            .spawn(move || watch(&received, woken, &reports))
            .context("starting the lip watcher")?;
        let orders = Some(orders);
        Ok((LipWatch { orders, wake }, reported))
    }

    /// Watches the rails of a game whose client area is `game`, or nothing.
    pub fn watch(&self, game: Option<PhysicalRect>) {
        self.send(Order::Game(game));
    }

    /// Takes Windows' report of a new foreground window (`game_window::watch_foreground`): the
    /// watching follows the game to the front and away from it.
    pub fn foreground(&self, foreground: Foreground) {
        self.send(Order::Foreground(foreground));
    }

    /// Hands `order` to the thread, and wakes it.
    fn send(&self, order: Order) {
        if let Some(orders) = &self.orders
            && orders.send(order).is_ok()
        {
            let _ = unsafe { SetEvent(self.wake.0) };
        }
    }
}

impl Drop for LipWatch {
    /// The orders end, and the thread is woken to find so.
    fn drop(&mut self) {
        self.orders = None;
        let _ = unsafe { SetEvent(self.wake.0) };
    }
}

fn watch(orders: &Receiver<Order>, wake: Arc<Handle>, reports: &async_channel::Sender<LipReport>) {
    let mut waiter = Waiter::new(wake);
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
        loop {
            let order = match orders.try_recv() {
                Ok(order) => order,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            };
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
        }
        let now = Instant::now();
        if when.settle_due(now) {
            window = game_window::game_window();
            when.asked(window.is_some_and(game_window::in_front), now);
        }
        let Some(client) = game.filter(|_| when.watching(now)) else {
            // Nothing to look at: the duplication and its device go, and the thread sleeps till
            // an order comes, or the watching may start by itself.
            if duplication.take().is_some() {
                log::debug!("lip watch: the duplication let go");
            }
            fill = None;
            if reported.take().is_some() {
                let _ = reports.try_send(LipReport::Idle);
            }
            waiter.idle(when.idle_wait(now).map(|wait| now + wait));
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
        let now = Instant::now();
        match open.collect(now) {
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
            // The device lost to the driver: opened anew on the next round.
            Err(err) => {
                log::debug!("lip watch: {err:#}");
                duplication = None;
                continue;
            }
        }
        let last_input = waiter.input.last(now);
        // Whether the latest look read showed every rail: nothing over them to go by itself.
        let clear = reported.is_some_and(|(rails, _)| rails.flask && rails.skill);
        if open
            .next_look(now, last_input, clear)
            .is_some_and(|due| due <= now)
            && let Err(err) = open.look(&lips, bar.as_ref(), window, now)
        {
            // Lost to a mode change, the secure desktop or another program's fullscreen: opened
            // anew on the next round.
            log::debug!("lip watch: {err:#}");
            duplication = None;
            continue;
        }
        let due = [
            open.next_look(now, last_input, clear),
            open.read_at,
            when.next_change(now),
        ]
        .into_iter()
        .flatten()
        .min();
        waiter.watching(due, last_input);
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
    /// The textures the looks are copied into to be read -- the flask lip's rows, the skill lip's
    /// under them, the bar's under those -- each with the look it holds until it's read.
    staging: [Staging; STAGING],
    /// Their size, which has room for the bar whether it's copied or not.
    size: (u32, u32),
    /// How many looks were copied: each copy's number, in the order they're read back.
    copies: u64,
    /// When the last look was: the next is paced from it.
    last_look: Option<Instant>,
    /// Whether a look took a frame yet.
    framed: bool,
    /// When a copy is next tried to be read back, while one waits for it.
    read_at: Option<Instant>,
    /// The tries in a row that found the GPU not done with the oldest copy.
    misses: u32,
    /// When the bar was last copied.
    bar_read: Option<Instant>,
    /// One lip's or the bar's rows, reused from look to look.
    rows: Vec<u8>,
}

/// A staging texture, made at its first use.
#[derive(Default)]
struct Staging {
    texture: Option<ID3D11Texture2D>,
    /// The look copied into it, till it's read back.
    copied: Option<Copied>,
}

/// A look's copy, waiting to be read back.
struct Copied {
    /// Its place in the order the copies were made.
    number: u64,
    lips: [PhysicalRect; 2],
    /// The bar if it was due a reading: copied if the game itself showed it, else `None`, which
    /// reads as covered.
    bar: Option<Option<XpBarGeometry>>,
}

/// What one look read.
#[derive(Clone, Copy)]
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
                    staging: Default::default(),
                    size: (0, 0),
                    copies: 0,
                    last_look: None,
                    framed: false,
                    read_at: None,
                    misses: 0,
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

    /// A staging texture free for a look's copy, if any.
    fn room(&self) -> Option<usize> {
        self.staging
            .iter()
            .position(|staging| staging.copied.is_none())
    }

    /// The staging texture of the oldest copy waiting to be read back, if any.
    fn oldest(&self) -> Option<usize> {
        (0..STAGING)
            .filter_map(|slot| Some((self.staging[slot].copied.as_ref()?.number, slot)))
            .min()
            .map(|(_, slot)| slot)
    }

    /// When the next look is due (`lip_schedule::next_look`), the player's last input at
    /// `last_input` and `clear` whether the latest look read showed every rail: at `now` for the
    /// first; `None` while every staging texture holds a copy -- a read-back frees one.
    fn next_look(&self, now: Instant, last_input: Instant, clear: bool) -> Option<Instant> {
        self.room()?;
        Some(
            self.last_look
                .map_or(now, |last| lip_schedule::next_look(last, last_input, clear)),
        )
    }

    /// Takes a look at `now`: queues the copy of `lips` out of the desktop's latest frame into a
    /// free staging texture, and of `bar` if it's due a reading and the game itself shows it on
    /// `game`'s window, to be read back once the GPU has made it ([`Duplication::collect`]).
    /// Nothing is copied if no frame came since the last look, or one where only the pointer
    /// moved: nothing on screen changed.
    fn look(
        &mut self,
        lips: &[PhysicalRect; 2],
        bar: Option<&XpBarGeometry>,
        game: Option<HWND>,
        now: Instant,
    ) -> Result<()> {
        let Some(slot) = self.room() else {
            return Ok(());
        };
        self.last_look = Some(now);
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource = None;
        // A new duplication's first frame is waited for; any later is taken only if it's there.
        let wait = if self.framed {
            0
        } else {
            lip_schedule::FIRST_FRAME_WAIT.as_millis() as u32
        };
        match unsafe {
            self.duplication
                .AcquireNextFrame(wait, &mut info, &mut resource)
        } {
            Err(err) if err.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(()),
            other => other.context("AcquireNextFrame")?,
        }
        self.framed = true;
        // A present time of zero: only the pointer moved.
        let copied = match resource {
            Some(frame) if info.LastPresentTime != 0 => {
                self.copy(slot, &frame, lips, bar, game, now).map(Some)
            }
            _ => Ok(None),
        };
        // Windows holds the next frame back until this one is released. The copies are queued
        // on the GPU before the release, so they read this frame.
        unsafe { self.duplication.ReleaseFrame() }.context("ReleaseFrame")?;
        if let Some(copied) = copied? {
            // On the GPU's way now, not with whatever this device is asked next.
            unsafe { self.context.Flush() };
            self.staging[slot].copied = Some(copied);
            self.read_at = self.read_at.or(Some(now + lip_schedule::READ_AFTER));
        }
        Ok(())
    }

    /// Queues the copy of `lips`, and of `bar` if it's due a reading and shown, out of `frame`
    /// into staging texture `slot`: what's copied.
    fn copy(
        &mut self,
        slot: usize,
        frame: &IDXGIResource,
        lips: &[PhysicalRect; 2],
        bar: Option<&XpBarGeometry>,
        game: Option<HWND>,
        now: Instant,
    ) -> Result<Copied> {
        let frame: ID3D11Texture2D = frame.cast().context("the frame as a texture")?;
        let rects = || lips.iter().chain(bar.map(|bar| &bar.capture));
        let size = (
            rects().map(|rect| rect.width).max().unwrap_or(0) as u32,
            rects().map(|rect| rect.height).sum::<i32>() as u32,
        );
        if self.size != size {
            // The game was resized: what the textures hold is of the old size.
            self.staging = Default::default();
            self.size = size;
        }
        let texture = match &self.staging[slot].texture {
            Some(texture) => texture.clone(),
            None => {
                let texture = staging_texture(&self.device, size)?;
                self.staging[slot].texture = Some(texture.clone());
                texture
            }
        };
        // The bar counts only where the game itself shows it: a window over it -- the price
        // panel spans its middle -- would read as a wrong fill (see `xp_bar::shows_the_game`).
        let bar = bar
            .filter(|_| lip_schedule::bar_due(self.bar_read, now))
            .map(|geometry| {
                game.is_some_and(|game| shows_the_game(game, geometry.capture))
                    .then(|| geometry.clone())
            });
        if bar.is_some() {
            self.bar_read = Some(now);
        }
        let shown = bar.as_ref().and_then(Option::as_ref);
        // The lips first, then the bar.
        let mut top = 0;
        for rect in lips.iter().chain(shown.map(|bar| &bar.capture)) {
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
                self.context
                    .CopySubresourceRegion(&texture, 0, 0, top, 0, &frame, 0, Some(&area));
            }
            top += rect.height as u32;
        }
        self.copies += 1;
        Ok(Copied {
            number: self.copies,
            lips: *lips,
            bar,
        })
    }

    /// Reads back the copies the GPU has made by `now`, oldest first, if a read-back is due: the
    /// latest look read -- its rails -- with the bar's latest reading among them; `None` if none
    /// was. A copy the GPU hasn't made yet is tried again later (`lip_schedule::read_retry`):
    /// nothing here waits for it.
    fn collect(&mut self, now: Instant) -> Result<Option<Look>> {
        if self.read_at.is_none_or(|at| now < at) {
            return Ok(None);
        }
        let mut latest: Option<Look> = None;
        while let Some(slot) = self.oldest() {
            let Some(texture) = self.staging[slot].texture.clone() else {
                self.staging[slot].copied = None;
                continue;
            };
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            match unsafe {
                self.context.Map(
                    &texture,
                    0,
                    D3D11_MAP_READ,
                    D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                    Some(&mut mapped),
                )
            } {
                Err(err) if err.code() == DXGI_ERROR_WAS_STILL_DRAWING => break,
                other => other.context("Map")?,
            }
            self.misses = 0;
            let look = self.staging[slot]
                .copied
                .take()
                .map(|copied| read_look(&mut self.rows, &mapped, &copied));
            unsafe { self.context.Unmap(&texture, 0) };
            if let Some(look) = look {
                let fill = look.fill.or(latest.and_then(|latest| latest.fill));
                latest = Some(Look { fill, ..look });
            }
        }
        self.read_at = match self.oldest() {
            Some(_) => {
                self.misses += 1;
                Some(now + lip_schedule::read_retry(self.misses))
            }
            None => {
                self.misses = 0;
                None
            }
        };
        Ok(latest)
    }
}

impl Drop for Duplication {
    /// Direct3D destroys what's let go only once nothing on its context holds it: cleared and
    /// flushed first, the device goes with the last reference -- its video memory, the frame's,
    /// and whatever the graphics driver keeps for it -- not some time later.
    fn drop(&mut self) {
        unsafe {
            self.context.ClearState();
            self.context.Flush();
        }
    }
}

/// A staging texture of `size`, which the CPU reads a look's rows from.
fn staging_texture(device: &ID3D11Device, (width, height): (u32, u32)) -> Result<ID3D11Texture2D> {
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
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }
        .context("CreateTexture2D")?;
    texture.context("CreateTexture2D returned no texture")
}

/// What the look `copied` shows, from its staging texture, `mapped`; `rows` is room to gather a
/// lip's or the bar's rows in.
fn read_look(rows: &mut Vec<u8>, mapped: &D3D11_MAPPED_SUBRESOURCE, copied: &Copied) -> Look {
    let data = mapped.pData.cast::<u8>().cast_const();
    let pitch = mapped.RowPitch as usize;
    let mut seen = [false; 2];
    let mut top = 0;
    for (lip, seen) in copied.lips.iter().zip(&mut seen) {
        let (width, height) = (lip.width as usize, lip.height as usize);
        // SAFETY: the mapped texture is `pitch` bytes a row, as wide as the widest of the lips
        // and the bar it was made for, and holds all their rows (`Duplication::copy`).
        unsafe { gather(rows, data, pitch, top, width, height) };
        *seen = rail_seen(rows, width);
        top += height;
    }
    let fill = copied.bar.as_ref().map(|shown| {
        shown.as_ref().and_then(|geometry| {
            let capture = geometry.capture;
            // SAFETY: as for the lips; the bar's rows are the last ones.
            unsafe {
                gather(
                    rows,
                    data,
                    pitch,
                    top,
                    capture.width as usize,
                    capture.height as usize,
                );
            }
            read_fill(geometry, rows)
        })
    });
    Look {
        rails: RailsSeen {
            flask: seen[0],
            skill: seen[1],
        },
        fill,
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

/// A handle of a kernel object -- an event, a timer -- closed when dropped.
struct Handle(HANDLE);

// SAFETY: a kernel object's handle stands for the object in the whole process: any thread may wait
// on it, set it, or close it.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// What ended a wait of the watching thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Woken {
    /// An order came, or the handle was dropped.
    Orders,
    /// Its time came.
    Due,
    /// The player moved the mouse or pressed a key.
    Input,
}

/// What the watching thread sleeps on: its orders' event, a timer, and the player's input.
struct Waiter {
    orders: Arc<Handle>,
    /// A high-resolution waitable timer -- a wait's own timeout keeps to the system's ticks, 15.6
    /// ms apart unless a program asks for finer -- or `None` if none could be made.
    timer: Option<Handle>,
    input: InputWake,
}

impl Waiter {
    /// On the watching thread, whose waits its input window's raw input ends.
    fn new(orders: Arc<Handle>) -> Waiter {
        // High resolution from Windows 10 1803; before, a timer on the system's ticks.
        let timer = [CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, 0]
            .into_iter()
            .find_map(|flags| {
                unsafe { CreateWaitableTimerExW(None, PCWSTR::null(), flags, TIMER_ALL_ACCESS.0) }
                    .ok()
            })
            .map(Handle);
        Waiter {
            orders,
            timer,
            input: InputWake::new(),
        }
    }

    /// Sleeps, not watching, till `until` or an order: no input is waited for.
    fn idle(&mut self, until: Option<Instant>) {
        self.input.reset();
        self.wait(until, false);
    }

    /// Sleeps, watching, till `due` or an order -- and while the player keeps still, their last
    /// input at `last_input`, till they move (`lip_schedule::wait_until`).
    fn watching(&mut self, due: Option<Instant>, last_input: Instant) {
        let now = Instant::now();
        let input_wakes = if lip_schedule::still(now, last_input) {
            self.input.arm(now, last_input)
        } else {
            self.input.disarm();
            false
        };
        let until = lip_schedule::wait_until(due, now, last_input, input_wakes);
        if self.wait(until, input_wakes) == Woken::Input {
            self.input.woke(Instant::now());
        }
    }

    /// Sleeps till `until` (`None`: no end), an order, or -- `on_input` -- the player's input.
    fn wait(&self, until: Option<Instant>, on_input: bool) -> Woken {
        let mut handles = [self.orders.0, HANDLE::default()];
        let mut count = 1;
        let mut timeout = INFINITE;
        if let Some(until) = until {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Woken::Due;
            }
            // Relative, in 100 ns units.
            let due = -i64::try_from(left.as_nanos().div_ceil(100)).unwrap_or(i64::MAX);
            match &self.timer {
                Some(timer)
                    if unsafe { SetWaitableTimer(timer.0, &due, 0, None, None, false) }.is_ok() =>
                {
                    handles[1] = timer.0;
                    count = 2;
                }
                _ => {
                    timeout = u32::try_from(left.as_micros().div_ceil(1000)).unwrap_or(INFINITE - 1)
                }
            }
        }
        let (mask, flags) = if on_input {
            (QS_RAWINPUT, MWMO_INPUTAVAILABLE)
        } else {
            (QUEUE_STATUS_FLAGS(0), MWMO_NONE)
        };
        let woken =
            unsafe { MsgWaitForMultipleObjectsEx(Some(&handles[..count]), timeout, mask, flags) };
        if woken == WAIT_OBJECT_0 {
            Woken::Orders
        } else if woken.0 == WAIT_OBJECT_0.0 + count as u32 {
            Woken::Input
        } else {
            if woken == WAIT_FAILED {
                // Never seen: a pause, so that a failing wait doesn't spin.
                log::debug!("lip watch: {}", windows::core::Error::from_thread());
                std::thread::sleep(lip_schedule::INPUT_POLL);
            }
            Woken::Due
        }
    }
}

/// Raw input from the mouse and the keyboard, which ends the watching thread's wait while the
/// player keeps still.
struct InputWake {
    /// The message-only window it's sent to, on the watching thread; `None` if none could be
    /// made, or raw input couldn't be asked for: a still wait asks Windows every `INPUT_POLL`.
    window: Option<HWND>,
    /// Since when raw input is asked for.
    armed: Option<Instant>,
    /// Raw input didn't come for input Windows saw: till the watching stops, a still wait asks
    /// Windows every `INPUT_POLL`.
    blind: bool,
    /// Whether that was logged.
    told_blind: bool,
    /// When input last ended a wait.
    woke: Option<Instant>,
}

impl InputWake {
    fn new() -> InputWake {
        let window = input_window()
            .inspect_err(|err| {
                log::warn!(
                    "lip watch: {err:#}; a still player is asked after every {} ms instead",
                    lip_schedule::INPUT_POLL.as_millis()
                );
            })
            .ok();
        InputWake {
            window,
            armed: None,
            blind: false,
            told_blind: false,
            woke: None,
        }
    }

    /// When the player's last input was, at `now`: the later of what Windows says
    /// (`GetLastInputInfo`) and the last input that woke the thread; `now` if Windows can't say,
    /// as if they had just moved.
    fn last(&self, now: Instant) -> Instant {
        let told = told_last_input(now).unwrap_or(now);
        self.woke.map_or(told, |woke| woke.max(told))
    }

    /// Asks for raw input to end the thread's waits, unless it already did; whether it will.
    fn arm(&mut self, now: Instant, last_input: Instant) -> bool {
        let Some(window) = self.window.filter(|_| !self.blind) else {
            return false;
        };
        if let Some(armed) = self.armed {
            if !lip_schedule::input_missed(armed, last_input, now) || raw_input_queued() {
                return true;
            }
            if !self.told_blind {
                log::info!(
                    "lip watch: input doesn't wake the watcher (is the game run as \
                     administrator?); a still player is asked after every {} ms instead",
                    lip_schedule::INPUT_POLL.as_millis()
                );
                self.told_blind = true;
            }
            self.disarm();
            self.blind = true;
            return false;
        }
        match register(Some(window), RIDEV_INPUTSINK) {
            Ok(()) => {
                self.armed = Some(now);
                true
            }
            Err(err) => {
                log::warn!(
                    "lip watch: raw input: {err}; a still player is asked after every {} ms \
                     instead",
                    lip_schedule::INPUT_POLL.as_millis()
                );
                let _ = unsafe { DestroyWindow(window) };
                self.window = None;
                false
            }
        }
    }

    /// Asks for no more raw input, and takes what came of it.
    fn disarm(&mut self) {
        if self.armed.take().is_some() {
            if let Err(err) = register(None, RIDEV_REMOVE) {
                log::debug!("lip watch: raw input off: {err}");
            }
            pump();
        }
    }

    /// Input ended a wait at `at`: the player moves, and the looks follow without it.
    fn woke(&mut self, at: Instant) {
        self.woke = Some(at);
        self.disarm();
    }

    /// The watching stopped: next time, raw input is tried again -- a game run anew may send it.
    fn reset(&mut self) {
        self.disarm();
        self.blind = false;
    }
}

impl Drop for InputWake {
    fn drop(&mut self) {
        self.disarm();
        if let Some(window) = self.window {
            let _ = unsafe { DestroyWindow(window) };
        }
    }
}

/// Makes the message-only window raw input is sent to, on this thread: its waits end on that.
fn input_window() -> Result<HWND> {
    const CLASS: PCWSTR = w!("PoE2OracleLipWatchInput");
    let module = unsafe { GetModuleHandleW(PCWSTR::null()) }.context("GetModuleHandleW")?;
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(input_proc),
        hInstance: module.into(),
        lpszClassName: CLASS,
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0
        && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
    {
        return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
    }
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS,
            PCWSTR::null(),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(module.into()),
            None,
        )
    }
    .context("CreateWindowExW")
}

/// The input window's procedure: nothing of its own.
unsafe extern "system" fn input_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: the message as this procedure got it.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// Asks for the mouse's and the keyboard's raw input to go to `window` (`RIDEV_INPUTSINK`, in the
/// background too), or no more (`RIDEV_REMOVE`, no window).
fn register(window: Option<HWND>, flags: RAWINPUTDEVICE_FLAGS) -> windows::core::Result<()> {
    // The generic desktop page's mouse and keyboard.
    let device = |usage| RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: usage,
        dwFlags: flags,
        hwndTarget: window.unwrap_or_default(),
    };
    unsafe {
        RegisterRawInputDevices(
            &[device(0x02), device(0x06)],
            size_of::<RAWINPUTDEVICE>() as u32,
        )
    }
}

/// Whether raw input waits in this thread's queue.
fn raw_input_queued() -> bool {
    // The high word: what kinds of message the queue holds.
    (unsafe { GetQueueStatus(QS_RAWINPUT) } >> 16) & QS_RAWINPUT.0 != 0
}

/// Takes the messages that came to this thread's queue -- its input window's raw input.
fn pump() {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
        unsafe { DispatchMessageW(&message) };
    }
}

/// When the player's last input was, anywhere in the session, as Windows tells it
/// (`GetLastInputInfo`) at `now`: `None` if it can't say.
fn told_last_input(now: Instant) -> Option<Instant> {
    let mut last = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if !unsafe { GetLastInputInfo(&mut last) }.as_bool() {
        return None;
    }
    // Both on the 32-bit millisecond tick, which wraps every 49.7 days.
    let still = unsafe { GetTickCount() }.wrapping_sub(last.dwTime);
    now.checked_sub(Duration::from_millis(u64::from(still)))
}
