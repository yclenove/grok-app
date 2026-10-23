//! The engine and every authority check share one monotonic clock/deadline.
use std::{fmt::Debug, sync::Arc, time::Duration};

use super::{model::LeaseState, FixtureScope, ScopeError, ScopeLease};

/// Injectable engine clock. Implementations must be monotonic and nonblocking.
pub trait Clock: Debug + Send + Sync + 'static {
    /// Time since this clock's origin. Memory only; no waits or cancellation.
    fn now(&self) -> Duration;
}

/// Tokio's monotonic clock, also used by paused-time tests and teardown timers.
#[derive(Debug)]
pub struct Monotonic(tokio::time::Instant);
impl Default for Monotonic {
    fn default() -> Self {
        Self(tokio::time::Instant::now())
    }
}
impl Clock for Monotonic {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

impl ScopeLease {
    /// Trusted engine installation. Data work requires a carried authority.
    /// Memory only; no waits other than the lease lock, no cancellation.
    pub fn install_with_deadline(
        scope: FixtureScope,
        clock: Arc<dyn Clock>,
        deadline: Duration,
    ) -> Self {
        Self::install_inner(scope, Some(clock), Some(deadline))
    }

    /// Trusted control renewal. An expired lease is terminal, even before the
    /// teardown timer runs. Memory only; never awaits or invokes data work.
    pub fn renew_deadline(&self, deadline: Duration) -> Result<(), ScopeError> {
        self.expire();
        let mut guard = self.shared.state.write().unwrap_or_else(|e| e.into_inner());
        if self.expired(&guard) {
            drop(guard);
            self.expire();
            return Err(ScopeError::Revoked);
        }
        match &mut *guard {
            LeaseState::Active {
                deadline: expires, ..
            } => {
                *expires = Some(deadline);
                Ok(())
            }
            LeaseState::Revoked => Err(ScopeError::Revoked),
        }
    }

    /// Remaining engine lease time; no deadline means no automatic expiry.
    /// Memory only; no waits other than the lease lock or cancellation.
    pub fn remaining(&self) -> Option<Duration> {
        self.expire();
        let guard = self.shared.state.read().unwrap_or_else(|e| e.into_inner());
        match &*guard {
            LeaseState::Active {
                deadline: Some(deadline),
                ..
            } => Some(deadline.saturating_sub(self.shared.clock.as_ref()?.now())),
            LeaseState::Active { deadline: None, .. } => None,
            LeaseState::Revoked => Some(Duration::ZERO),
        }
    }

    pub(super) fn expired(&self, state: &LeaseState) -> bool {
        matches!(state, LeaseState::Active { deadline: Some(deadline), .. }
            if self.shared.clock.as_ref().is_some_and(|clock| clock.now() >= *deadline))
    }

    pub(super) fn expire(&self) {
        let guard = self.shared.state.read().unwrap_or_else(|e| e.into_inner());
        if !self.expired(&guard) {
            return;
        }
        drop(guard);
        let mut guard = self.shared.state.write().unwrap_or_else(|e| e.into_inner());
        if self.expired(&guard) {
            *guard = LeaseState::Revoked;
            drop(guard);
            super::model::publish(&self.shared, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn paused_clock_crosses_the_authority_deadline_without_a_timer() {
        let clock = Arc::new(Monotonic::default());
        let scope =
            FixtureScope::try_new("run", 1, "engine", "desktop", "control", vec![]).unwrap();
        let lease = ScopeLease::install_with_deadline(scope, clock.clone(), Duration::from_secs(3));
        let mut authority = lease.arm_attempt().unwrap();
        tokio::time::advance(Duration::from_secs(3)).await;
        assert!(clock.now() >= Duration::from_secs(3));
        assert!(!authority.confirm(&lease));
        assert!(lease.arm_attempt().is_none());
        assert!(lease
            .commit_if_current::<()>(&[("run", 1)], || panic!("expired commit ran"))
            .is_err());
        assert!(lease.renew_deadline(Duration::from_secs(9)).is_err());
    }
}
