//! CI-only controls: direct UIA (bypassing the driver) and native PostMessage.
use super::{unsafe_activation, unsafe_desktop, Fixture};
use crate::harness::*;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
use windows::{
    core::{w, Interface},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            Com::*, LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId,
            Variant::VARIANT,
        },
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
};
static CLICKS: AtomicU64 = AtomicU64::new(0);
// SAFETY: static registered procedure; uses only scalar message values.
unsafe extern "system" fn procedure(h: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if message == WM_COMMAND && wp.0 == 1001 && lp.0 != 0 {
        CLICKS.fetch_add(1, Ordering::SeqCst);
    }
    // SAFETY: forward unchanged OS-owned parameters.
    unsafe { DefWindowProcW(h, message, wp, lp) }
}
struct Native {
    root: u64,
    button: u64,
    thread: u32,
}
impl Native {
    fn start() -> Result<Self, String> {
        let (ready, wait) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            // SAFETY: static class/procedure; windows and pump belong to this thread.
            let result = unsafe {
                (|| -> windows::core::Result<(HWND, HWND)> {
                    let module = GetModuleHandleW(None)?;
                    let instance = HINSTANCE(module.0);
                    RegisterClassW(&WNDCLASSW {
                        lpfnWndProc: Some(procedure),
                        hInstance: instance,
                        lpszClassName: w!("GcuNativeBackgroundProbe"),
                        ..Default::default()
                    });
                    let root = CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        w!("GcuNativeBackgroundProbe"),
                        w!("GCU native background control"),
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        20,
                        20,
                        300,
                        180,
                        None,
                        None,
                        Some(instance),
                        None,
                    )?;
                    match CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        w!("BUTTON"),
                        w!("GCU native counter"),
                        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                        20,
                        20,
                        200,
                        50,
                        Some(root),
                        Some(HMENU(1001usize as *mut _)),
                        Some(instance),
                        None,
                    ) {
                        Ok(button) => Ok((root, button)),
                        Err(error) => {
                            let _ = DestroyWindow(root);
                            Err(error)
                        }
                    }
                })()
            };
            let (root, button) = match result {
                Ok(pair) => pair,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return;
                }
            };
            // SAFETY: own-thread ID is a scalar.
            let thread = unsafe { GetCurrentThreadId() };
            if ready
                .send(Ok((
                    root.0 as usize as u64,
                    button.0 as usize as u64,
                    thread,
                )))
                .is_err()
            {
                // SAFETY: destroy the owned window when setup was abandoned.
                let _ = unsafe { DestroyWindow(root) };
                return;
            }
            let mut message = MSG::default();
            // SAFETY: initialized MSG; this thread owns both windows.
            unsafe {
                while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                let _ = DestroyWindow(root);
            }
        });
        let (root, button, thread) = wait
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())??;
        Ok(Self {
            root,
            button,
            thread,
        })
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        // SAFETY: pointer-free quit to our own window/pump thread; no blocking join.
        let _ = unsafe { PostThreadMessageW(self.thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
    }
}
fn hwnd(id: u64) -> HWND {
    HWND(id as usize as *mut _)
}
async fn invoke(root: u64, name: Option<&str>, expected: u64) -> Result<(), String> {
    let name = name.map(str::to_owned);
    let closed = Arc::new(AtomicBool::new(false));
    let abandoned = closed.clone();
    let (reply, job) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let result = (|| {
            // SAFETY: dedicated MTA, with all interfaces dropped before CoUninitialize.
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .map_err(|e| e.to_string())?;
            struct Com;
            impl Drop for Com {
                fn drop(&mut self) {
                    // SAFETY: balances this thread's successful initialization.
                    unsafe { CoUninitialize() };
                }
            }
            let _com = Com;
            // SAFETY: COM initialized here; targets are exact disposable fixture HWNDs.
            let result = unsafe {
                (|| -> windows::core::Result<()> {
                    let automation: IUIAutomation =
                        CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
                    let limits: IUIAutomation2 = automation.cast()?;
                    limits.SetConnectionTimeout(500)?;
                    limits.SetTransactionTimeout(500)?;
                    let root = automation.ElementFromHandle(hwnd(root))?;
                    let element = if let Some(name) = name {
                        let condition = automation.CreatePropertyCondition(
                            UIA_NamePropertyId,
                            &VARIANT::from(name.as_str()),
                        )?;
                        root.FindFirst(TreeScope_Descendants, &condition)?
                    } else {
                        root
                    };
                    let pattern: IUIAutomationInvokePattern =
                        element.GetCurrentPatternAs(UIA_InvokePatternId)?;
                    let before_invoke = unsafe_desktop::foreground();
                    progress(format!(
                    "DIRECT UIA: resolved; before_Invoke={before_invoke:x}; expected={expected:x}"
                ));
                    if abandoned.load(Ordering::Acquire) || before_invoke != expected {
                        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                            0x80004004u32 as i32,
                        )));
                    }
                    pattern.Invoke()
                })()
            };
            result.map_err(|e| format!("direct UIA Invoke: {e}"))
        })();
        let _ = reply.send(result);
    });
    let result = tokio::time::timeout(Duration::from_secs(5), job).await;
    closed.store(true, Ordering::Release);
    result
        .map_err(|_| "direct UIA diagnostic timed out; mutation unknown, no retry".to_string())?
        .map_err(|e| e.to_string())?
}
async fn native_row(
    native: &Native,
    cover: &unsafe_desktop::Cover,
    direct: bool,
) -> Result<(), String> {
    cover.acquire_foreground_for_setup().await?;
    let trace = unsafe_activation::Trace::start()?;
    trace.record("native before", native.root, cover.hwnd).await;
    require(
        unsafe_desktop::foreground() == cover.hwnd,
        "native precondition foreground changed",
    )?;
    let before = CLICKS.load(Ordering::SeqCst);
    let label = if direct {
        "native direct UIA Invoke"
    } else {
        "native PostMessage BM_CLICK"
    };
    let dispatch = if direct {
        invoke(native.button, None, cover.hwnd).await
    } else {
        // SAFETY: pointer-free BM_CLICK to our exact native child BUTTON, once.
        unsafe { PostMessageW(Some(hwnd(native.button)), BM_CLICK, WPARAM(0), LPARAM(0)) }
            .map_err(|e| e.to_string())
    };
    let after_dispatch = unsafe_desktop::foreground();
    let effect = poll(
        &mut DesktopClock::default(),
        Duration::from_secs(5),
        label,
        || Ok(CLICKS.load(Ordering::SeqCst)),
        |count| *count == before + 1,
    )
    .await;
    trace.record(label, native.root, cover.hwnd).await;
    let actual = unsafe_desktop::foreground();
    progress(format!("BACKGROUND CONTROL: {label}; count_before={before}; count_after={}; expected={:x}; after_dispatch={after_dispatch:x}; actual={actual:x}; equal={}", CLICKS.load(Ordering::SeqCst), cover.hwnd, actual == cover.hwnd));
    progress(format!(
        "BACKGROUND CONTROL EFFECT: {label}; dispatch={dispatch:?}; effect={effect:?}"
    ));
    dispatch?;
    effect?;
    require(actual == cover.hwnd, format!("{label} stole foreground"))
}

#[tokio::test]
#[ignore = "CI diagnostic only: disposable interactive unelevated Windows desktop"]
async fn live_background_activation_probe() {
    assert_eq!(std::env::var("GCU_DESKTOP").as_deref(), Ok("1"));
    let native = Native::start().unwrap();
    let fixture = Fixture::start().await.unwrap();
    let driver = super::driver().await.unwrap();
    let win = super::window(&driver, &fixture).await.unwrap();
    super::node(&driver, &win, "counter_button").await.unwrap();
    let cover =
        unsafe_desktop::Cover::start(unsafe_desktop::window_rect(&win, fixture.pid()).unwrap())
            .unwrap();
    // Independent controls run even if one comparison fails. They cannot mark C16 passed.
    let post = native_row(&native, &cover, false).await;
    progress(format!("BACKGROUND CONTROL RESULT: PostMessage={post:?}"));
    let uia = native_row(&native, &cover, true).await;
    progress(format!("BACKGROUND CONTROL RESULT: direct_UIA={uia:?}"));
    cover.acquire_foreground_for_setup().await.unwrap();
    let trace = unsafe_activation::Trace::start().unwrap();
    let root = u64::from_str_radix(win.id.split(':').next().unwrap(), 16).unwrap();
    trace
        .record("egui direct UIA before", root, cover.hwnd)
        .await;
    let count = fixture.read().unwrap()["counter"].as_u64().unwrap();
    let result = invoke(root, Some("counter_button"), cover.hwnd).await;
    let after_dispatch = unsafe_desktop::foreground();
    progress(format!("EGUI DIRECT UIA DISPATCH: result={result:?}; expected={:x}; after_dispatch={after_dispatch:x}", cover.hwnd));
    let effect = if result.is_ok() {
        let effect = fixture
            .wait("direct Invoke counter", |v| v["counter"] == count + 1)
            .await;
        progress(format!(
            "EGUI DIRECT UIA: effect={effect:?}; window_health={:?}",
            unsafe_desktop::responsiveness(&win).await
        ));
        effect.map(|_| ())
    } else {
        Err("Invoke failed; no replay".to_string())
    };
    trace
        .record("egui direct UIA after", root, cover.hwnd)
        .await;
    progress(format!("BACKGROUND COMPARISON: native_PostMessage={post:?}; native_direct_UIA={uia:?}; egui_direct_UIA={result:?}; egui_foreground_equal={}; provider attribution requires fixture trace and exact owner PID", unsafe_desktop::foreground() == cover.hwnd));
    result.unwrap();
    effect.unwrap();
}
