//! Clip for one saved operation end.
//!
//! `machine` scheduling enters here. A later poll cannot extend the end saved
//! at begin. The charge room is a side channel and is not the recorded deadline.

use std::time::Duration;

use super::super::deadline::{HostClock, MonoInstant};
use super::super::guest::GuestChannel;
use super::super::ledger::{AttemptLedger, VmIdentity};
use super::super::lock::CleanupLock;
use super::super::owner::OwnerSource;
use super::super::receipt::{NotRunReason, Phase};
use super::super::runner::TartRunner;
use super::{Attempt, Scope, StepError};

pub(super) enum Trip {
    Time,
    Overflow,
}

impl<'a, R, G, L, K, O, C> Attempt<'a, R, G, L, K, O, C>
where
    R: TartRunner,
    G: GuestChannel,
    L: AttemptLedger,
    K: CleanupLock,
    O: OwnerSource,
    C: HostClock,
{
    pub(super) fn scope_for_phase(&self) -> Scope {
        if self.cleanup_origin.is_some()
            || matches!(self.phase, Phase::Revoking | Phase::Destroying)
        {
            Scope::Cleanup
        } else if matches!(self.phase, Phase::Ready | Phase::Running) {
            Scope::Task
        } else {
            Scope::Prep
        }
    }

    /// Offline pair written before clone so a lost reply still names one incarnation.
    pub(super) fn fake_allocation() -> VmIdentity {
        VmIdentity::from_parts("inc1".to_owned(), "cfg1".to_owned())
    }

    pub(super) fn bound(
        &self,
        caller: MonoInstant,
        budget: Duration,
        scope: Scope,
        absolute: Option<MonoInstant>,
    ) -> Result<MonoInstant, Trip> {
        let now = self.clock.now();
        let mut end = caller.min(now.checked_add(budget).map_err(|_| Trip::Overflow)?);
        let absolute = match scope {
            Scope::Recovery => None,
            _ => absolute.or(self.absolute),
        };
        if let Some(limit) = absolute {
            if now >= limit {
                return Err(Trip::Time);
            }
            end = end.min(limit);
        }
        if let Some(group) = self.aggregate(now, scope)? {
            end = end.min(group);
        }
        if now >= end {
            Err(Trip::Time)
        } else {
            Ok(end)
        }
    }

    fn aggregate(&self, now: MonoInstant, scope: Scope) -> Result<Option<MonoInstant>, Trip> {
        let (origin, budget) = match scope {
            Scope::Prep => (self.origin.unwrap_or(now), self.budgets.prep),
            Scope::Cleanup => (self.cleanup_origin.unwrap_or(now), self.budgets.cleanup),
            Scope::Task | Scope::Recovery => return Ok(None),
        };
        let end = origin.checked_add(budget).map_err(|_| Trip::Overflow)?;
        if now >= end {
            Err(Trip::Time)
        } else {
            Ok(Some(end))
        }
    }

    pub(super) fn map_trip(&mut self, err: Trip) -> StepError {
        match err {
            Trip::Overflow => StepError::Overflow,
            Trip::Time => {
                self.fail(NotRunReason::Deadline);
                StepError::Expired
            }
        }
    }

    pub(super) fn above_budget(
        &self,
        caller: MonoInstant,
        budget: Duration,
    ) -> Result<(), StepError> {
        let budget_end = self
            .clock
            .now()
            .checked_add(budget)
            .map_err(|_| StepError::Overflow)?;
        if caller > budget_end {
            Err(StepError::AboveBudget)
        } else {
            Ok(())
        }
    }

    /// Time left in this step, capped by `call_end`.
    pub(super) fn arm_room(&self, call_end: MonoInstant) {
        let now = self.clock.now();
        let room = if now >= call_end {
            Duration::ZERO
        } else {
            call_end.saturating_duration_since(now)
        };
        self.clock.set_charge_room(Some(room));
    }

    pub(super) fn clear_room(&self) {
        self.clock.set_charge_room(None);
    }
}
