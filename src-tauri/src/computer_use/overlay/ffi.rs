//! OS calls are isolated here; portable safety ordering is tested behind Surface.
#[cfg(target_os = "macos")]
pub use gcu_host_protocol::overlay::backing_placement as placement;
#[cfg(not(target_os = "macos"))]
pub use gcu_host_protocol::overlay::placement;
use gcu_host_protocol::overlay::{Placement, Surface};
#[cfg(not(target_os = "macos"))]
use tauri::Manager;
use tauri::WebviewWindow;
#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests;
#[cfg(target_os = "linux")]
mod x11;

pub fn position(win: &WebviewWindow, r: gcu_protocol::Rect, _scale: f64) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // PhysicalPosition is divided by the window's *old* monitor scale by
        // Tao. Explicit points avoid a wrong origin on a cross-DPI transition.
        win.set_position(tauri::LogicalPosition::new(r.x / _scale, r.y / _scale))
            .map_err(|e| e.to_string())?;
        win.set_size(tauri::LogicalSize::new(r.w / _scale, r.h / _scale))
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let pos = tauri::PhysicalPosition::new(r.x.round() as i32, r.y.round() as i32);
        let size = tauri::PhysicalSize::new(r.w.round() as u32, r.h.round() as u32);
        if win.outer_position().map_err(|e| e.to_string())? != pos {
            win.set_position(pos).map_err(|e| e.to_string())?;
        }
        if win.outer_size().map_err(|e| e.to_string())? != size {
            win.set_size(size).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub struct WindowSurface<'a>(pub &'a WebviewWindow);
impl Surface for WindowSurface<'_> {
    type Error = String;
    fn nonactivating(&mut self) -> Result<(), String> {
        #[cfg(not(target_os = "macos"))]
        {
            let menu = tauri::menu::Menu::new(self.0.app_handle()).map_err(|e| e.to_string())?;
            self.0.set_menu(menu).map_err(|e| e.to_string())?;
            self.0.hide_menu().map_err(|e| e.to_string())?;
        }
        self.0.set_focusable(false).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
                WS_EX_TOOLWINDOW,
            };
            let hwnd = self.0.hwnd().map_err(|e| e.to_string())?;
            // SAFETY: This is the live HWND owned by this main-thread surface.
            unsafe {
                let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(
                    hwnd,
                    GWL_EXSTYLE,
                    style | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize,
                );
                if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_NOACTIVATE.0 as isize == 0 {
                    return Err("overlay activation guard failed".into());
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            use objc2::{msg_send, runtime::AnyObject};
            let ptr = self.0.ns_window().map_err(|e| e.to_string())? as *mut AnyObject;
            if ptr.is_null() {
                return Err("missing native overlay".into());
            }
            // SAFETY: Tauri supplies a live NSWindow on the main thread. Public
            // nonactivating style and orderFrontRegardless avoid makeKeyAndOrderFront.
            unsafe {
                let mask: usize = msg_send![ptr, styleMask];
                let _: () = msg_send![ptr, setStyleMask: mask | (1 << 7)];
            }
        }
        #[cfg(target_os = "linux")]
        x11::configure(self.0)?;
        Ok(())
    }
    fn passthrough(&mut self, cursor: bool) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            x11::passthrough(self.0, cursor)
        }
        #[cfg(windows)]
        {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_LAYERED, WS_EX_TRANSPARENT,
            };
            let hwnd = self.0.hwnd().map_err(|e| e.to_string())?;
            // SAFETY: Live main-thread HWND. Apply before first show and read
            // back; do not rely on an asynchronously queued style change.
            unsafe {
                let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                let next = if cursor {
                    style | WS_EX_LAYERED.0 as isize | WS_EX_TRANSPARENT.0 as isize
                } else {
                    style & !(WS_EX_TRANSPARENT.0 as isize)
                };
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
                if (GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TRANSPARENT.0 as isize != 0)
                    != cursor
                {
                    return Err("overlay passthrough failed".into());
                }
            }
            Ok(())
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            self.0
                .set_ignore_cursor_events(cursor)
                .map_err(|e| e.to_string())
        }
    }
    fn exclude_capture(&mut self) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            x11::register(self.0)
        }
        #[cfg(windows)]
        {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetWindowDisplayAffinity, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
            };
            let hwnd = self.0.hwnd().map_err(|e| e.to_string())?;
            let mut affinity = 0;
            // SAFETY: Live owned HWND; checked public API and initialized output.
            unsafe {
                SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)
                    .map_err(|e| e.to_string())?;
                GetWindowDisplayAffinity(hwnd, &mut affinity).map_err(|e| e.to_string())?;
            }
            if affinity != WDA_EXCLUDEFROMCAPTURE.0 {
                return Err("overlay capture exclusion failed".into());
            }
            Ok(())
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            self.0
                .set_content_protected(true)
                .map_err(|e| e.to_string())
        }
    }
    fn hide(&mut self) {
        let _ = self.0.hide();
    }
}

pub fn present(
    draw: &WebviewWindow,
    controls: &WebviewWindow,
    _placement: &Placement,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        x11::present(draw, controls)
    }
    #[cfg(target_os = "macos")]
    {
        use objc2::{msg_send, runtime::AnyObject};
        for win in [draw, controls] {
            let ptr = win.ns_window().map_err(|e| e.to_string())? as *mut AnyObject;
            if ptr.is_null() {
                return Err("missing native overlay".into());
            }
            // SAFETY: Main-thread live NSWindow. This public method never makes
            // the window key and does not activate the owning application.
            unsafe {
                let _: () = msg_send![ptr, orderFrontRegardless];
            }
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        for win in [draw, controls] {
            if !win.is_visible().map_err(|e| e.to_string())? {
                win.show().map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}
