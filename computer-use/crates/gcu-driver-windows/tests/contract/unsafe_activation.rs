//! Window identity and out-of-context foreground events; no activation or input.
use super::unsafe_desktop;
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::HWND,
    UI::{Accessibility::*, WindowsAndMessaging::*},
};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Identity {
    pub hwnd: String,
    pub pid: u32,
    pub thread: u32,
    pub class: String,
    pub title: String,
}
pub async fn identity(id: u64) -> Result<Identity, String> {
    let (reply, job) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let h = HWND(id as usize as *mut _);
        let mut pid = 0;
        let mut class = [0u16; 256];
        let mut title = [0u16; 512];
        // SAFETY: bounded local output buffers; queries validate the scalar HWND.
        let (thread, class_len, title_len) = unsafe {
            (
                GetWindowThreadProcessId(h, Some(&mut pid)),
                GetClassNameW(h, &mut class).max(0) as usize,
                GetWindowTextW(h, &mut title).max(0) as usize,
            )
        };
        let identity = Identity {
            hwnd: format!("{id:x}"),
            pid,
            thread,
            class: String::from_utf16_lossy(&class[..class_len]),
            title: String::from_utf16_lossy(&title[..title_len]),
        };
        let _ = reply.send(identity);
    });
    tokio::time::timeout(Duration::from_secs(1), job)
        .await
        .map_err(|_| "window identity query timed out".to_string())?
        .map_err(|e| e.to_string())
}
#[derive(Clone, Debug, serde::Serialize)]
struct Event {
    hwnd: u64,
    event_thread: u32,
    window_thread: u32,
    pid: u32,
    os_event_ms: u32,
    elapsed_us: u128,
}
struct Sink {
    started: Instant,
    events: Arc<Mutex<Vec<Event>>>,
}
thread_local! { static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) }; }
// SAFETY: Windows calls this static WinEvent ABI on the installing pump thread.
unsafe extern "system" fn callback(
    _: HWINEVENTHOOK,
    _: u32,
    h: HWND,
    _: i32,
    _: i32,
    event_thread: u32,
    os_event_ms: u32,
) {
    let _ = std::panic::catch_unwind(|| {
        SINK.with(|slot| {
            if let Some(sink) = slot.borrow().as_ref() {
                let mut pid = 0;
                // SAFETY: scalar HWND query, stack-owned PID output; no window messages.
                let window_thread = unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
                let mut events = sink.events.lock().unwrap_or_else(|p| p.into_inner());
                if events.len() < 128 {
                    events.push(Event {
                        hwnd: h.0 as usize as u64,
                        event_thread,
                        window_thread,
                        pid,
                        os_event_ms,
                        elapsed_us: sink.started.elapsed().as_micros(),
                    });
                }
            }
        })
    });
}
pub struct Trace {
    close: Arc<AtomicBool>,
    events: Arc<Mutex<Vec<Event>>>,
}
impl Trace {
    pub fn start() -> Result<Self, String> {
        let close = Arc::new(AtomicBool::new(false));
        let events = Arc::new(Mutex::new(Vec::new()));
        let (closed, observed) = (close.clone(), events.clone());
        let (ready, wait) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            SINK.with(|slot| {
                *slot.borrow_mut() = Some(Sink {
                    started: Instant::now(),
                    events: observed,
                })
            });
            // SAFETY: no DLL injection; callback lives for this thread's hook lifetime.
            let hook = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(callback),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            let _ = ready.send(!hook.0.is_null());
            if hook.0.is_null() {
                return;
            }
            while !closed.load(Ordering::Acquire) {
                let mut message = MSG::default();
                // SAFETY: bounded same-thread message pumping for WinEvent delivery.
                unsafe {
                    for _ in 0..128 {
                        if !PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                            break;
                        }
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            // SAFETY: owned hook retired on its installing thread.
            let _ = unsafe { UnhookWinEvent(hook) };
            SINK.with(|slot| *slot.borrow_mut() = None);
        });
        match wait.recv_timeout(Duration::from_secs(1)) {
            Ok(true) => Ok(Self { close, events }),
            result => {
                close.store(true, Ordering::Release);
                Err(format!("foreground event hook unavailable: {result:?}"))
            }
        }
    }
    pub async fn record(&self, label: &str, fixture: u64, cover: u64) {
        let current = unsafe_desktop::foreground();
        let events = self
            .events
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        // Event thread is the thread generating the accessibility notification;
        // it is evidence, not a stack trace or attribution of SetForegroundWindow.
        super::super::harness::progress(format!("ACTIVATION: {label}; foreground={:?}; fixture={:?}; cover={:?}; events={events:?}; event_thread is notifier, API caller unproven", identity(current).await, identity(fixture).await, identity(cover).await));
    }
}
impl Drop for Trace {
    fn drop(&mut self) {
        self.close.store(true, Ordering::Release);
    }
}
