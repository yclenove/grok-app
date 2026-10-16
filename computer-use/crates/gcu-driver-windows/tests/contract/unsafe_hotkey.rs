//! Independent native evidence when low-level hook proofs are unavailable.
use std::{sync::mpsc, time::Duration};
use windows::Win32::UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*};

fn register(id: i32) -> Result<(), String> {
    // SAFETY: initialize the calling thread's queue; validated fixture-only
    // Ctrl+Alt+Shift+F10 chord. No keys are injected and no actions are delivered.
    unsafe {
        let _ = PeekMessageW(&mut MSG::default(), None, 0, 0, PM_NOREMOVE);
        RegisterHotKey(
            None,
            id,
            MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
            0x79,
        )
    }
    .map_err(|e| {
        let hresult = e.code().0 as u32;
        let win32 = if hresult & 0xffff0000 == 0x80070000 {
            (hresult & 0xffff).to_string()
        } else {
            "unmapped HRESULT".into()
        };
        format!("RegisterHotKey: HRESULT=0x{hresult:08x}; Win32={win32}")
    })
}

pub async fn probe() -> Result<String, String> {
    let work = tokio::task::spawn_blocking(|| {
        let (ready, wait) = mpsc::sync_channel(1);
        let (close, closed) = mpsc::channel::<()>();
        std::thread::Builder::new()
            .name("gcu-hotkey-oracle".into())
            .spawn(move || {
                let result = register(0x4830);
                let registered = result.is_ok();
                let _ = ready.send(result);
                if registered {
                    let _ = closed.recv_timeout(Duration::from_secs(5));
                    // SAFETY: retire this thread's successful registration exactly once.
                    unsafe {
                        let _ = UnregisterHotKey(None, 0x4830);
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        wait.recv_timeout(Duration::from_secs(1))
            .map_err(|e| format!("hotkey oracle readiness: {e}"))??;
        let result = register(0x4831);
        if result.is_ok() {
            // SAFETY: unexpected successful registration still belongs to this thread.
            unsafe {
                let _ = UnregisterHotKey(None, 0x4831);
            }
        }
        drop(close);
        match result {
            Err(reason) if reason.ends_with("Win32=1409") => Ok(format!("RegisterHotKey owner=success (Win32=0); duplicate refused: {reason}; no hook/input injection used")),
            other => Err(format!("native hotkey oracle expected Win32=1409; got {other:?}")),
        }
    });
    tokio::time::timeout(Duration::from_secs(2), work)
        .await
        .map_err(|_| "native hotkey oracle timed out; FFI result unobserved".to_string())?
        .map_err(|e| e.to_string())?
}
