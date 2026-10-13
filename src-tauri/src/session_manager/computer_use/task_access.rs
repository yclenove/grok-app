//! Host task decisions route through the original local ACP endpoint.
use super::*;

impl SessionManager {
    pub(crate) async fn approve_computer_use_managed_task(
        self: &Arc<Self>,
        session: &str,
        key: &grok_computer_use_core::task_consent::HostRequestKey,
    ) -> Result<grok_computer_use_core::task_consent::CommitOutcome, String> {
        let endpoint = self.task_picker_endpoint(session)?;
        let outcome = endpoint
            .acp
            .approve_computer_use_managed_task(
                session,
                &endpoint.agent_session_id,
                key,
                self.task_approval_pin(session, &endpoint),
            )
            .await?;
        if !self.endpoint_is_current(session, &endpoint) {
            return Err("Computer Use request owner retired".into());
        }
        Ok(outcome)
    }

    pub(crate) async fn pause_computer_use_task_execution(
        &self,
        session: &str,
        address: &grok_computer_use_core::broker::HostExecutionAddress,
    ) -> Result<(), String> {
        let endpoint = self.task_picker_endpoint(session)?;
        let call = endpoint.acp.computer_use_execution_call(
            session,
            &endpoint.agent_session_id,
            address,
        )?;
        let result = crate::computer_use::task_controls::run(move || {
            call.pause()?;
            Ok(crate::computer_use::task_controls::TaskControlOutput::Paused)
        })
        .await?;
        if !self.endpoint_is_current(session, &endpoint) {
            return Err("Computer Use request owner retired".into());
        }
        match result {
            crate::computer_use::task_controls::TaskControlOutput::Paused => Ok(()),
            _ => Err("Computer Use control reply changed".into()),
        }
    }

    pub(crate) async fn preview_computer_use_task_execution(
        &self,
        session: &str,
        address: &grok_computer_use_core::broker::HostExecutionAddress,
    ) -> Result<grok_computer_use_core::protocol::Observation, String> {
        let endpoint = self.task_picker_endpoint(session)?;
        let call = endpoint.acp.computer_use_execution_call(
            session,
            &endpoint.agent_session_id,
            address,
        )?;
        let result = crate::computer_use::task_controls::run(move || {
            call.preview().map(|observation| {
                crate::computer_use::task_controls::TaskControlOutput::Preview(Box::new(
                    observation,
                ))
            })
        })
        .await?;
        if !self.endpoint_is_current(session, &endpoint) {
            return Err("Computer Use request owner retired".into());
        }
        match result {
            crate::computer_use::task_controls::TaskControlOutput::Preview(observation) => {
                Ok(*observation)
            }
            _ => Err("Computer Use control reply changed".into()),
        }
    }

    pub(crate) fn computer_use_task_watch(
        self: &Arc<Self>,
        session: &str,
    ) -> Result<crate::computer_use::task_events::TaskWatchSource, String> {
        let endpoint = self.task_picker_endpoint(session)?;
        let watch = endpoint
            .acp
            .computer_use_task_watch(session, &endpoint.agent_session_id)?;
        let changes = watch.changes.clone();
        let host_instance_id = watch.host_instance_id.clone();
        let manager = self.clone();
        let session = session.to_string();
        Ok(crate::computer_use::task_events::TaskWatchSource {
            changes,
            host_instance_id,
            read: Arc::new(move || {
                if !manager.endpoint_is_current(&session, &endpoint) {
                    return Err("Computer Use request owner retired".into());
                }
                let view = endpoint.acp.read_computer_use_task_watch(
                    &session,
                    &endpoint.agent_session_id,
                    &watch,
                )?;
                if !manager.endpoint_is_current(&session, &endpoint) {
                    return Err("Computer Use request owner retired".into());
                }
                Ok(view)
            }),
        })
    }

    pub(crate) fn begin_computer_use_task_picker(
        &self,
        session: &str,
        key: &grok_computer_use_core::task_consent::HostRequestKey,
        surface: &str,
    ) -> Result<crate::acp_client::computer_use::TaskPickerStarted, String> {
        let endpoint = self.task_picker_endpoint(session)?;
        self.with_task_publication_endpoint(session, &endpoint, || {
            endpoint.acp.begin_computer_use_task_picker(
                session,
                &endpoint.agent_session_id,
                key,
                surface,
                &endpoint.process_id,
            )
        })
    }

    pub(crate) fn cancel_computer_use_task_picker(
        &self,
        session: &str,
        key: &grok_computer_use_core::task_consent::HostRequestKey,
        picker_id: &str,
    ) -> Result<bool, String> {
        let endpoint = self.task_picker_endpoint(session)?;
        self.with_task_publication_endpoint(session, &endpoint, || {
            if !endpoint.acp.computer_use_task_picker_owner_matches(
                session,
                &endpoint.agent_session_id,
                key,
                picker_id,
                &endpoint.process_id,
            )? {
                return Ok(false);
            }
            endpoint.acp.cancel_computer_use_task_picker(
                session,
                &endpoint.agent_session_id,
                key,
                picker_id,
            )
        })
    }

    pub(crate) fn computer_use_task_picker_preparation(
        &self,
        session: &str,
        key: &grok_computer_use_core::task_consent::HostRequestKey,
        picker_id: &str,
    ) -> Result<crate::acp_client::computer_use::TaskPickerPreparation, String> {
        self.with_original_task_picker(session, key, picker_id, |endpoint| {
            endpoint.acp.computer_use_task_picker_preparation(
                session,
                &endpoint.agent_session_id,
                key,
                picker_id,
            )
        })
    }

    pub(crate) fn confirm_computer_use_task_picker_pairing(
        &self,
        session: &str,
        key: &grok_computer_use_core::task_consent::HostRequestKey,
        picker_id: &str,
        nonce: &str,
    ) -> Result<(), String> {
        self.with_original_task_picker(session, key, picker_id, |endpoint| {
            endpoint.acp.confirm_computer_use_task_picker_pairing(
                session,
                &endpoint.agent_session_id,
                key,
                picker_id,
                nonce,
            )
        })
    }

    pub(crate) async fn load_computer_use_task_picker(
        &self,
        session: &str,
        key: &grok_computer_use_core::task_consent::HostRequestKey,
        picker_id: &str,
    ) -> Result<crate::acp_client::computer_use::TaskPickerView, String> {
        let endpoint = self
            .with_original_task_picker(session, key, picker_id, |endpoint| Ok(endpoint.clone()))?;
        let view = endpoint
            .acp
            .load_computer_use_task_picker(session, &endpoint.agent_session_id, key, picker_id)
            .await?;
        if !self.endpoint_is_current(session, &endpoint) {
            return Err("Computer Use request owner retired".into());
        }
        Ok(view)
    }

    pub(crate) async fn approve_computer_use_task_picker(
        self: &Arc<Self>,
        session: &str,
        key: &grok_computer_use_core::task_consent::HostRequestKey,
        picker_id: &str,
        candidate_ids: Vec<String>,
    ) -> Result<grok_computer_use_core::task_consent::CommitOutcome, String> {
        let endpoint = self
            .with_original_task_picker(session, key, picker_id, |endpoint| Ok(endpoint.clone()))?;
        let outcome = endpoint
            .acp
            .approve_computer_use_task_picker(
                session,
                &endpoint.agent_session_id,
                key,
                picker_id,
                candidate_ids,
                self.task_approval_pin(session, &endpoint),
            )
            .await?;
        if !self.endpoint_is_current(session, &endpoint) {
            return Err("Computer Use request owner retired".into());
        }
        Ok(outcome)
    }

    pub(super) fn task_picker_endpoint(&self, session: &str) -> Result<McpEndpoint, String> {
        crate::computer_use::refuse_if_not_local(session)?;
        let endpoint = self
            .mcp_endpoint(session)
            .ok_or("Computer Use request owner unavailable")?;
        if !self.endpoint_is_current(session, &endpoint) {
            return Err("Computer Use request owner retired".into());
        }
        Ok(endpoint)
    }

    fn with_original_task_picker<R>(
        &self,
        session: &str,
        key: &grok_computer_use_core::task_consent::HostRequestKey,
        picker_id: &str,
        operation: impl FnOnce(&McpEndpoint) -> Result<R, String>,
    ) -> Result<R, String> {
        let endpoint = self.task_picker_endpoint(session)?;
        self.with_task_publication_endpoint(session, &endpoint, || {
            if !endpoint.acp.computer_use_task_picker_owner_matches(
                session,
                &endpoint.agent_session_id,
                key,
                picker_id,
                &endpoint.process_id,
            )? {
                return Err("Computer Use original App picker owner changed".into());
            }
            operation(&endpoint)
        })
    }

    pub(crate) fn computer_use_task_view(
        &self,
        session: &str,
    ) -> Result<grok_computer_use_core::broker::HostTaskView, String> {
        self.with_computer_use_task_endpoint(session, |endpoint| {
            endpoint
                .acp
                .computer_use_task_view(session, &endpoint.agent_session_id)
        })
    }

    pub(crate) fn decide_computer_use_task(
        &self,
        session: &str,
        key: &grok_computer_use_core::task_consent::HostRequestKey,
        decision: grok_computer_use_core::broker::HostTaskDecision,
    ) -> Result<grok_computer_use_core::broker::HostTaskDecisionReceipt, String> {
        self.with_computer_use_task_endpoint(session, |endpoint| {
            endpoint.acp.decide_computer_use_task(
                session,
                &endpoint.agent_session_id,
                key,
                decision,
            )
        })
    }

    fn with_computer_use_task_endpoint<T>(
        &self,
        session: &str,
        operation: impl FnOnce(&McpEndpoint) -> Result<T, String>,
    ) -> Result<T, String> {
        crate::computer_use::refuse_if_not_local(session)?;
        let endpoint = self
            .mcp_endpoint(session)
            .ok_or("Computer Use request owner unavailable")?;
        if !self.endpoint_is_current(session, &endpoint) {
            return Err("Computer Use request owner retired".into());
        }
        let view = operation(&endpoint)?;
        if !self.endpoint_is_current(session, &endpoint) {
            return Err("Computer Use request owner retired".into());
        }
        Ok(view)
    }

    pub(crate) async fn approve_computer_use_task(
        self: &Arc<Self>,
        session: &str,
        request: &str,
        revision: u64,
        selected_targets: Vec<crate::computer_use::task_scope::TaskScopeTarget>,
    ) -> Result<grok_computer_use_core::task_consent::CommitOutcome, String> {
        crate::computer_use::refuse_if_not_local(session)?;
        let endpoint = self
            .mcp_endpoint(session)
            .ok_or("Computer Use request owner unavailable")?;
        if !self.endpoint_is_current(session, &endpoint) {
            return Err("Computer Use request owner retired".into());
        }
        let outcome = endpoint
            .acp
            .approve_computer_use_task(
                session,
                &endpoint.agent_session_id,
                request,
                revision,
                selected_targets,
                self.task_approval_pin(session, &endpoint),
            )
            .await?;
        if !self.endpoint_is_current(session, &endpoint) {
            return Err("Computer Use request owner retired".into());
        }
        Ok(outcome)
    }
}
