//! One-shot barrier after the actual producer snapshot, before original proof checks.
//! Test only: no production field, call, model tool or IPC approval interface.
use super::*;

impl ComputerUseBroker {
    pub fn arm_scope_discovery_hook_for_test(&self, hook: Box<dyn FnOnce(usize) + Send>) {
        let mut slot = self.scope_discovery_hook.lock();
        assert!(slot.is_none(), "original discovery hook already armed");
        *slot = Some(hook);
    }

    pub(super) fn run_scope_discovery_hook(&self, count: usize) {
        let hook = self.scope_discovery_hook.lock().take();
        if let Some(hook) = hook {
            // Native candidates have been captured; no Host/Broker/App lock
            // remains held across the test's real HTTP revocation barrier.
            hook(count);
        }
    }
}
