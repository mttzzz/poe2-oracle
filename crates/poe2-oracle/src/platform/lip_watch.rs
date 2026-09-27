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
//! otherwise. Once it stops, the duplication is let go -- the video memory of its frames with it,
//! and its Direct3D device if it made one -- and [`LipReport::Idle`] leaves the plates and the bar
//! to the sampler's slower look.
//!
//! Each report wakes the UI thread, so a report goes only when a rail is seen or missed where it
//! wasn't, at once ([`LipReport::Seen`]); the bar's reading, which changes as experience comes in,
//! waits in a slot the sampler takes it from every two seconds ([`LipWatch::fill`]), and wakes
//! nothing.
//!
//! A look waits for nothing. Measured 2026-09-27 on the test machine (the build of 93296e4, the
//! game in front, idle), this thread took 1.45 ms of CPU a look at four looks a second, and four
//! context switches a look: three steps of the input poll, and one wait in the look itself -- `Map`
//! of the staging texture right after the copy was queued, which waited inside the graphics
//! driver, on this thread, for the GPU to get to the copy behind the game's own work. So a look
//! takes the latest frame without waiting for one (`AcquireNextFrame` with no timeout: none new
//! since the last look, or one where only the pointer moved, and nothing on screen changed),
//! queues the copy of the rows into one of [`STAGING`] staging textures, sends it to the GPU and
//! lets the frame go at once. Held till the next look instead, the frame would spare Windows
//! copying each composed frame into it, but that look would first wait for a composition:
//! Lightpack found half its looks lost so (psieg/Lightpack#373).
//!
//! The GPU signals once it has made a look's copy ([`Done`]) -- a fence of the device's set to the
//! look's number, or an event it sets once the device's work so far is done -- and the signal ends
//! the thread's wait: the copy is read back then, `D3D11_MAP_FLAG_DO_NOT_WAIT` and all, at one
//! wake a look. Tried on a timer instead -- `lip_schedule::READ_AFTER` after the look, then later
//! and later while the GPU hadn't made it -- a look took 3.3 wakes in play (measured 2026-09-27
//! on the test machine, the build of 786f69b): the copy waits behind the game's own work, longer
//! the busier the game keeps the GPU. The timer stays where the GPU can't signal, and behind a
//! signal that doesn't come (`lip_schedule::read_due`).
//!
//! The duplication is made on GPUI's own Direct3D device where it can be ([`Owner`]): a device of
//! the watcher's own came with 15 threads of the graphics driver, one of which woke 75-80 times a
//! second whatever the watcher did -- 20 ms of CPU a minute in play, a third of the app's CPU with
//! the game in front and the player idle (measured 2026-09-27 on the test machine, NVIDIA). GPUI
//! uses its device's immediate context on its UI thread when it likes, so the sharing rests on:
//! the device's multithread protection, turned on as GPUI's device is made
//! (`gpu_memory::note_device`), which makes each call on the context and each DXGI call on the
//! device hold the device's lock; the watcher holding that lock through each of its sequences of
//! calls -- a look, a read-back ([`Owner::lock`]) -- and never waiting under it, for a frame or
//! anything else (`lip_schedule::FIRST_FRAME_WAIT`); nothing done to the context's state, which is
//! GPUI's (no `ClearState`); and the game's monitor on GPUI's graphics card, which a duplication
//! has to be made on. Where that can't be -- another card, a device without the protection, or a
//! duplication DXGI won't make on GPUI's device (`lip_schedule::gpui_device_to_blame`), which isn't
//! asked again -- the watcher makes a device of its own, as before, and says why in the log. A
//! device lost to the driver takes the duplication with it, and the next opens on the device GPUI
//! makes anew.
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

use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use windows::Win32::Foundation::{
    CloseHandle, E_ACCESSDENIED, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HANDLE, HMODULE, HWND,
    LPARAM, LRESULT, WAIT_FAILED, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_FLAG, D3D11_FENCE_FLAG_NONE,
    D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11Device5,
    ID3D11DeviceContext, ID3D11DeviceContext4, ID3D11Fence, ID3D11Multithread, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_UNSPECIFIED,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_NOT_CURRENTLY_AVAILABLE, DXGI_ERROR_SESSION_DISCONNECTED,
    DXGI_ERROR_UNSUPPORTED, DXGI_ERROR_WAIT_TIMEOUT, DXGI_ERROR_WAS_STILL_DRAWING,
    DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1, IDXGIDevice, IDXGIDevice2, IDXGIFactory1, IDXGIOutput1,
    IDXGIOutput5, IDXGIOutputDuplication, IDXGIResource,
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
use crate::platform::gpu_memory;
use crate::platform::lip_schedule::{self, WhenToWatch};
use crate::platform::xp_bar::{RailsSeen, shows_the_game};
use crate::xp_tracker::{XpBarGeometry, read_fill};

/// How many looks' copies may wait for the GPU at once: a look that finds as many waiting is let
/// go, and the next comes once one is read.
const STAGING: usize = 3;

/// How often, while it watches, the debug log gets what the looks and their read-backs came to
/// ([`Tally`]); the rest as the duplication goes.
const TALLY_EVERY: Duration = Duration::from_secs(60);

/// Whether DXGI wouldn't duplicate the game's monitor on GPUI's Direct3D device for a reason of the
/// device's own (`lip_schedule::gpui_device_to_blame`): the watcher makes a device of its own from
/// then on, as long as the app runs.
static GPUI_DEVICE_REFUSED: AtomicBool = AtomicBool::new(false);

// The values `lip_schedule` knows them by, which build everywhere.
const _: () = assert!(
    lip_schedule::E_ACCESSDENIED == E_ACCESSDENIED.0
        && lip_schedule::DXGI_ERROR_NOT_CURRENTLY_AVAILABLE == DXGI_ERROR_NOT_CURRENTLY_AVAILABLE.0
        && lip_schedule::DXGI_ERROR_UNSUPPORTED == DXGI_ERROR_UNSUPPORTED.0
        && lip_schedule::DXGI_ERROR_SESSION_DISCONNECTED == DXGI_ERROR_SESSION_DISCONNECTED.0
);

/// What the watcher saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LipReport {
    /// Whether each rail's lip shows in the latest look read, as `overlay_layout::rail_seen`
    /// reads it: sent at the first look, and whenever that changes.
    Seen(RailsSeen),
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
    /// The bar's latest reading ([`LipWatch::fill`]).
    fill: Arc<FillSlot>,
}

/// The bar's fill as the looks last read it (`xp_tracker::read_fill`): its bits, or
/// [`FillSlot::NONE`] when the bar was covered or unreadable, or not read yet.
struct FillSlot(AtomicU64);

impl FillSlot {
    /// What no reading's bits are: a NaN's, which a fill never is.
    const NONE: u64 = u64::MAX;

    fn new() -> FillSlot {
        FillSlot(AtomicU64::new(Self::NONE))
    }

    fn set(&self, fill: Option<f64>) {
        self.0
            .store(fill.map_or(Self::NONE, f64::to_bits), Ordering::Relaxed);
    }

    fn get(&self) -> Option<f64> {
        let bits = self.0.load(Ordering::Relaxed);
        (bits != Self::NONE).then(|| f64::from_bits(bits))
    }
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
        let fill = Arc::new(FillSlot::new());
        let (orders, received) = mpsc::channel();
        let (reports, reported) = async_channel::unbounded();
        let woken = Arc::clone(&wake);
        let read = Arc::clone(&fill);
        std::thread::Builder::new()
            .name("lip-watch".into())
            .spawn(move || watch(&received, woken, &reports, &read))
            .context("starting the lip watcher")?;
        let orders = Some(orders);
        Ok((LipWatch { orders, wake, fill }, reported))
    }

    /// The fill the bar showed when the looks last read it, `None` if it was covered or
    /// unreadable -- a look reads the bar every `lip_schedule::BAR_EVERY` at the most. Meant only
    /// while the watcher reports it watches ([`LipReport::Seen`]): the reading of that watch.
    pub fn fill(&self) -> Option<f64> {
        self.fill.get()
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

fn watch(
    orders: &Receiver<Order>,
    wake: Arc<Handle>,
    reports: &async_channel::Sender<LipReport>,
    slot: &FillSlot,
) {
    let mut waiter = Waiter::new(wake);
    let mut when = WhenToWatch::default();
    let mut game = None;
    // The game's window, found anew with each order and each settle: whether it's in front, and
    // whether it shows the bar.
    let mut window: Option<HWND> = None;
    let mut duplication: Option<Duplication> = None;
    // The rails reported last: `None` for `Idle`, and before the first report.
    let mut reported: Option<RailsSeen> = None;
    // The bar's fill as last read, while watching.
    let mut fill: Option<f64> = None;
    let mut last_error = String::new();
    // The first duplication of the run is logged, as a sign in the diagnostics report that the
    // fast look works, with the device it's made on and how its copies come back; each later one,
    // on every return to the game, only at debug -- unless those changed.
    let mut announced: Option<String> = None;
    // Whether the GPU's signal that a look's copy is made ended the last wait.
    let mut signalled = false;
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
            // Nothing to look at: the duplication goes, with its device if it's the watcher's own,
            // and the thread sleeps till an order comes, or the watching may start by itself.
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
            let opened = Duplication::open(client, waiter.done.as_ref()).and_then(|opened| {
                if !opened.shows(rects()) {
                    bail!("the rails and the bar aren't all on the game's monitor");
                }
                Ok(opened)
            });
            match opened {
                Ok(opened) => {
                    let monitor = opened.monitor;
                    let how = opened.how();
                    if announced.as_ref() == Some(&how) {
                        log::debug!("lip watch: duplicating the monitor at {monitor:?}");
                    } else {
                        log::info!(
                            "lip watch: watching the rails frame by frame on {monitor:?}, {how}"
                        );
                        announced = Some(how);
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
        match open.collect(now, std::mem::take(&mut signalled)) {
            Ok(Some(look)) => {
                if let Some(read) = look.fill {
                    fill = read;
                }
                // For the sampler, which reads it only while this watch's reports say it watches:
                // stored before the first of them, so a watch's last reading, kept past its end,
                // is never taken for the next one's.
                slot.set(fill);
                if reported != Some(look.rails) {
                    reported = Some(look.rails);
                    let _ = reports.try_send(LipReport::Seen(look.rails));
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
        open.tally.log_if_due(now);
        let last_input = waiter.input.last(now);
        // Whether the latest look read showed every rail: nothing over them to go by itself.
        let clear = reported.is_some_and(|rails| rails.flask && rails.skill);
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
        signalled = waiter.watching(due, last_input) == Woken::Done;
    }
}

/// The duplication of the monitor the game is on, and what reading its lips and bar takes, on
/// GPUI's Direct3D device or on one of its own ([`Owner`]).
struct Duplication {
    /// The monitor's area of the desktop, physical pixels.
    monitor: PhysicalRect,
    /// Let go of first as the duplication goes, under the device's lock (`Drop`).
    duplication: ManuallyDrop<IDXGIOutputDuplication>,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    owner: Owner,
    /// How the GPU signals that it has made a look's copies, if it can.
    done: Option<Done>,
    /// The textures the looks are copied into to be read -- the flask lip's rows, the skill lip's
    /// under them, the bar's under those -- each with the look it holds until it's read.
    staging: [Staging; STAGING],
    /// Their size, which has room for the bar whether it's copied or not.
    size: (u32, u32),
    /// How many looks were copied: each copy's number, in the order they're read back.
    copies: u64,
    /// When it was made: its first frame is looked for from then.
    opened: Instant,
    /// When the last look was: the next is paced from it.
    last_look: Option<Instant>,
    /// Whether a look took a frame of the desktop yet, not one where only the pointer moved.
    framed: bool,
    /// When the oldest copy waiting is next tried to be read back on the timer
    /// (`lip_schedule::read_due`), while one waits.
    read_at: Option<Instant>,
    /// The tries in a row that found the GPU not done with the oldest copy, as
    /// `lip_schedule::read_missed` counts them.
    misses: u32,
    /// When the bar was last copied.
    bar_read: Option<Instant>,
    /// One lip's or the bar's rows, reused from look to look.
    rows: Vec<u8>,
    /// What the looks and their read-backs came to, for the debug log.
    tally: Tally,
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
    /// When its look was.
    at: Instant,
    /// Whether the GPU is to signal once it has made it.
    signalled: bool,
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

/// Whose Direct3D device a duplication is made on.
enum Owner {
    /// GPUI's (`gpu_memory::gpui_device`), its multithread protection on: the lock each of the
    /// watcher's sequences of calls holds ([`Owner::lock`]).
    Gpui(ID3D11Multithread),
    /// One of the watcher's own, made for the duplication and let go with it; why GPUI's isn't
    /// shared.
    Own(&'static str),
}

impl Owner {
    /// Holds the device's lock till the returned guard goes, if the device is GPUI's: GPUI's calls
    /// on it, on the UI thread, wait till then -- microseconds, since nothing waits under it.
    fn lock(&self) -> Option<Locked> {
        let Owner::Gpui(lock) = self else {
            return None;
        };
        // SAFETY: the lock of a device whose multithread protection is on (`device_on`), entered
        // and left on this thread.
        unsafe { lock.Enter() };
        Some(Locked(lock.clone()))
    }
}

/// GPUI's device's lock, held till this goes.
struct Locked(ID3D11Multithread);

impl Drop for Locked {
    fn drop(&mut self) {
        // SAFETY: entered on this thread by `Owner::lock`.
        unsafe { self.0.Leave() };
    }
}

/// How the GPU signals that it has made a look's copies: it sets `event`, one of the watching
/// thread's waits ([`Waiter`]).
struct Done {
    event: Arc<Handle>,
    signal: Signal,
}

/// What sets a [`Done`]'s event.
enum Signal {
    /// A fence of the device's (`ID3D11Fence`, Windows 10 1703 on), set to each look's number
    /// once the work queued on the context before it is done: the looks' copies on a device of
    /// the watcher's own, or on GPUI's, whatever GPUI queued ahead of them too.
    Fence {
        fence: ID3D11Fence,
        context: ID3D11DeviceContext4,
    },
    /// `IDXGIDevice2::EnqueueSetEvent`: the event is set once all the work the device was given so
    /// far is done -- kept to a device of the watcher's own, where that work is the looks' alone.
    Enqueued(IDXGIDevice2),
}

impl Done {
    /// How the GPU can signal on `device`, `owner`'s, that a look's copies are made, by setting
    /// `event`: a fence where Direct3D has them; else `EnqueueSetEvent` on a device of the
    /// watcher's own, and nothing on GPUI's.
    fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        owner: &Owner,
        event: &Arc<Handle>,
    ) -> Option<Done> {
        let fence = device.cast::<ID3D11Device5>().ok().and_then(|device| {
            let mut fence: Option<ID3D11Fence> = None;
            // SAFETY: a device method: GPUI's device isn't single-threaded (`gpui_windows`'
            // `directx_devices.rs`), and the watcher's own is its alone.
            unsafe { device.CreateFence(0, D3D11_FENCE_FLAG_NONE, &mut fence) }.ok()?;
            fence
        });
        let signal = match (fence, context.cast::<ID3D11DeviceContext4>()) {
            (Some(fence), Ok(context)) => Signal::Fence { fence, context },
            _ if matches!(owner, Owner::Own(_)) => {
                Signal::Enqueued(device.cast::<IDXGIDevice2>().ok()?)
            }
            _ => return None,
        };
        Some(Done {
            event: Arc::clone(event),
            signal,
        })
    }
}

/// What the looks and their read-backs came to since `since`, for the debug log: whether the
/// GPU's signals bring the read-backs, and how long after its look a copy is read.
struct Tally {
    since: Instant,
    looks: u32,
    /// Looks that found a new frame and copied it.
    copied: u32,
    /// Copies read back at the GPU's signal, and on the timer.
    on_signal: u32,
    on_timer: u32,
    /// Tries on the timer that found the GPU not done with the copy.
    misses: u32,
    /// Tries the copy's own signal brought that found it not readable yet: signals a moment early
    /// (`lip_schedule::SIGNAL_LAG`).
    early_signals: u32,
    /// The time from a look to its copy's read-back: all of them, and the longest.
    waited: Duration,
    longest: Duration,
}

impl Tally {
    fn new(since: Instant) -> Tally {
        Tally {
            since,
            looks: 0,
            copied: 0,
            on_signal: 0,
            on_timer: 0,
            misses: 0,
            early_signals: 0,
            waited: Duration::ZERO,
            longest: Duration::ZERO,
        }
    }

    /// A try at reading back -- `signalled` if the GPU's signal brought it -- that read `read`
    /// copies, `missed` whether it found the next still being made, and `early` whether that was
    /// the next one's own signal.
    fn tried(&mut self, signalled: bool, read: u32, missed: bool, early: bool) {
        if signalled {
            self.on_signal += read;
            self.early_signals += u32::from(early);
        } else {
            self.on_timer += read;
            self.misses += u32::from(missed);
        }
    }

    /// A copy read back `waited` after its look.
    fn read(&mut self, waited: Duration) {
        self.waited += waited;
        self.longest = self.longest.max(waited);
    }

    /// Logs the tally at `now` and starts a new one, once it's `TALLY_EVERY` old.
    fn log_if_due(&mut self, now: Instant) {
        if now.saturating_duration_since(self.since) >= TALLY_EVERY {
            self.log(now);
            *self = Tally::new(now);
        }
    }

    /// Logs the tally at `now`, at debug, if a look came since it started.
    fn log(&self, now: Instant) {
        if self.looks == 0 {
            return;
        }
        let read = self.on_signal + self.on_timer;
        log::debug!(
            "lip watch: in {:.1?}, {} looks, {} copied, {read} read back ({} on the GPU's signal, \
             {} on the timer), {} timer tries too early, {} signals early; a copy read \
             {:.1?} after its look on average, {:.1?} at most",
            now.saturating_duration_since(self.since),
            self.looks,
            self.copied,
            self.on_signal,
            self.on_timer,
            self.misses,
            self.early_signals,
            self.waited.checked_div(read).unwrap_or_default(),
            self.longest
        );
    }
}

impl Duplication {
    /// Duplicates the monitor under the middle of the game's client area, on GPUI's device where
    /// it may be shared, else on one of the watcher's own ([`device_on`]); the GPU is asked to set
    /// `event` once each look's copies are made, where it can.
    fn open(client: PhysicalRect, event: Option<&Arc<Handle>>) -> Result<Duplication> {
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
                // 8-bit BGRA whatever the desktop is, HDR included; outputs before Windows 10
                // 1703 duplicate the legacy way.
                let newer = output.cast::<IDXGIOutput5>().ok();
                let legacy: IDXGIOutput1 = output.cast().context("IDXGIOutput1")?;
                let duplicate = |device: &ID3D11Device| unsafe {
                    match &newer {
                        Some(newer) => {
                            newer.DuplicateOutput1(device, 0, &[DXGI_FORMAT_B8G8R8A8_UNORM])
                        }
                        None => legacy.DuplicateOutput(device),
                    }
                };
                // Twice at the most: once DXGI wouldn't duplicate on GPUI's device, `device_on`
                // makes one of the watcher's own, tried at once.
                let (device, owner, duplication) = loop {
                    let (device, owner) = device_on(&adapter)?;
                    let duplicated = {
                        let _locked = owner.lock();
                        duplicate(&device)
                    };
                    match duplicated {
                        Err(err)
                            if matches!(owner, Owner::Gpui(_))
                                && lip_schedule::gpui_device_to_blame(
                                    err.code().0,
                                    unsafe { device.GetDeviceRemovedReason() }.is_err(),
                                ) =>
                        {
                            log::warn!(
                                "lip watch: duplicating the game's monitor on GPUI's Direct3D \
                                 device failed ({}: {}); on a device of its own from now on",
                                err.code(),
                                err.message().trim_end()
                            );
                            GPUI_DEVICE_REFUSED.store(true, Ordering::Relaxed);
                        }
                        duplicated => {
                            let duplication =
                                duplicated.context("duplicating the game's monitor")?;
                            break (device, owner, duplication);
                        }
                    }
                };
                let context =
                    unsafe { device.GetImmediateContext() }.context("GetImmediateContext")?;
                let done = event.and_then(|event| Done::new(&device, &context, &owner, event));
                let opened = Instant::now();
                return Ok(Duplication {
                    monitor,
                    duplication: ManuallyDrop::new(duplication),
                    context,
                    device,
                    owner,
                    done,
                    staging: Default::default(),
                    size: (0, 0),
                    copies: 0,
                    opened,
                    last_look: None,
                    framed: false,
                    read_at: None,
                    misses: 0,
                    bar_read: None,
                    rows: Vec::new(),
                    tally: Tally::new(opened),
                });
            }
        }
        bail!("no monitor shows the game")
    }

    /// The device it's made on and how its copies come back, for the log.
    fn how(&self) -> String {
        let device = match self.owner {
            Owner::Gpui(_) => "on GPUI's Direct3D device".to_owned(),
            Owner::Own(why) => format!("on a Direct3D device of its own ({why})"),
        };
        let back = match self.done.as_ref().map(|done| &done.signal) {
            Some(Signal::Fence { .. }) => "read back on the GPU's fence",
            Some(Signal::Enqueued(_)) => "read back on the GPU's event (EnqueueSetEvent)",
            None => "read back on a timer",
        };
        format!("{device}, {back}")
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
    /// first, soon after for the first frame (`lip_schedule::first_frame_look`); `None` while
    /// every staging texture holds a copy -- a read-back frees one.
    fn next_look(&self, now: Instant, last_input: Instant, clear: bool) -> Option<Instant> {
        self.room()?;
        let Some(last) = self.last_look else {
            return Some(now);
        };
        let paced = lip_schedule::next_look(last, last_input, clear);
        Some(if self.framed {
            paced
        } else {
            lip_schedule::first_frame_look(self.opened, last, paced)
        })
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
        self.tally.looks += 1;
        // The bar counts only where the game itself shows it: a window over it -- the price
        // panel spans its middle -- would read as a wrong fill (see `xp_bar::shows_the_game`).
        // Windows is asked that before the device's lock is taken.
        let reading = bar
            .filter(|_| lip_schedule::bar_due(self.bar_read, now))
            .map(|geometry| {
                game.is_some_and(|game| shows_the_game(game, geometry.capture))
                    .then(|| geometry.clone())
            });
        let _locked = self.owner.lock();
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource = None;
        // Only a frame that's there, a new duplication's first too: a wait would hold the lock.
        match unsafe {
            self.duplication
                .AcquireNextFrame(0, &mut info, &mut resource)
        } {
            Err(err) if err.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(()),
            other => other.context("AcquireNextFrame")?,
        }
        // A present time of zero: only the pointer moved.
        let copied = match resource {
            Some(frame) if info.LastPresentTime != 0 => {
                self.framed = true;
                self.copy(slot, &frame, lips, bar, reading, now).map(Some)
            }
            _ => Ok(None),
        };
        // Windows holds the next frame back until this one is released. The copies are queued
        // on the GPU before the release, so they read this frame.
        unsafe { self.duplication.ReleaseFrame() }.context("ReleaseFrame")?;
        if let Some(mut copied) = copied? {
            copied.signalled = self.flush(copied.number);
            self.tally.copied += 1;
            self.read_at = self.read_at.or(Some(lip_schedule::read_due(
                now,
                copied.signalled,
                0,
                false,
                now,
            )));
            self.staging[slot].copied = Some(copied);
        }
        Ok(())
    }

    /// Queues the copy of `lips`, and of the bar if it's due a `reading` and shown, out of
    /// `frame` into staging texture `slot`, which has room for `bar`: what's copied.
    fn copy(
        &mut self,
        slot: usize,
        frame: &IDXGIResource,
        lips: &[PhysicalRect; 2],
        bar: Option<&XpBarGeometry>,
        reading: Option<Option<XpBarGeometry>>,
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
        if reading.is_some() {
            self.bar_read = Some(now);
        }
        let shown = reading.as_ref().and_then(Option::as_ref);
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
            at: now,
            signalled: false,
            lips: *lips,
            bar: reading,
        })
    }

    /// Sends the copies of the look numbered `number`, just queued, to the GPU, asking it to
    /// signal once it has made them ([`Done`]): whether it will. They're on the GPU's way then --
    /// a fence's signal with them -- not with whatever the device is asked next.
    fn flush(&self, number: u64) -> bool {
        let asked = match &self.done {
            Some(Done {
                event,
                signal: Signal::Fence { fence, context },
            }) => unsafe {
                context
                    .Signal(fence, number)
                    .and_then(|()| fence.SetEventOnCompletion(number, event.0))
                    .is_ok()
            },
            Some(Done {
                event,
                signal: Signal::Enqueued(device),
            }) => {
                // It flushes as it asks.
                if unsafe { device.EnqueueSetEvent(event.0) }.is_ok() {
                    return true;
                }
                false
            }
            None => false,
        };
        unsafe { self.context.Flush() };
        asked
    }

    /// Whether the GPU has made the copy in staging texture `slot`, as the fence its signal comes
    /// by says; `None` where it has no such fence.
    fn made(&self, slot: usize) -> Option<bool> {
        let copied = self.staging[slot].copied.as_ref()?;
        match &self.done {
            Some(Done {
                signal: Signal::Fence { fence, .. },
                ..
            }) if copied.signalled => Some(unsafe { fence.GetCompletedValue() } >= copied.number),
            _ => None,
        }
    }

    /// Reads back the copies the GPU has made by `now`, oldest first, if a read-back is due on
    /// the timer or `signalled` -- the GPU's signal ended the wait: the latest look read -- its
    /// rails -- with the bar's latest reading among them; `None` if none was. A copy the GPU
    /// hasn't made yet is left to its signal, or tried again later (`lip_schedule::read_due`):
    /// nothing here waits for it.
    fn collect(&mut self, now: Instant, signalled: bool) -> Result<Option<Look>> {
        if !signalled && self.read_at.is_none_or(|at| now < at) {
            return Ok(None);
        }
        let mut latest: Option<Look> = None;
        let mut read = 0;
        let mut missed = false;
        while let Some(slot) = self.oldest() {
            let Some(texture) = self.staging[slot].texture.clone() else {
                self.staging[slot].copied = None;
                continue;
            };
            // A copy its fence doesn't say made isn't tried: `Map` could only say so too.
            if self.made(slot) == Some(false) {
                missed = true;
                break;
            }
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            let map = {
                let _locked = self.owner.lock();
                unsafe {
                    self.context.Map(
                        &texture,
                        0,
                        D3D11_MAP_READ,
                        D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                        Some(&mut mapped),
                    )
                }
            };
            match map {
                Err(err) if err.code() == DXGI_ERROR_WAS_STILL_DRAWING => {
                    missed = true;
                    break;
                }
                other => other.context("Map")?,
            }
            self.misses = 0;
            read += 1;
            // The rows are read with the lock let go: the mapping stays till `Unmap`.
            let look = self.staging[slot].copied.take().map(|copied| {
                self.tally.read(now.saturating_duration_since(copied.at));
                read_look(&mut self.rows, &mapped, &copied)
            });
            {
                let _locked = self.owner.lock();
                unsafe { self.context.Unmap(&texture, 0) };
            }
            if let Some(look) = look {
                let fill = look.fill.or(latest.and_then(|latest| latest.fill));
                latest = Some(Look { fill, ..look });
            }
        }
        let oldest = self.oldest().and_then(|slot| {
            let copied = self.staging[slot].copied.as_ref()?;
            Some((copied.at, copied.signalled, self.made(slot)))
        });
        let mut early = false;
        self.read_at = oldest.map(|(look, signalled_copy, made)| {
            let counts = missed
                && lip_schedule::read_missed(look, signalled_copy, signalled, made, read > 0, now);
            self.misses += u32::from(counts);
            early = counts && signalled;
            lip_schedule::read_due(look, signalled_copy, self.misses, early, now)
        });
        if self.read_at.is_none() {
            self.misses = 0;
        }
        self.tally.tried(signalled, read, missed, early);
        Ok(latest)
    }
}

/// The device to duplicate an output of `adapter` on, and whose it is: GPUI's where it may be
/// shared -- the hook saw it made, it's on this graphics card, its multithread protection is on,
/// and DXGI never refused to duplicate on it ([`GPUI_DEVICE_REFUSED`]) -- else a new one of the
/// watcher's own. An error while GPUI's device is lost: the next try takes the one GPUI makes
/// anew.
fn device_on(adapter: &IDXGIAdapter1) -> Result<(ID3D11Device, Owner)> {
    let why = match gpu_memory::gpui_device() {
        None => "GPUI's isn't known",
        Some(_) if GPUI_DEVICE_REFUSED.load(Ordering::Relaxed) => {
            "duplicating on GPUI's device failed"
        }
        Some(device) => {
            // SAFETY: device and adapter methods, which any thread may call: GPUI's device isn't
            // single-threaded (`gpui_windows`' `directx_devices.rs`). Its context's lock is only
            // asked about.
            let (lost, card, ours, lock) = unsafe {
                let card = device
                    .cast::<IDXGIDevice>()
                    .and_then(|dxgi| dxgi.GetAdapter())
                    .and_then(|card| card.GetDesc())
                    .map(|desc| desc.AdapterLuid);
                let lock = device
                    .GetImmediateContext()
                    .and_then(|context| context.cast::<ID3D11Multithread>())
                    .ok()
                    .filter(|lock| lock.GetMultithreadProtected().as_bool());
                (
                    device.GetDeviceRemovedReason().is_err(),
                    card,
                    adapter.GetDesc1().map(|desc| desc.AdapterLuid),
                    lock,
                )
            };
            if lost {
                bail!("GPUI's Direct3D device is lost; tried again on the one GPUI makes anew");
            }
            let same_card = card
                .ok()
                .zip(ours.ok())
                .is_some_and(|(card, ours)| card == ours);
            match (same_card, lock) {
                (false, _) => "GPUI's is on another graphics card",
                (true, None) => "GPUI's has no multithread protection",
                (true, Some(lock)) => return Ok((device, Owner::Gpui(lock))),
            }
        }
    };
    let mut device = None;
    unsafe {
        D3D11CreateDevice(
            adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
    }
    .context("D3D11CreateDevice")?;
    let device = device.context("D3D11CreateDevice returned no device")?;
    Ok((device, Owner::Own(why)))
}

impl Drop for Duplication {
    /// Direct3D destroys what's let go only once nothing on its context holds it, as the context
    /// flushes: the watcher's objects go first, then the context is flushed, so they don't stay
    /// till GPUI's next paint. A device of the watcher's own is cleared first and goes with its
    /// last reference -- its video memory, the frame's, and whatever the graphics driver keeps
    /// for it -- not some time later; GPUI's context, whose state is GPUI's, is only flushed.
    fn drop(&mut self) {
        self.tally.log(Instant::now());
        let _locked = self.owner.lock();
        // SAFETY: never used again: the duplication goes with `self`.
        unsafe { ManuallyDrop::drop(&mut self.duplication) };
        self.staging = Default::default();
        self.done = None;
        unsafe {
            if let Owner::Own(_) = self.owner {
                self.context.ClearState();
            }
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
    /// The GPU signalled that it has made a look's copy ([`Done`]).
    Done,
    /// The player moved the mouse or pressed a key.
    Input,
}

/// What the watching thread sleeps on: its orders' event, a timer, the GPU's signal that a look's
/// copy is made, and the player's input.
struct Waiter {
    orders: Arc<Handle>,
    /// A high-resolution waitable timer -- a wait's own timeout keeps to the system's ticks, 15.6
    /// ms apart unless a program asks for finer -- or `None` if none could be made.
    timer: Option<Handle>,
    /// The event the GPU sets once it has made a look's copies ([`Done`]), made once for the
    /// thread's life -- a signal may come after its duplication has gone -- or `None` if none
    /// could be made: the copies are read back on the timer then.
    done: Option<Arc<Handle>>,
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
        let done = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
            .ok()
            .map(|event| Arc::new(Handle(event)));
        Waiter {
            orders,
            timer,
            done,
            input: InputWake::new(),
        }
    }

    /// Sleeps, not watching, till `until` or an order: no input is waited for.
    fn idle(&mut self, until: Option<Instant>) {
        self.input.reset();
        self.wait(until, false, false);
    }

    /// Sleeps, watching, till `due`, an order or the GPU's signal -- and while the player keeps
    /// still, their last input at `last_input`, till they move (`lip_schedule::wait_until`); what
    /// ended the sleep.
    fn watching(&mut self, due: Option<Instant>, last_input: Instant) -> Woken {
        let now = Instant::now();
        let input_wakes = if lip_schedule::still(now, last_input) {
            self.input.arm(now, last_input)
        } else {
            self.input.disarm();
            false
        };
        let until = lip_schedule::wait_until(due, now, last_input, input_wakes);
        let woken = self.wait(until, input_wakes, true);
        if woken == Woken::Input {
            self.input.woke(Instant::now());
        }
        woken
    }

    /// Sleeps till `until` (`None`: no end), an order, or -- `on_done` -- the GPU's signal, or --
    /// `on_input` -- the player's input.
    fn wait(&self, until: Option<Instant>, on_input: bool, on_done: bool) -> Woken {
        let mut handles = [self.orders.0; 3];
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
                    handles[count] = timer.0;
                    count += 1;
                }
                _ => {
                    timeout = u32::try_from(left.as_micros().div_ceil(1000)).unwrap_or(INFINITE - 1)
                }
            }
        }
        let mut done = None;
        if on_done && let Some(event) = &self.done {
            handles[count] = event.0;
            done = Some(count);
            count += 1;
        }
        let (mask, flags) = if on_input {
            (QS_RAWINPUT, MWMO_INPUTAVAILABLE)
        } else {
            (QUEUE_STATUS_FLAGS(0), MWMO_NONE)
        };
        let woken =
            unsafe { MsgWaitForMultipleObjectsEx(Some(&handles[..count]), timeout, mask, flags) };
        let index = woken.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
        if woken == WAIT_OBJECT_0 {
            Woken::Orders
        } else if Some(index) == done {
            Woken::Done
        } else if index == count {
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
