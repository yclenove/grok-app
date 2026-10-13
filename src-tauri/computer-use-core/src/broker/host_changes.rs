//! Coalesced Host invalidations, never authority or retained target content.
use super::ComputerUseBroker;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::Notify;

#[derive(Default)]
struct ChangeState {
    version: AtomicU64,
    wake: Notify,
}

#[derive(Clone, Default)]
pub struct HostTaskChanges(Arc<ChangeState>);

impl HostTaskChanges {
    pub fn version(&self) -> u64 {
        self.0.version.load(Ordering::SeqCst)
    }

    /// Register before testing the version: a mutation between the initial
    /// metadata read and the wait must not be lost. No per-event queue grows.
    pub async fn changed_after(&self, observed: u64) -> u64 {
        loop {
            let notified = self.0.wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let current = self.version();
            if current != observed {
                return current;
            }
            notified.await;
        }
    }

    pub(super) fn publish(&self) {
        self.0.version.fetch_add(1, Ordering::SeqCst);
        self.0.wake.notify_waiters();
    }

    pub(super) fn notice(&self) -> HostChangeNotice {
        HostChangeNotice(self.clone())
    }
}

pub(super) struct HostChangeNotice(HostTaskChanges);
impl Drop for HostChangeNotice {
    fn drop(&mut self) {
        self.0.publish();
    }
}

impl ComputerUseBroker {
    /// Host-only invalidation source. Model tools cannot request this handle.
    /// It does not register a transport or prepare any execution resource.
    pub fn host_task_changes(&self) -> HostTaskChanges {
        self.changes.clone()
    }
    pub fn host_instance_id(&self) -> &str {
        &self.instance_id
    }
}
