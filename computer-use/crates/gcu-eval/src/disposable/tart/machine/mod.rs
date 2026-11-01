//! Caller-driven attempt machine (ADR 0018 §2).
//!
//! Each method is one synchronous step. The caller holds the runner, the guest
//! channel, the ledger, and the lock. Drop does not stop, delete, or seal.
//! There is no worker, no shared owner, and no asynchronous task.
//!
//! An already-due deadline fails before dispatch. That is not cancellation of
//! work the fake has already started. Runner, guest, and ledger calls go
//! through one dispatch: the earlier of the caller deadline, that call's phase
//! budget, the attempt absolute deadline, and the aggregate that encloses the
//! phase. Prep encloses reserve through ready and starts at the reserve entry,
//! including the reads before reserve returns. Run and the owner lease are not
//! shortened by a finished prep. Cleanup encloses revoke, destroy, and seal.
//! Recovery uses only the new call's deadlines.

mod cleanup;
mod io;
mod recover;
mod steps;

use std::time::Duration;

use super::budgets::PhaseBudgets;
use super::deadline::{HostClock, MonoInstant};
use super::guest::GuestChannel;
use super::identity::{CloneId, RunId};
use super::ledger::{AttemptLedger, ClonePermit, LedgerError, VmIdentity};
use super::lock::{CleanupLock, LockError};
use super::output::CallError;
use super::receipt::{AttemptReceipt, Evidence, NotRunReason, Phase, TaskScore};
use super::runner::TartRunner;

pub use cleanup::CleanupDeadlines;
pub use steps::ReserveInput;

/// Which aggregate, if any, encloses a dispatch.
///
/// Prep covers reserve through ready. Task is run and the owner lease.
/// Cleanup covers revoke, destroy, and seal. Recovery uses only the caller's
/// deadlines.
#[derive(Clone, Copy)]
pub(super) enum Scope {
    Prep,
    Task,
    Cleanup,
    Recovery,
}

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
    /// Start was issued. A later list that omits the name is not process-exit proof.
    pub(super) vm_started: bool,
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
            vm_started: false,
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

    pub(super) fn ensure_phase(
        &mut self,
        expected: &[Phase],
        caller: Option<MonoInstant>,
        scope: Scope,
    ) -> Result<(), StepError> {
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        if self.lease_expired() {
            self.expire_owner(caller, scope);
            return Err(StepError::OwnerDead);
        }
        if expected.contains(&self.phase) {
            return Ok(());
        }
        self.fail(NotRunReason::Unexpected);
        Err(StepError::Unexpected)
    }

    /// Phase budget, the dispatch clip, and one try-lock.
    ///
    /// Does not sleep. `AboveBudget` does not change the phase. Expiry and a
    /// busy lock do. The returned instant is the same clip later IO uses.
    pub(super) fn admit(
        &mut self,
        requested: MonoInstant,
        budget: Duration,
        scope: Scope,
        absolute: Option<MonoInstant>,
    ) -> Result<MonoInstant, StepError> {
        let now = self.clock.now();
        if self.lease_expired() {
            self.expire_owner(Some(requested), scope);
            return Err(StepError::OwnerDead);
        }
        let budget_end = now.checked_add(budget).map_err(|_| StepError::Overflow)?;
        if requested > budget_end {
            return Err(StepError::AboveBudget);
        }
        let at = match self.bound(requested, budget, scope, absolute) {
            Ok(at) => at,
            Err(err) => return Err(self.map_trip(err)),
        };
        if let Err(err) = self.lock.try_acquire(at) {
            return Err(self.on_lock(err));
        }
        Ok(at)
    }

    pub(super) fn gate(
        &mut self,
        expected: &[Phase],
        requested: MonoInstant,
        budget: Duration,
        scope: Scope,
        absolute: Option<MonoInstant>,
    ) -> Result<MonoInstant, StepError> {
        self.ensure_phase(expected, Some(requested), scope)?;
        self.admit(requested, budget, scope, absolute)
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
        self.admit(requested, budget, self.scope_for_phase(), None)?;
        self.fail(reason);
        Err(StepError::Lost(reason))
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
