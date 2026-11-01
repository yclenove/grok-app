//! One critical section for every scoped publication.
//!
//! Heavy work stays outside. [`ScopeLease::commit_if_current`] holds the lease
//! read lock across a memory-only registration, so revoke and rebind cannot
//! interleave it.

use super::model::LeaseState;
use super::{FixtureScope, ScopeError, ScopeLease};
use std::sync::Arc;

impl ScopeLease {
    /// Register `commit`'s result only while every pair in `authority` is current.
    ///
    /// Takes this lease's read lock, compares each `(run, generation)` with the
    /// active record, runs `commit` before dropping that lock, then releases.
    /// [`Self::revoke`] and [`Self::rebind`] take the write lock, so they wait
    /// until `commit` returns and cannot observe a half-registered product.
    ///
    /// `commit` is memory only. It must not await, write the disk, render,
    /// hash, or walk an element index. It must not call back into this lease:
    /// the lock is not reentrant. An empty `authority`, a revoked lease, or any
    /// pair that is not the active run and generation skips `commit`.
    ///
    /// Acquire all output locks before calling; the closure must not block on
    /// another mutex. Blocking: waits for a writer already in this section.
    /// Timeout: none (the armed test pause is bounded). Cancellation: none.
    /// A poisoned lock is recovered. A later revoke does not change the value
    /// returned here. A later read hides a product whose epoch is no longer
    /// current.
    pub fn commit_if_current<T>(
        &self,
        authority: &[(&str, u64)],
        commit: impl FnOnce() -> T,
    ) -> Result<T, ScopeError> {
        self.expire();
        let guard = self
            .shared
            .state
            .read()
            .unwrap_or_else(|err| err.into_inner());
        let LeaseState::Active { scope, .. } = &*guard else {
            return Err(ScopeError::Revoked);
        };
        if let Some(error) = reject_authority(scope, authority) {
            return Err(error);
        }
        if !self.commit_authority_matches(scope) {
            return Err(ScopeError::StaleGeneration);
        }
        self.shared.commit_during.enter();
        if self.expired(&guard) {
            drop(guard);
            self.expire();
            return Err(ScopeError::Revoked);
        }
        let value = commit();
        drop(guard);
        Ok(value)
    }

    /// Publisher test pause, entered before acquiring output locks or the lease.
    ///
    /// Publishers enter this explicitly outside `commit_if_current`, so test
    /// readers can inspect exports without waiting on a lock held by the pause.
    /// Memory only. Armed pauses are bounded; no cancellation. Production leaves
    /// this disarmed, so entering it does not wait.
    #[must_use]
    pub fn commit_before_gate(&self) -> Arc<crate::action::PublishGate> {
        Arc::clone(&self.shared.commit_before)
    }

    /// Pause inside the lease read lock, before the registration runs.
    ///
    /// Memory only. Armed pauses are bounded; no cancellation. Production leaves
    /// this disarmed.
    /// A revoke from another thread waits until the pause is released and the
    /// registration returns.
    #[must_use]
    pub fn commit_during_gate(&self) -> Arc<crate::action::PublishGate> {
        Arc::clone(&self.shared.commit_during)
    }

    /// Publisher test pause, entered after a successful registration and after
    /// releasing both the lease and all output locks.
    ///
    /// Publishers enter this explicitly outside `commit_if_current`.
    /// Memory only. Armed pauses are bounded; no cancellation. Production leaves
    /// this disarmed.
    /// Not entered when the authority check fails. A revoke here does not change
    /// the value already returned. A later read hides a stale epoch.
    #[must_use]
    pub fn commit_after_gate(&self) -> Arc<crate::action::PublishGate> {
        Arc::clone(&self.shared.commit_after)
    }
}

fn reject_authority(scope: &FixtureScope, authority: &[(&str, u64)]) -> Option<ScopeError> {
    if authority.is_empty() {
        return Some(ScopeError::StaleGeneration);
    }
    if authority.iter().any(|(run, _)| *run != scope.run_id()) {
        return Some(ScopeError::RunMismatch);
    }
    if authority
        .iter()
        .any(|(_, generation)| *generation != scope.generation())
    {
        return Some(ScopeError::StaleGeneration);
    }
    None
}
