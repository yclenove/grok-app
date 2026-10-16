//! Disposable harness preparation only. Never called after an action to repair its oracle.
use crate::harness::{progress, require};
use windows::Win32::{
    Foundation::{GetLastError, SetLastError, ERROR_SUCCESS, HWND},
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

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
        let current = GetCurrentThreadId();
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
            current,
            threads: Vec::new(),
        };
        for thread in [foreground_thread, target_thread] {
            if thread != 0 && thread != current && !attached.threads.contains(&thread) {
                SetLastError(ERROR_SUCCESS);
                let success = AttachThreadInput(current, thread, true).as_bool();
                progress(format!("HARNESS FOREGROUND SETUP: AttachThreadInput thread={thread}; success={success}; Win32={}", GetLastError().0));
                if success {
                    attached.threads.push(thread);
                }
            }
        }
        let accepted = SetForegroundWindow(target).as_bool();
        drop(attached);
        if GetForegroundWindow() == target {
            progress(format!("HARNESS FOREGROUND SETUP: SetForegroundWindow accepted={accepted}; expected={window:x}; equality=true; before action"));
            return Ok(());
        }
        // Windows may retain the foreground lock despite attachment. A balanced
        // Alt pair is permitted only in this disposable test preparation path.
        require(
            GetAsyncKeyState(i32::from(VK_MENU.0)) >= 0,
            "physical Alt held; setup refuses to change it",
        )?;
        let alt = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_MENU,
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        };
        let inputs = [alt(KEYBD_EVENT_FLAGS(0)), alt(KEYEVENTF_KEYUP)];
        SetLastError(ERROR_SUCCESS);
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        let code = GetLastError().0;
        let release = if sent == 1 {
            Some(SendInput(&inputs[1..], std::mem::size_of::<INPUT>() as i32))
        } else {
            None
        };
        progress(format!("HARNESS FOREGROUND SETUP: Alt down/up trick; sent={sent}/2; Win32={code}; partial_release={release:?}; preparation only"));
        require(
            sent == 2,
            "Alt setup pair incomplete; foreground preparation failed",
        )?;
        let accepted = SetForegroundWindow(target).as_bool();
        progress(format!("HARNESS FOREGROUND SETUP: after Alt, accepted={accepted}; expected={window:x}; actual={:x}; before action", GetForegroundWindow().0 as usize));
        require(
            GetForegroundWindow() == target,
            "cover could not acquire foreground before action (Allow/Attach/Alt attempted)",
        )
    }
}
