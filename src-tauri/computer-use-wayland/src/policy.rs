//! One authorization's sticky policy lease, shared through parent and adapter
//! handoff. Recovery requires a fresh explicit selection, never a bool reset.
use crate::PortalInputPolicy;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

pub(crate) struct PolicyLease {
    base: Arc<dyn PortalInputPolicy>,
    revoked: AtomicBool,
}

impl PolicyLease {
    pub(crate) fn new(base: Arc<dyn PortalInputPolicy>) -> Arc<Self> {
        Arc::new(Self {
            base,
            revoked: AtomicBool::new(false),
        })
    }

    pub(crate) fn is_revoked(&self) -> bool {
        self.revoked.load(Ordering::Acquire)
    }
}

impl PortalInputPolicy for PolicyLease {
    fn input_available(&self) -> bool {
        if self.is_revoked() {
            return false;
        }
        if !self.base.input_available() || self.base.user_input_active() {
            self.revoked.store(true, Ordering::Release);
        }
        !self.is_revoked()
    }

    fn user_input_active(&self) -> bool {
        !self.input_available()
    }
}

/// Policies must be fast, nonblocking snapshots from their native monitors.
/// Polling is a backstop, not an OS detector: the Host must latch native loss
/// signals so a brief lock/takeover cannot vanish between these checks.
pub(crate) async fn revoked(policy: &dyn PortalInputPolicy) {
    loop {
        if !policy.input_available() || policy.user_input_active() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
