//! Short shared publication boundary for first approval and explicit recovery.
use super::*;
use crate::acp_client::computer_use::{TaskApprovalOwner, TaskApprovalPin};

fn same_endpoint(
    expected: &McpEndpoint,
    agent: Option<&str>,
    process: &str,
    acp: Option<&Arc<AcpClient>>,
) -> bool {
    agent == Some(expected.agent_session_id.as_str())
        && process == expected.process_id
        && acp.is_some_and(|current| Arc::ptr_eq(current, &expected.acp))
        && expected.acp.is_alive()
}

impl SessionManager {
    /// Session slot → ACP → Broker, with no native I/O or owned handle Drop.
    pub(super) fn with_task_publication_endpoint<R>(
        &self,
        session: &str,
        endpoint: &McpEndpoint,
        publish: impl FnOnce() -> Result<R, String>,
    ) -> Result<R, String> {
        {
            let live = self.inner.lock();
            if let Some(current) = live.as_ref().filter(|s| s.app_session_id == session) {
                return if same_endpoint(
                    endpoint,
                    current.meta.agent_session_id.as_deref(),
                    &current.process_id,
                    current.acp.as_ref(),
                ) {
                    publish()
                } else {
                    Err("Computer Use request owner retired".into())
                };
            }
        }
        {
            let background = self.background.lock();
            if let Some(current) = background.get(session) {
                return if same_endpoint(
                    endpoint,
                    current.meta.agent_session_id.as_deref(),
                    &current.process_id,
                    current.acp.as_ref(),
                ) {
                    publish()
                } else {
                    Err("Computer Use request owner retired".into())
                };
            }
        }
        let parked = self.parked.lock();
        if let Some(current) = parked.get(session).filter(|current| {
            same_endpoint(
                endpoint,
                current.meta.agent_session_id.as_deref(),
                &current.process_id,
                Some(&current.acp),
            )
        }) {
            debug_assert_eq!(current.app_session_id, session);
            return publish();
        }
        Err("Computer Use request owner retired".into())
    }

    pub(super) fn task_approval_pin(
        self: &Arc<Self>,
        session: &str,
        endpoint: &McpEndpoint,
    ) -> TaskApprovalPin {
        let manager = Arc::downgrade(self);
        let session = session.to_owned();
        let endpoint = endpoint.clone();
        Arc::new(move |owner: &TaskApprovalOwner, approval| {
            let manager = manager
                .upgrade()
                .ok_or("Computer Use request owner retired")?;
            manager.with_task_publication_endpoint(&session, &endpoint, || {
                if !owner.matches_endpoint(&endpoint.acp, &session, &endpoint.agent_session_id) {
                    return Err("Computer Use approval owner changed".into());
                }
                owner.commit(approval)
            })
        })
    }
}

#[cfg(test)]
mod tests;
