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
    static CONNECTION_AUTHORITY: AttemptAuthority;
    static ATTEMPT_EPOCH: (String, u64);
}

/// Scope record plus the invalidation receiver armed before that read.
///
/// Memory only. Waiting on [`Self::changed`] is the caller's cancellation.
/// Dropping this value does not change the lease.
#[derive(Debug, Clone)]
pub struct AttemptAuthority {
    scope: FixtureScope,
    invalidation: watch::Receiver<u64>,
}

impl AttemptAuthority {
    /// Install the connection authority on this task, including nested work.
    /// Spawned tasks must call this explicitly; Tokio does not inherit locals.
    /// Waiting and cancellation are those of `work`.
    pub async fn enter<T>(&self, work: impl Future<Output = T>) -> T {
        CONNECTION_AUTHORITY.scope(self.clone(), work).await
    }

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
        if let Ok(mut authority) = CONNECTION_AUTHORITY.try_with(Clone::clone) {
            return authority.confirm(self).then_some(authority);
        }
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

    /// Data-plane admission. Engine leases require the original connection's
    /// authority; deadline-less S1 assemblers retain direct in-process access.
    /// Memory only, no waits except lease locks, no cancellation.
    pub fn authorized_current(&self) -> Option<FixtureScope> {
        let scope = self.current()?;
        match CONNECTION_AUTHORITY.try_with(Clone::clone) {
            Ok(mut authority) => {
                if !authority.confirm(self) || authority.scope() != &scope {
                    return None;
                }
            }
            Err(_) if self.shared.clock.is_some() => return None,
            Err(_) => {}
        }
        if let Some((run, generation)) = expected_epoch() {
            if scope.run_id() != run || scope.generation() != generation {
                return None;
            }
        }
        Some(scope)
    }

    // Called under the lease read lock: compare only carried memory, never
    // recursively read the lease. Explicit producing epochs remain sufficient
    // for private encoding workers which intentionally cross spawn_blocking.
    pub(super) fn commit_authority_matches(&self, scope: &FixtureScope) -> bool {
        CONNECTION_AUTHORITY
            .try_with(|authority| {
                authority.scope() == scope && *authority.invalidation.borrow() == scope.generation()
            })
            .unwrap_or(true)
    }

    /// Arm a data operation without acquiring a newer connection generation.
    /// Memory only; no waits except lease locks or cancellation.
    pub fn data_attempt(&self) -> Option<AttemptAuthority> {
        let scope = self.authorized_current()?;
        let mut authority = self.arm_attempt()?;
        (authority.scope() == &scope && authority.confirm(self)).then_some(authority)
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
