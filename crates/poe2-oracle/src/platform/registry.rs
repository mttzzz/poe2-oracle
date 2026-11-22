//! Values in the current user's part of the registry (`HKCU`), read whatever their size: the
//! "Start with Windows" entry (`autostart`) and the proxy Windows is set to use
//! (`http_clients::ProxySetting`).

use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::core::PCWSTR;

/// The data of the value `name` under `HKCU\<subkey>`; `None` if it's missing, not of the type
/// `flags` asks for, or unreadable.
pub fn user_value(subkey: PCWSTR, name: PCWSTR, flags: REG_ROUTINE_FLAGS) -> Option<Vec<u8>> {
    let mut data: Vec<u8> = Vec::new();
    loop {
        let mut size = u32::try_from(data.len()).ok()?;
        let buffer = (!data.is_empty()).then(|| data.as_mut_ptr().cast());
        // SAFETY: `buffer`, when present, points at `size` writable bytes; without one the call
        // only reports the size the data needs.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey,
                name,
                flags,
                None,
                buffer,
                Some(&mut size),
            )
        };
        match status {
            ERROR_SUCCESS if buffer.is_some() || size == 0 => {
                data.truncate(size as usize);
                return Some(data);
            }
            // Only the size so far -- or the value grew since it was measured.
            ERROR_SUCCESS | ERROR_MORE_DATA => data.resize(size as usize, 0),
            _ => return None,
        }
    }
}

/// The `REG_SZ` value `name` under `HKCU\<subkey>`, up to its terminator.
pub fn user_string(subkey: PCWSTR, name: PCWSTR) -> Option<String> {
    let data = user_value(subkey, name, RRF_RT_REG_SZ)?;
    let (units, _) = data.as_chunks::<2>();
    let units: Vec<u16> = units
        .iter()
        .map(|&unit| u16::from_le_bytes(unit))
        .take_while(|&unit| unit != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// The `REG_DWORD` value `name` under `HKCU\<subkey>`.
pub fn user_dword(subkey: PCWSTR, name: PCWSTR) -> Option<u32> {
    let data = user_value(subkey, name, RRF_RT_REG_DWORD)?;
    Some(u32::from_le_bytes(data.try_into().ok()?))
}
