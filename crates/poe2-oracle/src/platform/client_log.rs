//! Tail-follows the game's `Client.txt`. What the lines mean is the caller's: the XP overlay reads
//! them with `crate::xp_tracker::parse_log_line`, the trade overlay with
//! `crate::trade_requests::parse_chat_line` -- each through its own `ClientLog`.
//!
//! The log lives in `logs\` beside the game's executable -- on the test machine
//! `D:\SteamLibrary\steamapps\common\Path of Exile 2\logs\Client.txt`, next to
//! `PathOfExileSteam.exe` (verified live 2026-09-22) -- so it is found from the running game's
//! process instead of from a list of default install paths, EE2's `GameLogWatcher` approach, which
//! misses a Steam library on another drive like this one.
//!
//! The file is UTF-8 with `\r\n` line endings and is only ever appended to (158 MB on the test
//! machine, growing ~50 KB per hour of play). It is reopened for every poll rather than held open,
//! so nothing stops the player from deleting it or the game from recreating it.

use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::core::PWSTR;

use crate::platform::game_window;

/// How much of the log's end [`ClientLog::open`] replays for the XP tracker: ~20 hours of play at
/// the test machine's rate, enough to reach back to the last login.
pub const HISTORY_BYTES: u64 = 1 << 20;

/// Set to a file's path to read that file instead of the running game's log: a log the lines of
/// a test are appended to by hand (the game's own must not get fake whispers other tools read).
pub const LOG_PATH_ENV: &str = "POE2_ORACLE_CLIENT_LOG";

pub struct ClientLog {
    path: PathBuf,
    /// Bytes consumed so far.
    offset: u64,
    /// The start of a line the game hasn't finished writing.
    partial: Vec<u8>,
}

impl ClientLog {
    /// Finds the running game's log and opens it at its end, together with what `parse` reads in
    /// its last `history` bytes (none for 0). `None` while the game isn't running or its log can't
    /// be read.
    pub fn open<T>(history: u64, parse: impl Fn(&str) -> Option<T>) -> Option<(Self, Vec<T>)> {
        let path = std::env::var_os(LOG_PATH_ENV)
            .map(PathBuf::from)
            .or_else(game_log_path)?;
        let mut file = File::open(&path).ok()?;
        let end = file.metadata().ok()?.len();
        let start = end.saturating_sub(history);
        let mut replayed = Vec::new();
        file.seek(SeekFrom::Start(start)).ok()?;
        file.take(end - start).read_to_end(&mut replayed).ok()?;
        // Reading from the middle of the file starts mid-line.
        let first_line = if start == 0 {
            0
        } else {
            replayed
                .iter()
                .position(|&byte| byte == b'\n')
                .map_or(replayed.len(), |newline| newline + 1)
        };
        let events = parse_lines(&replayed[first_line..], &parse).collect();
        Some((
            Self {
                path,
                offset: end,
                partial: Vec::new(),
            },
            events,
        ))
    }

    /// What `parse` reads in the lines the game has completed since the last poll; empty when
    /// there are none or the log can't be read right now.
    pub fn poll<T>(&mut self, parse: impl Fn(&str) -> Option<T>) -> Vec<T> {
        let Ok(mut file) = File::open(&self.path) else {
            return Vec::new();
        };
        let Ok(len) = file.metadata().map(|metadata| metadata.len()) else {
            return Vec::new();
        };
        if len < self.offset {
            // Deleted and recreated: the new log starts from scratch.
            self.offset = 0;
            self.partial.clear();
        }
        let mut fresh = Vec::new();
        if len == self.offset
            || file.seek(SeekFrom::Start(self.offset)).is_err()
            || file
                .take(len - self.offset)
                .read_to_end(&mut fresh)
                .is_err()
        {
            return Vec::new();
        }
        self.offset += fresh.len() as u64;
        self.partial.extend_from_slice(&fresh);
        let complete = self
            .partial
            .iter()
            .rposition(|&byte| byte == b'\n')
            .map_or(0, |newline| newline + 1);
        let unfinished = self.partial.split_off(complete);
        let events = parse_lines(&self.partial, &parse).collect();
        self.partial = unfinished;
        events
    }
}

fn parse_lines<'a, T>(
    bytes: &'a [u8],
    parse: &'a impl Fn(&str) -> Option<T>,
) -> impl Iterator<Item = T> + 'a {
    bytes
        .split(|&byte| byte == b'\n')
        .filter_map(|line| std::str::from_utf8(line).ok())
        .filter_map(move |line| parse(line.trim_end_matches('\r')))
}

/// `logs\Client.txt` beside the executable of the process that owns the game window.
fn game_log_path() -> Option<PathBuf> {
    let hwnd = game_window::game_window()?;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    let queried = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    queried.ok()?;
    let exe = PathBuf::from(OsString::from_wide(&buffer[..len as usize]));
    Some(exe.parent()?.join("logs").join("Client.txt"))
}
