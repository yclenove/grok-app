//! Explicit model completion reduces authority for one exact task only.
use super::types::{RequestState, TransportContext};
use super::ConsentState;
use crate::error::BrokerError;

impl ConsentState {
    pub(crate) fn complete_model_task(
        &mut self,
        ctx: &TransportContext,
        task_id: &str,
    ) -> Result<Vec<String>, BrokerError> {
        self.require_model_owner(ctx)?;
        let current = self.owners[&ctx.app_session_id].task_id.as_deref();
        if current != Some(task_id) {
            // An exact retry is harmless until a successor task starts. Old
            // callbacks must never complete a new task on the same transport.
            if current.is_none()
                && self.latest_request(ctx).is_some_and(|request| {
                    request.task_id == task_id && request.state == RequestState::Completed
                })
            {
                return Ok(Vec::new());
            }
            return Err(BrokerError::IdentityMismatch("taskId"));
        }
        if !self.requests.iter().any(|request| {
            request.app_session_id == ctx.app_session_id
                && request.owner_nonce == ctx.owner_nonce
                && request.incarnation == ctx.incarnation
                && request.connection_generation == ctx.connection_generation
                && request.task_id == task_id
                && request.state == RequestState::Approved
        }) {
            // Waiting for a decision is an end_turn, never a completed task.
            return Err(BrokerError::TargetUnauthorized);
        }
        Ok(self.complete_task(&ctx.app_session_id))
    }
}
