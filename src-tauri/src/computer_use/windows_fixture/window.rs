//! Self-built Win32 fixture window for native observe/act/verify tests.

#![cfg(target_os = "windows")]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    ChildWindowFromPoint, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetDlgItem, GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindowTextW,
    GetWindowThreadProcessId, PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW,
    SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, CS_HREDRAW,
    CS_VREDRAW, CW_USEDEFAULT, HMENU, HWND_TOP, MSG, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_SHOW, WM_CLOSE, WM_COMMAND, WM_DESTROY,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD,
    WS_OVERLAPPED, WS_OVERLAPPEDWINDOW, WS_SYSMENU, WS_VISIBLE, WS_VSCROLL,
};

const IDC_BUTTON: i32 = 101;
const IDC_EDIT: i32 = 102;
const IDC_STATUS: i32 = 103;
const IDC_LIST: i32 = 104;
const IDC_DRAG: i32 = 105;
const IDC_DROP: i32 = 106;
const IDC_DIALOG: i32 = 107;
const IDC_DLG_OK: i32 = 201;

const LB_ADDSTRING: u32 = 0x0180;
const LB_GETCURSEL: u32 = 0x0188;
const LB_GETTOPINDEX: u32 = 0x018E;
const EN_CHANGE: u16 = 0x0300;
const LBN_SELCHANGE: u16 = 1;

fn unpack_lparam(lp: LPARAM) -> (i32, i32) {
    let v = lp.0 as u32;
    let x = (v & 0xffff) as i16 as i32;
    let y = ((v >> 16) & 0xffff) as i16 as i32;
    (x, y)
}

pub(crate) fn oracle_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(name)
}

fn registry() -> &'static Mutex<HashMap<isize, Arc<FixtureShared>>> {
    static REG: OnceLock<Mutex<HashMap<isize, Arc<FixtureShared>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

struct FixtureShared {
    hwnd: Mutex<isize>,
    ready: Mutex<bool>,
    startup_error: Mutex<Option<String>>,
    startup_stage: AtomicU32,
    startup_cancelled: AtomicBool,
    cv: Condvar,
    clicks: AtomicU32,
    drags: AtomicU32,
    dragging: AtomicBool,
    drag_pause: Mutex<Option<Arc<super::native_stop::DispatchPause>>>,
}

pub struct FixtureWindow {
    shared: Arc<FixtureShared>,
    thread: Option<JoinHandle<()>>,
}

impl FixtureWindow {
    pub fn spawn(title: &str) -> Result<Self, String> {
        crate::computer_use::windows_adapter::ensure_dpi_aware();
        let shared = Arc::new(FixtureShared {
            hwnd: Mutex::new(0),
            ready: Mutex::new(false),
            startup_error: Mutex::new(None),
            startup_stage: AtomicU32::new(0),
            startup_cancelled: AtomicBool::new(false),
            cv: Condvar::new(),
            clicks: AtomicU32::new(0),
            drags: AtomicU32::new(0),
            dragging: AtomicBool::new(false),
            drag_pause: Mutex::new(None),
        });
        let title_owned = title.to_string();
        let shared_t = shared.clone();
        let thread = thread::spawn(move || {
            if let Err(e) = run_loop(&title_owned, shared_t.clone()) {
                *shared_t.startup_error.lock().unwrap() = Some(e);
                let _ready = shared_t.ready.lock().unwrap();
                shared_t.cv.notify_all();
            }
        });
        if !wait_ready(&shared, Duration::from_secs(3)) {
            shared.startup_cancelled.store(true, Ordering::SeqCst);
            let hwnd = HWND(*shared.hwnd.lock().unwrap() as *mut _);
            if !hwnd.0.is_null() {
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            let error = shared.startup_error.lock().unwrap().clone();
            return Err(format!(
                "fixture window did not start: stage={} error={:?}",
                shared.startup_stage.load(Ordering::SeqCst),
                error
            ));
        }
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub fn hwnd(&self) -> HWND {
        HWND(*self.shared.hwnd.lock().unwrap() as *mut _)
    }

    pub fn clicks(&self) -> u32 {
        self.shared.clicks.load(Ordering::SeqCst)
    }

    pub fn drags(&self) -> u32 {
        self.shared.drags.load(Ordering::SeqCst)
    }

    pub(super) fn pause_next_drag(&self) -> Arc<super::native_stop::DispatchPause> {
        let pause = Arc::new(super::native_stop::DispatchPause::default());
        *self.shared.drag_pause.lock().unwrap() = Some(pause.clone());
        pause
    }

    pub(super) fn is_dragging(&self) -> bool {
        self.shared.dragging.load(Ordering::SeqCst)
    }

    pub fn dpi(&self) -> u32 {
        unsafe { GetDpiForWindow(self.hwnd()) }
    }

    pub fn edit_text(&self) -> String {
        unsafe {
            let Ok(edit) = GetDlgItem(Some(self.hwnd()), IDC_EDIT) else {
                return String::new();
            };
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(edit, &mut buf);
            if n <= 0 {
                String::new()
            } else {
                String::from_utf16_lossy(&buf[..n as usize])
            }
        }
    }

    pub fn list_top_index(&self) -> i32 {
        unsafe {
            let Ok(list) = GetDlgItem(Some(self.hwnd()), IDC_LIST) else {
                return -1;
            };
            SendMessageW(list, LB_GETTOPINDEX, None, None).0 as i32
        }
    }

    pub fn list_sel(&self) -> i32 {
        unsafe {
            let Ok(list) = GetDlgItem(Some(self.hwnd()), IDC_LIST) else {
                return -1;
            };
            SendMessageW(list, LB_GETCURSEL, None, None).0 as i32
        }
    }

    pub fn close(self) {}

    pub fn move_to_virtual_origin(&self) -> (i32, i32) {
        let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        let _ = unsafe {
            SetWindowPos(
                self.hwnd(),
                Some(HWND_TOP),
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW,
            )
        };
        (x, y)
    }
}

impl Drop for FixtureWindow {
    fn drop(&mut self) {
        let hwnd = HWND(*self.shared.hwnd.lock().unwrap() as *mut _);
        if !hwnd.0.is_null() {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(th) = self.thread.take() {
            let _ = th.join();
        }
    }
}

#[derive(Debug)]
pub(crate) struct ForegroundAttempt {
    pub foreground_attach_error: Option<i32>,
    pub target_attach_error: Option<i32>,
    pub requested: bool,
    pub active_before_detach: bool,
    pub active_after_detach: bool,
}

pub(crate) fn force_foreground(hwnd: HWND) -> ForegroundAttempt {
    unsafe {
        let fg = GetForegroundWindow();
        let mut fg_pid = 0u32;
        let fg_tid = GetWindowThreadProcessId(fg, Some(&mut fg_pid));
        let mut dst_pid = 0u32;
        let dst_tid = GetWindowThreadProcessId(hwnd, Some(&mut dst_pid));
        let me = GetCurrentThreadId();
        let foreground_attach = AttachThreadInput(me, fg_tid, true).ok();
        let target_attach = AttachThreadInput(me, dst_tid, true).ok();
        let requested = SetForegroundWindow(hwnd).as_bool();
        let active_before_detach = GetForegroundWindow() == hwnd;
        if target_attach.is_ok() {
            let _ = AttachThreadInput(me, dst_tid, false);
        }
        if foreground_attach.is_ok() {
            let _ = AttachThreadInput(me, fg_tid, false);
        }
        ForegroundAttempt {
            foreground_attach_error: foreground_attach.err().map(|e| e.code().0),
            target_attach_error: target_attach.err().map(|e| e.code().0),
            requested,
            active_before_detach,
            active_after_detach: GetForegroundWindow() == hwnd,
        }
    }
}

fn wait_ready(shared: &FixtureShared, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    let mut ready = shared.ready.lock().unwrap();
    while !*ready {
        if shared.startup_error.lock().unwrap().is_some() {
            return false;
        }
        let remaining = timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return false;
        }
        let (g, t) = shared.cv.wait_timeout(ready, remaining).unwrap();
        ready = g;
        if t.timed_out() {
            return *ready;
        }
    }
    true
}

fn run_loop(title: &str, shared: Arc<FixtureShared>) -> Result<(), String> {
    unsafe {
        shared.startup_stage.store(1, Ordering::SeqCst);
        let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = w!("GrokCuFixtureClass");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        shared.startup_stage.store(2, Ordering::SeqCst);
        let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let hwnd = CreateWindowExW(
            Default::default(),
            class,
            PCWSTR(title_wide.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            540,
            420,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .map_err(|e| e.to_string())?;
        *shared.hwnd.lock().unwrap() = hwnd.0 as isize;
        if shared.startup_cancelled.load(Ordering::SeqCst) {
            let _ = DestroyWindow(hwnd);
            return Err("fixture startup cancelled before publication".into());
        }
        shared.startup_stage.store(3, Ordering::SeqCst);
        registry()
            .lock()
            .unwrap()
            .insert(hwnd.0 as isize, shared.clone());
        let _ = CreateWindowExW(
            Default::default(),
            w!("BUTTON"),
            w!("Count"),
            WS_CHILD | WS_VISIBLE,
            20,
            20,
            100,
            32,
            Some(hwnd),
            Some(HMENU(IDC_BUTTON as isize as *mut _)),
            Some(hinst.into()),
            None,
        );
        let _ = CreateWindowExW(
            Default::default(),
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_VISIBLE,
            20,
            70,
            360,
            28,
            Some(hwnd),
            Some(HMENU(IDC_EDIT as isize as *mut _)),
            Some(hinst.into()),
            None,
        );
        let _ = CreateWindowExW(
            Default::default(),
            w!("STATIC"),
            w!("clicks=0"),
            WS_CHILD | WS_VISIBLE,
            20,
            110,
            360,
            24,
            Some(hwnd),
            Some(HMENU(IDC_STATUS as isize as *mut _)),
            Some(hinst.into()),
            None,
        );
        let list = CreateWindowExW(
            Default::default(),
            w!("LISTBOX"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_BORDER,
            20,
            145,
            200,
            160,
            Some(hwnd),
            Some(HMENU(IDC_LIST as isize as *mut _)),
            Some(hinst.into()),
            None,
        )
        .map_err(|e| e.to_string())?;
        for i in 0..30 {
            let item: Vec<u16> = format!("item-{i:02}")
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let _ = SendMessageW(
                list,
                LB_ADDSTRING,
                None,
                Some(LPARAM(item.as_ptr() as isize)),
            );
        }
        let _ = CreateWindowExW(
            Default::default(),
            w!("STATIC"),
            w!("Drag"),
            WS_CHILD | WS_VISIBLE | WS_BORDER,
            240,
            145,
            110,
            48,
            Some(hwnd),
            Some(HMENU(IDC_DRAG as isize as *mut _)),
            Some(hinst.into()),
            None,
        );
        let _ = CreateWindowExW(
            Default::default(),
            w!("STATIC"),
            w!("Drop"),
            WS_CHILD | WS_VISIBLE | WS_BORDER,
            370,
            145,
            110,
            48,
            Some(hwnd),
            Some(HMENU(IDC_DROP as isize as *mut _)),
            Some(hinst.into()),
            None,
        );
        let _ = CreateWindowExW(
            Default::default(),
            w!("BUTTON"),
            w!("Ask"),
            WS_CHILD | WS_VISIBLE,
            20,
            320,
            100,
            32,
            Some(hwnd),
            Some(HMENU(IDC_DIALOG as isize as *mut _)),
            Some(hinst.into()),
            None,
        );
        let dpi = GetDpiForWindow(hwnd);
        shared.startup_stage.store(4, Ordering::SeqCst);
        let _ = std::fs::write(oracle_path("grok-cu-fixture-dpi.txt"), format!("{dpi}"));
        let _ = std::fs::write(oracle_path("grok-cu-fixture-edit.txt"), "");
        let _ = std::fs::write(oracle_path("grok-cu-fixture-list.txt"), "top=0 sel=-1");
        let _ = std::fs::write(oracle_path("grok-cu-fixture-drag.txt"), "0");
        let _ = std::fs::write(oracle_path("grok-cu-fixture-dialog.txt"), "");
        let _ = ShowWindow(hwnd, SW_SHOW);
        shared.startup_stage.store(5, Ordering::SeqCst);
        if shared.startup_cancelled.load(Ordering::SeqCst) {
            let _ = DestroyWindow(hwnd);
        }
        {
            let mut r = shared.ready.lock().unwrap();
            *r = true;
            shared.cv.notify_all();
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        registry().lock().unwrap().remove(&(hwnd.0 as isize));
        Ok(())
    }
}

fn open_modeless_dialog(owner: HWND) {
    unsafe {
        let Ok(hinst) = GetModuleHandleW(None) else {
            return;
        };
        let class = w!("GrokCuDialogClass");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(dialog_proc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        let Ok(dlg) = CreateWindowExW(
            Default::default(),
            class,
            w!("GrokCuDialog"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            80,
            80,
            280,
            140,
            Some(owner),
            None,
            Some(hinst.into()),
            None,
        ) else {
            return;
        };
        let _ = CreateWindowExW(
            Default::default(),
            w!("STATIC"),
            w!("confirm fixture"),
            WS_CHILD | WS_VISIBLE,
            16,
            16,
            240,
            24,
            Some(dlg),
            None,
            Some(hinst.into()),
            None,
        );
        let _ = CreateWindowExW(
            Default::default(),
            w!("BUTTON"),
            w!("OK"),
            WS_CHILD | WS_VISIBLE,
            90,
            56,
            80,
            28,
            Some(dlg),
            Some(HMENU(IDC_DLG_OK as isize as *mut _)),
            Some(hinst.into()),
            None,
        );
        let _ = ShowWindow(dlg, SW_SHOW);
    }
}

unsafe extern "system" fn dialog_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as i32;
            if id == IDC_DLG_OK {
                let _ = std::fs::write(oracle_path("grok-cu-fixture-dialog.txt"), "ok");
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn hit_child(parent: HWND, x: i32, y: i32, id: i32) -> bool {
    unsafe {
        let child = ChildWindowFromPoint(parent, POINT { x, y });
        if child.0.is_null() {
            return false;
        }
        GetDlgItem(Some(parent), id)
            .ok()
            .map(|want| child == want)
            .unwrap_or(false)
    }
}

pub(crate) fn write_list_oracle(hwnd: HWND) {
    unsafe {
        let Ok(list) = GetDlgItem(Some(hwnd), IDC_LIST) else {
            return;
        };
        let top = SendMessageW(list, LB_GETTOPINDEX, None, None).0 as i32;
        let sel = SendMessageW(list, LB_GETCURSEL, None, None).0 as i32;
        let _ = std::fs::write(
            oracle_path("grok-cu-fixture-list.txt"),
            format!("top={top} sel={sel}"),
        );
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as i32;
            let notify = ((wparam.0 >> 16) & 0xffff) as u16;
            // Native calls below may synchronously re-enter another fixture's
            // window procedure. Never hold the process-wide registry across them.
            let shared = registry().lock().unwrap().get(&(hwnd.0 as isize)).cloned();
            if let Some(shared) = shared {
                if id == IDC_BUTTON {
                    let n = shared.clicks.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ =
                        std::fs::write(oracle_path("grok-cu-fixture-clicks.txt"), format!("{n}"));
                    let text: Vec<u16> = format!("clicks={n}")
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect();
                    if let Ok(status) = GetDlgItem(Some(hwnd), IDC_STATUS) {
                        let _ = SetWindowTextW(status, PCWSTR(text.as_ptr()));
                    }
                } else if id == IDC_EDIT && notify == EN_CHANGE {
                    if let Ok(edit) = GetDlgItem(Some(hwnd), IDC_EDIT) {
                        let mut buf = [0u16; 256];
                        let n = GetWindowTextW(edit, &mut buf);
                        let text = if n <= 0 {
                            String::new()
                        } else {
                            String::from_utf16_lossy(&buf[..n as usize])
                        };
                        let _ = std::fs::write(oracle_path("grok-cu-fixture-edit.txt"), text);
                    }
                } else if id == IDC_LIST && notify == LBN_SELCHANGE {
                    write_list_oracle(hwnd);
                } else if id == IDC_DIALOG {
                    // Modeless: MessageBoxW is STA-modal and deadlocks same-process UIA.
                    open_modeless_dialog(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = unpack_lparam(lparam);
            let shared = registry().lock().unwrap().get(&(hwnd.0 as isize)).cloned();
            if let Some(shared) = shared {
                if hit_child(hwnd, x, y, IDC_DRAG) {
                    shared.dragging.store(true, Ordering::SeqCst);
                    let pause = shared.drag_pause.lock().unwrap().take();
                    if let Some(pause) = pause {
                        pause.enter();
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => LRESULT(0),
        WM_LBUTTONUP => {
            let (x, y) = unpack_lparam(lparam);
            let shared = registry().lock().unwrap().get(&(hwnd.0 as isize)).cloned();
            if let Some(shared) = shared {
                if shared.dragging.swap(false, Ordering::SeqCst) && hit_child(hwnd, x, y, IDC_DROP)
                {
                    let n = shared.drags.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ = std::fs::write(oracle_path("grok-cu-fixture-drag.txt"), format!("{n}"));
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
