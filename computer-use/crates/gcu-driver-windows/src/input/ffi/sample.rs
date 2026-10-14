//! Bounded window-state and sparse pixel evidence; no images/payloads leave here.
use super::{failure, hwnd};
use crate::input::api::Target;
use gcu_core::driver::{DriverError, Result};
use std::hash::{Hash, Hasher};
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    Graphics::Gdi::*,
    UI::WindowsAndMessaging::*,
};
pub(super) fn sample(target: &Target) -> Result<u64> {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let mut text = [0u16; 4096];
    let mut length = 0;
    // SAFETY: WM_GETTEXT is system-marshalled, bounded writable UTF-16 buffer
    // kept alive until the synchronous call returns. Hung providers time out.
    let ok = unsafe {
        SendMessageTimeoutW(
            hwnd(target.hwnd),
            WM_GETTEXT,
            WPARAM(text.len()),
            LPARAM(text.as_mut_ptr() as isize),
            SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
            50,
            Some(&mut length),
        )
    };
    if ok.0 == 0 {
        return Err(DriverError::Timeout {
            maybe_executed: false,
        });
    }
    text[..length.min(text.len())].hash(&mut hash);
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: valid thread ID and initialized writable GUITHREADINFO.
    unsafe { GetGUIThreadInfo(GetWindowThreadProcessId(hwnd(target.hwnd), None), &mut info) }
        .map_err(|_| failure())?;
    (
        info.hwndFocus.0 as usize,
        info.hwndCapture.0 as usize,
        info.hwndMenuOwner.0 as usize,
        info.flags.0,
    )
        .hash(&mut hash);
    for msg in [0x00f0, 0x00b0] {
        // BM_GETCHECK / EM_GETSEL; no pointer arguments.
        let mut state = 0;
        // SAFETY: pointer-free system control query, bounded independently.
        if unsafe {
            SendMessageTimeoutW(
                hwnd(target.hwnd),
                msg,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                50,
                Some(&mut state),
            )
        }
        .0 == 0
        {
            return Err(DriverError::Timeout {
                maybe_executed: false,
            });
        }
        state.hash(&mut hash);
    }
    // SAFETY: borrowed client DC; never exported. Guard releases it once.
    let dc = unsafe { GetDC(Some(hwnd(target.hwnd))) };
    if dc.0.is_null() {
        return Err(failure());
    }
    let dc = ClientDc {
        window: target.hwnd,
        dc,
    };
    let mut rect = windows::Win32::Foundation::RECT::default();
    // SAFETY: initialized RECT output and live target HWND.
    unsafe { GetClientRect(hwnd(target.hwnd), &mut rect) }.map_err(|_| failure())?;
    for y in 1..=4 {
        for x in 1..=4 {
            // SAFETY: in-bounds sparse client pixel read from the live DC.
            let color = unsafe { GetPixel(dc.dc, rect.right * x / 5, rect.bottom * y / 5) };
            if color.0 == u32::MAX {
                return Err(failure());
            }
            color.0.hash(&mut hash);
        }
    }
    Ok(hash.finish())
}
struct ClientDc {
    window: u64,
    dc: HDC,
}
impl Drop for ClientDc {
    fn drop(&mut self) {
        // SAFETY: balances GetDC on this same thread and HWND; no references remain.
        unsafe {
            ReleaseDC(Some(hwnd(self.window)), self.dc);
        }
    }
}
