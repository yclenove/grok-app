//! Weak callbacks fired when a lease generation is published.
//!
//! Revoke, rebind, and expiry all reach `publish` after the state lock is
//! released. Renewal does not publish. There is no timer and no owner thread:
//! a deadline is published only when some reader calls `expire`.

use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::{Mutex, Weak};

/// Callbacks registered with [`super::ScopeLease::on_invalidated`].
pub(super) struct Invalidation {
    hooks: Mutex<Vec<Weak<dyn Fn() + Send + Sync>>>,
}

impl Invalidation {
    pub(super) fn new() -> Self {
        Self {
            hooks: Mutex::new(Vec::new()),
        }
    }

    /// Keep live hooks, drop dead ones, and append `callback`.
    ///
    /// Memory only. No timeout or cancellation. A poisoned lock is recovered.
    pub(super) fn push(&self, callback: Weak<dyn Fn() + Send + Sync>) {
        let mut hooks = self.hooks.lock().unwrap_or_else(|err| err.into_inner());
        hooks.retain(|hook| hook.upgrade().is_some());
        hooks.push(callback);
    }

    /// Call every live callback outside the hook list lock.
    ///
    /// A panic in one callback is discarded so revoke, rebind, and expiry still
    /// finish and later callbacks still run. The lease transition is already
    /// stored. Blocking: each callback runs inline on this thread. Timeout:
    /// none. Cancellation: none.
    pub(super) fn fire(&self) {
        let live = {
            let mut hooks = self.hooks.lock().unwrap_or_else(|err| err.into_inner());
            let mut live = Vec::with_capacity(hooks.len());
            hooks.retain(|hook| match hook.upgrade() {
                Some(callback) => {
                    live.push(callback);
                    true
                }
                None => false,
            });
            live
        };
        for callback in live {
            // The callback is `Fn`, not `UnwindSafe`. The lease lock is not
            // held here, and a store panic must not unwind into the publisher.
            let _panic = std::panic::catch_unwind(AssertUnwindSafe(|| callback()));
        }
    }
}

impl fmt::Debug for Invalidation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self
            .hooks
            .lock()
            .map(|hooks| hooks.len())
            .unwrap_or_else(|err| err.into_inner().len());
        formatter
            .debug_struct("Invalidation")
            .field("hooks", &count)
            .finish()
    }
}
