//! Atomic lifecycle transitions. Every check and update shares the same lock.

use super::continuation::{ContinuationSidecar, DispatchState};
use super::continuation_state::mutate;
use super::errors::{ErrorCode, ImportError};
use super::hash::sha256_bytes;

#[cfg(test)]
pub fn mark_dispatching(
    sid: &str,
    prompt: &str,
    agent_session_id: Option<&str>,
) -> Result<DispatchState, ImportError> {
    claim(
        sid,
        prompt,
        agent_session_id,
        &uuid::Uuid::new_v4().to_string(),
        |_| Ok(()),
    )
}

/// The preflight text and capability may change before claim. Recheck both
/// under the same lock used by draft edits and tool-exposure updates.
pub fn mark_dispatching_checked(
    sid: &str,
    prompt: &str,
    agent_session_id: Option<&str>,
    handoff_text_hash: &str,
    policy: super::mcp_stdio::SessionMcpPolicy,
    turn_id: &str,
) -> Result<DispatchState, ImportError> {
    claim(sid, prompt, agent_session_id, turn_id, |side| {
        let current = super::continuation::read_handoff_text(sid)?;
        if sha256_bytes(current.as_bytes()) != handoff_text_hash {
            return Err(ImportError::new(
                ErrorCode::HandoffChanged,
                "handoff changed before dispatch; review the updated text and send again",
            ));
        }
        super::continuation::require_dispatch_capability(side, policy)
    })
}

fn claim(
    sid: &str,
    prompt: &str,
    agent_session_id: Option<&str>,
    turn_id: &str,
    verify: impl FnOnce(&ContinuationSidecar) -> Result<(), ImportError>,
) -> Result<DispatchState, ImportError> {
    if turn_id.is_empty() {
        return Err(ImportError::new(
            ErrorCode::DispatchUnknown,
            "missing dispatch owner",
        ));
    }
    mutate(sid, true, true, |side| {
        if !matches!(
            side.state,
            DispatchState::Prepared | DispatchState::FailedBeforeDispatch
        ) {
            return Err(ImportError::new(
                ErrorCode::DispatchUnknown,
                "previous dispatch may have executed; not replaying",
            ));
        }
        verify(side)?;
        // Host turn UUID already names one attempt. Reuse it so callbacks can
        // capture ownership under the runtime lock without sidecar I/O there.
        side.dispatch_id = Some(turn_id.to_string());
        side.prompt_hash = Some(sha256_bytes(prompt.as_bytes()));
        side.agent_session_id = agent_session_id.map(str::to_string);
        side.state = DispatchState::Dispatching;
        Ok(true)
    })?;
    Ok(DispatchState::Dispatching)
}

/// A current claim is required before invoking ACP. A stale or already-failed
/// attempt must not dispatch merely because a lifecycle callback was a no-op.
pub fn mark_running_for_turn(sid: &str, turn_id: &str) -> Result<(), ImportError> {
    mutate(sid, true, true, |side| {
        if turn_id.is_empty()
            || side.dispatch_id.as_deref() != Some(turn_id)
            || side.state != DispatchState::Dispatching
        {
            return Err(ImportError::new(
                ErrorCode::DispatchUnknown,
                "dispatch attempt no longer owns this handoff",
            ));
        }
        side.state = DispatchState::Running;
        Ok(true)
    })
}

/// Bind the hash of the exact prompt that will be handed to ACP. Continuation
/// claim happens before host UUID hints, history bootstrap, and optional vision
/// rewriting, so the initial claim hash is only provisional.
pub fn finalize_prompt_hash_for_turn(
    sid: &str,
    turn_id: &str,
    prompt: &str,
) -> Result<(), ImportError> {
    mutate(sid, true, true, |side| {
        if turn_id.is_empty()
            || side.dispatch_id.as_deref() != Some(turn_id)
            || side.state != DispatchState::Dispatching
        {
            return Err(ImportError::new(
                ErrorCode::DispatchUnknown,
                "dispatch attempt no longer owns this handoff",
            ));
        }
        side.prompt_hash = Some(sha256_bytes(prompt.as_bytes()));
        Ok(true)
    })
}

/// Lock-free callers pass the identity captured before releasing the runtime
/// turn. The comparison and durable transition occur under the sidecar lock.
pub fn finish_for_turn(
    sid: &str,
    turn_id: &str,
    outcome: DispatchState,
) -> Result<(), ImportError> {
    mutate(sid, false, true, |side| {
        if turn_id.is_empty() || side.dispatch_id.as_deref() != Some(turn_id) {
            return Ok(false);
        }
        let allowed = match outcome {
            DispatchState::FailedBeforeDispatch => side.state == DispatchState::Dispatching,
            DispatchState::Completed | DispatchState::Unknown => {
                matches!(
                    side.state,
                    DispatchState::Dispatching | DispatchState::Running
                )
            }
            _ => false,
        };
        if allowed {
            side.state = outcome;
        }
        Ok(allowed)
    })
}

#[cfg(test)]
pub fn mark_unknown(sid: &str) -> Result<(), ImportError> {
    mutate(sid, true, true, |side| {
        if matches!(
            side.state,
            DispatchState::Completed | DispatchState::FailedBeforeDispatch | DispatchState::Unknown
        ) {
            return Ok(false);
        }
        side.state = DispatchState::Unknown;
        Ok(true)
    })
}

/// Only call with evidence that ACP has not been invoked. Running/Unknown can
/// never be downgraded by a late preflight/journal failure callback.
#[cfg(test)]
pub fn mark_failed_before_dispatch(sid: &str) -> Result<(), ImportError> {
    mutate(sid, false, true, |side| {
        if !matches!(
            side.state,
            DispatchState::Prepared | DispatchState::Dispatching
        ) {
            return Ok(false);
        }
        side.state = DispatchState::FailedBeforeDispatch;
        Ok(true)
    })
}

#[cfg(test)]
pub fn mark_running(sid: &str) -> Result<(), ImportError> {
    mutate(sid, false, true, |side| {
        if side.state != DispatchState::Dispatching {
            return Ok(false);
        }
        side.state = DispatchState::Running;
        Ok(true)
    })
}

#[cfg(test)]
pub fn mark_completed(sid: &str) -> Result<(), ImportError> {
    finish(sid, true, true)
}

#[cfg(test)]
pub fn on_continuation_turn_success(sid: &str) -> Result<(), ImportError> {
    finish(sid, false, true)
}

#[cfg(test)]
pub fn on_continuation_turn_failure(sid: &str) -> Result<(), ImportError> {
    finish(sid, false, false)
}

#[cfg(test)]
fn finish(sid: &str, required: bool, success: bool) -> Result<(), ImportError> {
    mutate(sid, required, true, |side| {
        if matches!(
            side.state,
            DispatchState::Completed | DispatchState::FailedBeforeDispatch | DispatchState::Unknown
        ) {
            return Ok(false);
        }
        side.state = if success {
            DispatchState::Completed
        } else if side.state == DispatchState::Prepared {
            DispatchState::FailedBeforeDispatch
        } else {
            DispatchState::Unknown
        };
        Ok(true)
    })
}
