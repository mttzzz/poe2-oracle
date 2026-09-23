//! The report the player sends with a bug: one zip on their desktop holding this run's and the
//! previous run's log, the settings, the kept item texts and a summary of the app and the system.
//! The player's user folder is masked as `%USERPROFILE%` in every file: it's in each path the logs
//! mention, and its last part is their Windows user name.

use std::fs::{self, File};
use std::io::Write as _;
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context as _, Result};
use windows::Win32::Foundation::{ERROR_SUCCESS, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::{BOOL, PCWSTR, w};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::logging::{LOG_FILE, PREVIOUS_LOG_FILE};
use crate::paths;
use crate::platform::game_config::{self, DisplayMode, GameConfig};
use crate::platform::{game_window, synth_input};

/// Writes the report and returns where it went: the desktop, or the log folder without one.
/// `app_summary` is the app's own state (league, catalogs, hotkey, ...), gathered by the caller
/// on the thread that owns it; the system half is gathered here.
pub fn write_report(app_summary: &str) -> Result<PathBuf> {
    let now = LocalTime::now();
    let dir = directories::UserDirs::new()
        .and_then(|dirs| dirs.desktop_dir().map(Path::to_path_buf))
        .unwrap_or_else(paths::logs_dir);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(format!("PoE2-Oracle-report-{}.zip", now.for_file_name()));

    let masker = Masker::from_env();
    let file = File::create(&path).with_context(|| format!("creating {}", path.display()))?;
    let mut zip = ZipWriter::new(file);
    let summary = format!(
        "PoE2 Oracle {} -- report of {}\n\n{app_summary}\n{}",
        env!("CARGO_PKG_VERSION"),
        now.for_humans(),
        system_summary()
    );
    add_text(&mut zip, "summary.txt", &masker.mask(&summary))?;
    for name in [LOG_FILE, PREVIOUS_LOG_FILE] {
        if let Ok(bytes) = fs::read(paths::logs_dir().join(name)) {
            let text = masker.mask(&String::from_utf8_lossy(&bytes));
            add_text(&mut zip, &format!("logs/{name}"), &text)?;
        }
    }
    if let Some(settings) = paths::settings_file()
        && let Ok(text) = fs::read_to_string(settings)
    {
        add_text(&mut zip, "settings.json", &masker.mask(&text))?;
    }
    for entry in fs::read_dir(paths::unparsed_dir())
        .into_iter()
        .flatten()
        .flatten()
    {
        if let Ok(text) = fs::read_to_string(entry.path()) {
            let name = entry.file_name().to_string_lossy().into_owned();
            add_text(&mut zip, &format!("unparsed/{name}"), &masker.mask(&text))?;
        }
    }
    zip.finish().context("finishing the report zip")?;
    Ok(path)
}

/// Opens an Explorer window with `path` selected.
pub fn reveal(path: &Path) {
    // Raw: Explorer wants the quotes around the path only, not around the whole switch.
    let spawned = Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn();
    if let Err(err) = spawned {
        log::warn!("opening Explorer at {} failed: {err}", path.display());
    }
}

/// Opens the log folder in Explorer.
pub fn open_logs_folder() {
    let dir = paths::logs_dir();
    if let Err(err) = Command::new("explorer.exe").arg(&dir).spawn() {
        log::warn!("opening {} failed: {err}", dir.display());
    }
}

fn add_text(zip: &mut ZipWriter<File>, name: &str, text: &str) -> Result<()> {
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(name, options)
        .with_context(|| format!("adding {name} to the report"))?;
    zip.write_all(text.as_bytes())
        .with_context(|| format!("writing {name} into the report"))
}

/// Replaces the player's user folder with `%USERPROFILE%`.
struct Masker {
    profile: Option<String>,
}

impl Masker {
    fn from_env() -> Masker {
        Masker {
            profile: std::env::var("USERPROFILE")
                .ok()
                .filter(|profile| !profile.is_empty()),
        }
    }

    fn mask(&self, text: &str) -> String {
        match &self.profile {
            Some(profile) => text.replace(profile.as_str(), "%USERPROFILE%"),
            None => text.to_owned(),
        }
    }
}

/// What in the player's setup keeps checks from working, worded for the player: the settings
/// window lists these at its top, and the report carries them too.
pub fn setup_problems(config: &GameConfig) -> Vec<String> {
    let mut problems = Vec::new();
    if config.display_mode == Some(DisplayMode::Fullscreen) {
        problems.push(
            "Игра в режиме «Полноэкранный»: поверх него панель не видна. Выберите в настройках \
             графики игры режим «Оконный полноэкранный»."
                .to_owned(),
        );
    }
    let copy_key = VIRTUAL_KEY(config.advanced_mod_desc_key);
    if synth_input::copy_combo_taken(copy_key) {
        problems.push(format!(
            "Сочетание {}, которым игра копирует предмет, занято другой программой — проверка \
             цены не сработает, пока его не освободить (чаще всего это оверлей видеокарты, запись \
             экрана или Discord).",
            synth_input::copy_combo_label(copy_key)
        ));
    }
    problems
}

/// The system half of the summary: Windows, the monitors, the game and its settings, the
/// cached files.
fn system_summary() -> String {
    let mut out = String::from("[system]\n");
    out += &format!("windows: {}\n", windows_version());
    for monitor in monitors() {
        out += &format!("monitor: {monitor}\n");
    }
    match game_window::game_window() {
        Some(hwnd) => {
            let dpi = unsafe { GetDpiForWindow(hwnd) };
            match game_window::client_rect_on_screen(hwnd) {
                Some(rect) => {
                    out += &format!(
                        "game window: {}x{} at ({}, {}), {dpi} dpi\n",
                        rect.width, rect.height, rect.x, rect.y
                    );
                }
                None => out += "game window: found, client area unreadable\n",
            }
        }
        None => out += "game window: not found\n",
    }
    let config = game_config::read();
    let config_found = game_config::config_path().is_some_and(|path| path.exists());
    out += &format!(
        "game settings: {} -- display {:?}, language {}, advanced descriptions key {}\n",
        if config_found { "found" } else { "not found" },
        config.display_mode,
        config.language.as_deref().unwrap_or("?"),
        config.advanced_mod_desc_key
    );
    for problem in setup_problems(&config) {
        out += &format!("problem: {problem}\n");
    }
    out += &format!("\n[cache: {}]\n", paths::cache_dir().display());
    let mut files: Vec<_> = fs::read_dir(paths::cache_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            meta.is_file().then(|| {
                let age = meta
                    .modified()
                    .ok()
                    .and_then(|at| SystemTime::now().duration_since(at).ok())
                    .map_or_else(
                        || "?".to_owned(),
                        |age| format!("{} min", age.as_secs() / 60),
                    );
                format!(
                    "{}: {} KB, written {age} ago",
                    entry.file_name().to_string_lossy(),
                    meta.len() / 1024
                )
            })
        })
        .collect();
    files.sort();
    for file in files {
        out += &file;
        out.push('\n');
    }
    out
}

/// "Windows 10 Pro 24H2 (build 26200.6899)" -- Windows 11 still names itself "Windows 10" in
/// `ProductName`; the build (22000 and up) is what tells them apart.
fn windows_version() -> String {
    let key = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let product = registry_string(key, w!("ProductName")).unwrap_or_else(|| "Windows".into());
    let display = registry_string(key, w!("DisplayVersion")).unwrap_or_default();
    let build = registry_string(key, w!("CurrentBuild")).unwrap_or_else(|| "?".into());
    let revision = registry_dword(key, w!("UBR")).map_or_else(String::new, |ubr| format!(".{ubr}"));
    format!("{product} {display} (build {build}{revision})")
}

fn registry_string(key: PCWSTR, value: PCWSTR) -> Option<String> {
    let mut buffer = [0u16; 256];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    read_registry(
        key,
        value,
        RRF_RT_REG_SZ,
        buffer.as_mut_ptr().cast(),
        &mut size,
    )?;
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buffer[..len]))
}

fn registry_dword(key: PCWSTR, value: PCWSTR) -> Option<u32> {
    let mut data = 0u32;
    let mut size = 4u32;
    read_registry(
        key,
        value,
        RRF_RT_REG_DWORD,
        (&raw mut data).cast(),
        &mut size,
    )?;
    Some(data)
}

fn read_registry(
    key: PCWSTR,
    value: PCWSTR,
    flags: REG_ROUTINE_FLAGS,
    buffer: *mut std::ffi::c_void,
    size: &mut u32,
) -> Option<()> {
    // SAFETY: `buffer` points at `*size` writable bytes, and both names are NUL-terminated.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key,
            value,
            flags,
            None,
            Some(buffer),
            Some(size),
        )
    };
    (status == ERROR_SUCCESS).then_some(())
}

/// Each monitor as "3840x2160 at (0, 0), 192 dpi, primary".
fn monitors() -> Vec<String> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        list: LPARAM,
    ) -> BOOL {
        // SAFETY: `list` is the `Vec` `monitors` passed in, alive for the whole enumeration.
        let list = unsafe { &mut *(list.0 as *mut Vec<String>) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            let r = info.rcMonitor;
            let (mut dpi, mut _dpi_y) = (0u32, 0u32);
            let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _dpi_y) };
            let primary = if info.dwFlags & MONITORINFOF_PRIMARY != 0 {
                ", primary"
            } else {
                ""
            };
            list.push(format!(
                "{}x{} at ({}, {}), {dpi} dpi{primary}",
                r.right - r.left,
                r.bottom - r.top,
                r.left,
                r.top
            ));
        }
        true.into()
    }
    let mut list: Vec<String> = Vec::new();
    // SAFETY: `collect` only touches `list` through the pointer, during this call.
    let _ =
        unsafe { EnumDisplayMonitors(None, None, Some(collect), LPARAM((&raw mut list) as isize)) };
    list
}

/// The local wall-clock time, for the report's name and header.
struct LocalTime {
    year: u16,
    month: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
}

impl LocalTime {
    fn now() -> LocalTime {
        let time = unsafe { GetLocalTime() };
        LocalTime {
            year: time.wYear,
            month: time.wMonth,
            day: time.wDay,
            hour: time.wHour,
            minute: time.wMinute,
            second: time.wSecond,
        }
    }

    fn for_file_name(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    fn for_humans(&self) -> String {
        format!(
            "{:02}.{:02}.{:04} {:02}:{:02}:{:02}",
            self.day, self.month, self.year, self.hour, self.minute, self.second
        )
    }
}
