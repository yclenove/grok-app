//! HWND enumeration, per-window generation properties and virtual-desktop COM.
use super::{native, security, string, Com, RealWinApi};
use crate::api::NativeWindow;
use ::windows::{
    core::{BOOL, PCWSTR},
    Win32::{
        Foundation::{HANDLE, HWND, LPARAM, RECT},
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        UI::{
            HiDpi::GetDpiForWindow,
            Shell::{IVirtualDesktopManager, VirtualDesktopManager},
            WindowsAndMessaging::*,
        },
    },
};
use gcu_core::driver::{DriverError, Result};

struct Enumeration<'a> {
    api: &'a RealWinApi,
    windows: Vec<NativeWindow>,
    error: Option<DriverError>,
}
unsafe extern "system" fn enumerate_window(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: EnumWindows receives the address of the live exclusive context.
    let context = unsafe { &mut *(data.0 as *mut Enumeration<'_>) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        read_window(context.api, hwnd)
    }));
    match result {
        Ok(Ok(Some(window))) => {
            context.windows.push(window);
            BOOL(1)
        }
        Ok(Ok(None)) => BOOL(1),
        Ok(Err(error)) => {
            context.error = Some(error);
            BOOL(0)
        }
        Err(_) => {
            context.error = Some(DriverError::Crashed {
                maybe_executed: false,
            });
            BOOL(0)
        }
    }
}
struct Children {
    pids: Vec<u32>,
    panicked: bool,
}
unsafe extern "system" fn enumerate_child(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: EnumChildWindows passes our live context; never retained by Windows.
    let children = unsafe { &mut *(data.0 as *mut Children) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut pid = 0;
        // SAFETY: EnumChildWindows supplies a live HWND and writable pid output.
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        if pid != 0 && !children.pids.contains(&pid) {
            children.pids.push(pid);
        }
    }));
    children.panicked = result.is_err();
    BOOL(i32::from(result.is_ok()))
}
fn read_window(api: &RealWinApi, hwnd: HWND) -> Result<Option<NativeWindow>> {
    // SAFETY: enumeration HWND may disappear; all failures are checked below.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return Ok(None);
        }
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if style & WS_EX_TOOLWINDOW.0 != 0 && style & WS_EX_APPWINDOW.0 == 0 {
            return Ok(None);
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || !IsWindow(Some(hwnd)).as_bool() {
            return Ok(None);
        }
        let mut title = vec![0u16; 4096];
        GetWindowTextW(hwnd, &mut title);
        let mut class = vec![0u16; 256];
        if GetClassNameW(hwnd, &mut class) == 0 {
            return Err(DriverError::WindowGone);
        }
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).map_err(native)?;
        // A null GW_OWNER is a normal unowned window; the generated
        // wrapper maps that sentinel to Err, so preserve the owner as optional.
        let owner = GetWindow(hwnd, GW_OWNER).ok();
        let mut marker = GetPropW(hwnd, PCWSTR(api.property.as_ptr()));
        if marker.is_invalid() {
            marker = HANDLE(api.generation() as usize as *mut _);
            if SetPropW(hwnd, PCWSTR(api.property.as_ptr()), Some(marker)).is_err() {
                let target = security::security(pid)?;
                use crate::api::WinApi;
                let engine = api.engine_security()?;
                if target.integrity > engine.integrity || target.ui_access {
                    // Read-only discovery can label an elevated window. Its zero
                    // marker is never actionable: WinGate always refuses it.
                    marker = HANDLE::default();
                } else {
                    return Err(DriverError::PermissionMissing {
                        permission: "window generation property".into(),
                    });
                }
            }
        }
        let mut children = Children {
            pids: Vec::new(),
            panicked: false,
        };
        // EnumChildWindows has no defined return value; callbacks are synchronous.
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(enumerate_child),
            LPARAM((&mut children as *mut Children) as isize),
        );
        if children.panicked {
            return Err(DriverError::Crashed {
                maybe_executed: false,
            });
        }
        let process_created = security::process_created(pid)?;
        let dpi = GetDpiForWindow(hwnd);
        Ok(Some(NativeWindow {
            hwnd: hwnd.0 as usize as u64,
            pid,
            process_created,
            created: marker.0 as usize as u64,
            class: string(&class),
            title: string(&title),
            visible: true,
            tool_window: style & WS_EX_TOOLWINDOW.0 != 0,
            app_window: style & WS_EX_APPWINDOW.0 != 0,
            owner: owner.map(|owner| owner.0 as usize as u64),
            minimized: IsIconic(hwnd).as_bool(),
            foreground: GetForegroundWindow() == hwnd,
            frame: gcu_protocol::Rect {
                x: f64::from(rect.left),
                y: f64::from(rect.top),
                w: f64::from(rect.right - rect.left),
                h: f64::from(rect.bottom - rect.top),
            },
            scale: if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 },
            child_pids: children.pids,
        }))
    }
}
pub(super) fn enumerate(api: &RealWinApi) -> Result<Vec<NativeWindow>> {
    let mut context = Enumeration {
        api,
        windows: Vec::new(),
        error: None,
    };
    // SAFETY: callback context stays live; callbacks catch Rust panics.
    let result = unsafe {
        EnumWindows(
            Some(enumerate_window),
            LPARAM((&mut context as *mut Enumeration<'_>) as isize),
        )
    };
    if let Some(error) = context.error {
        return Err(error);
    }
    result.map_err(native)?;
    Ok(context.windows)
}
pub(super) fn desktop(hwnd: u64) -> Result<Option<bool>> {
    let _com = Com::new()?;
    // SAFETY: COM initialized on this worker; interface is scoped to apartment.
    let manager: IVirtualDesktopManager =
        match unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_INPROC_SERVER) } {
            Ok(manager) => manager,
            // Virtual desktops are optional; None prevents any foreground action.
            Err(_) => return Ok(None),
        };
    // SAFETY: HWND conversion preserves native bits; COM validates liveness.
    unsafe {
        manager
            .IsWindowOnCurrentVirtualDesktop(HWND(hwnd as usize as *mut _))
            .map(|v| Some(v.as_bool()))
            .map_err(native)
    }
}
pub(super) fn restore(api: &RealWinApi, expected: &NativeWindow) -> Result<()> {
    use crate::api::WinApi;
    let hwnd = HWND(expected.hwnd as usize as *mut _);
    let current = read_window(api, hwnd)?.ok_or(DriverError::WindowGone)?;
    crate::desktop::HwndStamp::from(expected).validate(&current)?;
    crate::gate::WinGate.check(
        api,
        expected.pid,
        true,
        api.on_current_desktop(expected.hwnd)?,
    )?;
    // SAFETY: exact validated HWND; SW_SHOWNOACTIVATE never activates another app.
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return Err(DriverError::WindowGone);
        }
        if !ShowWindowAsync(hwnd, SW_SHOWNOACTIVATE).as_bool() {
            return Err(DriverError::WindowMinimized);
        }
    }
    // ShowWindowAsync may only queue a message. Await the exact HWND's result
    // without SendMessage; uncertainty after accepted delivery is never replayed.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        // SAFETY: read-only liveness/minimized checks on the previously stamped HWND.
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return Err(DriverError::Native {
                    message: "Window disappeared during restoration".into(),
                    maybe_executed: true,
                });
            }
            if !IsIconic(hwnd).as_bool() {
                return Ok(());
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(DriverError::Timeout {
                maybe_executed: true,
            });
        }
        std::thread::park_timeout(std::time::Duration::from_millis(10));
    }
}
