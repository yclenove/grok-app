//! Deterministic router-test controls; no changes to the Driver contract.
use crate::{faults::Fault, FakeDriver};
use gcu_core::driver::Result;
use std::sync::atomic::Ordering;
impl FakeDriver {
    /// Queue a fault consumed only by perform, so router reads cannot eat a
    /// mutation timeout/crash. Bounded memory lock; no OS IO or cancellation.
    pub async fn inject_action(&self, fault: Fault) -> Result<()> {
        self.action_faults.lock().await.push_back(fault);
        Ok(())
    }
    /// Number of perform attempts, including definite failures. Nonblocking.
    pub fn perform_calls(&self) -> usize {
        self.perform_calls.load(Ordering::SeqCst)
    }
    /// Number of input release calls. Nonblocking; independent of the world lock.
    pub fn release_calls(&self) -> usize {
        self.release_calls.load(Ordering::SeqCst)
    }
    /// Disable optional event subscription before sharing this driver. No IO/waits.
    pub fn with_events(mut self, enabled: bool) -> Self {
        self.capabilities.observe.events = enabled;
        self
    }
    /// Disable semantic delivery for routing tests. No IO/waits.
    pub fn with_accessibility(mut self, enabled: bool) -> Self {
        self.capabilities.delivery.accessibility = enabled;
        self
    }
}
