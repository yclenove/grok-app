//! Wait is a read-only typed action, not a preview polling escape hatch.
use serde::Deserialize;
use serde_json::{json, Value};

use crate::broker::ComputerUseBroker;
use crate::ipc::error;
use crate::protocol::{ActionKind, ActionRequest, ActionTarget, OutcomeKind};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WaitRequest {
    version: u32,
    action_id: String,
    run_id: String,
    target_id: String,
    target_generation: u64,
    snapshot_id: String,
    geometry_revision: u64,
    element_ref: String,
    name_equals: String,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    2000
}

pub(crate) fn wait_for_node(broker: &ComputerUseBroker, run: &str, args: Value) -> Value {
    let request: WaitRequest = match serde_json::from_value(args) {
        Ok(request) => request,
        Err(e) => return error(format!("invalid wait request: {e}")),
    };
    if request.run_id != run {
        return error("run does not match session credential");
    }
    let snapshot_id = request.snapshot_id.clone();
    let outcome = broker.act(ActionRequest {
        version: request.version,
        action_id: request.action_id,
        run_id: request.run_id,
        target_id: request.target_id,
        target_generation: request.target_generation,
        snapshot_id: request.snapshot_id,
        geometry_revision: request.geometry_revision,
        action: ActionKind::Wait,
        target: ActionTarget::Element {
            element_ref: request.element_ref,
        },
        parameters: json!({"nameEquals":request.name_equals,"timeoutMs":request.timeout_ms}),
    });
    let matched = outcome.kind == OutcomeKind::Verified && outcome.executed;
    json!({"isError": !matched, "content":[{"type":"text", "text":json!({
        "matched":matched,"runId":run,"snapshotId":snapshot_id,"outcome":outcome
    }).to_string()}]})
}
