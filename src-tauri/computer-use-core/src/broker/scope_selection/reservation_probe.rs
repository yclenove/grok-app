//! Per-Broker test barrier at the actual original selection reservation entry.
//! Absent from production builds and never exposed through IPC/model tools.
use super::*;

impl ComputerUseBroker {
    pub fn arm_scope_reservation_hook_for_test(&self, hook: Box<dyn FnOnce() + Send>) {
        let mut slot = self.scope_reservation_hook.lock();
        assert!(slot.is_none(), "original reservation hook already armed");
        *slot = Some(hook);
    }

    pub(super) fn run_scope_reservation_hook(&self) {
        let hook = self.scope_reservation_hook.lock().take();
        if let Some(hook) = hook {
            // No Broker, Host, App/ACP or hook registry lock crosses the wait.
            hook();
        }
    }
}
