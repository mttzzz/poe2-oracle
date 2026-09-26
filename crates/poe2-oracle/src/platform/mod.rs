// Windows-only for now: this project ships to Windows exclusively (PoE2 itself is Windows-only
// in practice for this workstation's testing setup), so there is no Linux platform module to
// maintain. `#[cfg]`-gated (not unconditional) so an accidental native `cargo check`/`build`
// without `--target x86_64-pc-windows-gnu` doesn't try to pull in and compile the Windows-only
// `windows` crate against a non-Windows host. `game_config` is plain file parsing, so it builds
// -- and its tests run -- everywhere.
#[cfg(target_os = "windows")]
pub mod autostart;
#[cfg(target_os = "windows")]
pub mod client_log;
#[cfg(target_os = "windows")]
pub mod clipboard_poll;
#[cfg(target_os = "windows")]
pub mod credentials;
#[cfg(target_os = "windows")]
pub mod esc_hook;
pub mod game_config;
#[cfg(target_os = "windows")]
pub mod game_window;
#[cfg(target_os = "windows")]
pub mod instance;
#[cfg(target_os = "windows")]
pub mod lip_watch;
#[cfg(target_os = "windows")]
pub mod login_window;
#[cfg(target_os = "windows")]
pub mod network;
#[cfg(target_os = "windows")]
pub mod redraw_filter;
#[cfg(target_os = "windows")]
pub mod synth_input;
#[cfg(target_os = "windows")]
pub mod taskbar;
#[cfg(target_os = "windows")]
pub mod win32;
#[cfg(target_os = "windows")]
pub mod xp_bar;
