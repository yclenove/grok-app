//! Owned-window display affinity; all Win32 calls remain inside the FFI module.
use super::OverlayApi;
use windows::{
    core::w,
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::{
        Foundation::HWND,
        System::{SystemInformation::OSVERSIONINFOW, Threading::GetCurrentProcessId},
        UI::WindowsAndMessaging::*,
    },
};
/// Production Win10 2004+ exclusion detector; older WDA_MONITOR is false.
pub struct RealOverlayApi;
impl OverlayApi for RealOverlayApi {
    fn probe(&self) -> bool {
        let mut version = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        // SAFETY: initialized writable version output with its exact byte size.
        // Documented RtlGetVersion avoids compatibility-manifest version spoofing.
        if unsafe { RtlGetVersion(&mut version) }.0 < 0
            || !super::supports_exclusion_build(version.dwMajorVersion, version.dwBuildNumber)
        {
            return false;
        }
        // SAFETY: system STATIC class, hidden process-owned layered top-level
        // window; no user data, raw pointers, child windows or message payloads.
        let window = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
        };
        let Ok(window) = window else {
            return false;
        };
        let success = self.exclude(window.0 as usize as u64);
        // SAFETY: this thread's probe window, destroyed once after final use.
        unsafe {
            let _ = DestroyWindow(window);
        }
        success
    }
    fn exclude(&self, id: u64) -> bool {
        let window = HWND(id as usize as *mut _);
        let mut pid = 0;
        let mut affinity = 0;
        // SAFETY: borrowed numeric handle, ownership/top-level validated before
        // changing affinity; initialized outputs. No foreign process mutation.
        unsafe {
            GetWindowThreadProcessId(window, Some(&mut pid));
            pid == GetCurrentProcessId()
                && GetAncestor(window, GA_ROOT) == window
                && SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE).is_ok()
                && GetWindowDisplayAffinity(window, &mut affinity).is_ok()
                && affinity == WDA_EXCLUDEFROMCAPTURE.0
        }
    }
}
