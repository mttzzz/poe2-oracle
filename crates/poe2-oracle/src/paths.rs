//! Where the app keeps its files, all in `directories`' per-user folders for "poe2-oracle": the
//! settings in the roaming config folder (`%APPDATA%\poe2-oracle\config`), the log, the kept item
//! texts, the last crash and the sign-in window's browser profile in the local data folder
//! (`%LOCALAPPDATA%\poe2-oracle\data`), and whatever can be fetched again -- trade catalogs,
//! exchange and poe2scout prices, downloaded updates -- in the local cache folder
//! (`%LOCALAPPDATA%\poe2-oracle\cache`). Without a home folder (never on a real Windows profile)
//! the data and cache fall back to the temp folder, and settings aren't kept.

use std::path::PathBuf;
use std::sync::LazyLock;

use directories::ProjectDirs;

static DIRS: LazyLock<Option<ProjectDirs>> =
    LazyLock::new(|| ProjectDirs::from("", "", "poe2-oracle"));

/// The player's settings.
pub fn settings_file() -> Option<PathBuf> {
    DIRS.as_ref()
        .map(|dirs| dirs.config_dir().join("settings.json"))
}

/// The trade catalogs and the market prices, kept between runs.
pub fn cache_dir() -> PathBuf {
    DIRS.as_ref()
        .map_or_else(temp_fallback, |dirs| dirs.cache_dir().to_path_buf())
}

/// Installers the updater downloaded.
pub fn updates_dir() -> PathBuf {
    cache_dir().join("updates")
}

/// Item texts the parser rejected or couldn't fully read (and every checked text while
/// `POE2_ORACLE_KEEP_ITEM_TEXTS` is set).
pub fn unparsed_dir() -> PathBuf {
    data_dir().join("unparsed")
}

/// The app's logs (`logging`).
pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

/// What the last panic said (`logging`), until a report of it is sent or dismissed (`report`).
pub fn crash_file() -> PathBuf {
    data_dir().join("crash").join("last-crash.txt")
}

/// The sign-in window's browser profiles (`platform::login_window`), each deleted once its
/// browser has exited.
pub fn login_browser_dir() -> PathBuf {
    data_dir().join("login-browser")
}

fn data_dir() -> PathBuf {
    DIRS.as_ref()
        .map_or_else(temp_fallback, |dirs| dirs.data_local_dir().to_path_buf())
}

fn temp_fallback() -> PathBuf {
    std::env::temp_dir().join("poe2-oracle")
}
