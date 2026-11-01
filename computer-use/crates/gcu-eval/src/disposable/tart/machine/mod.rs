//! Caller-driven attempt machine (ADR 0018 §2).
//!
//! Each method is one synchronous step. The caller holds the runner, the guest
//! channel, the ledger, and the lock. Drop does not stop, delete, or seal.
//! There is no worker, no shared owner, and no asynchronous task.
//!
//! An already-due deadline fails before dispatch. That is not cancellation of
//! work the fake has already started. The effective instant of a step after
//! reserve is the earlier of the call and the absolute attempt deadline.
//! Recovery of an unsealed row uses the new call's deadlines: the old absolute
//! deadline stays on the row and is not extended, and it does not block the
//! deletion that a restart still owes.

mod cleanup;
mod recover;
mod steps;

use std::time::Duration;

use super::budgets::PhaseBudgets;
use super::deadline::{HostClock, MonoInstant, QueryDeadline};
use super::guest::GuestChannel;
use super::identity::{CloneId, RunId};
use super::ledger::{AttemptLedger, ClonePermit, LedgerError, LedgerFact, VmIdentity};
use super::lock::{CleanupLock, LockError};
use super::output::CallError;
use super::receipt::{AttemptReceipt, Evidence, NotRunReason, Phase, TaskScore};
use super::runner::TartRunner;

pub use cleanup::CleanupDeadlines;
pub use steps::ReserveInput;

/// Why one step did not advance.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StepError {
    /// The attempt is already `Final`, `Quarantined`, or `Suspended`.
    #[error("attempt is already closed")]
    Terminal,
    /// The call is not legal in this phase.
    #[error("unexpected transition")]
    Unexpected,
    /// The caller's deadline is farther out than the configured phase budget.
    #[error("deadline is above the phase budget")]
    AboveBudget,
    /// The deadline was already due. Nothing was dispatched.
    #[error("deadline already expired")]
    Expired,
    /// A budget instant did not fit in the monotonic clock.
    #[error("deadline overflowed the monotonic clock")]
    Overflow,
    /// Owner, Stop, or manifest check refused the reserve.
    #[error("reserve refused")]
    Refused,
    /// Stop is latched. This call does not clear it.
    #[error("stop is latched")]
    StopLatched,
    /// The manifest does not match the approved digests.
    #[error("install manifest does not match")]
    ManifestMismatch,
    /// An unsealed row is still in the ledger.
    #[error("unsealed rows must be recovered first")]
    MustRecover,
    /// A quarantined row forbids another attempt.
    #[error("ledger is halted")]
    Halted,
    /// The lock was busy. This call did not wait.
    #[error("cleanup lock is busy")]
    LockBusy,
    /// The guest channel version is not the fixed version.
    #[error("guest channel version is not supported")]
    Protocol,
    /// A probe reported a breach.
    #[error("probe reported a breach")]
    Breach,
    /// A success body was not usable evidence.
    #[error("reply body was not usable")]
    MalformedBody,
    /// Owner death forbids claiming the VM deleted.
    #[error("vm delete is forbidden after owner death")]
    DeleteForbidden,
    /// The owner lease is still inside its deadline.
    #[error("owner lease is still live")]
    LeaseLive,
    /// A late lease was rejected and did not extend the deadline.
    #[error("owner lease is already expired")]
    LeaseExpired,
    /// The owner lease expired. The VM was not deleted.
    #[error("owner lease expired")]
    OwnerDead,
    /// The clone name is already in the authoritative list. The row was not written.
    #[error("clone destination already exists")]
    DestinationExists,
    /// A noted crash or cancel moved the attempt into cleanup.
    #[error("attempt moved into cleanup")]
    Lost(NotRunReason),
    /// Stop, delete, or absence could not be confirmed.
    #[error("cleanup could not be confirmed")]
    Cleanup(NotRunReason),
    /// The runner or the guest returned a call error.
    #[error(transparent)]
    Call(CallError),
    /// The ledger returned an error. The row was not given a success seal.
    #[error(transparent)]
    Ledger(LedgerError),
}

/// Loss of the guest, Tart, or Softnet. The machine does not restart the VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLoss {
    /// Guest process crashed.
    GuestCrash,
    /// Boot id changed.
    BootChanged,
    /// Tart exited.
    TartExited,
    /// Softnet exited.
    SoftnetExited,
}

impl RuntimeLoss {
    fn reason(self) -> NotRunReason {
        match self {
            Self::GuestCrash => NotRunReason::GuestCrash,
            Self::BootChanged => NotRunReason::BootChanged,
            Self::TartExited => NotRunReason::TartExited,
            Self::SoftnetExited => NotRunReason::SoftnetExited,
        }
    }
}

/// One attempt. The caller drives every step and keeps the fakes.
///
/// `Drop` does not stop, delete, seal, or spawn. Phase names are not launch
/// capabilities. This type does not mint an admission token.
pub struct Attempt<'a, R, G, L, K, C> {
    pub(super) runner: &'a mut R,
    pub(super) guest: &'a mut G,
    pub(super) ledger: &'a mut L,
    pub(super) lock: &'a mut K,
    pub(super) clock: &'a C,
    pub(super) budgets: PhaseBudgets,
    pub(super) phase: Phase,
    pub(super) run: Option<RunId>,
    pub(super) clone_id: Option<CloneId>,
    pub(super) permit: Option<ClonePermit>,
    pub(super) absolute: Option<MonoInstant>,
    pub(super) origin: Option<MonoInstant>,
    pub(super) cleanup_origin: Option<MonoInstant>,
    pub(super) evidence: Evidence,
    pub(super) trigger: Option<NotRunReason>,
    pub(super) revoke_ok: bool,
    pub(super) revoke_attempted: bool,
    pub(super) vm_cleanup_ok: bool,
    pub(super) no_allocation: bool,
    pub(super) delete_forbidden: bool,
    pub(super) halted: bool,
    pub(super) client_starts: u8,
    pub(super) lease_until: Option<MonoInstant>,
    pub(super) lease_seq: u64,
    pub(super) identity: Option<VmIdentity>,
    /// The ledger row was observed sealed. An allocated unsealed row needs recovery.
    pub(super) sealed: bool,
    pub(super) receipt: Option<AttemptReceipt>,
}

impl<'a, R, G, L, K, C> Attempt<'a, R, G, L, K, C>
where
    R: TartRunner,
    G: GuestChannel,
    L: AttemptLedger,
    K: CleanupLock,
    C: HostClock,
{
    /// Idle machine. Does not reserve, clone, or take the lock.
    ///
    /// `budgets` can be [`PhaseBudgets::adr`] or a test shrink. The fakes are
    /// borrowed for the life of this value. Drop does not clean them up.
    #[must_use]
    pub fn new(
        runner: &'a mut R,
        guest: &'a mut G,
        ledger: &'a mut L,
        lock: &'a mut K,
        clock: &'a C,
        budgets: PhaseBudgets,
    ) -> Self {
        Self {
            runner,
            guest,
            ledger,
            lock,
            clock,
            budgets,
            phase: Phase::Idle,
            run: None,
            clone_id: None,
            permit: None,
            absolute: None,
            origin: None,
            cleanup_origin: None,
            evidence: Evidence::new(),
            trigger: None,
            revoke_ok: false,
            revoke_attempted: false,
            vm_cleanup_ok: false,
            no_allocation: false,
            delete_forbidden: false,
            halted: false,
            client_starts: 0,
            lease_until: None,
            lease_seq: 0,
            identity: None,
            sealed: false,
            receipt: None,
        }
    }

    /// Current phase name. Not a capability.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Receipt once seal, quarantine, or owner death has stored one.
    #[must_use]
    pub fn receipt(&self) -> Option<&AttemptReceipt> {
        self.receipt.as_ref()
    }

    /// How many times the fake run command was issued.
    #[must_use]
    pub fn client_starts(&self) -> u8 {
        self.client_starts
    }

    pub(super) fn ensure_phase(&mut self, expected: &[Phase]) -> Result<(), StepError> {
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        if self.lease_expired() {
            self.expire_owner(None);
            return Err(StepError::OwnerDead);
        }
        if expected.contains(&self.phase) {
            return Ok(());
        }
        self.fail(NotRunReason::Unexpected);
        Err(StepError::Unexpected)
    }

    /// Budget, expiry, absolute deadline, and one try-lock.
    ///
    /// Does not sleep. `AboveBudget` does not change the phase. Expiry and a
    /// busy lock do.
    pub(super) fn admit(
        &mut self,
        requested: MonoInstant,
        budget: Duration,
        prep: bool,
    ) -> Result<MonoInstant, StepError> {
        let now = self.clock.now();
        if self.lease_expired() {
            self.expire_owner(Some(requested));
            return Err(StepError::OwnerDead);
        }
        let budget_end = now.checked_add(budget).map_err(|_| StepError::Overflow)?;
        if requested > budget_end {
            return Err(StepError::AboveBudget);
        }
        if prep {
            if let Some(origin) = self.origin {
                let prep_end = origin
                    .checked_add(self.budgets.prep)
                    .map_err(|_| StepError::Overflow)?;
                if now >= prep_end {
                    self.fail(NotRunReason::Deadline);
                    return Err(StepError::Expired);
                }
            }
        }
        if let Some(started) = self.cleanup_origin {
            let end = started
                .checked_add(self.budgets.cleanup)
                .map_err(|_| StepError::Overflow)?;
            if now >= end {
                self.fail(NotRunReason::Deadline);
                return Err(StepError::Expired);
            }
        }
        let mut effective = requested;
        if let Some(absolute) = self.absolute {
            if now >= absolute {
                self.fail(NotRunReason::Deadline);
                return Err(StepError::Expired);
            }
            effective = effective.min(absolute);
        }
        if let Some(group) = self.group_limit(now).map_err(|_| StepError::Overflow)? {
            effective = effective.min(group);
        }
        if now >= effective {
            self.fail(NotRunReason::Deadline);
            return Err(StepError::Expired);
        }
        if let Err(err) = self.lock.try_acquire(effective) {
            return Err(self.on_lock(err));
        }
        Ok(effective)
    }

    pub(super) fn gate(
        &mut self,
        expected: &[Phase],
        requested: MonoInstant,
        budget: Duration,
        prep: bool,
    ) -> Result<MonoInstant, StepError> {
        self.ensure_phase(expected)?;
        self.admit(requested, budget, prep)
    }

    /// Earlier of the request, the inner budget, and an optional absolute cap.
    ///
    /// Does not change phase. A due instant is [`NotRunReason::Deadline`].
    pub(super) fn earlier(
        &self,
        requested: MonoInstant,
        budget: Duration,
        absolute: Option<MonoInstant>,
    ) -> Result<MonoInstant, NotRunReason> {
        let now = self.clock.now();
        let budget_end = now
            .checked_add(budget)
            .map_err(|_| NotRunReason::Deadline)?;
        let mut effective = requested.min(budget_end);
        if let Some(absolute) = absolute {
            if now >= absolute {
                return Err(NotRunReason::Deadline);
            }
            effective = effective.min(absolute);
        }
        if let Some(group) = self.group_limit(now)? {
            effective = effective.min(group);
        }
        if now >= effective {
            Err(NotRunReason::Deadline)
        } else {
            Ok(effective)
        }
    }

    /// Prep end, cleanup end, or the earlier of the two when both are still ahead.
    ///
    /// A prep end that has already passed does not clip cleanup. Recovery has
    /// neither origin and is not clipped to a dead row.
    fn group_limit(&self, now: MonoInstant) -> Result<Option<MonoInstant>, NotRunReason> {
        let mut limit = None;
        if let Some(origin) = self.origin {
            let prep_end = origin
                .checked_add(self.budgets.prep)
                .map_err(|_| NotRunReason::Deadline)?;
            if now < prep_end {
                limit = Some(prep_end);
            }
        }
        if let Some(started) = self.cleanup_origin {
            let cleanup_end = started
                .checked_add(self.budgets.cleanup)
                .map_err(|_| NotRunReason::Deadline)?;
            if now < cleanup_end {
                limit = Some(match limit {
                    Some(prep_end) => prep_end.min(cleanup_end),
                    None => cleanup_end,
                });
            }
        }
        Ok(limit)
    }

    pub(super) fn lease_expired(&self) -> bool {
        self.lease_until
            .is_some_and(|until| self.clock.now() >= until)
    }

    pub(super) fn set_open(&mut self, phase: Phase) {
        if !self.phase.closed() {
            self.phase = phase;
        }
    }

    pub(super) fn fail(&mut self, reason: NotRunReason) {
        if self.phase.closed() {
            return;
        }
        if self.trigger.is_none() {
            self.trigger = Some(reason);
        }
        if self.clone_id.is_none() {
            self.no_allocation = true;
            self.phase = Phase::Destroying;
            return;
        }
        if self.delete_forbidden {
            self.phase = Phase::Suspended;
            return;
        }
        if !matches!(self.phase, Phase::Revoking | Phase::Destroying) {
            self.phase = Phase::Revoking;
            if self.cleanup_origin.is_none() {
                self.cleanup_origin = Some(self.clock.now());
            }
        }
    }

    pub(super) fn on_lock(&mut self, err: LockError) -> StepError {
        match err {
            LockError::Expired => {
                self.fail(NotRunReason::Deadline);
                StepError::Expired
            }
            LockError::Busy => {
                self.halted = true;
                self.fail(NotRunReason::LockBusy);
                if self.clone_id.is_some() && !self.phase.closed() {
                    self.store_quarantine(NotRunReason::LockBusy);
                }
                StepError::LockBusy
            }
        }
    }

    pub(super) fn store_quarantine(&mut self, reason: NotRunReason) {
        if self.trigger.is_none() {
            self.trigger = Some(reason);
        }
        let receipt = self.make_receipt(false, Some(reason));
        self.phase = Phase::Quarantined;
        self.receipt = Some(receipt);
    }

    pub(super) fn note_loss(
        &mut self,
        reason: NotRunReason,
        requested: MonoInstant,
        budget: Duration,
    ) -> Result<(), StepError> {
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        self.admit(requested, budget, false)?;
        self.fail(reason);
        Err(StepError::Lost(reason))
    }

    pub(super) fn query_deadline(
        &self,
        effective: MonoInstant,
    ) -> Result<QueryDeadline, StepError> {
        let at = self
            .earlier(effective, self.budgets.query, None)
            .map_err(|_| StepError::Expired)?;
        Ok(QueryDeadline::from_instant(at))
    }

    pub(super) fn append_fact(
        &mut self,
        fact: LedgerFact,
        effective: MonoInstant,
    ) -> Result<(), LedgerError> {
        let Some(clone) = self.clone_id.clone() else {
            return Err(LedgerError::NotFound);
        };
        let call = self
            .query_deadline(effective)
            .map_err(|_| LedgerError::Expired)?;
        self.ledger.append(&clone, fact, call)
    }

    pub(super) fn note_identity(
        &mut self,
        identity: &VmIdentity,
        effective: MonoInstant,
    ) -> Result<(), LedgerError> {
        let Some(clone) = self.clone_id.clone() else {
            return Err(LedgerError::NotFound);
        };
        let call = self
            .query_deadline(effective)
            .map_err(|_| LedgerError::Expired)?;
        self.ledger.note_identity(&clone, identity, call)
    }
}

pub(super) fn call_reason(err: &CallError) -> NotRunReason {
    match err {
        CallError::Expired => NotRunReason::Deadline,
        CallError::Failed(_) => NotRunReason::CommandFailed,
        CallError::TimedOut => NotRunReason::Timeout,
        CallError::Malformed(_) => NotRunReason::Malformed,
        CallError::OutputTooLong => NotRunReason::Overlong,
        CallError::ScriptExhausted => NotRunReason::ReplyLost,
    }
}

pub(super) fn ledger_reason(err: &LedgerError) -> NotRunReason {
    match err {
        LedgerError::Expired | LedgerError::AttemptAlreadyExpired => NotRunReason::Deadline,
        LedgerError::TimedOut => NotRunReason::Timeout,
        LedgerError::Failed => NotRunReason::CommandFailed,
        LedgerError::Malformed => NotRunReason::Malformed,
        LedgerError::Overlong => NotRunReason::Overlong,
        LedgerError::ReplyLost => NotRunReason::ReplyLost,
        LedgerError::Sealed => NotRunReason::ReceiptFailed,
        LedgerError::DuplicateRun | LedgerError::DuplicateClone | LedgerError::NotFound => {
            NotRunReason::Unexpected
        }
    }
}

pub(super) fn absent_reply(err: &CallError) -> bool {
    matches!(err, CallError::Failed(body) if body.as_bytes() == b"absent")
}

pub(super) fn score_of(mark: super::parse::ScoreMark) -> TaskScore {
    match mark {
        super::parse::ScoreMark::Pass => TaskScore::Pass,
        super::parse::ScoreMark::Fail => TaskScore::Fail,
        super::parse::ScoreMark::Partial => TaskScore::Partial,
        super::parse::ScoreMark::Timeout => TaskScore::Timeout,
        super::parse::ScoreMark::Crash => TaskScore::Crash,
    }
}
