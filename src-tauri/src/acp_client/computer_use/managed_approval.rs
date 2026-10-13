//! Explicit creation of one task browser, never an ambient target picker.
use super::*;
use grok_computer_use_core::broker::{AccessApproval, AccessApprovalReady};
use grok_computer_use_core::task_consent::{CommitOutcome, HostRequestKey};

pub(super) enum ManagedApprovalStart {
    Already(CommitOutcome),
    Prepare(Arc<RequestTransport>, AccessApproval),
}

impl AcpClient {
    pub(super) fn start_managed_approval(
        &self,
        session: &str,
        agent: &str,
        key: &HostRequestKey,
    ) -> Result<ManagedApprovalStart, String> {
        let slots = self.computer_use_connections.lock();
        let original = slots
            .get(session)
            .and_then(|slot| slot.transport.as_ref())
            .filter(|transport| self.is_alive() && transport.bound_to(agent))
            .ok_or("Computer Use request owner unavailable")?
            .clone();
        // Metadata/authority only: no worker, profile, native query or input
        // starts under the ACP lock. Core validates the complete key atomically.
        match original
            .broker
            .reserve_host_managed_access_approval(&original.token, key)
            .map_err(|_| "Computer Use request retired or managed selection unavailable")?
        {
            AccessApprovalReady::Already(outcome) => Ok(ManagedApprovalStart::Already(outcome)),
            AccessApprovalReady::Prepare(approval) => {
                Ok(ManagedApprovalStart::Prepare(original, approval))
            }
        }
    }

    pub(crate) async fn approve_computer_use_managed_task(
        self: &Arc<Self>,
        session: &str,
        agent: &str,
        key: &HostRequestKey,
        pin: TaskApprovalPin,
    ) -> Result<CommitOutcome, String> {
        self.approve_managed_with(session, agent, key, pin, |broker, approval, publication| {
            crate::computer_use::task_access::approve(broker, approval, publication)
        })
        .await
    }

    async fn approve_managed_with<A, AF>(
        self: &Arc<Self>,
        session: &str,
        agent: &str,
        key: &HostRequestKey,
        pin: TaskApprovalPin,
        approve: A,
    ) -> Result<CommitOutcome, String>
    where
        A: FnOnce(
            Arc<crate::computer_use::ComputerUseBroker>,
            AccessApproval,
            crate::computer_use::task_access::ApprovalPublication,
        ) -> AF,
        AF: std::future::Future<Output = Result<CommitOutcome, String>>,
    {
        let start = self.start_managed_approval(session, agent, key)?;
        let publication = match &start {
            ManagedApprovalStart::Already(_) => None,
            ManagedApprovalStart::Prepare(original, _) => {
                Some(self.approval_publication(session, agent, original.clone(), pin))
            }
        };
        self.finish_managed_approval(session, agent, start, move |broker, approval| async move {
            approve(
                broker,
                approval,
                publication.ok_or("Computer Use approval owner unavailable")?,
            )
            .await
        })
        .await
    }

    /// Same constructor/publication path with task-owned storage and supervisor.
    /// Never uses the user's settings, product runtime singleton or Grok home.
    #[cfg(all(test, windows))]
    pub(crate) async fn approve_managed_fixture<F>(
        self: &Arc<Self>,
        key: &HostRequestKey,
        pin: TaskApprovalPin,
        fixture: Arc<crate::computer_use::test_support::FixtureApprovals>,
        prepare_runtime: F,
    ) -> Result<CommitOutcome, String>
    where
        F: FnOnce() -> Result<(), String> + Send + 'static,
    {
        self.approve_managed_with(
            "view-app",
            "synthetic-view-agent",
            key,
            pin,
            move |broker, approval, publication| async move {
                fixture
                    .run(broker, approval, publication, prepare_runtime)
                    .await
            },
        )
        .await
    }

    async fn finish_managed_approval<A, AF>(
        &self,
        session: &str,
        agent: &str,
        start: ManagedApprovalStart,
        approve: A,
    ) -> Result<CommitOutcome, String>
    where
        A: FnOnce(Arc<crate::computer_use::ComputerUseBroker>, AccessApproval) -> AF,
        AF: std::future::Future<Output = Result<CommitOutcome, String>>,
    {
        let (original, approval) = match start {
            ManagedApprovalStart::Already(outcome) => return Ok(outcome),
            ManagedApprovalStart::Prepare(original, approval) => (original, approval),
        };
        if !self.scope_owner_is_current(session, agent, &original) {
            return Err("Computer Use request owner retired".into());
        }
        // The existing owned approval worker retains native preparation and
        // cleanup. Dropping this waiter fences; it never discards that owner.
        let outcome = approve(original.broker.clone(), approval).await?;
        if !self.scope_owner_is_current(session, agent, &original) {
            return Err("Computer Use request owner retired".into());
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests;
