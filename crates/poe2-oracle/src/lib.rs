// The PoE2 Oracle application: `main.rs` runs `app::run`.

pub mod bound_input;
pub mod brand;
pub mod bug_report;
pub mod item_refs;
pub mod league_chip;
pub mod listing_match;
pub mod live_search;
pub mod overlay_layout;
pub mod paths;
pub mod platform;
pub mod quick_action;
pub mod relative_time;
pub mod roll_slider;
pub mod session;
pub mod settings;
pub mod xp_tracker;
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
