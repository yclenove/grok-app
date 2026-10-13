//! The Host selection is pinned to the original ACP transport before any await.
use super::*;
use crate::computer_use::task_scope::{SelectionCall, TaskScopeTarget};
use grok_computer_use_core::broker::{AccessApproval, AccessApprovalReady, HostScopeSelectionCall};
use grok_computer_use_core::task_consent::CommitOutcome;

enum Start {
    Already(CommitOutcome),
    Select(Arc<RequestTransport>, HostScopeSelectionCall),
}

impl AcpClient {
    fn start_scope_approval(
        &self,
        app_session: &str,
        agent: &str,
        request: &str,
        revision: u64,
        selected_targets: &[TaskScopeTarget],
    ) -> Result<Start, String> {
        let targets = TaskScopeTarget::parse(selected_targets)?;
        let slots = self.computer_use_connections.lock();
        let transport = slots
            .get(app_session)
            .and_then(|slot| slot.transport.as_ref())
            .filter(|transport| self.is_alive() && transport.bound_to(agent))
            .ok_or("Computer Use request owner unavailable")?
            .clone();
        if let Some(outcome) = transport
            .broker
            .committed_scope_selection(&transport.token, request, revision, &targets)
            .map_err(|_| "Computer Use selection differs or owner retired")?
        {
            return Ok(Start::Already(outcome));
        }
        let selection = transport
            .broker
            .begin_task_scope_group(&transport.token, request, revision, &targets)
            .map_err(|_| "Computer Use request retired or selection unavailable")?;
        Ok(Start::Select(transport, selection))
    }

    pub(super) fn scope_owner_is_current(
        &self,
        app_session: &str,
        agent: &str,
        original: &Arc<RequestTransport>,
    ) -> bool {
        self.is_alive()
            && self
                .computer_use_connections
                .lock()
                .get(app_session)
                .and_then(|slot| slot.transport.as_ref())
                .is_some_and(|transport| {
                    Arc::ptr_eq(transport, original) && transport.bound_to(agent)
                })
    }

    pub(crate) async fn approve_computer_use_task(
        self: &Arc<Self>,
        app_session: &str,
        agent: &str,
        request: &str,
        revision: u64,
        selected_targets: Vec<TaskScopeTarget>,
        pin: TaskApprovalPin,
    ) -> Result<CommitOutcome, String> {
        let start =
            self.start_scope_approval(app_session, agent, request, revision, &selected_targets)?;
        let publication = match &start {
            Start::Already(_) => None,
            Start::Select(original, _) => {
                Some(self.approval_publication(app_session, agent, original.clone(), pin))
            }
        };
        self.finish_scope_approval(
            app_session,
            agent,
            start,
            crate::computer_use::task_scope::reserve,
            move |broker, approval| async move {
                crate::computer_use::task_access::approve(
                    broker,
                    approval,
                    publication.ok_or("Computer Use approval owner unavailable")?,
                )
                .await
            },
        )
        .await
    }

    async fn finish_scope_approval<R, A, RF, AF>(
        &self,
        app_session: &str,
        agent: &str,
        start: Start,
        reserve: R,
        approve: A,
    ) -> Result<CommitOutcome, String>
    where
        R: FnOnce(
            Arc<crate::computer_use::ComputerUseBroker>,
            String,
            HostScopeSelectionCall,
        ) -> RF,
        A: FnOnce(Arc<crate::computer_use::ComputerUseBroker>, AccessApproval) -> AF,
        RF: std::future::Future<Output = Result<AccessApprovalReady, String>>,
        AF: std::future::Future<Output = Result<CommitOutcome, String>>,
    {
        let (transport, selection) = match start {
            Start::Already(outcome) => return Ok(outcome),
            Start::Select(transport, selection) => (transport, selection),
        };
        let _call = SelectionCall::new(&selection);
        // No ACP/SessionManager/Broker mutex spans native capture or preparation.
        let reservation =
            reserve(transport.broker.clone(), transport.token.clone(), selection).await?;
        if !self.scope_owner_is_current(app_session, agent, &transport) {
            return Err("Computer Use request owner retired".into());
        }
        let outcome = match reservation {
            AccessApprovalReady::Already(outcome) => outcome,
            AccessApprovalReady::Prepare(approval) => {
                approve(transport.broker.clone(), approval).await?
            }
        };
        if !self.scope_owner_is_current(app_session, agent, &transport) {
            return Err("Computer Use request owner retired".into());
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests;
