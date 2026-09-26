// The PoE2 Oracle application: `main.rs` runs `app::run`.

pub mod bound_input;
pub mod brand;
pub mod craft_link;
pub mod i18n;
pub mod item_refs;
pub mod league_chip;
pub mod listing_match;
pub mod overlay_layout;
pub mod paths;
pub mod plate_art;
pub mod platform;
pub mod quick_action;
pub mod relative_time;
pub mod roll_slider;
pub mod session;
pub mod settings;
pub mod text_area;
pub mod tour;
pub mod xp_tracker;
// The app side of reporting, whose rules its tests check on every target; only Windows sends.
#[cfg(any(target_os = "windows", test))]
pub mod report;
// Windows-only, like `platform`'s own native submodules: they transitively depend on
// `platform::{game_config, synth_input, clipboard_poll}`, which only exist on that target (see
// `platform/mod.rs`). Gating here keeps `cargo build`/`clippy`/`test -p poe2-oracle --lib`
// resolving cleanly on this project's native (Linux) CI pass -- only the windows-gnu cross
// passes ever compile these.
#[cfg(target_os = "windows")]
pub mod app;
#[cfg(target_os = "windows")]
pub mod diagnostics;
#[cfg(target_os = "windows")]
pub mod game_chat;
#[cfg(target_os = "windows")]
pub mod logging;
#[cfg(target_os = "windows")]
pub mod login;
#[cfg(target_os = "windows")]
pub mod price_check;
#[cfg(target_os = "windows")]
pub mod ui;
#[cfg(target_os = "windows")]
pub mod updates;
