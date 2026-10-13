//! Model acknowledgement is a fence, not proof of physical cleanup or success.
use super::*;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompletionRequest {
    task_id: String,
}

impl ComputerUseBroker {
    pub fn complete_model_task(&self, token: &str, args: Value) -> Result<Value, BrokerError> {
        let request: CompletionRequest = serde_json::from_value(args)
            .map_err(|_| BrokerError::Schema("only taskId is accepted".into()))?;
        if request.task_id.trim().is_empty() || request.task_id.len() > 256 {
            return Err(BrokerError::IdentityMismatch("taskId"));
        }
        let _change = self.changes.notice();
        let ctx = self.live_context(token)?;
        let busy = self.busy_run_ids();
        let runs = {
            let mut inner = self.inner.lock();
            inner.consent.sweep_from_host(&busy);
            inner.consent.complete_model_task(&ctx, &request.task_id)
        };
        self.handoff_expired();
        self.handoff_fences(&runs?);
        Ok(json!({
            "taskId": request.task_id,
            "state": "completion_requested",
            "authority": "none"
        }))
    }
}
