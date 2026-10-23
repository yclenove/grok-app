//! One attempt, one authority.
//!
//! [`ScopeLease::arm_attempt`] subscribes to invalidation before it reads the
//! record. Callers carry that value through admission, dispatch, and publication.
//! A missing or different generation is a refusal. It is not replaced with the
//! generation that happens to be current later.

use std::future::Future;

use tokio::sync::watch;

use super::model::{FixtureScope, ScopeLease};

tokio::task_local! {
    static ATTEMPT_EPOCH: (String, u64);
}

/// Scope record plus the invalidation receiver armed before that read.
///
/// Memory only. Waiting on [`Self::changed`] is the caller's cancellation.
/// Dropping this value does not change the lease.
#[derive(Debug)]
pub struct AttemptAuthority {
    scope: FixtureScope,
    invalidation: watch::Receiver<u64>,
}

impl AttemptAuthority {
    /// Borrow the scope that was current when the receiver was armed.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn scope(&self) -> &FixtureScope {
        &self.scope
    }

    /// Accept this authority only when the lease and the watch still match it.
    ///
    /// A generation that arrived after arming is not marked seen. Memory only.
    /// No blocking, timeout, or cancellation.
    pub fn confirm(&mut self, lease: &ScopeLease) -> bool {
        if !lease.matches(&self.scope) {
            return false;
        }
        if *self.invalidation.borrow() != self.scope.generation() {
            return false;
        }
        let marked = *self.invalidation.borrow_and_update();
        marked == self.scope.generation() && lease.matches(&self.scope)
    }

    /// Wait until a later publish. Cancel-safe. The lease is not polled here.
    ///
    /// Blocking: until the next publish or until this future is dropped.
    /// Timeout: none. Cancellation: dropping the future.
    pub async fn changed(&mut self) -> Result<(), watch::error::RecvError> {
        self.invalidation.changed().await
    }
}

impl ScopeLease {
    /// Subscribe, then read. `None` when the record is gone or the watch disagrees.
    ///
    /// The receiver is not marked past the generation returned here. A publish
    /// that happens with zero receivers is still visible, because the lease stores
    /// it. Memory only. No blocking, timeout, or cancellation.
    pub fn arm_attempt(&self) -> Option<AttemptAuthority> {
        let invalidation = self.subscribe();
        let scope = self.current()?;
        if *invalidation.borrow() != scope.generation() {
            return None;
        }
        Some(AttemptAuthority {
            scope,
            invalidation,
        })
    }
}

/// Epoch the surrounding attempt authorized, if this task set one.
///
/// Memory only. No blocking, timeout, or cancellation. `None` when the caller
/// did not enter [`with_attempt`].
pub(crate) fn expected_epoch() -> Option<(String, u64)> {
    ATTEMPT_EPOCH.try_with(|epoch| epoch.clone()).ok()
}

/// Run `work` on this task with `epoch` as the only authority it may open.
///
/// `None` leaves the task unset, which is the unscoped and direct-driver path.
/// The value does not cross `spawn`. Blocking, timeout, and cancellation are
/// exactly those of `work`.
pub(crate) async fn with_attempt<T>(
    epoch: Option<(String, u64)>,
    work: impl Future<Output = T>,
) -> T {
    match epoch {
        Some(epoch) => ATTEMPT_EPOCH.scope(epoch, work).await,
        None => work.await,
    }
}
