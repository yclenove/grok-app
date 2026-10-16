//! CI-only empirical probe matrix; never linked into the product listener.
use std::{
    cell::RefCell,
    sync::mpsc,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{
        GetLastError, SetLastError, ERROR_SUCCESS, HINSTANCE, LPARAM, LRESULT, POINT, WPARAM,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
#[allow(dead_code)]
#[path = "../../src/input/hook_probe.rs"]
mod policy;
use policy::MouseProbe;
#[path = "../../src/input/ffi/safety/mouse.rs"]
mod mouse;

#[derive(Clone, Debug, serde::Serialize)]
struct Callback {
    message: usize,
    flags: u32,
    extra_info: usize,
    elapsed_us: u128,
}
struct Observation {
    tag: usize,
    started: Instant,
    callbacks: Vec<Callback>,
}
thread_local! { static OBSERVATION: RefCell<Option<Observation>> = const { RefCell::new(None) }; }
// SAFETY: static Win32 hook ABI; borrow the OS record only during HC_ACTION.
unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lp.0 != 0 {
        let _ = std::panic::catch_unwind(|| {
            // SAFETY: Windows supplies this live record for HC_ACTION.
            let event = unsafe { &*(lp.0 as *const MSLLHOOKSTRUCT) };
            OBSERVATION.with(|slot| {
                if let Ok(mut slot) = slot.try_borrow_mut() {
                    if let Some(observation) = slot.as_mut() {
                        if event.dwExtraInfo == observation.tag && observation.callbacks.len() < 16
                        {
                            observation.callbacks.push(Callback {
                                message: wp.0,
                                flags: event.flags,
                                extra_info: event.dwExtraInfo,
                                elapsed_us: observation.started.elapsed().as_micros(),
                            });
                        }
                    }
                }
            });
        });
    }
    // SAFETY: unchanged borrowed hook arguments; diagnostics never suppress input.
    unsafe { CallNextHookEx(None, code, wp, lp) }
}
struct Hook(HHOOK);
impl Drop for Hook {
    fn drop(&mut self) {
        // SAFETY: unregister exactly the owned hook on its installing thread.
        let _ = unsafe { UnhookWindowsHookEx(self.0) };
    }
}
fn cursor() -> Result<(i32, i32), String> {
    let mut point = POINT::default();
    // SAFETY: bounded writable POINT; no movement or activation.
    unsafe { GetPhysicalCursorPos(&mut point) }.map_err(|e| e.to_string())?;
    Ok((point.x, point.y))
}
fn pump() {
    // SAFETY: only this thread's queue and initialized MSG; bounded flood work.
    unsafe {
        let mut message = MSG::default();
        for _ in 0..128 {
            if !PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
#[derive(Debug, serde::Serialize)]
struct Row {
    mode: String,
    marked: bool,
    sent: Option<u32>,
    requested: usize,
    win32: Option<u32>,
    send_elapsed_us: Option<u128>,
    elapsed_us: u128,
    callback_seen: bool,
    injected_callback_seen: bool,
    dispatch_error: Option<String>,
    partial_button_release: Option<(u32, u32)>,
    callbacks: Vec<Callback>,
    before: (i32, i32),
    after: (i32, i32),
}
fn matrix() -> Result<Vec<Row>, String> {
    // SAFETY: process module is borrowed; hook code has static lifetime.
    let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    // SAFETY: hook is installed, pumped, and dropped on this same thread.
    let _hook = Hook(
        unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(hook), Some(HINSTANCE(module.0)), 0) }
            .map_err(|e| e.to_string())?,
    );
    let (request, receive) = mpsc::sync_channel::<(MouseProbe, usize)>(1);
    let (reply, result) = mpsc::sync_channel(1);
    // Exactly one sender. SendInput must not wait on its own LL hook thread.
    let sender = std::thread::spawn(move || {
        while let Ok((mode, tag)) = receive.recv() {
            let response = (|| {
                let inputs = mouse::inputs(mode, tag)?;
                let count = inputs.len();
                let started = Instant::now();
                // SAFETY: initialized pointer-free records on the disposable CI desktop.
                let (sent, code) = unsafe {
                    SetLastError(ERROR_SUCCESS);
                    let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
                    (sent, GetLastError().0)
                };
                let elapsed = started.elapsed().as_micros();
                let cleanup = if mode == MouseProbe::XButtonPair && sent == 1 {
                    // SAFETY: partial dispatch accepted only XDOWN; submit its
                    // matching initialized XUP once to avoid leaving a held button.
                    let (released, error) = unsafe {
                        SetLastError(ERROR_SUCCESS);
                        let released = SendInput(&inputs[1..], std::mem::size_of::<INPUT>() as i32);
                        (released, GetLastError().0)
                    };
                    Some((released, error))
                } else {
                    None
                };
                Ok::<_, String>((sent, count, code, elapsed, cleanup))
            })();
            if reply.send(response).is_err() {
                break;
            }
        }
    });
    let mut rows = Vec::new();
    for mode in [
        MouseProbe::ZeroWheel,
        MouseProbe::ZeroMove,
        MouseProbe::ZeroHWheel,
        MouseProbe::AbsoluteSamePosition,
        MouseProbe::RelativePair,
        MouseProbe::XButtonPair,
    ] {
        for marked in [true, false] {
            pump();
            let tag = if marked { 0x47430000 + rows.len() } else { 0 };
            let before = cursor()?;
            let started = Instant::now();
            OBSERVATION.with(|slot| {
                *slot.borrow_mut() = Some(Observation {
                    tag,
                    started,
                    callbacks: Vec::new(),
                })
            });
            request.try_send((mode, tag)).map_err(|e| e.to_string())?;
            let mut report = None;
            while started.elapsed() < Duration::from_millis(1000) {
                pump();
                match result.try_recv() {
                    Ok(value) => report = Some(value),
                    Err(mpsc::TryRecvError::Empty) => (),
                    Err(error) => return Err(error.to_string()),
                }
                // Retain enough pump time to see asynchronous/button-up callbacks.
                let seen = OBSERVATION.with(|slot| {
                    slot.borrow()
                        .as_ref()
                        .is_some_and(|o| !o.callbacks.is_empty())
                });
                if report.is_some() && seen && started.elapsed() >= Duration::from_millis(300) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            let callbacks = OBSERVATION.with(|slot| slot.borrow_mut().take().unwrap().callbacks);
            let dispatch = report.as_ref().and_then(|r| r.as_ref().ok());
            let row = Row {
                mode: format!("{mode:?}"),
                marked,
                sent: dispatch.map(|r| r.0),
                requested: if matches!(mode, MouseProbe::RelativePair | MouseProbe::XButtonPair) {
                    2
                } else {
                    1
                },
                win32: dispatch.map(|r| r.2),
                send_elapsed_us: dispatch.map(|r| r.3),
                elapsed_us: started.elapsed().as_micros(),
                callback_seen: !callbacks.is_empty(),
                injected_callback_seen: callbacks.iter().any(|c| c.flags & LLMHF_INJECTED != 0),
                dispatch_error: report.as_ref().and_then(|r| r.as_ref().err()).cloned(),
                partial_button_release: dispatch.and_then(|r| r.4),
                callbacks,
                before,
                after: cursor()?,
            };
            println!(
                "MOUSE PROBE: {}",
                serde_json::to_string(&row).map_err(|e| e.to_string())?
            );
            rows.push(row);
            if report.is_none() {
                return Err(
                    "SendInput still pending; matrix stopped without spawning another sender"
                        .into(),
                );
            }
        }
    }
    drop(request);
    sender
        .join()
        .map_err(|_| "mouse diagnostic sender panicked".to_string())?;
    Ok(rows)
}
#[test]
#[ignore = "CI diagnostic only: disposable unelevated interactive desktop; includes cursor movement and XBUTTON"]
fn live_mouse_probe_matrix() {
    assert_eq!(std::env::var("GCU_DESKTOP").as_deref(), Ok("1"));
    let rows = matrix().expect("empirical mouse probe matrix");
    if let Some(path) = std::env::var_os("GCU_MOUSE_PROBE_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
    }
    // No candidate is declared supported merely because SendInput accepted it.
    assert_eq!(rows.len(), 12);
}
