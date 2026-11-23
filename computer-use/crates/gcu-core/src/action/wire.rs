//! Wire errors for one router attempt.
//!
//! [`CallError::Code`] keeps the historical `error()` sentence, including the
//! denylist's "app denied". [`CallError::ScopeDenied`] is the fixed evaluation
//! sentence and never carries state.

use super::{ActionState, ToolCall, ToolOutcome};
use crate::driver::DriverError;
use crate::scope::SCOPE_DENIED_MESSAGE;
use gcu_protocol::{
    result::{ActionResult, Delivery, Effect, Outcome, Settle, SettleReason},
    ErrorCode, ToolError,
};

/// Router-internal failure. Policy denials stay [`CallError::Code`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallError {
    /// Historical wire error. The message is the code with underscores removed.
    Code(ErrorCode),
    /// Evaluation scope refusal. The message is [`SCOPE_DENIED_MESSAGE`].
    ScopeDenied,
}

impl From<ErrorCode> for CallError {
    fn from(code: ErrorCode) -> Self {
        Self::Code(code)
    }
}

impl CallError {
    /// Map a driver failure. Scope denial stays distinct from other `app_denied`.
    pub(crate) fn from_driver(err: &DriverError) -> Self {
        match err {
            DriverError::ScopeDenied => Self::ScopeDenied,
            other => Self::Code(other.error_code()),
        }
    }

    /// Map an engine `ErrorCode` that came from [`DriverError::error_code`].
    ///
    /// Wait and settle are the only callers. Policy denylist does not originate
    /// there, so `app_denied` from those engines is scope denial.
    pub(crate) fn from_driver_code(code: ErrorCode) -> Self {
        if code == ErrorCode::AppDenied {
            Self::ScopeDenied
        } else {
            Self::Code(code)
        }
    }

    pub(crate) fn tool_error(self) -> ToolError {
        match self {
            Self::Code(code) => error(code),
            Self::ScopeDenied => ToolError {
                code: ErrorCode::AppDenied,
                message: SCOPE_DENIED_MESSAGE.into(),
                hint: "Stay inside the registered evaluation targets.".into(),
                retryable: false,
                state: None,
            },
        }
    }
}

pub(crate) fn error(code: ErrorCode) -> ToolError {
    ToolError {
        code,
        message: code.as_str().replace('_', " "),
        hint: hint(code).into(),
        retryable: false,
        state: None,
    }
}

/// A next step the model can act on; the generic text left it guessing.
fn hint(code: ErrorCode) -> &'static str {
    use ErrorCode as C;
    match code {
        C::AppNotFound => "Call list_apps for the exact name, or pass the app name to launch_app.",
        C::WindowNotFound | C::WindowGone => {
            "The app has no usable window. Call list_windows, or launch_app to open it, then get_app_state again."
        }
        C::WindowMinimized => "Restore the window with launch_app, then call get_app_state again.",
        C::StaleElement | C::StaleGeometry | C::StaleScreenshot | C::ElementNotActionable => {
            "Call get_app_state for fresh state and use the new element indexes."
        }
        C::OutOfBounds => "Use coordinates inside the latest screenshot.",
        C::UserActive => "The user is using the computer. Wait a few seconds, then retry.",
        C::Busy | C::Timeout => "Retry once; if it fails again, call get_app_state.",
        C::StoppedByUser => {
            "The user pressed Stop. Do not retry; tell the user. They can resume from the Computer Use panel or with `gcu resume` in a terminal."
        }
        C::ApprovalDenied | C::ApprovalTimeout => {
            "The user did not approve this. Do not retry without asking the user."
        }
        C::PermissionMissing => {
            "Ask the user to grant the missing OS permission to Grok Computer Use (macOS: Accessibility and Screen Recording in Privacy & Security)."
        }
        _ => "Use the attached state; obtain fresh user authorization when required.",
    }
}

pub(crate) fn result(call: &ToolCall) -> ActionResult {
    ActionResult {
        tool: super::route::tool(&call.action),
        target: None,
        delivery: Delivery::None,
        outcome: Outcome::Refused,
        effect: Effect::NotChecked,
        verify: None,
        duration_ms: 0,
        settle: Settle {
            ms: 0,
            reason: SettleReason::Budget,
        },
        notes: Vec::new(),
    }
}

pub(crate) fn outcome(
    action: ActionResult,
    error: Option<ToolError>,
    state: Option<ActionState>,
) -> ToolOutcome {
    ToolOutcome {
        action,
        error,
        state,
        windows: Vec::new(),
    }
}

pub(crate) fn wire_delivery(delivery: crate::driver::Delivery) -> Delivery {
    match delivery {
        crate::driver::Delivery::Accessibility => Delivery::Accessibility,
        crate::driver::Delivery::TargetedEvent => Delivery::TargetedEvent,
        crate::driver::Delivery::Foreground => Delivery::Foreground,
        crate::driver::Delivery::Clipboard => Delivery::Clipboard,
    }
}
