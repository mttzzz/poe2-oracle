//! Gives back the GPU memory a window of the app let go of, and says how much the app holds; and
//! keeps GPUI's Direct3D device, which the lip watcher shares (`lip_watch`), guarded for that.
//!
//! A hidden window taken down to a pixel (`Win32Overlay::shrink`) or closed releases its swap
//! chain's buffers and its paths' two textures, 32 bytes a device pixel: 61 MiB for the price
//! panel at 4K, 253 MiB for the tour's spotlight. Direct3D 11 destroys a released object only as
//! its device's context flushes ("Direct3D 11 defers the destruction of objects",
//! `ID3D11DeviceContext::Flush`'s documentation), and the graphics driver frees the memory only
//! once the GPU is done with it. The one frame a shrink draws at the pixel flushes right after the
//! release, in the same moment. Live on 2026-09-27 with the build of 93296e4 -- the first whose
//! devices come without the driver's own threads (`d3d_threading`), which do such work in the
//! background -- the app held 84 MB of dedicated GPU memory before the first price check and 130 MB
//! after it with the panel hidden again, minutes later still: nothing had drawn since.
//!
//! So [`release`] flushes GPUI's device a moment after a window let go of its memory, once the GPU
//! is long done with that frame, and trims it (`IDXGIDevice3::Trim`: the driver also gives back
//! the memory it keeps to make later requests quicker), then logs the process's dedicated GPU
//! memory before and after. GPUI's device is the one `d3d_threading`'s `D3D11CreateDevice` hook
//! saw made on the UI thread, or on GPUI's vsync thread, which makes a new one after a lost device
//! ([`note_device`]).
//!
//! The lip watcher duplicates the game's monitor on that device where it can, rather than on one
//! of its own, which would bring threads of the graphics driver with it. GPUI calls its device's
//! immediate context on the UI thread whenever it likes, unguarded -- `gpui_windows`'
//! `direct_write.rs` says it must stay on that thread -- so [`note_device`] turns on the device's
//! multithread protection as it's made, before GPUI's first call: from then on each call on the
//! context, and each DXGI call on the device, holds the device's lock, and the watcher holds it
//! for each of its sequences of calls (`ID3D11Multithread::Enter`). Turned on later, it would
//! race GPUI's calls.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Multithread};
use windows::Win32::Graphics::Dxgi::{
    DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3, IDXGIDevice,
    IDXGIDevice3,
};
use windows::core::Interface;

/// How long after a window let go of its memory [`release`] runs: long past the GPU's work on the
/// frame the shrink drew, and past GPUI's destroying a closed window, which it does from a task
/// of its own (`Drop for WindowsWindow`).
pub const RELEASE_AFTER: Duration = Duration::from_millis(500);

/// GPUI's Direct3D device, as the hook saw it made; `None` before, and with the hook out.
static DEVICE: Mutex<Option<ID3D11Device>> = Mutex::new(None);

/// The threads GPUI makes its device on: the UI thread, as the application is built, and its
/// vsync thread after a lost device (`gpui_windows`' `handle_gpu_device_lost`). The lip watcher
/// makes a device of its own, where it doesn't share GPUI's, on its thread.
const GPUI_DEVICE_THREADS: [&str; 2] = ["main", "VSyncProvider"];

/// Keeps `device` for [`release`] and the lip watcher if the thread that made it is GPUI's, its
/// multithread protection turned on first: from `d3d_threading`'s hook, right after
/// `D3D11CreateDevice` made it and before GPUI has it. A device GPUI made anew replaces the old
/// one.
pub(super) fn note_device(device: &ID3D11Device) {
    let thread = std::thread::current();
    if !GPUI_DEVICE_THREADS.contains(&thread.name().unwrap_or_default()) {
        return;
    }
    if protect(device) {
        log::debug!("gpu memory: GPUI's Direct3D device made, guarded for the lip watcher");
    } else {
        log::warn!(
            "gpu memory: GPUI's Direct3D device has no multithread protection; the lip watcher \
             makes a device of its own"
        );
    }
    *DEVICE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(device.clone());
}

/// Turns on `device`'s multithread protection (`ID3D11Multithread`, on its immediate context);
/// whether it's on.
fn protect(device: &ID3D11Device) -> bool {
    // SAFETY: a device just made, which no other thread has yet.
    unsafe {
        device
            .GetImmediateContext()
            .and_then(|context| context.cast::<ID3D11Multithread>())
            .is_ok_and(|lock| {
                let _ = lock.SetMultithreadProtected(true);
                lock.GetMultithreadProtected().as_bool()
            })
    }
}

/// GPUI's device, if the hook saw it made: the one GPUI draws with now.
pub fn gpui_device() -> Option<ID3D11Device> {
    DEVICE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// The process's dedicated GPU memory -- its use of its graphics card's own memory, Task
/// Manager's «Dedicated GPU memory» -- in bytes, read through `device`'s adapter.
fn dedicated_of(device: &ID3D11Device) -> Option<u64> {
    let adapter: IDXGIAdapter3 = unsafe { device.cast::<IDXGIDevice>().ok()?.GetAdapter() }
        .ok()?
        .cast()
        .ok()?;
    let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
    unsafe { adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info) }.ok()?;
    Some(info.CurrentUsage)
}

/// The process's dedicated GPU memory in bytes, if GPUI's device is known.
pub fn dedicated() -> Option<u64> {
    dedicated_of(&gpui_device()?)
}

/// Bytes as MiB, for the log.
pub fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024. * 1024.)
}

/// Has the driver free what `what` -- a window hidden and taken down to a pixel, or closed --
/// let go of: GPUI's device's context unbinds the texture a path's copy may have left bound and
/// flushes, and the device is trimmed. On the UI thread, where GPUI uses the context, and outside
/// its drawing. Logs the dedicated GPU memory before and after; nothing without GPUI's device.
pub fn release(what: &str) {
    let Some(device) = gpui_device() else {
        log::debug!("{what}: no Direct3D device of GPUI's at hand to flush");
        return;
    };
    let before = dedicated_of(&device);
    let start = Instant::now();
    // SAFETY: GPUI's own device and its immediate context, used on the UI thread as GPUI uses
    // them, between its frames. Slot 0 is where each of GPUI's textured draws binds its texture
    // before it draws (`draw_with_texture`, `draw_range_with_texture`), so nothing relies on what
    // stays bound there.
    unsafe {
        if let Ok(context) = device.GetImmediateContext() {
            context.PSSetShaderResources(0, Some(&[None]));
            context.Flush();
        }
        if let Ok(dxgi) = device.cast::<IDXGIDevice3>() {
            dxgi.Trim();
        }
    }
    let took = start.elapsed();
    match (before, dedicated_of(&device)) {
        (Some(before), Some(after)) => log::info!(
            "{what}: dedicated GPU memory {:.1} MiB, {:.1} MiB once flushed and trimmed ({took:.1?})",
            mib(before),
            mib(after)
        ),
        _ => log::info!("{what}: GPU memory flushed and trimmed ({took:.1?})"),
    }
}
