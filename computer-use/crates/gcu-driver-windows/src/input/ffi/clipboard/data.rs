//! Bounded HGLOBAL text/data copies. Unsupported handle types fail before clear.
use super::failure;
use gcu_core::driver::{DriverError, Result};
use windows::Win32::{
    Foundation::{GetLastError, GlobalFree, SetLastError, ERROR_SUCCESS, HANDLE, HGLOBAL, HWND},
    System::{DataExchange::*, Memory::*},
};
const MAX_BYTES: usize = 8 * 1024 * 1024;
const UNICODE: u32 = 13;
pub(super) fn sequence() -> u32 {
    // SAFETY: scalar, nonblocking OS query.
    unsafe { GetClipboardSequenceNumber() }
}
pub(super) struct Open;
impl Open {
    pub(super) fn new(window: HWND) -> Result<Self> {
        // SAFETY: valid message-only owner HWND on this thread. No retry.
        unsafe { OpenClipboard(Some(window)) }.map_err(|_| failure())?;
        Ok(Self)
    }
}
impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: the matching OpenClipboard succeeded on this same thread.
        if unsafe { CloseClipboard() }.is_err() {
            tracing::warn!("clipboard close failed");
        }
    }
}
fn supported(id: u32) -> bool {
    if matches!(id, 1 | 7 | 8 | 13 | 15 | 16 | 17) {
        return true;
    }
    let mut name = [0u16; 128];
    // SAFETY: initialized bounded UTF-16 output buffer. Unknown formats refuse;
    // do not assume arbitrary registered formats contain HGLOBAL memory.
    let size = unsafe { GetClipboardFormatNameW(id, &mut name) };
    size > 0
        && matches!(
            String::from_utf16_lossy(&name[..size as usize]).as_str(),
            "HTML Format"
                | "Rich Text Format"
                | "Rich Text Format Without Objects"
                | "CanIncludeInClipboardHistory"
                | "CanUploadToCloudClipboard"
        )
}
pub(super) fn snapshot() -> Result<Vec<(u32, Vec<u8>)>> {
    let mut entries = Vec::new();
    let mut id = 0;
    let mut remaining = MAX_BYTES;
    loop {
        // SAFETY: caller holds OpenClipboard. LastError distinguishes enumeration
        // failure from end-of-list; handles are only borrowed under this lock.
        unsafe {
            SetLastError(ERROR_SUCCESS);
        }
        // SAFETY: scalar enumeration under the clipboard lock.
        let next = unsafe { EnumClipboardFormats(id) };
        if next == 0 {
            // SAFETY: this thread's error from the immediately preceding query.
            if unsafe { GetLastError() } != ERROR_SUCCESS {
                return Err(failure());
            }
            break;
        }
        if entries.len() >= 256 || !supported(next) || entries.iter().any(|(id, _)| *id == next) {
            return Err(DriverError::CapabilityUnavailable);
        }
        let bytes = read(next, remaining)?;
        remaining -= bytes.len();
        entries.push((next, bytes));
        id = next;
    }
    Ok(entries)
}
fn read(id: u32, limit: usize) -> Result<Vec<u8>> {
    // SAFETY: caller holds clipboard open; supported format uses HGLOBAL data.
    let handle = unsafe { GetClipboardData(id) }.map_err(|_| failure())?;
    let global = HGLOBAL(handle.0);
    // SAFETY: borrowed HGLOBAL remains owned by the clipboard until CloseClipboard.
    let size = unsafe { GlobalSize(global) };
    if size == 0 || size > limit {
        return Err(DriverError::CapabilityUnavailable);
    }
    // SAFETY: valid borrowed HGLOBAL; null is checked before creating a slice.
    let source = unsafe { GlobalLock(global) };
    if source.is_null() {
        return Err(failure());
    }
    // SAFETY: lock pins the allocation, and size is bounded by GlobalSize/limit.
    let bytes = unsafe { std::slice::from_raw_parts(source.cast::<u8>(), size) }.to_vec();
    // SAFETY: balances successful GlobalLock; no pointers survive the unlock.
    let _ = unsafe { GlobalUnlock(global) };
    Ok(bytes)
}
pub(super) fn text() -> Result<String> {
    let bytes = read(UNICODE, MAX_BYTES)?;
    if bytes.len() % 2 != 0 {
        return Err(failure());
    }
    let units: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let end = units.iter().position(|u| *u == 0).ok_or_else(failure)?;
    String::from_utf16(&units[..end]).map_err(|_| failure())
}
pub(super) struct Format {
    pub id: u32,
    pub bytes: Vec<u8>,
    handle: Option<HGLOBAL>,
}
impl Format {
    pub(super) fn new(id: u32, mut bytes: Vec<u8>) -> Result<Self> {
        // SAFETY: bounded nonzero allocation; clipboard requires movable memory.
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len()) }
            .map_err(|_| failure())?;
        let mut value = Self {
            id,
            bytes: Vec::new(),
            handle: Some(handle),
        };
        // SAFETY: private live allocation; GlobalAlloc may round its size up.
        // Include zero padding in our receipt so reads of GlobalSize bytes match.
        let allocated = unsafe { GlobalSize(handle) };
        if allocated < bytes.len() || allocated > MAX_BYTES {
            return Err(failure());
        }
        bytes.resize(allocated, 0);
        value.bytes = bytes;
        // SAFETY: newly allocated private HGLOBAL, not yet transferred.
        let target = unsafe { GlobalLock(handle) };
        if target.is_null() {
            return Err(failure());
        }
        // SAFETY: allocation has exactly bytes.len() capacity and does not overlap.
        unsafe {
            std::ptr::copy_nonoverlapping(value.bytes.as_ptr(), target.cast(), value.bytes.len());
        }
        // SAFETY: balances successful GlobalLock, no pointers retained.
        let _ = unsafe { GlobalUnlock(handle) };
        Ok(value)
    }
    pub(super) fn unicode(text: &str) -> Result<Self> {
        let bytes = text
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        Self::new(UNICODE, bytes)
    }
    pub(super) fn published(&self) -> bool {
        self.handle.is_none()
    }
    pub(super) fn publish(&mut self) -> Result<()> {
        if let Some(handle) = self.handle {
            // SAFETY: caller holds OpenClipboard after EmptyClipboard. On success
            // ownership transfers to Windows; on failure this object retains it.
            unsafe { SetClipboardData(self.id, Some(HANDLE(handle.0))) }.map_err(|_| failure())?;
            self.handle = None;
        }
        Ok(())
    }
}
impl Drop for Format {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // SAFETY: only unpublished handles remain owned here; none is locked.
            let _ = unsafe { GlobalFree(Some(handle)) };
        }
    }
}
