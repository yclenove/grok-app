//! Versioned worker lifecycle acknowledgements. Idle means physical requests
//! settled; it does not prove that a Stop closed browser contexts.
use serde_json::Value;

use super::WorkerError;
use crate::protocol::JS_MAX_SAFE_INTEGER;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerRunRevision(u64);

impl WorkerRunRevision {
    pub const INITIAL: Self = Self(1);

    pub fn new(value: u64) -> Result<Self, WorkerError> {
        if value == 0 || value > JS_MAX_SAFE_INTEGER {
            return Err(WorkerError::not_started(
                400,
                "invalid_run_revision",
                "worker revision is outside the supported range",
            ));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u64 {
        self.0
    }

    pub fn successor(self) -> Result<Self, WorkerError> {
        Self::new(self.0 + 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerRunPhase {
    Running,
    Paused,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerRunState {
    pub revision: WorkerRunRevision,
    pub phase: WorkerRunPhase,
    pub active_operations: u64,
}

impl WorkerRunState {
    pub fn is_idle(self) -> bool {
        self.active_operations == 0
    }
}

pub fn parse_worker_run_state(
    value: &Value,
    expected: WorkerRunRevision,
) -> Result<WorkerRunState, WorkerError> {
    let invalid = || WorkerError::invalid_response(200);
    if value.get("ok") != Some(&Value::Bool(true))
        || value.get("runRevision").and_then(Value::as_u64) != Some(expected.get())
    {
        return Err(invalid());
    }
    let phase = match value.get("phase").and_then(Value::as_str) {
        Some("running") => WorkerRunPhase::Running,
        Some("paused") => WorkerRunPhase::Paused,
        Some("stopped") => WorkerRunPhase::Stopped,
        _ => return Err(invalid()),
    };
    let active_operations = value
        .get("activeOperations")
        .and_then(Value::as_u64)
        .ok_or_else(invalid)?;
    // This includes retained physical-resource cleanup in addition to the 64
    // ordinary admission slots. Do not reject a legitimately busy worker just
    // because all request slots and a context-cleanup lease coexist.
    if active_operations > JS_MAX_SAFE_INTEGER
        || value.get("idle").and_then(Value::as_bool) != Some(active_operations == 0)
    {
        return Err(invalid());
    }
    Ok(WorkerRunState {
        revision: expected,
        phase,
        active_operations,
    })
}

pub(crate) fn lifecycle_unavailable() -> WorkerError {
    WorkerError::not_started(
        409,
        "run_lifecycle_unavailable",
        "repair the managed browser runtime before controlling its run lifecycle",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::WorkerCompletion;
    use serde_json::json;

    #[test]
    fn resource_cleanup_above_ordinary_capacity_remains_valid_and_busy() {
        // The production worker separately retains owned-resource cleanup.
        // Sixty-four ordinary operations plus one closing context is 65, not
        // an invalid response and never a claim that physical work is idle.
        for phase in ["paused", "stopped"] {
            let pending = json!({"ok":true,"runRevision":1,"phase":phase,
                "activeOperations":65,"idle":false});
            let state = parse_worker_run_state(&pending, WorkerRunRevision::INITIAL).unwrap();
            assert_eq!(state.active_operations, 65);
            assert!(!state.is_idle());
        }
    }

    #[test]
    fn operation_count_must_remain_an_exact_javascript_integer() {
        for count in [JS_MAX_SAFE_INTEGER + 1, u64::MAX] {
            let invalid = json!({"ok":true,"runRevision":1,"phase":"stopped",
                "activeOperations":count,"idle":false});
            assert_eq!(
                parse_worker_run_state(&invalid, WorkerRunRevision::INITIAL)
                    .unwrap_err()
                    .completion,
                WorkerCompletion::Unknown
            );
        }
    }

    #[test]
    fn stale_or_inconsistent_acknowledgements_never_establish_idle() {
        let good =
            json!({"ok":true,"runRevision":1,"phase":"paused","activeOperations":0,"idle":true});
        let initial = WorkerRunRevision::INITIAL;
        assert!(parse_worker_run_state(&good, initial).unwrap().is_idle());
        for (field, wrong) in [
            ("ok", json!(false)),
            ("runRevision", json!(2)),
            ("runRevision", json!("1")),
            ("phase", json!("unknown")),
            ("activeOperations", json!(1)),
            ("activeOperations", json!(65)),
            ("activeOperations", json!(-1)),
            ("activeOperations", json!(0.5)),
            ("idle", json!(false)),
            ("idle", json!("true")),
        ] {
            let mut bad = good.clone();
            bad[field] = wrong;
            assert_eq!(
                parse_worker_run_state(&bad, initial)
                    .unwrap_err()
                    .completion,
                WorkerCompletion::Unknown
            );
        }
        for field in ["ok", "runRevision", "phase", "activeOperations", "idle"] {
            let mut bad = good.clone();
            bad.as_object_mut().unwrap().remove(field);
            assert!(parse_worker_run_state(&bad, initial).is_err());
        }
        let pending =
            json!({"ok":true,"runRevision":1,"phase":"paused","activeOperations":2,"idle":false});
        assert!(!parse_worker_run_state(&pending, initial).unwrap().is_idle());
        assert!(WorkerRunRevision::new(0).is_err());
        assert!(WorkerRunRevision::new(JS_MAX_SAFE_INTEGER)
            .unwrap()
            .successor()
            .is_err());
        assert_eq!(initial.successor().unwrap().get(), 2);
    }
}
