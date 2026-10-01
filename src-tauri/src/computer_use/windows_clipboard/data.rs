use super::native::ClipboardLock;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::Graphics::Gdi::{
    CopyEnhMetaFileW, DeleteEnhMetaFile, DeleteMetaFile, DeleteObject, GetEnhMetaFileBits,
    GetMetaFileBitsEx, GetObjectW, BITMAP, HENHMETAFILE, HGDIOBJ,
};
use windows::Win32::System::DataExchange::{SetClipboardData, METAFILEPICT};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Ole::{OleDuplicateData, CLIPBOARD_FORMAT};

pub(super) const MAX_BYTES: usize = 64 * 1024 * 1024;

pub(super) struct OwnedFormat {
    pub format: u32,
    handle: HANDLE,
}

impl OwnedFormat {
    pub fn duplicate(format: u32, source: HANDLE, budget: &mut usize) -> Result<Self, String> {
        // Private/owner-display handles have application-defined lifetime.
        // Never drop a format or memcpy an opaque native handle.
        if !(1..=17).contains(&format) && !(0xc000..=0xffff).contains(&format) {
            return Err(format!(
                "clipboard format {format} requires its original owner"
            ));
        }
        let bytes = unsafe {
            match format {
                2 => {
                    let mut bitmap = BITMAP::default();
                    if GetObjectW(
                        HGDIOBJ(source.0),
                        std::mem::size_of::<BITMAP>() as i32,
                        Some((&mut bitmap as *mut BITMAP).cast()),
                    ) == 0
                    {
                        return Err("clipboard bitmap could not be inspected".into());
                    }
                    usize::try_from(bitmap.bmWidthBytes)
                        .ok()
                        .and_then(|row| row.checked_mul(bitmap.bmHeight.unsigned_abs() as usize))
                        .ok_or("clipboard bitmap size is invalid")?
                }
                3 => {
                    let memory = HGLOBAL(source.0);
                    if GlobalSize(memory) < std::mem::size_of::<METAFILEPICT>() {
                        return Err("clipboard metafile structure is truncated".into());
                    }
                    let picture = GlobalLock(memory).cast::<METAFILEPICT>();
                    if picture.is_null() {
                        return Err("clipboard metafile could not be locked".into());
                    }
                    let size = GetMetaFileBitsEx((*picture).hMF, 0, None) as usize;
                    let _ = GlobalUnlock(memory);
                    size + std::mem::size_of::<METAFILEPICT>()
                }
                9 => 65536,
                14 => GetEnhMetaFileBits(HENHMETAFILE(source.0), None) as usize,
                _ => GlobalSize(HGLOBAL(source.0)),
            }
        };
        if bytes == 0 || bytes > *budget {
            return Err(format!(
                "clipboard format {format} cannot be safely snapshotted"
            ));
        }
        *budget -= bytes;
        let handle = unsafe {
            if format == 14 {
                // OleDuplicateData handles bitmap/palette/METAFILEPICT specially,
                // but enhanced metafiles require their own native duplication.
                HANDLE(CopyEnhMetaFileW(HENHMETAFILE(source.0), None).0)
            } else {
                OleDuplicateData(source, CLIPBOARD_FORMAT(format as u16), GMEM_MOVEABLE)
            }
        };
        if handle.is_invalid() {
            return Err(format!("clipboard format {format} duplication failed"));
        }
        Ok(Self { format, handle })
    }

    pub fn text(text: &str) -> Result<Self, String> {
        if text.encode_utf16().count() >= MAX_BYTES / 2 {
            return Err("clipboard text exceeds its allocation budget".into());
        }
        if text.contains('\0') {
            return Err("clipboard text contains a NUL character".into());
        }
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let bytes: Vec<u8> = wide.iter().flat_map(|c| c.to_le_bytes()).collect();
        Self::bytes(13, &bytes)
    }

    pub fn bytes(format: u32, bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > MAX_BYTES {
            return Err("clipboard allocation exceeds its budget".into());
        }
        let memory =
            unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }.map_err(|e| e.to_string())?;
        let owned = Self {
            format,
            handle: HANDLE(memory.0),
        };
        let ptr = unsafe { GlobalLock(memory) };
        if ptr.is_null() {
            return Err("clipboard allocation could not be locked".into());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(memory);
        }
        Ok(owned)
    }

    pub fn publish(&mut self, _locked: &ClipboardLock) -> Result<(), String> {
        if self.handle.is_invalid() {
            return Ok(());
        }
        #[cfg(feature = "computer-use-probe")]
        if super::publication_probe::reject(self.format) {
            return Err("injected clipboard publication failure".into());
        }
        unsafe { SetClipboardData(self.format, Some(self.handle)) }
            .map_err(|_| format!("clipboard format {} publication failed", self.format))?;
        self.handle = HANDLE::default(); // Windows owns it only after success.
        Ok(())
    }
}

impl Drop for OwnedFormat {
    fn drop(&mut self) {
        if self.handle.is_invalid() {
            return;
        }
        unsafe {
            match self.format {
                2 | 9 => {
                    let _ = DeleteObject(HGDIOBJ(self.handle.0));
                }
                14 => {
                    let _ = DeleteEnhMetaFile(Some(HENHMETAFILE(self.handle.0)));
                }
                3 => {
                    let memory = HGLOBAL(self.handle.0);
                    let picture = GlobalLock(memory).cast::<METAFILEPICT>();
                    if !picture.is_null() {
                        let _ = DeleteMetaFile((*picture).hMF);
                        let _ = GlobalUnlock(memory);
                    }
                    let _ = GlobalFree(Some(memory));
                }
                _ => {
                    let _ = GlobalFree(Some(HGLOBAL(self.handle.0)));
                }
            }
        }
    }
}
