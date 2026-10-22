//! Connection completion never holds a runtime-map lock across storage IO.
use super::*;
use crate::{
    acp_client::AcpClient,
    error::{AgentError, AgentErrorCode},
    store,
};

struct OpenedExecution {
    expected: store::SessionMeta,
    client: Arc<AcpClient>,
    receipt: crate::session_execution::ExecutionReceipt,
    context: RuntimeWorkspaceContext,
    lease: crate::session_fsm::ConnectionLease,
    needs_bootstrap: bool,
}

/// Tokio cannot abort an already-running blocking worker. Dropping the async
/// completion future must revoke its publication BEFORE timeout cleanup tries
/// to access the runtime, even if that worker has not acquired the store yet.
struct AbandonPublication(crate::session_fsm::ConnectionLease);

impl Drop for AbandonPublication {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl OpenedExecution {
    fn check(&self) -> Result<(), String> {
        self.lease.check()?;
        self.client
            .validate_execution_publication(&self.expected, &self.receipt.agent_session_id)?;
        if !self.client.is_alive()
            || self.client.agent_session_id().as_deref() != Some(&self.receipt.agent_session_id)
        {
            return Err("opened transport ended during execution publication".into());
        }
        Ok(())
    }

    fn matches(&self, session: &LiveSession) -> Result<(), String> {
        self.check()?;
        if session.app_session_id != self.expected.id
            || Some(session.process_id.as_str()) != self.receipt.process_id.as_deref()
            || session.fsm.state() != SessionState::Connecting
            || session.prompt_in_flight
            || session.meta.agent_session_id != self.expected.agent_session_id
            || session.meta.agent_execution != self.expected.agent_execution
            || !session
                .acp
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &self.client))
            || workspace_context::Binding::from(&session.meta)
                != workspace_context::Binding::from(&self.expected)
            || session.workspace_context.as_ref() != Some(&self.context)
        {
            return Err("connection runtime owner changed".into());
        }
        Ok(())
    }
}

pub(super) fn validate_resume(meta: &store::SessionMeta, client: &AcpClient) -> Result<(), String> {
    client.validate_execution_resume(meta)
}

impl SessionManager {
    fn capture_opened_execution(
        &self,
        expected: &store::SessionMeta,
        client: &Arc<AcpClient>,
        process: &str,
        agent: &str,
        needs_bootstrap: bool,
    ) -> Result<OpenedExecution, String> {
        validate_resume(expected, client)?;
        let receipt = client.execution_receipt(agent, process)?;
        self.with_session_mut(&expected.id, |session| {
            let opened = OpenedExecution {
                expected: expected.clone(),
                client: client.clone(),
                receipt,
                context: session
                    .workspace_context
                    .clone()
                    .ok_or("connection execution context is unavailable")?,
                lease: session
                    .fsm
                    .connection_lease()
                    .ok_or("connection lifecycle is unavailable")?,
                needs_bootstrap,
            };
            opened.matches(session)?;
            Ok(opened)
        })
        .ok_or("connection runtime no longer exists")?
    }

    /// Blocking worker only. Cancellation checks touch the lifecycle witness
    /// and transport, NEVER a runtime map from inside a store transaction.
    fn publish_captured_execution(
        &self,
        opened: OpenedExecution,
    ) -> Result<store::SessionMeta, String> {
        opened.check()?;
        let current = crate::session_execution_publication::publish(
            &opened.expected,
            opened.receipt.clone(),
            &opened.context,
            &|| opened.check(),
        )?;
        self.with_session_mut(&opened.expected.id, |session| {
            opened.matches(session)?;
            opened.client.note_execution_publication(&current)?;
            // Only observations are installed. Desired prefs/title may have
            // changed while IO ran; a whole-row runtime replacement is stale.
            session.meta.agent_session_id = current.agent_session_id.clone();
            session.meta.agent_execution = current.agent_execution.clone();
            session.meta.fork_agent_session = current.fork_agent_session;
            session.meta.fork_rewind_prompt_index = current.fork_rewind_prompt_index;
            session.meta.updated_at = session.meta.updated_at.max(current.updated_at);
            session.needs_history_bootstrap = opened.needs_bootstrap;
            session.backend = "grok_agent_stdio".into();
            // No IO while holding this lock. Stop/timeout/replacement of the
            // FSM revokes the old lease, including a same-process reconnect.
            session
                .fsm
                .handshake_ok()
                .map_err(|error| error.to_string())?;
            Self::touch_activity_locked(session);
            Ok(current)
        })
        .ok_or("connection runtime no longer exists")?
    }

    #[cfg(test)]
    pub(super) fn publish_opened_execution(
        &self,
        expected: &store::SessionMeta,
        client: &Arc<AcpClient>,
        process: &str,
        agent: &str,
        needs_bootstrap: bool,
    ) -> Result<store::SessionMeta, String> {
        // Synchronous test entry into the exact two production phases.
        let opened =
            self.capture_opened_execution(expected, client, process, agent, needs_bootstrap)?;
        let _abandon = AbandonPublication(opened.lease.clone());
        self.publish_captured_execution(opened)
    }

    pub(super) async fn complete_opened_execution(
        self: &Arc<Self>,
        expected: &store::SessionMeta,
        client: &Arc<AcpClient>,
        process: &str,
        agent: &str,
        needs_bootstrap: bool,
    ) -> Result<store::SessionMeta, String> {
        let manager = Arc::clone(self);
        let opened =
            self.capture_opened_execution(expected, client, process, agent, needs_bootstrap)?;
        let _abandon = AbandonPublication(opened.lease.clone());
        tokio::task::spawn_blocking(move || manager.publish_captured_execution(opened))
            .await
            .map_err(|error| format!("execution publication worker failed: {error}"))?
    }

    pub(super) async fn reject_opened_execution(
        &self,
        app: &tauri::AppHandle,
        id: &str,
        client: &Arc<AcpClient>,
        process: &str,
        error: &str,
    ) {
        let preserve_owner = self
            .with_session_mut(id, |session| {
                if session.process_id == process
                    && session
                        .acp
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, client))
                {
                    if session.fsm.state() != SessionState::Connecting {
                        // A stale failure cannot detach a newer Ready/busy owner.
                        return session.fsm.state() != SessionState::Disconnected;
                    }
                    let _ = session
                        .fsm
                        .connect_failed(AgentError::new(AgentErrorCode::ConnectFailed, error));
                    session.acp = None;
                }
                false
            })
            .unwrap_or(false);
        self.unregister_pending_child(process);
        // A failed/stale connection owns only its child, never a co-tenant.
        if !preserve_owner && !self.has_other_process_tenant(process, id) {
            Self::kill_acp_bounded(client).await;
        }
        Self::emit_state(app, &self.snapshot());
    }
}
