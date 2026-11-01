//! The only dispatch path. Callers pass their deadline. This applies
//! `min(caller, phase budget, attempt absolute, enclosing aggregate)`.

use std::time::Duration;

use super::super::super::Probe;
use super::super::deadline::{
    AttemptDeadline, CloneDeadline, DeleteDeadline, LeaseDeadline, LoginDeadline, MonoInstant,
    ProbeDeadline, QueryDeadline, RevokeDeadline, RunDeadline, SealDeadline, StageDeadline,
    StartDeadline, StopDeadline,
};
use super::super::guest::GuestChannel;
use super::super::identity::{CloneId, GoldenImage, RunId};
use super::super::ledger::{
    AttemptLedger, ClonePermit, LedgerError, LedgerFact, Reservation, VmIdentity,
};
use super::super::lock::CleanupLock;
use super::super::output::{BoundedOutput, CallError};
use super::super::parse::{self, VmStatus};
use super::super::policy::SoftnetFlags;
use super::super::receipt::{NotRunReason, Phase};
use super::super::runner::TartRunner;
use super::{call_reason, Attempt, Scope, StepError};

pub(super) enum Trip {
    Time,
    Overflow,
    OwnerDead,
}

struct Timed<T> {
    at: MonoInstant,
    value: T,
}

macro_rules! phase_fn {
    ($($name:ident($($p:ident: $ty:ty),*) $budget:ident => $body:expr;)*) => {$(
        pub(super) fn $name(
            &mut self,
            $($p: $ty,)*
            caller: MonoInstant,
            scope: Scope,
        ) -> Result<BoundedOutput, StepError> {
            self.phase_out(caller, self.budgets.$budget, scope, $body)
        }
    )*};
}

macro_rules! ledger_fn {
    ($($name:ident($($p:ident: $ty:ty),*) -> $ret:ty, $budget:ident => $body:expr;)*) => {$(
        pub(super) fn $name(
            &mut self,
            $($p: $ty,)*
            caller: MonoInstant,
            scope: Scope,
            absolute: Option<MonoInstant>,
        ) -> Result<$ret, LedgerError> {
            self.ledger_at(caller, self.budgets.$budget, scope, absolute, $body)
        }
    )*};
}

impl<'a, R, G, L, K, C> Attempt<'a, R, G, L, K, C>
where
    R: TartRunner,
    G: GuestChannel,
    L: AttemptLedger,
    K: CleanupLock,
    C: super::super::deadline::HostClock,
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

    /// Stop and delete recheck the lease here. Expiry enters the hold and does not dispatch.
    fn issue<T>(
        &mut self,
        caller: MonoInstant,
        budget: Duration,
        scope: Scope,
        absolute: Option<MonoInstant>,
        destructive: bool,
        call: impl FnOnce(&mut Self, MonoInstant) -> T,
    ) -> Result<Timed<T>, Trip> {
        if destructive && self.lease_expired() {
            self.expire_owner(Some(caller), scope);
            return Err(Trip::OwnerDead);
        }
        let at = self.bound(caller, budget, scope, absolute)?;
        Ok(Timed {
            at,
            value: call(self, at),
        })
    }

    pub(super) fn map_trip(&mut self, err: Trip) -> StepError {
        match err {
            Trip::OwnerDead => StepError::OwnerDead,
            Trip::Overflow => StepError::Overflow,
            Trip::Time => {
                self.fail(NotRunReason::Deadline);
                StepError::Expired
            }
        }
    }

    fn trip_reason(err: Trip) -> NotRunReason {
        match err {
            Trip::OwnerDead => NotRunReason::OwnerDeath,
            Trip::Time | Trip::Overflow => NotRunReason::Deadline,
        }
    }

    fn phase_out(
        &mut self,
        caller: MonoInstant,
        budget: Duration,
        scope: Scope,
        call: impl FnOnce(&mut Self, MonoInstant) -> Result<BoundedOutput, CallError>,
    ) -> Result<BoundedOutput, StepError> {
        match self.issue(caller, budget, scope, None, false, call) {
            Err(err) => Err(self.map_trip(err)),
            Ok(timed) => timed.value.map_err(|err| {
                self.fail(call_reason(&err));
                StepError::Call(err)
            }),
        }
    }

    fn runner_out(
        &mut self,
        caller: MonoInstant,
        budget: Duration,
        scope: Scope,
        absolute: Option<MonoInstant>,
        call: impl FnOnce(&mut Self, MonoInstant) -> Result<BoundedOutput, CallError>,
    ) -> Result<BoundedOutput, NotRunReason> {
        self.issue(caller, budget, scope, absolute, false, call)
            .map_err(Self::trip_reason)?
            .value
            .map_err(|err| call_reason(&err))
    }

    /// An `absent` body is success and still returns the clipped instant.
    fn owned_gone(
        &mut self,
        caller: MonoInstant,
        budget: Duration,
        scope: Scope,
        call: impl FnOnce(&mut Self, MonoInstant) -> Result<BoundedOutput, CallError>,
    ) -> Result<MonoInstant, NotRunReason> {
        let timed = self
            .issue(caller, budget, scope, None, true, call)
            .map_err(Self::trip_reason)?;
        match timed.value {
            Ok(_) => Ok(timed.at),
            Err(err) if super::absent_reply(&err) => Ok(timed.at),
            Err(err) => Err(call_reason(&err)),
        }
    }

    phase_fn! {
        guest_inventory() query => |machine, at| {
            machine.guest.inventory(QueryDeadline::from_instant(at))
        };
        guest_probe(probe: Probe) probe => |machine, at| {
            machine.guest.probe(probe, ProbeDeadline::from_instant(at))
        };
        guest_bootstrap() stage => |machine, at| {
            machine.guest.bootstrap(StageDeadline::from_instant(at))
        };
        guest_login() login => |machine, at| {
            machine.guest.login(LoginDeadline::from_instant(at))
        };
        guest_run() run => |machine, at| {
            machine.client_starts = machine.client_starts.saturating_add(1);
            machine.guest.run(RunDeadline::from_instant(at))
        };
        guest_revoke() revoke => |machine, at| {
            machine.guest.revoke(RevokeDeadline::from_instant(at))
        };
        runner_clone(permit: ClonePermit, golden: GoldenImage) clone_limit => |machine, at| {
            machine.runner.clone_vm(permit, golden, CloneDeadline::from_instant(at))
        };
    }

    /// Clipped instant stored as the lease end. The body is not kept.
    pub(super) fn guest_lease(
        &mut self,
        seq: std::num::NonZeroU64,
        caller: MonoInstant,
        scope: Scope,
    ) -> Result<MonoInstant, StepError> {
        match self.issue(
            caller,
            self.budgets.lease,
            scope,
            None,
            false,
            |machine, at| machine.guest.lease(seq, LeaseDeadline::from_instant(at)),
        ) {
            Err(err) => Err(self.map_trip(err)),
            Ok(timed) => match timed.value {
                Ok(_) => Ok(timed.at),
                Err(err) => {
                    self.fail(call_reason(&err));
                    Err(StepError::Call(err))
                }
            },
        }
    }

    pub(super) fn listed(
        &mut self,
        caller: MonoInstant,
        scope: Scope,
        absolute: Option<MonoInstant>,
    ) -> Result<Vec<String>, NotRunReason> {
        let body = self.runner_out(
            caller,
            self.budgets.query,
            scope,
            absolute,
            |machine, at| machine.runner.list(QueryDeadline::from_instant(at)),
        )?;
        parse::parse_list(body.as_bytes()).map_err(|_| NotRunReason::Malformed)
    }

    pub(super) fn vm_status(
        &mut self,
        clone: &CloneId,
        caller: MonoInstant,
        scope: Scope,
        absolute: Option<MonoInstant>,
    ) -> Result<VmStatus, NotRunReason> {
        let body = self.runner_out(
            caller,
            self.budgets.query,
            scope,
            absolute,
            |machine, at| {
                machine
                    .runner
                    .status(clone, QueryDeadline::from_instant(at))
            },
        )?;
        parse::parse_status(body.as_bytes()).map_err(|_| NotRunReason::Malformed)
    }

    pub(super) fn stop_owned(
        &mut self,
        clone: &CloneId,
        caller: MonoInstant,
        scope: Scope,
    ) -> Result<MonoInstant, NotRunReason> {
        self.owned_gone(caller, self.budgets.stop, scope, |machine, at| {
            machine.runner.stop(clone, StopDeadline::from_instant(at))
        })
    }

    pub(super) fn delete_owned(
        &mut self,
        clone: &CloneId,
        caller: MonoInstant,
        scope: Scope,
    ) -> Result<MonoInstant, NotRunReason> {
        self.owned_gone(caller, self.budgets.delete, scope, |machine, at| {
            machine
                .runner
                .delete(clone, DeleteDeadline::from_instant(at))
        })
    }

    pub(super) fn runner_start(
        &mut self,
        clone: &CloneId,
        flags: &SoftnetFlags,
        caller: MonoInstant,
        scope: Scope,
    ) -> Result<(), StepError> {
        self.phase_out(caller, self.budgets.start, scope, |machine, at| {
            machine
                .runner
                .start(clone, flags, StartDeadline::from_instant(at))
        })
        .map(|_| ())
    }

    fn ledger_at<T>(
        &mut self,
        caller: MonoInstant,
        budget: Duration,
        scope: Scope,
        absolute: Option<MonoInstant>,
        call: impl FnOnce(&mut Self, MonoInstant) -> Result<T, LedgerError>,
    ) -> Result<T, LedgerError> {
        match self.issue(caller, budget, scope, absolute, false, call) {
            Ok(timed) => timed.value,
            Err(_) => Err(LedgerError::Expired),
        }
    }

    ledger_fn! {
        ledger_rows() -> Vec<Reservation>, query => |machine, at| {
            machine.ledger.rows(QueryDeadline::from_instant(at))
        };
        ledger_reserve(
            run: RunId,
            clone: CloneId,
            absolute_deadline: AttemptDeadline
        ) -> ClonePermit, query => |machine, at| {
            machine.ledger.reserve(run, clone, absolute_deadline, QueryDeadline::from_instant(at))
        };
        ledger_query(clone: &CloneId) -> Reservation, query => |machine, at| {
            machine.ledger.query(clone, QueryDeadline::from_instant(at))
        };
        ledger_append(clone: &CloneId, fact: LedgerFact) -> (), query => |machine, at| {
            machine.ledger.append(clone, fact, QueryDeadline::from_instant(at))
        };
    }

    pub(super) fn ledger_note(
        &mut self,
        clone: &CloneId,
        identity: &VmIdentity,
        caller: MonoInstant,
        scope: Scope,
    ) -> Result<(), LedgerError> {
        self.ledger_at(caller, self.budgets.query, scope, None, |machine, at| {
            machine
                .ledger
                .note_identity(clone, identity, QueryDeadline::from_instant(at))
        })
    }

    pub(super) fn ledger_seal(
        &mut self,
        clone: &CloneId,
        caller: MonoInstant,
        scope: Scope,
    ) -> Result<(), LedgerError> {
        self.ledger_at(caller, self.budgets.seal, scope, None, |machine, at| {
            machine.ledger.seal(clone, SealDeadline::from_instant(at))
        })
    }

    fn read_back(
        &mut self,
        wrote: Result<(), LedgerError>,
        clone: &CloneId,
        caller: MonoInstant,
        scope: Scope,
        present: impl FnOnce(&Reservation) -> bool,
    ) -> Result<bool, LedgerError> {
        match wrote {
            Ok(()) => Ok(true),
            Err(LedgerError::ReplyLost) => match self.ledger_query(clone, caller, scope, None) {
                Ok(row) => Ok(present(&row)),
                Err(LedgerError::NotFound) => Ok(false),
                Err(_) => Ok(true),
            },
            Err(err) => Err(err),
        }
    }

    /// `Ok(true)` when `fact` is on the row, including a committed lost append.
    pub(super) fn fact_landed(
        &mut self,
        clone: &CloneId,
        fact: LedgerFact,
        caller: MonoInstant,
        scope: Scope,
    ) -> Result<bool, LedgerError> {
        let wrote = self.ledger_append(clone, fact, caller, scope, None);
        self.read_back(wrote, clone, caller, scope, |row| {
            row.facts().contains(&fact)
        })
    }

    pub(super) fn identity_landed(
        &mut self,
        clone: &CloneId,
        identity: &VmIdentity,
        caller: MonoInstant,
        scope: Scope,
    ) -> Result<bool, LedgerError> {
        let wrote = self.ledger_note(clone, identity, caller, scope);
        self.read_back(wrote, clone, caller, scope, |row| {
            row.expected() == Some(identity)
        })
    }

    /// Readback. Unknown, including a deadline that cannot fit, is unsealed.
    pub(super) fn row_sealed(&mut self, caller: MonoInstant, scope: Scope) -> bool {
        let Some(clone) = self.clone_id.clone() else {
            return false;
        };
        self.ledger_query(&clone, caller, scope, None)
            .map(|row| row.is_sealed())
            .unwrap_or(false)
    }
}
