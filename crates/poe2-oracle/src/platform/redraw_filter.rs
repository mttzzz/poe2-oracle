//! Keeps `gpui_windows` from waking the UI thread for windows with nothing new to show.
//!
//! `gpui_windows` invalidates every window of the app on each refresh of the display: its vsync
//! thread (`platform.rs`'s `begin_vsync_thread`) calls `RedrawWindow(hwnd, None, None,
//! RDW_INVALIDATE)` for each, 60 to 165 times a second, and each call wakes the UI thread for a
//! `WM_PAINT`. A window whose paints [`Win32Overlay::gate_paints`] gates swallows that paint
//! outside its bursts (`win32::overlay_proc`), but the wake-ups alone kept the UI thread busy for
//! the XP overlay's plates, which are up the whole time the game is played and change their words
//! every few seconds. Measured 2026-09-24 on the test machine, idle in the hideout for 30 s: 60
//! paints a second for each plate, the UI thread woken 158 times a second for 143 ms of CPU;
//! with this filter, 2 to 3 paints a second each, 12 wake-ups a second and 12 ms.
//!
//! So this executable's own import of `RedrawWindow` -- its slot in the import address table,
//! which every call from the executable goes through, `gpui_windows`' included -- is pointed at
//! [`filtered_redraw_window`]: it drops exactly that call for a gated window whose next paint
//! isn't due, and hands every other call to user32's. Nothing outside this process changes. If the
//! import isn't there (another linker laid the imports out otherwise), the gate works alone, at
//! the wake-ups' cost.
//!
//! [`install`] runs before GPUI starts. The vsync thread reads the slot once, before its loop --
//! the release build keeps the function's address in a register from then on
//! (`mov r13, [__imp_RedrawWindow]`, its only read of the slot, seen in the 2026-09-24 build) --
//! so a slot changed after it started never reaches it.
//!
//! [`Win32Overlay::gate_paints`]: crate::platform::win32::Win32Overlay::gate_paints

use std::ffi::{CStr, c_char};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context as _, Result, bail, ensure};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{HRGN, RDW_INVALIDATE, REDRAW_WINDOW_FLAGS};
use windows::Win32::System::Diagnostics::Debug::{
    IMAGE_DIRECTORY_ENTRY_IMPORT, IMAGE_NT_HEADERS64, IMAGE_NT_OPTIONAL_HDR64_MAGIC,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{PAGE_PROTECTION_FLAGS, PAGE_READWRITE, VirtualProtect};
use windows::Win32::System::SystemServices::{
    IMAGE_DOS_HEADER, IMAGE_DOS_SIGNATURE, IMAGE_IMPORT_BY_NAME, IMAGE_IMPORT_DESCRIPTOR,
    IMAGE_NT_SIGNATURE, IMAGE_ORDINAL_FLAG64,
};
use windows::core::{BOOL, PCWSTR};

use crate::platform::win32;

type RedrawWindowFn =
    unsafe extern "system" fn(HWND, *const RECT, HRGN, REDRAW_WINDOW_FLAGS) -> BOOL;

/// user32's `RedrawWindow`: the import slot's value before [`filtered_redraw_window`] took it.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// Whether the filter is in: tried once, on the first call of [`install`].
static INSTALLED: LazyLock<bool> = LazyLock::new(|| match point_import_at_filter() {
    Ok(slots) => {
        log::info!("redraw filter: {slots} RedrawWindow import(s) filtered");
        true
    }
    Err(err) => {
        log::warn!("redraw filter: {err:#}; gated windows still wake the UI thread each refresh");
        false
    }
});

/// Filters the display refreshes' invalidations of gated windows from now on; whether it could.
/// Call before GPUI starts (see the module's doc).
pub fn install() -> bool {
    *INSTALLED
}

/// Stands in for `RedrawWindow` in this executable: `gpui_windows`' per-refresh invalidation --
/// the whole window, nothing else asked for -- of a gated window whose paint isn't due is dropped
/// as done; anything else goes to user32.
unsafe extern "system" fn filtered_redraw_window(
    hwnd: HWND,
    update: *const RECT,
    region: HRGN,
    flags: REDRAW_WINDOW_FLAGS,
) -> BOOL {
    if flags == RDW_INVALIDATE && update.is_null() && region.is_invalid() && !win32::paint_due(hwnd)
    {
        return BOOL::from(true);
    }
    // SAFETY: stored before the slot was pointed here: user32's `RedrawWindow`, which has this
    // signature.
    let original =
        unsafe { std::mem::transmute::<usize, RedrawWindowFn>(ORIGINAL.load(Ordering::Acquire)) };
    unsafe { original(hwnd, update, region, flags) }
}

/// Points every `RedrawWindow` slot among this executable's user32 imports at
/// [`filtered_redraw_window`]; how many there were.
fn point_import_at_filter() -> Result<usize> {
    let module = unsafe { GetModuleHandleW(PCWSTR::null()) }.context("GetModuleHandleW")?;
    let base = module.0 as *const u8;
    // SAFETY: `base` is this executable's image as the loader mapped it: a DOS header, the NT
    // headers `e_lfanew` bytes in, and every RVA below within the image. The headers are read
    // unaligned: the DOS header's layout is packed.
    let slots = unsafe {
        let dos = base.cast::<IMAGE_DOS_HEADER>().read_unaligned();
        ensure!(dos.e_magic == IMAGE_DOS_SIGNATURE, "no DOS header");
        let nt = base
            .offset(dos.e_lfanew as isize)
            .cast::<IMAGE_NT_HEADERS64>()
            .read_unaligned();
        ensure!(
            nt.Signature == IMAGE_NT_SIGNATURE
                && nt.OptionalHeader.Magic == IMAGE_NT_OPTIONAL_HDR64_MAGIC,
            "no 64-bit NT headers"
        );
        let imports = nt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT.0 as usize];
        ensure!(imports.VirtualAddress != 0, "no import directory");
        let mut descriptor = base
            .add(imports.VirtualAddress as usize)
            .cast::<IMAGE_IMPORT_DESCRIPTOR>();
        let mut slots = Vec::new();
        loop {
            let entry = descriptor.read_unaligned();
            if entry.Name == 0 {
                break;
            }
            descriptor = descriptor.add(1);
            let dll = CStr::from_ptr(base.add(entry.Name as usize).cast::<c_char>());
            let names = entry.Anonymous.OriginalFirstThunk;
            if !dll.to_bytes().eq_ignore_ascii_case(b"user32.dll") || names == 0 {
                continue;
            }
            // The lookup table names each import; the address table beside it holds, slot for
            // slot, where the loader found it.
            let names = base.add(names as usize).cast::<u64>();
            let addresses = base
                .add(entry.FirstThunk as usize)
                .cast::<usize>()
                .cast_mut();
            for index in 0.. {
                let name = names.add(index).read_unaligned();
                if name == 0 {
                    break;
                }
                if name & IMAGE_ORDINAL_FLAG64 != 0 {
                    continue;
                }
                let by_name = base
                    .add((name & 0x7FFF_FFFF) as usize)
                    .cast::<IMAGE_IMPORT_BY_NAME>();
                let function = CStr::from_ptr(std::ptr::addr_of!((*by_name).Name).cast::<c_char>());
                if function.to_bytes() == b"RedrawWindow" {
                    slots.push(addresses.add(index));
                }
            }
        }
        slots
    };
    if slots.is_empty() {
        bail!("this executable imports no RedrawWindow from user32");
    }
    let filter: RedrawWindowFn = filtered_redraw_window;
    for &slot in &slots {
        // SAFETY: an address table slot of this image, a pointer-sized, pointer-aligned word the
        // loader filled; made writable for the one write, then protected as it was.
        unsafe {
            let current = slot.read();
            if current == filter as usize {
                continue;
            }
            let mut was = PAGE_PROTECTION_FLAGS::default();
            VirtualProtect(slot.cast(), size_of::<usize>(), PAGE_READWRITE, &mut was)
                .context("VirtualProtect")?;
            ORIGINAL.store(current, Ordering::Release);
            slot.write(filter as usize);
            let mut unused = PAGE_PROTECTION_FLAGS::default();
            VirtualProtect(slot.cast(), size_of::<usize>(), was, &mut unused)
                .context("VirtualProtect back")?;
        }
    }
    Ok(slots.len())
}
