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
    lease: ScopeLease,
    scope: FixtureScope,
    invalidation: watch::Receiver<u64>,
}

impl AttemptAuthority {
    /// Active engine lease, unexpired deadline, and the original generation.
    /// Checks the lease itself, even when no invalidation watcher has run.
    /// Memory only; waits only for the lease lock, with no cancellation.
    pub fn usable(&self) -> bool {
        self.lease.matches(&self.scope) && *self.invalidation.borrow() == self.scope.generation()
    }

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
        if !self.usable() || !lease.matches(&self.scope) {
            return false;
        }
        let marked = *self.invalidation.borrow_and_update();
        marked == self.scope.generation() && self.usable() && lease.matches(&self.scope)
    }

    /// Wait for invalidation or the engine-owned deadline. Cancel-safe.
    ///
    /// Renewal may extend a pending sleep; recheck the same lease on waking.
    /// Deadline-less leases wait only for publication. Dropping cancels the wait.
    pub async fn changed(&mut self) -> Result<(), watch::error::RecvError> {
        loop {
            if !self.usable() {
                return Ok(());
            }
            match self.lease.remaining() {
                Some(remaining) => tokio::select! {
                    changed = self.invalidation.changed() => return changed,
                    _ = tokio::time::sleep(remaining) => {},
                },
                None => return self.invalidation.changed().await,
            }
        }
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
            lease: self.clone(),
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

#[cfg(test)]
mod tests {
    use super::super::Monotonic;
    use super::*;
    use std::{sync::Arc, time::Duration};

    #[tokio::test(start_paused = true)]
    async fn changed_tracks_renewal_and_expiry_without_a_control_watcher() {
        tokio::time::timeout(Duration::from_secs(8), async {
            let scope =
                FixtureScope::try_new("run", 1, "engine", "desktop", "control", vec![]).unwrap();
            let lease = ScopeLease::install_with_deadline(
                scope,
                Arc::new(Monotonic::default()),
                Duration::from_secs(3),
            );
            let mut authority = lease.arm_attempt().unwrap();
            assert!(authority.usable());
            let changed = authority.changed();
            tokio::pin!(changed);
            std::future::poll_fn(|cx| {
                assert!(changed.as_mut().poll(cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            tokio::time::advance(Duration::from_secs(2)).await;
            lease.renew_deadline(Duration::from_secs(6)).unwrap();
            tokio::time::advance(Duration::from_secs(1)).await;
            std::future::poll_fn(|cx| {
                assert!(
                    changed.as_mut().poll(cx).is_pending(),
                    "old deadline closed a renewed connection"
                );
                std::task::Poll::Ready(())
            })
            .await;
            tokio::time::advance(Duration::from_secs(3)).await;
            changed.await.unwrap();
            assert!(lease.current().is_none());
        })
        .await
        .unwrap();
    }
}
