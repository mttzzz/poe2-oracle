//! Secrets in the Windows Credential Manager: one generic credential per target name, which
//! Windows keeps encrypted for the signed-in user and lists in the Credential Manager control panel
//! under "Windows Credentials > Generic Credentials", where the player can see and delete it. The
//! app keeps its pathofexile.com session there (`crate::session`), never in its settings file.
//!
//! The secret is stored as UTF-16, as the control panel stores the passwords it writes, and
//! persisted for this user on this machine (`CRED_PERSIST_LOCAL_MACHINE`: it survives sign-out and
//! reboots, and never roams to another PC).

use anyhow::{Context as _, Result, ensure};
use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{
    CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW,
    CredDeleteW, CredFree, CredReadW, CredWriteW,
};
use windows::core::{PCWSTR, PWSTR};

/// The secret stored under `target`; `None` if there is none.
pub fn read(target: &str) -> Result<Option<String>> {
    let target = wide(target);
    let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: `target` is NUL-terminated and outlives the call; on success `credential` points at
    // a block the system allocated, released below with `CredFree`.
    match unsafe {
        CredReadW(
            PCWSTR(target.as_ptr()),
            CRED_TYPE_GENERIC,
            None,
            &mut credential,
        )
    } {
        Ok(()) => {}
        Err(err) if err.code() == ERROR_NOT_FOUND.to_hresult() => return Ok(None),
        Err(err) => return Err(err).context("reading the credential"),
    }
    // SAFETY: `CredReadW` succeeded, so `credential` is valid until `CredFree`, and its blob, when
    // it has one, is `CredentialBlobSize` readable bytes.
    let units: Vec<u16> = unsafe {
        let stored = &*credential;
        let units = if stored.CredentialBlob.is_null() {
            Vec::new()
        } else {
            let blob = std::slice::from_raw_parts(
                stored.CredentialBlob,
                stored.CredentialBlobSize as usize,
            );
            let (units, _) = blob.as_chunks::<2>();
            units.iter().map(|&unit| u16::from_le_bytes(unit)).collect()
        };
        CredFree(credential.cast());
        units
    };
    String::from_utf16(&units)
        .map(Some)
        .context("the stored credential isn't text")
}

/// Stores `secret` under `target` for `user`, replacing whatever was there.
pub fn write(target: &str, user: &str, secret: &str) -> Result<()> {
    let mut target = wide(target);
    let mut user = wide(user);
    let blob: Vec<u8> = secret.encode_utf16().flat_map(u16::to_le_bytes).collect();
    ensure!(
        blob.len() <= CRED_MAX_CREDENTIAL_BLOB_SIZE as usize,
        "the secret is too long for the Credential Manager"
    );
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        UserName: PWSTR(user.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_ptr().cast_mut(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    // SAFETY: every pointer in `credential` points into a buffer that outlives the call, which only
    // reads them.
    unsafe { CredWriteW(&credential, 0) }.context("writing the credential")
}

/// Deletes the credential under `target`; one that isn't there is already deleted.
pub fn delete(target: &str) -> Result<()> {
    let target = wide(target);
    // SAFETY: `target` is NUL-terminated and outlives the call.
    match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(err) if err.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
        Err(err) => Err(err).context("deleting the credential"),
    }
}

/// `text` as NUL-terminated UTF-16.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
