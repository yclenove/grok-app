//! Measurement only: where the pointer really is and which window is under it. Never an oracle repair.
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::*,
};

pub struct Under {
    pub pid: u32,
    pub class: String,
    pub window_rect: (i32, i32, i32, i32),
    pub frame_bounds: (i32, i32, i32, i32),
}
impl std::fmt::Display for Under {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "pid={} class={} window_rect={:?} frame_bounds={:?}",
            self.pid, self.class, self.window_rect, self.frame_bounds
        )
    }
}
pub fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    // SAFETY: bounded local output; read-only cursor query.
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}
pub fn set_cursor(x: i32, y: i32) {
    // SAFETY: scalar cursor move on the disposable desktop; only used to stage a start position.
    unsafe {
        let _ = SetCursorPos(x, y);
    }
}
pub fn under_cursor() -> Under {
    let (x, y) = cursor();
    let mut class = [0u16; 128];
    let mut pid = 0;
    let mut rect = RECT::default();
    let mut frame = RECT::default();
    // SAFETY: read-only window queries with bounded local buffers.
    unsafe {
        let leaf = WindowFromPoint(POINT { x, y });
        let root = GetAncestor(leaf, GA_ROOT);
        let root = if root == HWND::default() { leaf } else { root };
        GetWindowThreadProcessId(root, Some(&mut pid));
        let n = GetClassNameW(root, &mut class).max(0) as usize;
        let _ = GetWindowRect(root, &mut rect);
        let _ = DwmGetWindowAttribute(
            root,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut frame as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        );
        Under {
            pid,
            class: String::from_utf16_lossy(&class[..n]),
            window_rect: (rect.left, rect.top, rect.right, rect.bottom),
            frame_bounds: (frame.left, frame.top, frame.right, frame.bottom),
        }
    }
}

/// Records every cursor position change (with milliseconds since start) on a side thread.
pub struct Sampler {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    join: Option<std::thread::JoinHandle<Vec<(u128, i32, i32)>>>,
}
impl Sampler {
    pub fn start() -> Self {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        let join = std::thread::spawn(move || {
            let begin = std::time::Instant::now();
            let mut last = (i32::MIN, i32::MIN);
            let mut out = Vec::new();
            while !flag.load(std::sync::atomic::Ordering::Relaxed) && out.len() < 400 {
                let now = cursor();
                if now != last {
                    out.push((begin.elapsed().as_millis(), now.0, now.1));
                    last = now;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            out
        });
        Self {
            stop,
            join: Some(join),
        }
    }
    pub fn finish(mut self) -> Vec<(u128, i32, i32)> {
        std::thread::sleep(std::time::Duration::from_millis(300));
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.join
            .take()
            .map(|j| j.join().unwrap_or_default())
            .unwrap_or_default()
    }
}

/// Control: the same absolute-move SendInput formula the driver uses, issued from this (parent) process.
pub fn control_move(x: i32, y: i32) -> String {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE,
        MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
    };
    // SAFETY: nonblocking metric queries and one owned INPUT record with exact cbSize.
    unsafe {
        let (vx, vy, vw, vh) = (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        );
        let nx = (f64::from(x - vx) * 65535. / f64::from(vw - 1)).round() as i32;
        let ny = (f64::from(y - vy) * 65535. / f64::from(vh - 1)).round() as i32;
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: nx,
                    dy: ny,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let sent = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(std::time::Duration::from_millis(80));
        format!(
            "metrics=({vx},{vy},{vw},{vh}) normalized=({nx},{ny}) sent={sent} cursor_after={:?}",
            cursor()
        )
    }
}

/// Same axis math as `pointer_settle::normalize_axis` / `denormalize_axis`.
fn normalize_axis(pixel: i32, origin: i32, extent: i32) -> i32 {
    ((f64::from(pixel) - f64::from(origin)) * 65535. / f64::from(extent - 1)).round() as i32
}
fn denormalize_axis(norm: i32, origin: i32, extent: i32) -> i32 {
    origin + (f64::from(norm) * f64::from(extent - 1) / 65535.).round() as i32
}

fn bump(hist: &mut [u32; 17], delta: i32) {
    let index = (delta + 8).clamp(0, 16) as usize;
    hist[index] += 1;
}

fn format_hist(hist: &[u32; 17]) -> String {
    let mut parts = Vec::new();
    for (index, count) in hist.iter().enumerate() {
        let label = match index {
            0 => "<=-8".to_string(),
            16 => ">=8".to_string(),
            n => format!("{}", n as i32 - 8),
        };
        parts.push(format!("{label}:{count}"));
    }
    parts.join(" ")
}

/// `MOVE|ABSOLUTE|VIRTUALDESK` to at least 200 pixels, including the virtual-desktop edges.
/// Measurement only: the histogram is the result, not a pass/fail on the delta.
pub fn landing_histogram() -> Result<String, String> {
    use windows::Win32::{
        Foundation::POINT,
        UI::{
            Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE,
                MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
            },
            WindowsAndMessaging::{
                GetPhysicalCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
                SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
            },
        },
    };
    // SAFETY: nonblocking metric queries. No input is sent until the grid is built.
    let (origin_x, origin_y, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    if width <= 1 || height <= 1 {
        return Err(format!("virtual desktop is not usable: {width}x{height}"));
    }
    let mut targets = Vec::new();
    for row in 0..13 {
        for col in 0..16 {
            targets.push((
                origin_x + (col * (width - 1)) / 15,
                origin_y + (row * (height - 1)) / 12,
            ));
        }
    }
    if targets.len() < 200 {
        return Err(format!("grid has only {} targets", targets.len()));
    }
    let mut requested_dx = [0u32; 17];
    let mut requested_dy = [0u32; 17];
    let mut inverse_dx = [0u32; 17];
    let mut inverse_dy = [0u32; 17];
    let mut sent_ok = 0u32;
    let mut send_fail = 0u32;
    let mut unreadable = 0u32;
    let mut max_requested = 0i32;
    let mut max_inverse = 0i32;
    for (x, y) in targets {
        let nx = normalize_axis(x, origin_x, width);
        let ny = normalize_axis(y, origin_y, height);
        let ix = denormalize_axis(nx, origin_x, width);
        let iy = denormalize_axis(ny, origin_y, height);
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: nx,
                    dy: ny,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        // SAFETY: one owned INPUT record with exact cbSize. This moves the real cursor.
        let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        std::thread::sleep(std::time::Duration::from_millis(15));
        if sent != 1 {
            send_fail += 1;
            continue;
        }
        sent_ok += 1;
        let mut point = POINT::default();
        // SAFETY: bounded local output; read-only physical cursor query.
        if unsafe { GetPhysicalCursorPos(&mut point) }.is_err() {
            unreadable += 1;
            continue;
        }
        let rdx = point.x - x;
        let rdy = point.y - y;
        let idx = point.x - ix;
        let idy = point.y - iy;
        bump(&mut requested_dx, rdx);
        bump(&mut requested_dy, rdy);
        bump(&mut inverse_dx, idx);
        bump(&mut inverse_dy, idy);
        max_requested = max_requested.max(rdx.abs()).max(rdy.abs());
        max_inverse = max_inverse.max(idx.abs()).max(idy.abs());
    }
    Ok(format!(
        "landing probe targets=208 sent_ok={sent_ok} send_fail={send_fail} unreadable={unreadable} metrics=({origin_x},{origin_y},{width},{height})\nvs_requested dx [{}]\nvs_requested dy [{}]\nvs_inverse dx [{}]\nvs_inverse dy [{}]\nmax_abs requested={max_requested} inverse={max_inverse}",
        format_hist(&requested_dx),
        format_hist(&requested_dy),
        format_hist(&inverse_dx),
        format_hist(&inverse_dy),
    ))
}

static EVENTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
unsafe extern "system" fn mouse_log(
    code: i32,
    wp: windows::Win32::Foundation::WPARAM,
    lp: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    if code == HC_ACTION as i32 && lp.0 != 0 {
        // SAFETY: Windows supplies MSLLHOOKSTRUCT for HC_ACTION; read-only during this call.
        let s = unsafe { &*(lp.0 as *const MSLLHOOKSTRUCT) };
        let at = START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis();
        if let Ok(mut log) = EVENTS.lock() {
            if log.len() < 600 {
                log.push(format!(
                    "{at}ms msg=0x{:x} pt=({},{}) flags=0x{:x} extra={}",
                    wp.0, s.pt.x, s.pt.y, s.flags, s.dwExtraInfo
                ));
            }
        }
    }
    // SAFETY: forward the unchanged borrowed arguments; this log never suppresses input.
    unsafe { CallNextHookEx(None, code, wp, lp) }
}
/// Logs every low-level mouse event (including injected ones) seen system-wide while alive.
pub struct HookLog {
    thread: u32,
    join: Option<std::thread::JoinHandle<()>>,
}
impl HookLog {
    pub fn start() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let join = std::thread::spawn(move || {
            // SAFETY: this thread owns the hook and its message loop; unhooked before exit.
            unsafe {
                let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_log), None, 0).ok();
                let _ = tx.send(GetCurrentThreadId());
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
                if let Some(h) = hook {
                    let _ = UnhookWindowsHookEx(h);
                }
            }
        });
        let thread = rx.recv().unwrap_or(0);
        Self {
            thread,
            join: Some(join),
        }
    }
    pub fn take(&self) -> Vec<String> {
        EVENTS
            .lock()
            .map(|mut l| std::mem::take(&mut *l))
            .unwrap_or_default()
    }
}
impl Drop for HookLog {
    fn drop(&mut self) {
        // SAFETY: posts WM_QUIT to the hook thread only.
        unsafe {
            let _ =
                PostThreadMessageW(self.thread, WM_QUIT, Default::default(), Default::default());
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

static KEYS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
unsafe extern "system" fn key_log(
    code: i32,
    wp: windows::Win32::Foundation::WPARAM,
    lp: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    if code == HC_ACTION as i32 && lp.0 != 0 {
        // SAFETY: Windows supplies KBDLLHOOKSTRUCT for HC_ACTION; read-only during this call.
        let s = unsafe { &*(lp.0 as *const KBDLLHOOKSTRUCT) };
        let down = matches!(wp.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let entry = if s.vkCode == 0x87 {
            format!("F24{}", if down { "v" } else { "^" })
        } else if s.vkCode == 0xE7 {
            // VK_PACKET: the UTF-16 unit rides in scanCode.
            format!("U+{:04X}{}", s.scanCode, if down { "v" } else { "^" })
        } else {
            format!("vk{:02X}{}", s.vkCode, if down { "v" } else { "^" })
        };
        if let Ok(mut log) = KEYS.lock() {
            if log.len() < 800 {
                log.push(entry);
            }
        }
    }
    // SAFETY: forward the unchanged borrowed arguments; this log never suppresses input.
    unsafe { CallNextHookEx(None, code, wp, lp) }
}
/// Logs every low-level keyboard event (injected ones included) system-wide while alive.
pub struct KeyLog {
    thread: u32,
    join: Option<std::thread::JoinHandle<()>>,
}
impl KeyLog {
    pub fn start() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let join = std::thread::spawn(move || {
            // SAFETY: this thread owns the hook and its message loop; unhooked before exit.
            unsafe {
                let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_log), None, 0).ok();
                let _ = tx.send(GetCurrentThreadId());
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
                if let Some(h) = hook {
                    let _ = UnhookWindowsHookEx(h);
                }
            }
        });
        Self {
            thread: rx.recv().unwrap_or(0),
            join: Some(join),
        }
    }
    pub fn take(&self) -> String {
        KEYS.lock()
            .map(|mut l| std::mem::take(&mut *l).join(" "))
            .unwrap_or_default()
    }
}
impl Drop for KeyLog {
    fn drop(&mut self) {
        // SAFETY: posts WM_QUIT to the hook thread only.
        unsafe {
            let _ =
                PostThreadMessageW(self.thread, WM_QUIT, Default::default(), Default::default());
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// One keyboard step for the surrogate experiment: (virtual key, UTF-16 unit, key-up).
pub type KeyStep = (u16, u16, bool);
/// Sends the steps with one `SendInput` call per group; `pause_ms` separates groups.
pub fn send_key_groups(groups: &[&[KeyStep]], pause_ms: u64) -> String {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VIRTUAL_KEY,
    };
    let mut report = Vec::new();
    for group in groups {
        let inputs: Vec<INPUT> = group
            .iter()
            .map(|&(vk, unit, up)| {
                let mut flags = KEYBD_EVENT_FLAGS(0);
                if vk == 0 {
                    flags |= KEYEVENTF_UNICODE;
                }
                if up {
                    flags |= KEYEVENTF_KEYUP;
                }
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(vk),
                            wScan: unit,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                }
            })
            .collect();
        // SAFETY: owned initialized INPUT records with exact cbSize.
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        report.push(format!("{sent}/{}", inputs.len()));
        std::thread::sleep(std::time::Duration::from_millis(pause_ms));
    }
    report.join(",")
}
