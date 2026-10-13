//! macOS `MacApi`. Every OS call stays in this module tree.
//!
//! `on_current_desktop` is always `None`: public CoreGraphics does not report
//! Spaces, and this card does not resolve private SkyLight symbols (P1-06).
//! Window scale is `1.0` until P1-04. Launch uses `NSWorkspace` with
//! `activates = false` and does not prompt.

mod apps;
mod ax;
mod spotlight;
mod windows;

use crate::api::{AxWindow, BundleFacts, CgWindow, MacApi, RunningApp, SpotlightHit};
use gcu_core::driver::Result;

/// Production adapter. Constructing it does not prompt or touch accounts.
#[derive(Clone, Copy, Debug, Default)]
pub struct RealMacApi;

impl RealMacApi {
    /// Create the adapter. Permission prompts are left to the host.
    pub fn new() -> Self {
        Self
    }
}

impl MacApi for RealMacApi {
    fn engine_euid(&self) -> Result<u32> {
        // SAFETY: geteuid has no preconditions and does not modify process state.
        Ok(unsafe { libc::geteuid() })
    }

    fn accessibility_trusted(&self) -> Result<bool> {
        ax::accessibility_trusted()
    }

    fn screen_recording_ok(&self) -> Result<bool> {
        // Preflight only. CGRequestScreenCaptureAccess is intentionally not called.
        Ok(objc2_core_graphics::CGPreflightScreenCaptureAccess())
    }

    fn running_apps(&self) -> Result<Vec<RunningApp>> {
        apps::running_apps()
    }

    fn frontmost_pid(&self) -> Result<Option<u32>> {
        apps::frontmost_pid()
    }

    fn bundle_facts(&self, path: &str) -> Result<BundleFacts> {
        apps::bundle_facts(path)
    }

    fn spotlight_recent(&self) -> Result<Vec<SpotlightHit>> {
        spotlight::spotlight_recent()
    }

    fn windows(&self) -> Result<Vec<CgWindow>> {
        windows::windows()
    }

    fn ax_windows(&self, pid: u32) -> Result<Vec<AxWindow>> {
        ax::ax_windows(pid)
    }

    fn ax_raise(&self, pid: u32, index: u32) -> Result<()> {
        ax::ax_raise(pid, index)
    }

    fn unhide(&self, pid: u32) -> Result<()> {
        apps::unhide(pid)
    }

    fn launch(&self, path: &str) -> Result<()> {
        apps::launch(path)
    }
}
