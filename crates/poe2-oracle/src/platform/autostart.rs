//! "Start with Windows": the per-user `Run` registry value Windows launches at sign-in.
//!
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `PoE2 Oracle` (`REG_SZ`), data
//! `"<exe path>" --autostart` -- the path quoted because it may hold spaces, the shape Steam's
//! entry on the test machine has too (`"...\steam.exe" -silent`, read 2026-09-22). The
//! installer's checkbox and the uninstaller write and delete this same value.
//!
//! Task Manager's Startup tab disables an entry without touching `Run`: it keeps a `REG_BINARY` of
//! the same name under `...\Explorer\StartupApproved\Run`, first byte 02 while enabled and 03 while
//! disabled, then a FILETIME (read 2026-09-22 on the test machine, where the player had disabled
//! Steam, Discord and Overwolf this way; published notes also list 06/07 -- the low bit is the
//! switch). No value there means enabled. Enabling here deletes that value, so an entry the player
//! once disabled in Task Manager really starts again.

use std::os::windows::ffi::OsStrExt;

use anyhow::{Context as _, Result};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RegDeleteKeyValueW, RegSetKeyValueW,
};
use windows::core::{PCWSTR, w};

use crate::launch::AUTOSTART_ARG;
use crate::platform::registry;

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const STARTUP_APPROVED_KEY: PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");
const VALUE_NAME: PCWSTR = w!("PoE2 Oracle");

/// Makes Windows start this executable at sign-in, or stops it from doing so.
pub fn set_autostart(enabled: bool) -> Result<()> {
    if enabled {
        let mut data = run_command()?;
        data.push(0);
        // SAFETY: `data` is a NUL-terminated UTF-16 string that outlives the call, and the size
        // passed is its length in bytes, terminator included, as `REG_SZ` requires.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                VALUE_NAME,
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                u32::try_from(data.len() * 2).context("executable path too long")?,
            )
        }
        .ok()
        .context("writing the Run registry value")?;
    } else {
        delete_value(RUN_KEY).context("deleting the Run registry value")?;
    }
    delete_value(STARTUP_APPROVED_KEY).context("deleting the StartupApproved registry value")
}

/// Whether Windows starts this executable at sign-in: the `Run` value launches it (an entry left
/// by a copy elsewhere doesn't count) and Task Manager hasn't disabled it.
pub fn autostart_enabled() -> bool {
    let (Some(stored), Ok(expected)) = (registry::user_string(RUN_KEY, VALUE_NAME), run_command())
    else {
        return false;
    };
    // Windows paths compare case-insensitively.
    let launches_this = stored.to_lowercase() == String::from_utf16_lossy(&expected).to_lowercase();
    let disabled = registry::user_value(STARTUP_APPROVED_KEY, VALUE_NAME, RRF_RT_REG_BINARY)
        .and_then(|flags| flags.first().copied())
        .is_some_and(|flags| flags & 1 != 0);
    launches_this && !disabled
}

/// `"<this exe>" --autostart` as UTF-16, without a terminator.
fn run_command() -> Result<Vec<u16>> {
    let exe = std::env::current_exe().context("locating the running executable")?;
    let mut command = vec![u16::from(b'"')];
    command.extend(exe.as_os_str().encode_wide());
    command.extend(format!("\" {AUTOSTART_ARG}").encode_utf16());
    Ok(command)
}

/// Deletes this app's value under `HKCU\<subkey>`; one that isn't there (or a missing key) is
/// already deleted.
fn delete_value(subkey: PCWSTR) -> Result<()> {
    // SAFETY: both strings are NUL-terminated constants.
    match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, subkey, VALUE_NAME) } {
        ERROR_FILE_NOT_FOUND => Ok(()),
        status => status.ok().map_err(Into::into),
    }
}
