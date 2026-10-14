//! Owned offscreen DIB capture; never BitBlt from the desktop or target screen.
use super::super::{bgra_to_rgba, native, physical_size, CapturePath, RawPixels, SourceWindow};
use super::{map_error, preflight, Request};
use gcu_core::driver::Result;
use windows::Win32::{
    Graphics::Gdi::*,
    Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS},
    UI::WindowsAndMessaging::PW_RENDERFULLCONTENT,
};

struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}
impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: these handles are exclusively owned by this thread; restore
        // the original object before deleting a selected bitmap and memory DC.
        unsafe {
            if !self.previous.is_invalid() {
                SelectObject(self.dc, self.previous);
            }
            if !self.bitmap.is_invalid() && !DeleteObject(self.bitmap.into()).as_bool() {
                tracing::debug!("capture bitmap cleanup failed");
            }
            if !self.dc.is_invalid() && !DeleteDC(self.dc).as_bool() {
                tracing::debug!("capture DC cleanup failed");
            }
        }
    }
}
pub(in crate::capture) fn capture(window: &SourceWindow, request: &Request) -> Result<RawPixels> {
    let hwnd = preflight(window, request, CapturePath::PrintWindow)?;
    let (width, height) = physical_size(window.frame.w, window.frame.h)
        .ok_or_else(|| native("PrintWindow dimensions invalid"))?;
    let len = super::super::source::byte_len(width, height)
        .ok_or_else(|| native("PrintWindow buffer too large"))?;
    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width as i32;
    info.bmiHeader.biHeight = -(height as i32);
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;
    // SAFETY: creates a memory-only DC, never a screen/foreign window DC.
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.is_invalid() {
        return Err(map_error(windows::core::Error::from_thread()));
    }
    let mut dib = Dib {
        dc,
        bitmap: HBITMAP::default(),
        previous: HGDIOBJ::default(),
    };
    let mut bits = std::ptr::null_mut();
    // SAFETY: correctly sized top-down BGRA descriptor, owned DC and writable
    // pixel-pointer output. The returned DIB owns len bytes until Dib drops.
    dib.bitmap = unsafe { CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) }
        .map_err(map_error)?;
    if bits.is_null() {
        return Err(native("PrintWindow DIB buffer missing"));
    }
    // SAFETY: memory DC/bitmap are owned and live; previous object restored in Drop.
    dib.previous = unsafe { SelectObject(dc, dib.bitmap.into()) };
    if dib.previous.is_invalid() || dib.previous.0 as isize == -1 {
        dib.previous = HGDIOBJ::default();
        return Err(native("PrintWindow bitmap selection failed"));
    }
    // SAFETY: validated len matches this allocated 32-bit DIB. Zero-fill prevents
    // unpainted regions from containing previous allocations/other app pixels.
    unsafe {
        std::ptr::write_bytes(bits.cast::<u8>(), 0, len);
    }
    request.check()?;
    // SAFETY: authorized nonminimized HWND, offscreen destination, full-window
    // rendering flag. Call may hang; caller timeout/abandonment never joins it.
    let painted =
        unsafe { PrintWindow(hwnd, dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)) }.as_bool();
    request.check()?;
    if !painted {
        return Err(map_error(windows::core::Error::from_thread()));
    }
    // SAFETY: flush this thread's GDI work before reading the live DIB pixels.
    if !unsafe { GdiFlush() }.as_bool() {
        return Err(native("PrintWindow GDI flush failed"));
    }
    preflight(window, request, CapturePath::PrintWindow)?;
    // SAFETY: owned mapped DIB has len initialized bytes, retained until copying.
    let data = unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), len) };
    let rgba = bgra_to_rgba(data, width, height, width as usize * 4)
        .ok_or_else(|| native("PrintWindow stride invalid"))?;
    request.check()?;
    Ok(RawPixels {
        rgba,
        width,
        height,
    })
}
