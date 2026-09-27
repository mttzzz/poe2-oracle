//! PoE2 Oracle's mark in the one raster form the running app needs itself: the tray icon. The exe
//! icon (embedded by `build.rs`) and the installer icon use `assets/icon/poe2-oracle.ico`; all of
//! them come from `packaging/icon/generate_icon.py` -- regenerate them, never edit by hand.

/// The User-Agent every request of the app carries: to the trade site and pathofexile.com,
/// poe2scout, the game's picture and exchange server, and oracle.pushka.biz. It names the app and
/// its version, and where to learn about it, as GGG asks of tools that call its sites -- not a
/// browser's, which the app isn't. The same for every player: it says nothing about the player.
pub const USER_AGENT: &str = concat!(
    "PoE2-Oracle/",
    env!("CARGO_PKG_VERSION"),
    " (+https://oracle.pushka.biz)"
);

/// Edge of [`TRAY_ICON_RGBA`] in pixels: the notification area's 16-logical-pixel icon at the
/// owner's 200% display scale, so it is shown 1:1 there; Windows rescales it at other scales.
pub const TRAY_ICON_SIZE: u32 = 32;

/// The mark as straight (non-premultiplied) RGBA8, row-major from the top-left -- exactly what
/// `tray_icon::Icon::from_rgba` takes, so the tray needs no image decoder. The array type makes a
/// regenerated file of the wrong size a compile error rather than a garbled icon.
pub const TRAY_ICON_RGBA: &[u8; (TRAY_ICON_SIZE * TRAY_ICON_SIZE * 4) as usize] =
    include_bytes!("../assets/icon/tray-32.rgba");
