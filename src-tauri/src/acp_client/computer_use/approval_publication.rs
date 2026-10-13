//! Borrowed approval publication pins exactly the original RequestTransport.
use super::*;
use crate::computer_use::task_access::ApprovalPublication;
use grok_computer_use_core::broker::AccessApproval;
use grok_computer_use_core::task_consent::CommitOutcome;

pub(crate) type TaskApprovalPin = Arc<
    dyn Fn(&TaskApprovalOwner, &mut AccessApproval) -> Result<CommitOutcome, String> + Send + Sync,
>;

pub(crate) struct TaskApprovalOwner {
    client: std::sync::Weak<AcpClient>,
    app_session: String,
    agent: String,
    original: Arc<RequestTransport>,
}

impl TaskApprovalOwner {
    pub(crate) fn matches_endpoint(
        &self,
        client: &Arc<AcpClient>,
        session: &str,
        agent: &str,
    ) -> bool {
        self.app_session == session
            && self.agent == agent
            && self
                .client
                .upgrade()
                .is_some_and(|original| Arc::ptr_eq(&original, client))
    }
    /// Caller holds its exact App session slot. This takes only ACP/Broker
    /// authority locks; native work and approval Drop stay with the worker.
    pub(crate) fn commit(&self, approval: &mut AccessApproval) -> Result<CommitOutcome, String> {
        let client = self
            .client
            .upgrade()
            .ok_or("Computer Use request owner retired")?;
        let slots = client.computer_use_connections.lock();
        if !slots
            .get(&self.app_session)
            .and_then(|slot| slot.transport.as_ref())
            .is_some_and(|current| {
                Arc::ptr_eq(current, &self.original)
                    && client.is_alive()
                    && current.bound_to(&self.agent)
            })
        {
            return Err("Computer Use request owner retired".into());
        }
        approval
            .commit_for_transport(&self.original.token)
            .map_err(|_| "Computer Use approval retired before publication".into())
    }
}

impl AcpClient {
    pub(super) fn approval_publication(
        self: &Arc<Self>,
        app_session: &str,
        agent: &str,
        original: Arc<RequestTransport>,
        pin: TaskApprovalPin,
    ) -> ApprovalPublication {
        let owner = TaskApprovalOwner {
            client: Arc::downgrade(self),
            app_session: app_session.into(),
            agent: agent.into(),
            original,
        };
        Arc::new(move |approval| pin(&owner, approval))
    }
}
