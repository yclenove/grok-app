//! Identity fences around synchronous native callbacks, not an HWND lifetime lock.
use super::*;
use crate::computer_use::windows_identity;
use windows::Win32::Foundation::{GetLastError, SetLastError, ERROR_SUCCESS};

pub(super) type DirectedMessage = (u32, WPARAM, LPARAM);

#[derive(Clone, Copy)]
struct WindowInstance {
    hwnd: HWND,
    pid: u32,
    tid: u32,
    stamp: u64,
}

impl WindowInstance {
    fn capture(hwnd: HWND) -> Result<Self, String> {
        let mut pid = 0;
        let tid = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == 0 || tid == 0 || !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return Err("native input target is no longer alive".into());
        }
        let instance = Self {
            hwnd,
            pid,
            tid,
            stamp: windows_identity::stamp_window(hwnd),
        };
        // stamp_window may fail under UIPI. Never accept an uninstalled stamp.
        instance.check()?;
        Ok(instance)
    }

    fn check(&self) -> Result<(), String> {
        let mut pid = 0;
        let tid = unsafe { GetWindowThreadProcessId(self.hwnd, Some(&mut pid)) };
        if !unsafe { IsWindow(Some(self.hwnd)) }.as_bool()
            || pid != self.pid
            || tid != self.tid
            || windows_identity::read_stamp(self.hwnd) != Some(self.stamp)
        {
            return Err("native input target identity retired".into());
        }
        Ok(())
    }
}

/// Keep the resolved control and its original root across focus, provider and
/// window-procedure calls. Do not recapture after a failed fence or follow a
/// control into a different root. Checks cannot make Win32 dispatch atomic.
pub(super) struct NativeTarget {
    instance: WindowInstance,
    root: WindowInstance,
}

impl NativeTarget {
    pub(super) fn capture(hwnd: HWND) -> Result<Self, String> {
        let instance = WindowInstance::capture(hwnd)?;
        let root_hwnd = unsafe { GetAncestor(hwnd, GA_ROOT) };
        let root = if root_hwnd == hwnd {
            instance
        } else {
            WindowInstance::capture(root_hwnd)?
        };
        let target = Self { instance, root };
        target.check()?;
        Ok(target)
    }

    pub(super) fn authorized(hwnd: HWND, target_id: &str) -> Result<Self, String> {
        let (pid, root, stamp) = windows_identity::parse_target_id(target_id)
            .ok_or("invalid native input authorization")?;
        if unsafe { GetAncestor(hwnd, GA_ROOT) } != root
            || windows_identity::read_stamp(root) != Some(stamp)
        {
            return Err("native control is outside the authorized root".into());
        }
        let target = Self::capture(hwnd)?;
        if target.root.hwnd != root || target.root.pid != pid || target.root.stamp != stamp {
            return Err("native control authorization identity changed".into());
        }
        if windows_identity::is_protected_window(hwnd, target.instance.pid)
            || !windows_identity::can_control_process(target.instance.pid)
        {
            return Err("native control is protected or inaccessible".into());
        }
        target.check()?;
        Ok(target)
    }

    pub(super) fn hwnd(&self) -> HWND {
        self.instance.hwnd
    }

    pub(super) fn check(&self) -> Result<(), String> {
        self.root.check()?;
        self.instance.check()?;
        if unsafe { GetAncestor(self.instance.hwnd, GA_ROOT) } != self.root.hwnd {
            return Err("native control moved outside its original root".into());
        }
        Ok(())
    }

    pub(super) fn send_pair(
        &self,
        down: DirectedMessage,
        up: DirectedMessage,
        uncertain: &Cell<bool>,
    ) -> Result<(), String> {
        self.check()?;
        unsafe { SendMessageW(self.hwnd(), down.0, Some(down.1), Some(down.2)) };
        // A callback may retire/reparent the target during down. Cleanup needs
        // the same native instance, not renewed root authorization.
        self.release_owned(up, uncertain)?;
        self.check()
    }

    pub(super) fn release_owned(
        &self,
        up: DirectedMessage,
        uncertain: &Cell<bool>,
    ) -> Result<(), String> {
        if self.instance.check().is_err() {
            // Never send an up to a replacement. A live but unprovable HWND
            // keeps the native slot held; absence is not permission to inject
            // a global release. No new directed operation can reach a dead HWND.
            if unsafe { IsWindow(Some(self.hwnd())) }.as_bool() {
                uncertain.set(true);
            }
            return Err("native input identity retired before owned release".into());
        }
        unsafe {
            SetLastError(ERROR_SUCCESS);
            SendMessageW(self.hwnd(), up.0, Some(up.1), Some(up.2));
            if GetLastError() != ERROR_SUCCESS {
                uncertain.set(true);
                return Err("native directed input cleanup unconfirmed".into());
            }
        }
        Ok(())
    }
}
