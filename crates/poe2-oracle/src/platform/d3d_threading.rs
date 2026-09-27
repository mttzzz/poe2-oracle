//! Makes the app's Direct3D devices without threads of the graphics driver's own (`install`, on
//! Windows). Which devices go without them is decided apart from the Windows calls, so it builds
//! and is tested on every target.
//!
//! A Direct3D 11 device comes with threads of the graphics driver, which take the device's work
//! off the threads that make the calls. Measured 2026-09-26 on the test machine (NVIDIA RTX 4090,
//! driver 32.0.16.1714, a 60 Hz display), the app idle over the game: 43 threads of
//! `nvwgf2umx.dll`, NVIDIA's Direct3D driver, made as the app started, the busiest woken 71 times
//! a second for 20 ms of CPU a minute -- more than GPUI's UI thread took. It keeps a pace of its
//! own: 69 wake-ups a second while GPUI's vsync thread woke 63 times, 71 once `vsync_park` let
//! that thread sleep (8 times a second), and six for each of the UI thread's, whose paints are all
//! the work GPUI's device gets. The lip watcher's device (`lip_watch`, made each time it starts
//! watching) came with 17 threads more, which took 119 ms a minute while it watched, measured the
//! same day; the watcher shares GPUI's device now wherever it can.
//!
//! `D3D11_CREATE_DEVICE_PREVENT_INTERNAL_THREADING_OPTIMIZATIONS` asks for a device without them:
//! Direct3D hands it to the driver's `CreateDevice` as
//! `D3D10DDI_CREATEDEVICE_FLAG_DISABLE_EXTRA_THREAD_CREATION`, and the driver does the device's
//! work on the threads that call it. Microsoft doesn't recommend it in general -- the thousands of
//! draws of a game's frame are what those threads take off its hands -- but GPUI's few draws a
//! paint and the lip watcher's copy and read-back a look have nothing to gain from them, and
//! Firefox has made its compositor's device with it since 2014 (`gfx/thebes/DeviceManagerDx.cpp`:
//! "IE 11 also uses this flag").
//!
//! So this executable's import of `D3D11CreateDevice` is pointed at `create_device`, the way
//! `redraw_filter` points `RedrawWindow`'s: it adds the flag ([`first_try`]) to GPUI's device,
//! made as the application is built and again after a lost device (`gpui_windows`'
//! `directx_devices.rs`), and to a device of the lip watcher's own, where it can't share GPUI's.
//! A device the driver won't make with it is made as asked. A software device -- WARP, or
//! Microsoft's Basic Render Driver on a machine without a graphics card -- is left as asked: there
//! the flag moves the rasterising itself onto the calling thread (Firefox leaves WARP out too).
//!
//! [`THREADING_ENV`]`=1` in the app's environment leaves the driver its threads: the other side
//! of a comparison. Either way the hook hands GPUI's device to `gpu_memory`, which turns on its
//! multithread protection for the lip watcher to share it, and flushes and trims it once a window
//! let go of its memory. `install` runs before GPUI starts, which makes its device while the
//! application is built (`platform.rs`' `WindowsPlatform::new`).

use std::ffi::OsStr;

/// The environment variable that, set to `1`, leaves the graphics driver its threads.
pub const THREADING_ENV: &str = "POE2_ORACLE_D3D_THREADING";

/// `D3D11_CREATE_DEVICE_PREVENT_INTERNAL_THREADING_OPTIMIZATIONS`.
pub const PREVENT_THREADING: u32 = 0x8;

/// `D3D_DRIVER_TYPE_UNKNOWN`: the driver of the adapter passed along.
pub const DRIVER_UNKNOWN: i32 = 0;

/// `D3D_DRIVER_TYPE_HARDWARE`: the driver of the default adapter.
pub const DRIVER_HARDWARE: i32 = 1;

/// Microsoft's PCI vendor ID: the adapter of its Basic Render Driver, WARP by another name.
pub const MICROSOFT: u32 = 0x1414;

/// Whether `value`, the environment's [`THREADING_ENV`], leaves the driver its threads.
pub fn leaves_threads(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| value == "1")
}

/// The flags a device asked with `asked` is made with first: those and [`PREVENT_THREADING`], for
/// a device of the `driver` type (`D3D_DRIVER_TYPE`) on an adapter of `vendor` -- `None` when no
/// adapter was passed or its description couldn't be read. `None` when the asked ones stand: a
/// software device, or one asked without the driver's threads already.
pub fn first_try(asked: u32, driver: i32, vendor: Option<u32>) -> Option<u32> {
    let card = matches!(driver, DRIVER_UNKNOWN | DRIVER_HARDWARE) && vendor != Some(MICROSOFT);
    (card && asked & PREVENT_THREADING == 0).then_some(asked | PREVENT_THREADING)
}

#[cfg(target_os = "windows")]
pub use hook::install;

#[cfg(target_os = "windows")]
mod hook {
    use std::ffi::c_void;
    use std::sync::LazyLock;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CREATE_DEVICE_FLAG, D3D11_CREATE_DEVICE_PREVENT_INTERNAL_THREADING_OPTIMIZATIONS,
        ID3D11Device,
    };
    use windows::Win32::Graphics::Dxgi::IDXGIAdapter;
    use windows::core::{HRESULT, Interface};

    use super::{
        DRIVER_HARDWARE, DRIVER_UNKNOWN, PREVENT_THREADING, THREADING_ENV, first_try,
        leaves_threads,
    };
    use crate::platform::{gpu_memory, redraw_filter};

    // The values `first_try` knows them by, which build everywhere.
    const _: () = assert!(
        PREVENT_THREADING == D3D11_CREATE_DEVICE_PREVENT_INTERNAL_THREADING_OPTIMIZATIONS.0
            && DRIVER_UNKNOWN == D3D_DRIVER_TYPE_UNKNOWN.0
            && DRIVER_HARDWARE == D3D_DRIVER_TYPE_HARDWARE.0
    );

    type CreateDeviceFn = unsafe extern "system" fn(
        *mut c_void,
        D3D_DRIVER_TYPE,
        HMODULE,
        D3D11_CREATE_DEVICE_FLAG,
        *const D3D_FEATURE_LEVEL,
        u32,
        u32,
        *mut *mut c_void,
        *mut D3D_FEATURE_LEVEL,
        *mut *mut c_void,
    ) -> HRESULT;

    /// d3d11's `D3D11CreateDevice`: the import slot's value before [`create_device`] took it.
    static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

    /// Whether a device was made without the driver's threads yet: the first one is logged.
    static ANNOUNCED: AtomicBool = AtomicBool::new(false);

    /// Whether the driver keeps its threads: [`THREADING_ENV`]`=1` in the app's environment.
    static LEAVE_THREADS: LazyLock<bool> =
        LazyLock::new(|| leaves_threads(std::env::var_os(THREADING_ENV).as_deref()));

    /// Whether the hook is in: tried once, on the first call of [`install`].
    static INSTALLED: LazyLock<bool> = LazyLock::new(|| {
        let hook: CreateDeviceFn = create_device;
        match redraw_filter::point_import(
            "d3d11.dll",
            "D3D11CreateDevice",
            hook as usize,
            &ORIGINAL,
        ) {
            Ok(slots) if *LEAVE_THREADS => {
                log::info!(
                    "d3d threading: {THREADING_ENV}=1, the graphics driver keeps its threads \
                     ({slots} D3D11CreateDevice import(s) hooked for GPUI's device alone)"
                );
                true
            }
            Ok(slots) => {
                log::info!(
                    "d3d threading: {slots} D3D11CreateDevice import(s) hooked: devices on a \
                     graphics card come without the driver's threads"
                );
                true
            }
            Err(err) => {
                log::warn!("d3d threading: {err:#}; the graphics driver keeps its threads");
                false
            }
        }
    });

    /// Makes Direct3D devices on a graphics card without the driver's threads from now on --
    /// unless [`THREADING_ENV`]`=1` -- and keeps GPUI's device for `gpu_memory`; whether the hook
    /// is in. Call before GPUI starts (see the module's doc).
    pub fn install() -> bool {
        *INSTALLED
    }

    /// Stands in for `D3D11CreateDevice` in this executable: a device on a graphics card is made
    /// without the driver's threads, or as asked if the driver won't; any other as asked. GPUI's
    /// goes to `gpu_memory` once made.
    unsafe extern "system" fn create_device(
        adapter: *mut c_void,
        driver: D3D_DRIVER_TYPE,
        software: HMODULE,
        flags: D3D11_CREATE_DEVICE_FLAG,
        levels: *const D3D_FEATURE_LEVEL,
        level_count: u32,
        sdk_version: u32,
        device: *mut *mut c_void,
        level: *mut D3D_FEATURE_LEVEL,
        context: *mut *mut c_void,
    ) -> HRESULT {
        // SAFETY: stored before the slot was pointed here: d3d11's `D3D11CreateDevice`, which has
        // this signature.
        let original = unsafe {
            std::mem::transmute::<usize, CreateDeviceFn>(ORIGINAL.load(Ordering::Acquire))
        };
        // SAFETY: the caller's arguments, only the flags changed.
        let make = |flags: u32| unsafe {
            original(
                adapter,
                driver,
                software,
                D3D11_CREATE_DEVICE_FLAG(flags),
                levels,
                level_count,
                sdk_version,
                device,
                level,
                context,
            )
        };
        let without_threads = if *LEAVE_THREADS {
            None
        } else {
            first_try(flags.0, driver.0, vendor(adapter))
        };
        let made = match without_threads.map(make) {
            None => make(flags.0),
            Some(made) if made.is_ok() => {
                if ANNOUNCED.swap(true, Ordering::Relaxed) {
                    log::debug!(
                        "d3d threading: a device made without the graphics driver's threads"
                    );
                } else {
                    log::info!(
                        "d3d threading: a device made without the graphics driver's threads"
                    );
                }
                made
            }
            Some(made) => {
                log::warn!(
                    "d3d threading: the graphics driver made no device without its threads \
                     ({made}: {}); made as asked",
                    made.message().trim_end()
                );
                make(flags.0)
            }
        };
        if made.is_ok() && !device.is_null() {
            // SAFETY: the caller asked for the device (`device` isn't null) and `D3D11CreateDevice`
            // made it: `*device` holds it, a reference the caller owns.
            if let Some(made_device) = unsafe { ID3D11Device::from_raw_borrowed(&*device) } {
                gpu_memory::note_device(made_device);
            }
        }
        made
    }

    /// The PCI vendor of `adapter`, an `IDXGIAdapter` or null: `None` if null, or if its
    /// description can't be read.
    fn vendor(adapter: *mut c_void) -> Option<u32> {
        // SAFETY: the adapter `D3D11CreateDevice` was called with, which its caller holds for the
        // call.
        let adapter = unsafe { IDXGIAdapter::from_raw_borrowed(&adapter) }?;
        unsafe { adapter.GetDesc() }.ok().map(|desc| desc.VendorId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `D3D11_CREATE_DEVICE_BGRA_SUPPORT`, which GPUI asks for, and `D3D11_CREATE_DEVICE_DEBUG`,
    /// which its debug build adds when the SDK layers are there.
    const BGRA: u32 = 0x20;
    const DEBUG: u32 = 0x2;
    /// `D3D_DRIVER_TYPE_REFERENCE`, `_NULL`, `_SOFTWARE` and `_WARP`.
    const SOFTWARE_DRIVERS: [i32; 4] = [2, 3, 4, 5];
    const NVIDIA: u32 = 0x10DE;

    #[test]
    fn a_device_on_a_graphics_card_is_made_without_the_drivers_threads_first() {
        // GPUI's device, on the adapter it picked.
        assert_eq!(
            first_try(BGRA, DRIVER_UNKNOWN, Some(NVIDIA)),
            Some(BGRA | PREVENT_THREADING)
        );
        // The lip watcher's, on the game's monitor's adapter.
        assert_eq!(
            first_try(0, DRIVER_UNKNOWN, Some(NVIDIA)),
            Some(PREVENT_THREADING)
        );
        // On the default adapter, with the debug layer.
        assert_eq!(
            first_try(BGRA | DEBUG, DRIVER_HARDWARE, None),
            Some(BGRA | DEBUG | PREVENT_THREADING)
        );
    }

    #[test]
    fn a_software_device_keeps_its_threads() {
        // WARP behind an adapter: a machine without a graphics card, or a remote session.
        assert_eq!(first_try(BGRA, DRIVER_UNKNOWN, Some(MICROSOFT)), None);
        for driver in SOFTWARE_DRIVERS {
            assert_eq!(first_try(BGRA, driver, None), None);
        }
    }

    #[test]
    fn a_device_asked_without_the_drivers_threads_is_made_as_asked() {
        assert_eq!(
            first_try(BGRA | PREVENT_THREADING, DRIVER_UNKNOWN, Some(NVIDIA)),
            None
        );
    }

    #[test]
    fn only_1_leaves_the_driver_its_threads() {
        assert!(leaves_threads(Some(OsStr::new("1"))));
        for value in [None, Some(""), Some("0")] {
            assert!(!leaves_threads(value.map(OsStr::new)));
        }
    }
}
