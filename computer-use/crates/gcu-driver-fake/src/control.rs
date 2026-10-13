//! Deterministic router-test controls; no changes to the Driver contract.
use crate::{faults::Fault, FakeDriver};
use gcu_core::driver::Result;
use std::sync::atomic::Ordering;
impl FakeDriver {
    /// Attach a deterministic fixture image digest before sharing the desktop.
    /// Test-only identity evidence, not a hash of a real executable. Synchronous
    /// try-lock; returns a definite-no-effect Timeout if the desktop is locked.
    /// No IO/waits.
    pub fn with_image_digest(self, digest: [u8; 32]) -> Result<Self> {
        {
            let mut world =
                self.world
                    .try_lock()
                    .map_err(|_| gcu_core::driver::DriverError::Timeout {
                        maybe_executed: false,
                    })?;
            let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            for app in &mut world.apps {
                if let Some(identity) = &mut app.info.identity {
                    identity.code_hash = Some(hex.clone());
                }
            }
            for window in &mut world.windows {
                if let Some(identity) = &mut window.reference.app.info.identity {
                    identity.code_hash = Some(hex.clone());
                }
            }
        }
        Ok(self)
    }
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
