//! Disposable harness preparation only. Never called after an action to repair its oracle.
use crate::harness::{progress, require};
use windows::Win32::{
    Foundation::{GetLastError, SetLastError, ERROR_SUCCESS, HWND},
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::WindowsAndMessaging::*,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Foreground {
    pub hwnd: u64,
    pub class: String,
    pub pid: u32,
}
pub fn current() -> Foreground {
    let mut class = [0u16; 256];
    let mut pid = 0;
    // SAFETY: read-only scalar HWND queries with bounded, local output buffers.
    unsafe {
        let h = GetForegroundWindow();
        GetWindowThreadProcessId(h, Some(&mut pid));
        let length = GetClassNameW(h, &mut class).max(0) as usize;
        Foreground {
            hwnd: h.0 as usize as u64,
            class: String::from_utf16_lossy(&class[..length]),
            pid,
        }
    }
}

struct Attached {
    current: u32,
    threads: Vec<u32>,
}
impl Drop for Attached {
    fn drop(&mut self) {
        for thread in self.threads.iter().rev() {
            // SAFETY: balance only successful attachments on this same setup thread.
            let result = unsafe { AttachThreadInput(self.current, *thread, false) };
            progress(format!(
                "HARNESS FOREGROUND SETUP: detach thread={thread}; success={}",
                result.as_bool()
            ));
        }
    }
}

pub fn acquire(window: u64) -> Result<(), String> {
    require(
        std::env::var("GCU_DESKTOP").as_deref() == Ok("1"),
        "foreground setup requires disposable desktop",
    )?;
    let target = HWND(window as usize as *mut _);
    // SAFETY: only the test-owned cover is activated, before the measured action.
    // Every successful input attachment is scoped and detached, including errors.
    unsafe {
        // A spawn_blocking thread may not have a user32 queue yet. Attachment
        // requires one, independently of the cover owner's existing queue.
        let _ = PeekMessageW(&mut MSG::default(), None, 0, 0, PM_NOREMOVE);
        let current_thread = GetCurrentThreadId();
        let mut pid = 0;
        let target_thread = GetWindowThreadProcessId(target, Some(&mut pid));
        require(
            pid == std::process::id(),
            "setup target is not the harness cover",
        )?;
        let allowed = AllowSetForegroundWindow(pid);
        progress(format!("HARNESS FOREGROUND SETUP: AllowSetForegroundWindow own process; result={allowed:?}; preparation only"));
        let foreground_thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let mut attached = Attached {
            current: current_thread,
            threads: Vec::new(),
        };
        for thread in [foreground_thread, target_thread] {
            if thread != 0 && thread != current_thread && !attached.threads.contains(&thread) {
                SetLastError(ERROR_SUCCESS);
                let success = AttachThreadInput(current_thread, thread, true).as_bool();
                progress(format!("HARNESS FOREGROUND SETUP: AttachThreadInput thread={thread}; success={success}; Win32={}", GetLastError().0));
                if success {
                    attached.threads.push(thread);
                }
            }
        }
        SetLastError(ERROR_SUCCESS);
        let accepted = SetForegroundWindow(target).as_bool();
        let code = GetLastError().0;
        drop(attached);
        let mut lock_timeout = 0u32;
        let spi = SystemParametersInfoW(
            SPI_GETFOREGROUNDLOCKTIMEOUT,
            0,
            Some((&mut lock_timeout as *mut u32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let lock_timeout = spi.as_ref().ok().map(|_| lock_timeout);
        let actual = current();
        let detail = format!("SetForegroundWindow returned={accepted}; Win32={code}; expected={window:x}; GetForegroundWindow={actual:?}; SPI_GETFOREGROUNDLOCKTIMEOUT={lock_timeout:?}; SPI result={spi:?}; AllowSetForegroundWindow={allowed:?}; one preparation attempt, no action dispatched");
        progress(format!("HARNESS FOREGROUND SETUP: {detail}"));
        require(
            actual.hwnd == window,
            format!("cover could not acquire foreground: {detail}"),
        )
    }
}
