//! Windows HKCU registry adapter. FFI is confined to this module.
use super::install::Browser;
use std::io;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND},
        System::Registry::*,
    },
};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn missing(error: windows::Win32::Foundation::WIN32_ERROR) -> bool {
    error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND
}

pub(super) fn get(browser: Browser) -> io::Result<Option<String>> {
    let key = wide(&browser.registry_key());
    let mut buffer = vec![0u16; 32_768];
    let mut bytes = (buffer.len() * 2) as u32;
    // SAFETY: terminated key, live aligned UTF-16 output buffer, correct byte capacity.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR::null(),
            RRF_RT_REG_SZ | RRF_NOEXPAND,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if missing(status) {
        return Ok(None);
    }
    status.ok().map_err(io::Error::other)?;
    let length = bytes as usize / 2;
    if !bytes.is_multiple_of(2) || length == 0 || length > buffer.len() || buffer[length - 1] != 0 {
        return Err(io::Error::other("invalid registry string"));
    }
    let text = String::from_utf16(&buffer[..length - 1]).map_err(io::Error::other)?;
    if text.contains('\0') {
        return Err(io::Error::other("invalid registry string"));
    }
    Ok(Some(text))
}

pub(super) fn set(browser: Browser, path: Option<&str>) -> io::Result<()> {
    let key = wide(&browser.registry_key());
    if let Some(path) = path {
        let value = wide(path);
        // SAFETY: key/value are terminated, type and byte count match the live buffer.
        return unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR::null(),
                REG_SZ.0,
                Some(value.as_ptr().cast()),
                (value.len() * 2) as u32,
            )
        }
        .ok()
        .map_err(io::Error::other);
    }
    // SAFETY: only the fixed per-user host key's default value is deleted.
    let status =
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR::null()) };
    if missing(status) {
        return Ok(());
    }
    status.ok().map_err(io::Error::other)?;
    remove_empty_key(&key)
}

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: handle came from RegOpenKeyExW and is owned only by this wrapper.
        if unsafe { RegCloseKey(self.0) }.ok().is_err() {
            tracing::error!("Cannot close native host registry handle");
        }
    }
}
fn remove_empty_key(key: &[u16]) -> io::Result<()> {
    let mut opened = HKEY::default();
    // SAFETY: key is terminated; opened is writable handle storage.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            None,
            KEY_READ,
            &mut opened,
        )
    };
    if missing(status) {
        return Ok(());
    }
    status.ok().map_err(io::Error::other)?;
    let opened = Key(opened);
    let mut subkeys = 0;
    let mut values = 0;
    // SAFETY: handle is live, count outputs are valid and remaining outputs are optional.
    unsafe {
        RegQueryInfoKeyW(
            opened.0,
            None,
            None,
            None,
            Some(&mut subkeys),
            None,
            None,
            Some(&mut values),
            None,
            None,
            None,
            None,
        )
    }
    .ok()
    .map_err(io::Error::other)?;
    drop(opened);
    if subkeys == 0 && values == 0 {
        // SAFETY: fixed per-user key was checked empty. No recursive deletion is used.
        let status = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr())) };
        if !missing(status) {
            status.ok().map_err(io::Error::other)?;
        }
    }
    Ok(())
}
