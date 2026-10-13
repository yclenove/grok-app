//! Pure HWND filtering, UWP unwrapping and generation validation.
use crate::{
    api::{NativeWindow, WinApi},
    identity::normalize_path,
};
use gcu_core::driver::{DriverError, Result};

/// Stamped HWND identity; bare handle/PID/class is insufficient for reuse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HwndStamp {
    /// Numeric HWND.
    pub hwnd: u64,
    /// Top-level owner process (ApplicationFrameHost for packaged windows).
    pub pid: u32,
    /// OS process creation time.
    pub process_created: u64,
    /// Window creation generation (removed by Windows on HWND destruction).
    pub created: u64,
    /// Class name, encoded as UTF-8 bytes in the opaque ID.
    pub class: String,
}
impl From<&NativeWindow> for HwndStamp {
    fn from(w: &NativeWindow) -> Self {
        Self {
            hwnd: w.hwnd,
            pid: w.pid,
            process_created: w.process_created,
            created: w.created,
            class: w.class.clone(),
        }
    }
}
impl HwndStamp {
    /// Opaque identifier; client-provided values must match a retained record.
    pub fn id(&self) -> String {
        let class: String = self.class.bytes().map(|b| format!("{b:02x}")).collect();
        format!(
            "{:x}:{}:{:x}:{:x}:{class}",
            self.hwnd, self.pid, self.process_created, self.created
        )
    }
    /// Verify fresh enumeration against retained stamp; never choose a substitute.
    pub fn validate(&self, current: &NativeWindow) -> Result<()> {
        if self == &Self::from(current) {
            Ok(())
        } else {
            Err(DriverError::WindowGone)
        }
    }
}

/// Filter invisible/tool/untitled utility windows while retaining owned dialogs.
/// Ownership is metadata, not a reason to fold distinct modal HWNDs into the owner.
pub fn visible_windows(windows: Vec<NativeWindow>) -> Vec<NativeWindow> {
    windows
        .into_iter()
        .filter(|w| {
            w.visible
                && (!w.tool_window || w.app_window)
                && (!w.title.is_empty() || w.owner.is_some() || w.app_window)
        })
        .collect()
}

/// Unwrap an ApplicationFrameHost to one unambiguous real child process.
/// An unresolvable/ambiguous frame fails closed; it is never authorized as the host.
pub fn window_process(api: &dyn WinApi, window: &NativeWindow) -> Result<u32> {
    let path = normalize_path(&api.process_path(window.pid)?)?;
    if path.rsplit('\\').next() != Some("applicationframehost.exe") {
        return Ok(window.pid);
    }
    let mut candidates = Vec::new();
    for pid in &window.child_pids {
        if *pid != window.pid && !candidates.contains(pid) {
            let child = normalize_path(&api.process_path(*pid)?)?;
            if child.rsplit('\\').next() != Some("applicationframehost.exe") {
                candidates.push(*pid);
            }
        }
    }
    match candidates.as_slice() {
        [pid] => Ok(*pid),
        [] => Err(DriverError::AppNotFound),
        _ => Err(DriverError::AmbiguousApp),
    }
}
