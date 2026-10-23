//! `computer_batch` runs every step through [`crate::action::ActionRouter`].
//!
//! Preflight is pure: a bad parameter, reference, or condition delivers nothing.
//! The batch then enqueues once. Each step still takes the normal authorization,
//! confirmation, Stop, and stale paths. A step runs at most once; an unknown
//! outcome is never replayed.
//!
//! The batch wall clock is [`BATCH_DEADLINE`] (60s) on the settle clock. Each
//! native call stays on [`crate::driver::NATIVE_TIMEOUT`] (5s) and
//! [`crate::driver::DRIVER_DEADLINE`]. This module does not change those ceilings.
//! An in-flight step may finish after the 60s mark; later steps are not started.

mod executor;
mod preflight;

use crate::action::{ActionRouter, ActionState};
use gcu_protocol::{
    result::{ActionResult, BatchStepResult, Effect, Outcome},
    tools::{BatchStep, ComputerBatchParams},
    ErrorCode, ToolError,
};
use std::time::Duration;

/// Wall-clock budget for one batch, measured on the settle clock.
///
/// This is not a native-call budget. [`crate::driver::NATIVE_TIMEOUT`] and
/// [`crate::driver::DRIVER_DEADLINE`] stay 5s and 4750ms.
pub const BATCH_DEADLINE: Duration = Duration::from_secs(60);

/// Maximum steps in one batch, matching architecture §11.4.
pub const MAX_STEPS: usize = 32;

/// One batch attempt. Values only; constructing it does not wait or call a driver.
#[derive(Debug)]
pub struct BatchReport {
    /// One record per submitted step, including steps that never ran.
    pub steps: Vec<BatchStepResult>,
    /// First step that failed, was refused, or was not started after a hard stop.
    /// `None` when every step completed or the batch shape itself was rejected.
    pub failed_step: Option<u32>,
    /// Router or batch error for the stopping step. Expectation misses leave this
    /// empty and report [`gcu_protocol::result::VerifyState::Unsatisfied`] on [`Self::failure`].
    pub error: Option<ToolError>,
    /// Action facts for the step that stopped the batch.
    pub failure: Option<ActionResult>,
    /// Action facts for the last step when the batch ran to completion.
    pub last: Option<ActionResult>,
    /// Freshest observation when the caller asked for final state.
    pub state: Option<ActionState>,
    /// Fixture run that produced the batch. `None` outside a scope.
    /// Not a protocol field.
    pub scope_run: Option<String>,
    /// Fixture generation captured when the batch armed its attempt.
    ///
    /// Memory only. No blocking, timeout, or cancellation. An encoder must not
    /// substitute the current generation when this is missing or stale.
    pub scope_generation: Option<u64>,
}

/// Preflight, then run `params` on `router`.
///
/// Blocking: the session queue has no deadline. Timeout: [`BATCH_DEADLINE`] on the
/// settle clock; each native call still caps at 5s. Cancellation: Stop, session
/// close, or dropping the future. A closed session returns before any driver call.
pub(crate) async fn run(
    router: &ActionRouter,
    session: &crate::session::Session,
    call_id: &str,
    params: ComputerBatchParams,
) -> BatchReport {
    if let Err(err) = preflight::check(&params) {
        return preflight::reject(&params, err);
    }
    match session
        .enqueue(executor::execute(router, session, call_id, &params))
        .await
    {
        Ok(report) => report,
        Err(_) => closed_report(&params),
    }
}

pub(crate) fn refused_steps(params: &ComputerBatchParams) -> Vec<BatchStepResult> {
    params
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| skipped(index as u32, step))
        .collect()
}

pub(crate) fn skipped(index: u32, step: &BatchStep) -> BatchStepResult {
    BatchStepResult {
        i: index,
        tool: step_name(step),
        outcome: Outcome::Refused,
        effect: Effect::NotChecked,
        ms: 0,
    }
}

pub(crate) fn step_name(step: &BatchStep) -> String {
    match step {
        BatchStep::Click(_) => "click",
        BatchStep::PerformSecondaryAction(_) => "perform_secondary_action",
        BatchStep::SetValue(_) => "set_value",
        BatchStep::SelectText(_) => "select_text",
        BatchStep::TypeText(_) => "type_text",
        BatchStep::PressKey(_) => "press_key",
        BatchStep::Scroll(_) => "scroll",
        BatchStep::Drag(_) => "drag",
        BatchStep::WaitFor(_) => "wait_for",
        BatchStep::Expect(_) => "expect",
    }
    .to_owned()
}

pub(crate) fn tool_error(code: ErrorCode, message: impl Into<String>) -> ToolError {
    ToolError {
        code,
        message: message.into(),
        hint: "Use the attached state and correct the arguments before calling again.".into(),
        retryable: false,
        state: None,
    }
}

pub(crate) fn timeout_error(index: u32) -> ToolError {
    ToolError {
        code: ErrorCode::Timeout,
        message: format!("batch exceeded 60s before step {index}"),
        hint: "Inspect completed steps and submit a new batch for any remainder.".into(),
        retryable: false,
        state: None,
    }
}

fn closed_report(params: &ComputerBatchParams) -> BatchReport {
    BatchReport {
        steps: refused_steps(params),
        failed_step: if params.steps.is_empty() {
            None
        } else {
            Some(0)
        },
        error: Some(tool_error(ErrorCode::ApprovalDenied, "approval denied")),
        failure: None,
        last: None,
        state: None,
        scope_run: None,
        scope_generation: None,
    }
}
