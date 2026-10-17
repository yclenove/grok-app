//! Pure batch checks. No driver calls, waits, or cancellation.

use super::{refused_steps, tool_error, BatchReport, MAX_STEPS};
use crate::wait::supported;
use gcu_protocol::{
    tools::{BatchStep, ComputerBatchParams, Condition},
    ErrorCode,
};

pub(crate) struct PreflightError {
    index: Option<u32>,
    code: ErrorCode,
    message: String,
}

/// Authorization class of one step. Observation steps never plan a mutation.
#[derive(Clone, Copy)]
enum AuthClass {
    Observe,
    Mutate,
}

pub(crate) fn check(params: &ComputerBatchParams) -> Result<(), PreflightError> {
    if params.app.trim().is_empty() {
        return Err(fail(
            None,
            ErrorCode::CapabilityUnavailable,
            "app is required",
        ));
    }
    if params.steps.len() > MAX_STEPS {
        return Err(fail(
            None,
            ErrorCode::CapabilityUnavailable,
            "batch must contain 1..=32 steps",
        ));
    }
    if let Err(err) = params.validate() {
        let count_bad = !(1..=MAX_STEPS).contains(&params.steps.len());
        let index = if count_bad {
            None
        } else {
            params
                .steps
                .iter()
                .position(|step| step.validate().is_err())
                .map(|index| index as u32)
        };
        let code = if err.0.contains("key") {
            ErrorCode::InvalidKey
        } else {
            ErrorCode::CapabilityUnavailable
        };
        return Err(fail(index, code, err.0));
    }
    for (index, step) in params.steps.iter().enumerate() {
        match auth_class(step) {
            AuthClass::Observe => {
                if let Some(condition) = observe_condition(step) {
                    if supported(condition).is_err() {
                        return Err(fail(
                            Some(index as u32),
                            ErrorCode::CapabilityUnavailable,
                            "condition is not supported",
                        ));
                    }
                }
            }
            AuthClass::Mutate => {
                if empty_screenshot(step) {
                    return Err(fail(
                        Some(index as u32),
                        ErrorCode::CapabilityUnavailable,
                        "empty screenshot reference",
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn reject(params: &ComputerBatchParams, err: PreflightError) -> BatchReport {
    BatchReport {
        steps: refused_steps(params),
        failed_step: err.index,
        error: Some(tool_error(err.code, err.message)),
        failure: None,
        last: None,
        state: None,
    }
}

fn auth_class(step: &BatchStep) -> AuthClass {
    match step {
        BatchStep::WaitFor(_) | BatchStep::Expect(_) => AuthClass::Observe,
        BatchStep::Click(_)
        | BatchStep::PerformSecondaryAction(_)
        | BatchStep::SetValue(_)
        | BatchStep::SelectText(_)
        | BatchStep::TypeText(_)
        | BatchStep::PressKey(_)
        | BatchStep::Scroll(_)
        | BatchStep::Drag(_) => AuthClass::Mutate,
    }
}

fn observe_condition(step: &BatchStep) -> Option<&Condition> {
    match step {
        BatchStep::WaitFor(args) => Some(&args.until),
        BatchStep::Expect(condition) => Some(condition),
        BatchStep::Click(_)
        | BatchStep::PerformSecondaryAction(_)
        | BatchStep::SetValue(_)
        | BatchStep::SelectText(_)
        | BatchStep::TypeText(_)
        | BatchStep::PressKey(_)
        | BatchStep::Scroll(_)
        | BatchStep::Drag(_) => None,
    }
}

fn empty_screenshot(step: &BatchStep) -> bool {
    let id = match step {
        BatchStep::Click(args) => args.screenshot_id.as_deref(),
        BatchStep::Drag(args) => args.screenshot_id.as_deref(),
        _ => None,
    };
    id.is_some_and(|id| id.trim().is_empty())
}

fn fail(index: Option<u32>, code: ErrorCode, message: impl Into<String>) -> PreflightError {
    PreflightError {
        index,
        code,
        message: message.into(),
    }
}
