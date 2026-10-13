//! Stable MCP tool errors, serialized inside the transport's `error` envelope.

use crate::types::AppState;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

macro_rules! error_codes {
    ($($variant:ident => $wire:literal: $doc:literal),+ $(,)?) => {
        /// All architecture §11.2 error codes, with stable snake_case serialization.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = "snake_case")]
        pub enum ErrorCode { $(#[doc = $doc] $variant),+ }
        impl ErrorCode {
            /// Complete protocol error catalog in architecture order.
            pub const ALL: [Self; 26] = [$(Self::$variant),+];
            /// Stable wire name, without allocation or IO.
            pub const fn as_str(self) -> &'static str { match self { $(Self::$variant => $wire),+ } }
        }
    };
}

error_codes! {
    AppNotFound => "app_not_found": "No app matches the query.",
    AmbiguousApp => "ambiguous_app": "Multiple apps match; ask the caller to choose.",
    AppDenied => "app_denied": "The app is on the denylist.",
    ApprovalDenied => "approval_denied": "The user refused authorization.",
    ApprovalTimeout => "approval_timeout": "Authorization timed out.",
    WindowMinimized => "window_minimized": "The target window is minimized; restore it or ask the user.",
    WindowNotFound => "window_not_found": "No matching window exists.",
    WindowGone => "window_gone": "The target window closed; attach fresh state when available.",
    StaleElement => "stale_element": "The element fingerprint changed; attach fresh state.",
    StaleGeometry => "stale_geometry": "Window geometry changed; attach fresh state.",
    StaleScreenshot => "stale_screenshot": "The screenshot is no longer current; attach fresh state.",
    OutOfBounds => "out_of_bounds": "Coordinates are outside the screenshot.",
    ElementNotActionable => "element_not_actionable": "The element cannot perform that action; attach fresh state.",
    NotSettable => "not_settable": "The element value cannot be set.",
    InvalidKey => "invalid_key": "The chord cannot be parsed.",
    KeyBlocked => "key_blocked": "Policy blocked the chord or text input.",
    UserActive => "user_active": "Foreground input must wait for the user to become idle.",
    StoppedByUser => "stopped_by_user": "The user stopped execution.",
    Busy => "busy": "The window lock could not be acquired in time.",
    HandoffRequired => "handoff_required": "The user must handle the current prompt.",
    ElevationRequired => "elevation_required": "The target has a higher integrity level.",
    PermissionMissing => "permission_missing": "An OS permission is missing.",
    CapabilityUnavailable => "capability_unavailable": "The required capability is unavailable.",
    Timeout => "timeout": "The operation exceeded its timeout.",
    DriverError => "driver_error": "A driver operation failed.",
    Internal => "internal": "An internal error occurred.",
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Tool-error payload; the MCP adapter wraps this as `{error: ...}` with isError.
/// The retryable flag does not authorize replay of an action with unknown outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Error)]
#[serde(deny_unknown_fields)]
#[error("error: {code}: {message}")]
pub struct ToolError {
    /// Stable protocol code.
    pub code: ErrorCode,
    /// Explanation suitable for the caller.
    pub message: String,
    /// Suggested corrective step.
    pub hint: String,
    /// Whether a new call is appropriate after addressing the cause.
    pub retryable: bool,
    /// Fresh observation for stale targets or unavailable actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<AppState>,
}

/// Generate the error schema and complete code catalog synchronously, with no IO.
pub fn errors_json() -> serde_json::Value {
    serde_json::json!({"codes": ErrorCode::ALL, "error": schemars::schema_for!(ToolError)})
}
