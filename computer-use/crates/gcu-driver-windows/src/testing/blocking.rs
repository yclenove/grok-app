//! Explicit native-call barriers for deterministic queue-isolation tests.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Condvar, Mutex,
};

/// A fake native operation remains blocked until `release`, independently of
/// wall time. `wait_entered` acknowledges the exact point the call is blocked.
#[derive(Default)]
pub struct ControlledBlock {
    entered: AtomicBool,
    notice: tokio::sync::Notify,
    released: Mutex<bool>,
    wake: Condvar,
}
impl ControlledBlock {
    pub(super) fn wait(&self) {
        let mut released = self.released.lock().unwrap_or_else(|e| e.into_inner());
        self.entered.store(true, Ordering::Release);
        self.notice.notify_one();
        while !*released {
            released = self.wake.wait(released).unwrap_or_else(|e| e.into_inner());
        }
    }
    /// Wait for a fake native call to acknowledge this barrier. No sleeps or
    /// elapsed-time assertions; use the test runner's watchdog for deadlocks.
    pub async fn wait_entered(&self) {
        loop {
            let notice = self.notice.notified();
            if self.entered.load(Ordering::Acquire) {
                return;
            }
            notice.await;
        }
    }
    /// Release current and future calls using this gate.
    pub fn release(&self) {
        *self.released.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.wake.notify_all();
    }
}
