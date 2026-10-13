//! Shared read-only native fixture queries; no input or authority mutation.
use super::*;

pub(super) fn authority(
    runtime: &tokio::runtime::Runtime,
    fixture: &mut TaskTransportFixture,
    manager: &SessionManager,
    broker: &crate::computer_use::ComputerUseBroker,
    approvals: &FixtureApprovals,
    allowed: bool,
    require_empty_wire: bool,
) -> Result<Value, String> {
    if !allowed {
        return Err("original lifecycle authority witness unavailable".into());
    }
    if require_empty_wire {
        runtime.block_on(fixture.original_continuation_wire_empty())?;
    }
    Ok(json!({"grants":broker.executable_grant_count("view-app"),
        "continuations":broker.continuation_count("view-app"),
        "ownedWorkers":approvals.owners(),
        "state":manager.computer_use_task_view("view-app")?}))
}

pub(super) fn command(
    broker: &crate::computer_use::ComputerUseBroker,
    value: &Value,
) -> Result<Value, String> {
    match value["command"].as_str().unwrap_or_default() {
        "stopped" => Ok(json!({"ok": broker
            .stop_state(value["runId"].as_str().unwrap_or_default())
            .map_err(|error| error.to_string())? == StopState::Stopped})),
        "preparation-after-stop" => {
            Ok(json!({"shared": broker.tabs().list_shared_candidates().len()}))
        }
        "shared-count" => Ok(json!({"count": broker.tabs().list_shared_candidates().len()})),
        _ => Err("unknown native fixture readback command".into()),
    }
}
