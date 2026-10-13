//! Action delivery, outcome, and verification wire semantics (architecture §6).

use crate::tools::ToolName;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Actual delivery route, including refusals with no delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// Accessibility semantic action.
    Accessibility,
    /// Process/window-targeted event.
    TargetedEvent,
    /// Policy-authorized foreground input.
    Foreground,
    /// Clipboard-assisted input.
    Clipboard,
    /// No input was delivered.
    None,
}

/// Execution status; done alone does not establish business success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Delivery succeeded without an error.
    Done,
    /// Policy refused; definitely not executed.
    Refused,
    /// Failure with definitely no side effect.
    Failed,
    /// Delivery started but execution is uncertain; never replay automatically.
    Unknown,
}

/// Visible effect after settling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// State or pixels changed.
    Changed,
    /// No visible change; the action may not have taken effect.
    NoVisibleChange,
    /// No comparison was made.
    NotChecked,
}

/// Element or screenshot-pixel action target, matching architecture §6.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum ActionTarget {
    /// Indexed accessibility element.
    Element {
        /// Element index.
        element_index: u32,
        /// Accessible label.
        label: String,
    },
    /// Screenshot-pixel coordinates.
    Coordinates {
        /// Horizontal screenshot coordinate.
        x: f64,
        /// Vertical screenshot coordinate.
        y: f64,
        /// Screenshot used for mapping.
        screenshot_id: String,
    },
}

/// Verification status of an explicit batch expectation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerifyState {
    /// Expectation is true.
    Satisfied,
    /// Expectation is false.
    Unsatisfied,
    /// Expectation could not be checked.
    Unknown,
}

/// An expectation and its observed truth state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Verification {
    /// Human-readable expectation.
    pub expect: String,
    /// Evaluation state.
    pub state: VerifyState,
}

/// Why the settle wait ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettleReason {
    /// Two successive tree hashes matched.
    TreeStable,
    /// Events were quiet for the required interval.
    EventQuiet,
    /// The hard settle budget expired.
    Budget,
}

/// Reported settling time, without implementing waits in the protocol layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Settle {
    /// Elapsed milliseconds.
    pub ms: u64,
    /// Completion reason.
    pub reason: SettleReason,
}

/// Serializable action result; no blocking, timeouts, or cancellation occur here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionResult {
    /// Tool called.
    pub tool: ToolName,
    /// Resolved target, or null for untargeted keyboard/app actions.
    pub target: Option<ActionTarget>,
    /// Actual delivery route.
    pub delivery: Delivery,
    /// Execution outcome.
    pub outcome: Outcome,
    /// Observed visible effect.
    pub effect: Effect,
    /// Explicit verification or null.
    pub verify: Option<Verification>,
    /// Total action duration in milliseconds.
    pub duration_ms: u64,
    /// Settling metadata.
    pub settle: Settle,
    /// Relevant delivery and observation notes.
    pub notes: Vec<String>,
}

/// Compact per-step batch result (architecture §11.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchStepResult {
    /// Zero-based step number.
    pub i: u32,
    /// Step tool or expect.
    pub tool: String,
    /// Execution outcome.
    pub outcome: Outcome,
    /// Observed effect.
    pub effect: Effect,
    /// Step duration in milliseconds.
    pub ms: u64,
}
