// Windows-only for now: this project ships to Windows exclusively (PoE2 itself is Windows-only
// in practice for this workstation's testing setup), so there is no Linux platform module to
// maintain. `#[cfg]`-gated (not unconditional) so an accidental native `cargo check`/`build`
// without `--target x86_64-pc-windows-gnu` doesn't try to pull in and compile the Windows-only
// `windows` crate against a non-Windows host.
#[cfg(target_os = "windows")]
pub mod win32;
