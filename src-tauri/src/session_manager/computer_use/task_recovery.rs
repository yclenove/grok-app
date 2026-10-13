//! The original session slot and ACP pointer are pinned only for publication.
use super::*;
use crate::acp_client::computer_use::{TaskResumeCall, TaskResumeStarted};
use crate::computer_use::task_controls::TaskControlOutput;
use grok_computer_use_core::broker::{HostExecutionAddress, HostTaskResumeReceipt};

#[derive(serde::Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub(crate) enum TaskResumeOutcome {
    InProgress,
    Resumed { address: HostExecutionAddress },
}

impl SessionManager {
    pub(crate) async fn resume_computer_use_task_execution(
        self: &Arc<Self>,
        session: &str,
        address: &HostExecutionAddress,
    ) -> Result<TaskResumeOutcome, String> {
        let endpoint = self.task_picker_endpoint(session)?;
        let execution = endpoint.acp.computer_use_execution_call(
            session,
            &endpoint.agent_session_id,
            address,
        )?;
        let TaskResumeStarted::Prepare(mut call) = execution.reserve_resume()? else {
            return Ok(TaskResumeOutcome::InProgress);
        };
        let cancel = call.cancel_handle();
        let manager = self.clone();
        let owned_session = session.to_string();
        let original = endpoint.clone();
        // The registry, rather than the command waiter, owns preparation and
        // error cleanup. Shutdown fences this reservation and joins its worker.
        let result = crate::computer_use::task_controls::run_fenced(
            move || {
                call.prepare_native(Duration::from_secs(15))?;
                manager
                    .commit_task_resume(&owned_session, &original, &mut call)
                    .map(TaskControlOutput::Resumed)
            },
            move || cancel.fence(),
        )
        .await?;
        if !self.endpoint_is_current(session, &endpoint) {
            return Err("Computer Use request owner retired".into());
        }
        match result {
            TaskControlOutput::Resumed(receipt) => Ok(TaskResumeOutcome::Resumed {
                address: receipt.address,
            }),
            _ => Err("Computer Use control reply changed".into()),
        }
    }

    fn commit_task_resume(
        &self,
        session: &str,
        endpoint: &McpEndpoint,
        call: &mut TaskResumeCall,
    ) -> Result<HostTaskResumeReceipt, String> {
        self.with_task_publication_endpoint(session, endpoint, || call.commit())
    }
}

#[cfg(test)]
mod tests;
