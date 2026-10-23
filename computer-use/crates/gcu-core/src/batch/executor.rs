//! Sequential router attempts. No second policy path and no replay.

use super::{skipped, step_name, timeout_error, BatchReport, BATCH_DEADLINE};
use crate::action::{ActionRouter, ActionState, ToolCall};
use crate::scope::{ScopeLease, SCOPE_DENIED_MESSAGE};
use crate::session::Session;
use gcu_protocol::{
    result::{BatchStepResult, Outcome},
    tools::{BatchStep, ComputerBatchParams},
    ErrorCode, ToolError,
};
use std::time::Duration;

pub(crate) async fn execute(
    router: &ActionRouter,
    session: &Session,
    call_id: &str,
    params: &ComputerBatchParams,
) -> BatchReport {
    let lease = router.fixture_lease();
    let captured = lease.as_ref().and_then(|lease| {
        let authority = lease.data_attempt()?;
        Some((
            authority.scope().run_id().to_owned(),
            authority.scope().generation(),
        ))
    });
    let clock = router.settle_engine.clock();
    let deadline = clock.now().saturating_add(BATCH_DEADLINE);
    let mut steps = Vec::with_capacity(params.steps.len());
    let mut failed_step = None;
    let mut error = None;
    let mut failure = None;
    let mut last = None;
    let mut state = None;

    for (index, step) in params.steps.iter().enumerate() {
        let now = clock.now();
        if now >= deadline {
            error = Some(timeout_error(index as u32));
            failed_step = Some(index as u32);
            push_skipped(&mut steps, &params.steps[index..], index);
            break;
        }
        let mut action = step.clone();
        clamp_wait(&mut action, deadline.saturating_sub(now));
        let name = step_name(&action);
        let mut call = ToolCall::new(format!("{call_id}:{index}"), params.app.clone(), action);
        call.window = params.window.clone();
        let before = clock.now();
        let mut out = router.execute_enqueued(session, call).await;
        let ms = u64::try_from(clock.now().saturating_sub(before).as_millis()).unwrap_or(u64::MAX);
        let record = BatchStepResult {
            i: index as u32,
            tool: name,
            outcome: out.action.outcome,
            effect: out.action.effect,
            ms,
        };
        if params.final_state && out.state.is_some() {
            state = out.state.take();
        }
        let stop = should_stop(&out, params.stop_on_error);
        if stop {
            failed_step = Some(index as u32);
            error = out.error.take();
            if scope_hard_stop(error.as_ref()) {
                state = None;
                if let Some(err) = error.as_mut() {
                    err.state = None;
                }
            }
            failure = Some(out.action);
            last = None;
            steps.push(record);
            push_skipped(&mut steps, &params.steps[index + 1..], index + 1);
            break;
        }
        last = Some(out.action);
        steps.push(record);
    }

    if output_expired(
        lease.as_ref(),
        captured.as_ref(),
        state.as_ref(),
        error.as_ref(),
    ) {
        state = None;
        if let Some(err) = error.as_mut() {
            err.state = None;
        }
    }

    let (scope_run, scope_generation) = match &captured {
        Some((run, generation)) => (Some(run.clone()), Some(*generation)),
        None => (None, None),
    };
    BatchReport {
        steps,
        failed_step,
        error,
        failure,
        last,
        state,
        scope_run,
        scope_generation,
    }
}

fn push_skipped(steps: &mut Vec<BatchStepResult>, rest: &[BatchStep], start: usize) {
    for (offset, step) in rest.iter().enumerate() {
        steps.push(skipped((start + offset) as u32, step));
    }
}

fn clamp_wait(step: &mut BatchStep, remaining: Duration) {
    let BatchStep::WaitFor(args) = step else {
        return;
    };
    let capped = remaining.as_millis().min(30_000);
    let cap = u32::try_from(capped).unwrap_or(30_000).max(1);
    if args.timeout_ms > cap {
        args.timeout_ms = cap;
    }
}

/// Hard stops always end the batch. Other failures honor `stop_on_error`.
/// Unknown is never sent to the driver again.
fn should_stop(out: &crate::action::ToolOutcome, stop_on_error: bool) -> bool {
    if out.action.outcome == Outcome::Unknown || hard_code(out.error.as_ref()) {
        return true;
    }
    stop_on_error && (out.error.is_some() || out.action.outcome != Outcome::Done)
}

fn scope_hard_stop(error: Option<&ToolError>) -> bool {
    error.is_some_and(|err| err.code == ErrorCode::AppDenied && err.message == SCOPE_DENIED_MESSAGE)
}

/// Clear retained output when the attempt epoch is gone, independent of the code.
///
/// Unscoped batches keep their historical state. A scoped line with no producing
/// epoch is cleared. Unknown stays unknown.
fn output_expired(
    lease: Option<&ScopeLease>,
    captured: Option<&(String, u64)>,
    state: Option<&ActionState>,
    error: Option<&ToolError>,
) -> bool {
    if scope_hard_stop(error) {
        return true;
    }
    let Some(lease) = lease else {
        return false;
    };
    let Some((run, generation)) = captured else {
        return state.is_some() || error.is_some_and(|err| err.state.is_some());
    };
    if !lease.same_epoch(run, *generation) {
        return true;
    }
    match state.map(|item| (&item.scope_run, item.scope_generation)) {
        Some((Some(held_run), Some(held_generation))) => {
            held_run != run || held_generation != *generation
        }
        Some((None, None)) => true,
        Some(_) => true,
        None => false,
    }
}

fn hard_code(error: Option<&ToolError>) -> bool {
    matches!(
        error.map(|error| error.code),
        Some(
            ErrorCode::StoppedByUser
                | ErrorCode::ApprovalDenied
                | ErrorCode::ApprovalTimeout
                | ErrorCode::AppDenied
                | ErrorCode::StaleElement
                | ErrorCode::StaleGeometry
                | ErrorCode::StaleScreenshot
                | ErrorCode::Timeout
                | ErrorCode::WindowGone
                | ErrorCode::WindowNotFound
                | ErrorCode::WindowMinimized
        )
    )
}
