//! The PoE2 Oracle executable. Not a console app: it's an overlay launched detached while the
//! player plays, and a console window popping up alongside it would be a visible bug.
#![windows_subsystem = "windows"]

fn main() {
    #[cfg(target_os = "windows")]
    poe2_oracle::app::run();
    #[cfg(not(target_os = "windows"))]
    eprintln!("PoE2 Oracle runs on Windows only.");
}
