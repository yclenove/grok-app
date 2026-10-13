//! Startup and observation gate.
//!
//! Root is refused for every operation, including `list_apps`. Accessibility and
//! Screen Recording are required for windows and actions, not for listing or
//! resolving bundles through `NSWorkspace`. The gate never prompts and never
//! clicks a system dialog.
//!
//! `com.apple.systemuiserver` is in the P0-08a security-prompt denylist because
//! it owns the menu bar, but it is always running. Treating it as a secure
//! desktop would make the driver unusable. Only a visible layer-0 window owned
//! by SecurityAgent, CoreAuthUI, or UserNotificationCenter closes the gate.

use crate::api::{CgWindow, RunningApp};
use gcu_core::driver::{DriverError, Result};

/// Bundles whose own visible window means a system security prompt is up.
pub const SECURITY_PROMPT_BUNDLES: &[&str] = &[
    "com.apple.SecurityAgent",
    "com.apple.CoreAuthentication.CoreAuthUI",
    "com.apple.UserNotificationCenter",
];

/// Pure permission and elevation checks shared by every guarded route.
#[derive(Clone, Copy, Debug, Default)]
pub struct MacGate;

impl MacGate {
    /// Refuse uid 0. The process is not relaunched or downgraded.
    pub fn engine(euid: u32) -> Result<()> {
        if euid == 0 {
            Err(DriverError::ElevationRequired)
        } else {
            Ok(())
        }
    }

    /// Accessibility is checked before Screen Recording. Names match the host copy.
    pub fn permissions(accessibility: bool, screen_recording: bool) -> Result<()> {
        if !accessibility {
            return Err(DriverError::PermissionMissing {
                permission: "accessibility".into(),
            });
        }
        if !screen_recording {
            return Err(DriverError::PermissionMissing {
                permission: "screen_recording".into(),
            });
        }
        Ok(())
    }
}

/// True when `bundle_id` is one of the three prompt bundles.
pub fn is_security_prompt_bundle(bundle_id: &str) -> bool {
    SECURITY_PROMPT_BUNDLES
        .iter()
        .any(|id| id.eq_ignore_ascii_case(bundle_id))
}

/// A secure desktop is a visible layer-0 window owned by a prompt bundle.
/// Menu-bar processes that are merely running do not count.
pub fn security_prompt(apps: &[RunningApp], windows: &[CgWindow]) -> bool {
    windows.iter().any(|window| {
        window.layer == 0
            && window.alpha > 0.01
            && window.frame.w >= 2.0
            && window.frame.h >= 2.0
            && apps.iter().any(|app| {
                app.pid == window.pid
                    && app
                        .bundle_id
                        .as_deref()
                        .is_some_and(is_security_prompt_bundle)
            })
    })
}
