//! Win32 nonactivating layered windows, Direct2D/DirectWrite, physical-input gate.
use crate::{
    desktop::{parts, Part, UserEvent},
    Error, Rect, Scene,
};
use std::{
    cell::{Cell, RefCell},
    ptr::NonNull,
    rc::Rc,
};
use windows::{
    core::w,
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::*,
        },
        System::{LibraryLoader::GetModuleHandleW, SystemInformation::OSVERSIONINFOW},
        UI::{
            Controls::WM_MOUSELEAVE,
            HiDpi::*,
            Input::{KeyboardAndMouse::*, *},
            WindowsAndMessaging::*,
        },
    },
};
thread_local! { static EVENTS: RefCell<Vec<UserEvent>> = const { RefCell::new(Vec::new()) }; }
struct Context {
    action: Option<crate::ButtonAction>,
    revision: Cell<u64>,
    down: Cell<bool>,
}
struct Window {
    hwnd: HWND,
    target: ID2D1HwndRenderTarget,
    context: Rc<Context>,
}
impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: UI-thread-owned HWND, context remains allocated until destruction returns.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
pub(crate) struct Surface {
    windows: Vec<Window>,
    factory: ID2D1Factory,
    text: IDWriteTextFormat,
    dpi: DPI_AWARENESS_CONTEXT,
}
impl Surface {
    pub fn open() -> Result<Self, Error> {
        // SAFETY: initialized version output, exact size; no compatibility manifest spoofing.
        unsafe {
            let mut version = OSVERSIONINFOW {
                dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
                ..Default::default()
            };
            if RtlGetVersion(&mut version).0 < 0
                || version.dwMajorVersion < 10
                || (version.dwMajorVersion == 10 && version.dwBuildNumber < 19041)
            {
                return Err(Error::native(
                    "WDA_EXCLUDEFROMCAPTURE requires Windows 10 2004+",
                ));
            }
        }
        // SAFETY: thread-local DPI context, restored on Drop. The owner cannot move threads.
        let dpi =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if dpi.0.is_null() {
            return Err(Error::native(
                "per-monitor physical pixel coordinates unavailable",
            ));
        }
        let result = Self::initialize(dpi);
        if result.is_err() {
            // SAFETY: restore the exact borrowed context returned above on the same thread.
            unsafe {
                SetThreadDpiAwarenessContext(dpi);
            }
        }
        result
    }
    fn initialize(dpi: DPI_AWARENESS_CONTEXT) -> Result<Self, Error> {
        // SAFETY: factories and text format are owned, used and dropped on this UI thread.
        let (factory, text) = unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                .map_err(Error::native)?;
            let write: IDWriteFactory =
                DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).map_err(Error::native)?;
            let text = write
                .CreateTextFormat(
                    w!("Segoe UI"),
                    None,
                    DWRITE_FONT_WEIGHT_MEDIUM,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    14.,
                    w!(""),
                )
                .map_err(Error::native)?;
            text.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)
                .map_err(Error::native)?;
            text.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)
                .map_err(Error::native)?;
            let module = GetModuleHandleW(None).map_err(Error::native)?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: module.into(),
                lpszClassName: w!("GcuOverlayV2"),
                ..Default::default()
            };
            // A previously registered identical process-local class is harmless.
            if RegisterClassW(&class) == 0
                && windows::core::Error::from_thread().code()
                    != windows::Win32::Foundation::ERROR_CLASS_ALREADY_EXISTS.to_hresult()
            {
                return Err(Error::native("overlay window registration failed"));
            }
            (factory, text)
        };
        Ok(Self {
            windows: Vec::new(),
            factory,
            text,
            dpi,
        })
    }
    pub fn monitor(&self, target: Option<Rect>) -> Result<Rect, Error> {
        let target = target.unwrap_or(Rect {
            x: 0.,
            y: 0.,
            w: 1.,
            h: 1.,
        });
        let rect = RECT {
            left: target.x as i32,
            top: target.y as i32,
            right: (target.x + target.w) as i32,
            bottom: (target.y + target.h) as i32,
        };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: readonly RECT and initialized exact-size monitor output.
        if !unsafe { GetMonitorInfoW(MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST), &mut info) }
            .as_bool()
        {
            return Err(Error::native("monitor geometry unavailable"));
        }
        let r = info.rcWork;
        Ok(Rect {
            x: r.left.into(),
            y: r.top.into(),
            w: (r.right - r.left).into(),
            h: (r.bottom - r.top).into(),
        })
    }
    pub fn events(&mut self) -> Result<Vec<UserEvent>, Error> {
        let mut msg = MSG::default();
        for _ in 0..128 {
            // SAFETY: bounded, nonblocking UI-thread message dispatch, valid writable MSG.
            unsafe {
                if !PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    break;
                }
                if msg.message == WM_QUIT {
                    return Err(Error::native("overlay message loop closed"));
                }
                DispatchMessageW(&msg);
            }
        }
        Ok(EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut())))
    }
    pub fn render(&mut self, scene: &Scene) -> Result<(), Error> {
        let next = parts(scene);
        if self.windows.len() != next.len()
            || self
                .windows
                .iter()
                .zip(&next)
                .any(|(w, p)| w.context.action != p.action)
        {
            self.windows.clear();
            for part in &next {
                let window = self.create(part)?;
                self.windows.push(window);
            }
        }
        for (window, part) in self.windows.iter_mut().zip(&next) {
            if window.context.revision.get() != part.revision
                && part.action != Some(crate::ButtonAction::Stop)
            {
                window.context.down.set(false);
            }
            window.context.revision.set(part.revision);
            draw(window, part, &self.text)?;
        }
        Ok(())
    }
    fn create(&self, part: &Part) -> Result<Window, Error> {
        let context = Rc::new(Context {
            action: part.action,
            revision: Cell::new(part.revision),
            down: Cell::new(false),
        });
        // SAFETY: hidden own top-level window, no controls/UIA actions. Userdata
        // points at a stable Rc with interior-mutability cells, installed after
        // CreateWindow and alive past reentrant DestroyWindow callbacks.
        unsafe {
            let style = WS_EX_LAYERED
                | WS_EX_TOPMOST
                | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE
                | if part.action.is_none() {
                    WS_EX_TRANSPARENT
                } else {
                    WINDOW_EX_STYLE(0)
                };
            let hwnd = CreateWindowExW(
                style,
                w!("GcuOverlayV2"),
                w!(""),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(GetModuleHandleW(None).map_err(Error::native)?.into()),
                None,
            )
            .map_err(Error::native)?;
            // Create the RAII guard before fallible affinity/renderer calls.
            let result = (|| {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::as_ptr(&context) as isize);
                SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).map_err(Error::native)?;
                let mut affinity = 0;
                GetWindowDisplayAffinity(hwnd, &mut affinity).map_err(Error::native)?;
                if affinity != WDA_EXCLUDEFROMCAPTURE.0 {
                    return Err(Error::native("capture exclusion readback failed"));
                }
                SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_COLORKEY | LWA_ALPHA)
                    .map_err(Error::native)?;
                self.factory
                    .CreateHwndRenderTarget(
                        &D2D1_RENDER_TARGET_PROPERTIES {
                            pixelFormat: D2D1_PIXEL_FORMAT {
                                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                alphaMode: D2D1_ALPHA_MODE_IGNORE,
                            },
                            dpiX: 96.,
                            dpiY: 96.,
                            ..Default::default()
                        },
                        &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                            hwnd,
                            pixelSize: D2D_SIZE_U {
                                width: 1,
                                height: 1,
                            },
                            presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
                        },
                    )
                    .map_err(Error::native)
            })();
            match result {
                Ok(target) => Ok(Window {
                    hwnd,
                    target,
                    context,
                }),
                Err(error) => {
                    let _ = DestroyWindow(hwnd);
                    Err(error)
                }
            }
        }
    }
    pub fn hide(&mut self) {
        self.windows.clear();
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        self.windows.clear();
        // SAFETY: restoring the saved thread context after destroying all physical-pixel windows.
        unsafe {
            SetThreadDpiAwarenessContext(self.dpi);
        }
    }
}
fn draw(window: &Window, part: &Part, text: &IDWriteTextFormat) -> Result<(), Error> {
    let r = part.frame;
    let size = D2D_SIZE_U {
        width: r.w.ceil().max(1.) as u32,
        height: r.h.ceil().max(1.) as u32,
    };
    // SAFETY: UI-thread-owned HWND/Direct2D objects; bounded initialized rectangles,
    // colors and UTF-16 slices. No native pointer escapes the synchronous draw.
    unsafe {
        SetWindowPos(
            window.hwnd,
            Some(HWND_TOPMOST),
            r.x.round() as i32,
            r.y.round() as i32,
            size.width as i32,
            size.height as i32,
            SWP_NOACTIVATE,
        )
        .map_err(Error::native)?;
        window.target.Resize(&size).map_err(Error::native)?;
        let color = if part.circle || part.text.is_empty() {
            D2D1_COLOR_F {
                r: 0.1,
                g: 0.8,
                b: 0.85,
                a: 1.,
            }
        } else if part.warning {
            D2D1_COLOR_F {
                r: 0.48,
                g: 0.16,
                b: 0.09,
                a: 1.,
            }
        } else {
            D2D1_COLOR_F {
                r: 0.09,
                g: 0.1,
                b: 0.12,
                a: 1.,
            }
        };
        let brush = window
            .target
            .CreateSolidColorBrush(&color, None)
            .map_err(Error::native)?;
        let foreground = window
            .target
            .CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.96,
                    g: 0.96,
                    b: 0.97,
                    a: 1.,
                },
                None,
            )
            .map_err(Error::native)?;
        window.target.BeginDraw();
        window.target.Clear(Some(&D2D1_COLOR_F {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 1.,
        }));
        let bounds = D2D_RECT_F {
            left: 0.,
            top: 0.,
            right: size.width as f32,
            bottom: size.height as f32,
        };
        if part.circle {
            let mut ellipse = D2D1_ELLIPSE {
                radiusX: size.width as f32 / 2.,
                radiusY: size.height as f32 / 2.,
                ..Default::default()
            };
            ellipse.point.X = ellipse.radiusX;
            ellipse.point.Y = ellipse.radiusY;
            window.target.FillEllipse(&ellipse, &brush);
        } else {
            window.target.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: bounds,
                    radiusX: 10_f32.min(bounds.bottom / 2.),
                    radiusY: 10_f32.min(bounds.bottom / 2.),
                },
                &brush,
            );
        }
        let content = D2D_RECT_F {
            left: 8.,
            right: bounds.right - 8.,
            ..bounds
        };
        window.target.DrawText(
            &part.text.encode_utf16().collect::<Vec<_>>(),
            text,
            &content,
            &foreground,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            DWRITE_MEASURING_MODE_NATURAL,
        );
        window.target.EndDraw(None, None).map_err(Error::native)?;
        let _ = ShowWindow(window.hwnd, SW_SHOWNOACTIVATE);
    }
    Ok(())
}
// SAFETY: OS callback ABI. All Rust access/unwinding is contained; userdata is
// installed after creation and remains alive until DestroyWindow returns.
unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let result = std::panic::catch_unwind(|| {
        if message == WM_MOUSEACTIVATE {
            return Some(LRESULT(MA_NOACTIVATE as isize));
        }
        // No keyboard focus, accessibility action or synthetic command route.
        if message == WM_GETOBJECT
            || message == WM_SETFOCUS
            || message == WM_KEYDOWN
            || message == WM_COMMAND
        {
            return Some(LRESULT(0));
        }
        if matches!(
            message,
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_MOUSELEAVE | WM_CANCELMODE | WM_CAPTURECHANGED
        ) {
            let mut source = INPUT_MESSAGE_SOURCE::default();
            // SAFETY: writable initialized source; borrowed UI-thread context is valid.
            unsafe {
                let Some(context) =
                    NonNull::new(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Context)
                else {
                    return Some(LRESULT(0));
                };
                let context = context.as_ref();
                if matches!(message, WM_MOUSELEAVE | WM_CANCELMODE | WM_CAPTURECHANGED) {
                    context.down.set(false);
                    return Some(LRESULT(0));
                }
                let physical = GetCurrentInputMessageSource(&mut source).is_ok()
                    && hardware_source(source.originId, source.deviceType);
                if message == WM_LBUTTONDOWN {
                    // Nonactivating windows do not capture the foreground app's
                    // mouse. Cancel the press on leave, so a later release cannot
                    // reuse a down whose release occurred in another window.
                    let mut tracking = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        ..Default::default()
                    };
                    context
                        .down
                        .set(physical && TrackMouseEvent(&mut tracking).is_ok());
                } else {
                    let down = context.down.replace(false);
                    let mut client = RECT::default();
                    let inside =
                        GetClientRect(hwnd, &mut client).is_ok() && inside_client(lp, client);
                    if physical && down && inside {
                        if let Some(action) = context.action {
                            EVENTS.with(|events| {
                                let mut events = events.borrow_mut();
                                if events.len() < 128 {
                                    events.push(UserEvent {
                                        action,
                                        revision: context.revision.get(),
                                    });
                                }
                            });
                        }
                    }
                }
            }
            return Some(LRESULT(0));
        }
        None
    });
    if let Ok(Some(value)) = result {
        return value;
    }
    // SAFETY: forward unchanged OS-owned parameters without retaining any pointer.
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}
fn hardware_source(origin: INPUT_MESSAGE_ORIGIN_ID, device: INPUT_MESSAGE_DEVICE_TYPE) -> bool {
    origin == IMO_HARDWARE && device == IMDT_MOUSE
}
fn inside_client(point: LPARAM, rect: RECT) -> bool {
    let x = i32::from(point.0 as i16);
    let y = i32::from((point.0 >> 16) as i16);
    x >= rect.left && y >= rect.top && x < rect.right && y < rect.bottom
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn injected_unavailable_and_keyboard_sources_cannot_activate_overlay_controls() {
        assert!(!hardware_source(IMO_INJECTED, IMDT_MOUSE));
        assert!(!hardware_source(IMO_UNAVAILABLE, IMDT_MOUSE));
        assert!(!hardware_source(IMO_HARDWARE, IMDT_KEYBOARD));
        assert!(hardware_source(IMO_HARDWARE, IMDT_MOUSE));
    }
    #[test]
    fn captured_release_outside_custom_button_cannot_activate_it() {
        let bounds = RECT {
            left: 0,
            top: 0,
            right: 100,
            bottom: 40,
        };
        assert!(inside_client(LPARAM((20 << 16) | 50), bounds));
        assert!(!inside_client(LPARAM((20 << 16) | 100), bounds));
        assert!(!inside_client(LPARAM((40 << 16) | 50), bounds));
        assert!(!inside_client(LPARAM((20 << 16) | 0xffff), bounds));
    }
}
