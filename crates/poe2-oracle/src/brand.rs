//! PoE2 Oracle's mark in the one raster form the running app needs itself: the tray icon. The exe
//! icon (embedded by `build.rs`) and the installer icon use `assets/icon/poe2-oracle.ico`; all of
//! them come from `packaging/icon/generate_icon.py` -- regenerate them, never edit by hand.

/// Edge of [`TRAY_ICON_RGBA`] in pixels: the notification area's 16-logical-pixel icon at the
/// owner's 200% display scale, so it is shown 1:1 there; Windows rescales it at other scales.
pub const TRAY_ICON_SIZE: u32 = 32;

/// The mark as straight (non-premultiplied) RGBA8, row-major from the top-left -- exactly what
/// `tray_icon::Icon::from_rgba` takes, so the tray needs no image decoder. The array type makes a
/// regenerated file of the wrong size a compile error rather than a garbled icon.
pub const TRAY_ICON_RGBA: &[u8; (TRAY_ICON_SIZE * TRAY_ICON_SIZE * 4) as usize] =
    include_bytes!("../assets/icon/tray-32.rgba");
