//! The app's log. Everything the app reports goes through `log`'s macros into `poe2-oracle.log` in
//! `paths::logs_dir` -- started fresh each launch, with the previous run kept beside it as
//! `poe2-oracle.previous.log` -- which the diagnostics report (`diagnostics`) sends along. While
//! stderr is attached (a console, or a script redirecting it) every line goes there too.
//!
//! Levels: this app's info and up -- its trade client's too, whose info lines are the trade API
//! rate-limit audit trail -- and everyone else's warnings; `RUST_LOG`, when set, replaces them.
//! A panic is logged before the default hook runs: without a console, the log is the only place
//! it can be seen.

use std::fs::{self, File};
use std::io::{self, Write};

use log::LevelFilter;
use windows::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE};

use crate::paths;

/// This run's log, in `paths::logs_dir`.
pub const LOG_FILE: &str = "poe2-oracle.log";
/// The run before this one's.
pub const PREVIOUS_LOG_FILE: &str = "poe2-oracle.previous.log";

/// Installs the logger and the panic hook. Call once, before anything logs -- whatever is logged
/// before it is lost -- and only in the copy that stays (`platform::instance`): it starts a new
/// log file.
pub fn init() {
    let sink = Tee {
        file: open_log_file(),
        stderr: stderr_attached().then(io::stderr),
    };
    let mut builder = env_logger::Builder::new();
    match std::env::var("RUST_LOG") {
        Ok(filters) => builder.parse_filters(&filters),
        Err(_) => builder
            .filter_level(LevelFilter::Warn)
            .filter_module("poe2_oracle", LevelFilter::Info)
            .filter_module("trade_client", LevelFilter::Info),
    };
    // Milliseconds: what a price check spends its time on shows only at that grain.
    builder
        .format_timestamp_millis()
        .write_style(env_logger::WriteStyle::Never)
        .target(env_logger::Target::Pipe(Box::new(sink)))
        .init();

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("{info}\n{}", std::backtrace::Backtrace::force_capture());
        default_hook(info);
    }));
    log::info!("PoE2 Oracle {} started", env!("CARGO_PKG_VERSION"));
}

/// This run's log file, with the last run's moved aside. `None` if the folder can't be written:
/// the app runs on, logging only to stderr if it has one.
fn open_log_file() -> Option<File> {
    let dir = paths::logs_dir();
    fs::create_dir_all(&dir).ok()?;
    let current = dir.join(LOG_FILE);
    // Replaces the older previous log; fails harmlessly on the first run.
    let _ = fs::rename(&current, dir.join(PREVIOUS_LOG_FILE));
    File::create(current).ok()
}

/// A GUI-subsystem process has no stderr unless whoever started it redirected one.
fn stderr_attached() -> bool {
    unsafe { GetStdHandle(STD_ERROR_HANDLE) }.is_ok_and(|handle| !handle.is_invalid())
}

/// The log file and stderr, each optional. Writes never fail: a full disk must not take the app
/// down with it.
struct Tee {
    file: Option<File>,
    stderr: Option<io::Stderr>,
}

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(file) = &mut self.file {
            let _ = file.write_all(buf);
        }
        if let Some(stderr) = &mut self.stderr {
            let _ = stderr.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = &mut self.file {
            let _ = file.flush();
        }
        if let Some(stderr) = &mut self.stderr {
            let _ = stderr.flush();
        }
        Ok(())
    }
}
