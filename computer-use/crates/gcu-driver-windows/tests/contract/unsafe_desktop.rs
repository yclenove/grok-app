//! Independent Win32 fixture setup/oracles. Never resolves targets by label.
use super::*;
use gcu_protocol::Rect;
use windows::{
    core::{w, BOOL, PCWSTR},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};

// Keep the oracle in physical pixels even when the test executable has no DPI
// manifest. This guard never crosses an await or leaves its creating thread.
struct PhysicalDpi(DPI_AWARENESS_CONTEXT);
impl PhysicalDpi {
    fn enter() -> Result<Self, String> {
        // SAFETY: change only the calling thread; retain its previous context.
        let old =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        require(
            !old.0.is_null(),
            "external physical DPI context unavailable",
        )?;
        Ok(Self(old))
    }
}
impl Drop for PhysicalDpi {
    fn drop(&mut self) {
        // SAFETY: restore the context returned by this thread's successful set.
        unsafe {
            SetThreadDpiAwarenessContext(self.0);
        }
    }
}

static COVER_INPUTS: AtomicU64 = AtomicU64::new(0);
// SAFETY: Windows calls the registered static procedure with its window ABI.
// Only atomic scalar accounting is done before forwarding the borrowed message.
unsafe extern "system" fn cover_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if matches!(
        msg,
        WM_LBUTTONDOWN | WM_NCLBUTTONDOWN | WM_KEYDOWN | WM_CHAR
    ) {
        COVER_INPUTS.fetch_add(1, Ordering::SeqCst);
    }
    // SAFETY: forward unchanged parameters to the standard window procedure.
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

fn hwnd(id: &str) -> Result<HWND, String> {
    u64::from_str_radix(id.split(':').next().ok_or("missing HWND")?, 16)
        .map(|n| HWND(n as usize as *mut _))
        .map_err(|e| e.to_string())
}
pub fn pid(id: &str) -> Option<u32> {
    let h = hwnd(id).ok()?;
    let mut pid = 0;
    // SAFETY: query-only HWND; Windows validates it; output storage lives through the call.
    unsafe {
        GetWindowThreadProcessId(h, Some(&mut pid));
    }
    (pid != 0).then_some(pid)
}
fn owned(win: &WindowRef, expected_pid: u32) -> Result<HWND, String> {
    require(
        pid(&win.id) == Some(expected_pid),
        "fixture HWND PID changed",
    )?;
    hwnd(&win.id)
}
pub fn foreground() -> u64 {
    // SAFETY: read-only scalar query; no borrowed native memory.
    unsafe { GetForegroundWindow().0 as usize as u64 }
}

#[derive(Debug)]
pub struct WindowProbe {
    pub hwnd: u64,
    pub visible: bool,
    pub class: String,
}
struct WindowProbes {
    pid: u32,
    windows: Vec<WindowProbe>,
    panicked: bool,
}
// SAFETY: EnumWindows calls synchronously with our live exclusive context.
// No process/token/property access: this oracle must survive UIPI denial.
unsafe extern "system" fn probe_window(h: HWND, data: LPARAM) -> BOOL {
    // SAFETY: caller keeps WindowProbes alive until EnumWindows returns.
    let probes = unsafe { &mut *(data.0 as *mut WindowProbes) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut pid = 0;
        // SAFETY: Windows supplies the HWND; all outputs are bounded local storage.
        unsafe {
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == probes.pid {
                let mut class = [0u16; 256];
                let length = GetClassNameW(h, &mut class).max(0) as usize;
                probes.windows.push(WindowProbe {
                    hwnd: h.0 as usize as u64,
                    visible: IsWindowVisible(h).as_bool(),
                    class: String::from_utf16_lossy(&class[..length]),
                });
            }
        }
    }));
    probes.panicked = result.is_err();
    BOOL(i32::from(result.is_ok()))
}
pub async fn raw_windows(pid: u32) -> Result<Vec<WindowProbe>, String> {
    let work = tokio::task::spawn_blocking(move || {
        let mut probes = WindowProbes {
            pid,
            windows: Vec::new(),
            panicked: false,
        };
        // SAFETY: synchronous callbacks borrow probes only for this call.
        let result = unsafe {
            EnumWindows(
                Some(probe_window),
                LPARAM((&mut probes as *mut WindowProbes) as isize),
            )
        };
        if probes.panicked {
            return Err("raw EnumWindows oracle panicked".into());
        }
        result.map_err(|e| format!("raw EnumWindows: HRESULT=0x{:08x}; {e}", e.code().0 as u32))?;
        Ok(probes.windows)
    });
    tokio::time::timeout(Duration::from_secs(1), work)
        .await
        .map_err(|_| "raw EnumWindows FFI timed out; native result unobserved".to_string())?
        .map_err(|e| e.to_string())?
}

#[derive(Debug)]
pub struct Responsiveness {
    pub responsive: bool,
    pub win32: u32,
    pub elapsed_ms: u128,
}
pub async fn responsiveness(win: &WindowRef) -> Result<Responsiveness, String> {
    let win = win.clone();
    let work = tokio::task::spawn_blocking(move || {
        let h = owned(&win, win.app.info.pid.ok_or("missing fixture PID")?)?;
        let started = std::time::Instant::now();
        // SAFETY: read-only WM_NULL to the exact fixture; no payload or input.
        // A 100ms native limit distinguishes a hung UI thread from UIA-only hangs.
        let (result, code) = unsafe {
            windows::Win32::Foundation::SetLastError(windows::Win32::Foundation::ERROR_SUCCESS);
            let result = SendMessageTimeoutW(
                h,
                WM_NULL,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                100,
                None,
            );
            (result.0 != 0, windows::Win32::Foundation::GetLastError().0)
        };
        Ok(Responsiveness {
            responsive: result,
            win32: code,
            elapsed_ms: started.elapsed().as_millis(),
        })
    });
    tokio::time::timeout(Duration::from_secs(1), work)
        .await
        .map_err(|_| "fixture responsiveness FFI timed out; Win32 result unobserved".to_string())?
        .map_err(|e| e.to_string())?
}
pub fn window_rect(win: &WindowRef, expected_pid: u32) -> Result<Rect, String> {
    let _dpi = PhysicalDpi::enter()?;
    let h = owned(win, expected_pid)?;
    let mut r = RECT::default();
    // SAFETY: checked fixture HWND, writable RECT, no input or activation.
    unsafe { GetWindowRect(h, &mut r) }.map_err(|e| e.to_string())?;
    Ok(Rect {
        x: f64::from(r.left),
        y: f64::from(r.top),
        w: f64::from(r.right - r.left),
        h: f64::from(r.bottom - r.top),
    })
}
pub fn move_window(win: &WindowRef, expected_pid: u32, rect: Rect) -> Result<(), String> {
    let _dpi = PhysicalDpi::enter()?;
    let h = owned(win, expected_pid)?;
    // SAFETY: exact disposable fixture only; preserve size, Z order and activation.
    unsafe {
        SetWindowPos(
            h,
            None,
            rect.x as i32,
            rect.y as i32,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
    .map_err(|e| e.to_string())
}
pub fn close(win: &WindowRef, expected_pid: u32) -> Result<(), String> {
    let h = owned(win, expected_pid)?;
    // SAFETY: pointer-free close message only to the owned disposable fixture.
    unsafe { PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0)) }.map_err(|e| e.to_string())
}
pub struct Title {
    win: WindowRef,
    pid: u32,
    original: String,
}
impl Title {
    pub fn set(win: &WindowRef, expected_pid: u32, title: &str) -> Result<Self, String> {
        let h = owned(win, expected_pid)?;
        let mut text = [0u16; 512];
        // SAFETY: checked fixture handle; bounded writable buffer; no pointer retained.
        let n = unsafe { GetWindowTextW(h, &mut text) };
        let guard = Self {
            win: win.clone(),
            pid: expected_pid,
            original: String::from_utf16_lossy(&text[..n as usize]),
        };
        guard.write(title)?;
        Ok(guard)
    }
    fn write(&self, text: &str) -> Result<(), String> {
        let h = owned(&self.win, self.pid)?;
        let text: Vec<u16> = text.encode_utf16().chain([0]).collect();
        // SAFETY: fixture HWND and NUL-terminated buffer valid for synchronous copy.
        unsafe { SetWindowTextW(h, PCWSTR(text.as_ptr())) }.map_err(|e| e.to_string())
    }
}
impl Drop for Title {
    fn drop(&mut self) {
        let _ = self.write(&self.original);
    }
}

/// An unrelated own-process native window that completely covers the fixture.
pub struct Cover {
    pub hwnd: u64,
    thread: u32,
    join: Option<std::thread::JoinHandle<()>>,
}
impl Cover {
    pub fn input_count(&self) -> u64 {
        COVER_INPUTS.load(Ordering::SeqCst)
    }
    pub fn start(rect: Rect) -> Result<Self, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let join = std::thread::spawn(move || {
            let _dpi = match PhysicalDpi::enter() {
                Ok(dpi) => dpi,
                Err(error) => {
                    let _ = tx.send(Err(error));
                    return;
                }
            };
            // SAFETY: the class name and callback have static process lifetimes.
            // This thread owns the window/loop. Only scalar HWND bits cross threads.
            unsafe {
                static CLASS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
                let Ok(module) = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                else {
                    let _ = tx.send(Err("cover module unavailable".to_string()));
                    return;
                };
                if !CLASS.get_or_init(|| {
                    RegisterClassW(&WNDCLASSW {
                        lpfnWndProc: Some(cover_proc),
                        hInstance: HINSTANCE(module.0),
                        lpszClassName: w!("GcuContractCover"),
                        hbrBackground: windows::Win32::Graphics::Gdi::GetSysColorBrush(
                            windows::Win32::Graphics::Gdi::COLOR_WINDOW,
                        ),
                        ..Default::default()
                    }) != 0
                }) {
                    let _ = tx.send(Err("cover class registration failed".to_string()));
                    return;
                }
                let result = CreateWindowExW(
                    WS_EX_TOPMOST,
                    w!("GcuContractCover"),
                    w!("GCU Contract Cover"),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    rect.x as i32,
                    rect.y as i32,
                    rect.w as i32,
                    rect.h as i32,
                    None,
                    None,
                    Some(HINSTANCE(module.0)),
                    None,
                );
                let Ok(h) = result else {
                    let _ = tx.send(Err("cover creation failed".to_string()));
                    return;
                };
                let _ = SetForegroundWindow(h);
                let _ = tx.send(Ok((
                    h.0 as usize as u64,
                    windows::Win32::System::Threading::GetCurrentThreadId(),
                )));
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let _ = DestroyWindow(h);
            }
        });
        let (hwnd, thread) = rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())??;
        Ok(Self {
            hwnd,
            thread,
            join: Some(join),
        })
    }
}
impl Drop for Cover {
    fn drop(&mut self) {
        // SAFETY: pointer-free quit message to our fixture thread; loop destroys HWND.
        let _ = unsafe { PostThreadMessageW(self.thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
