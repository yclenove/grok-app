//! Phase budgets from ADR 0018 §2.
//!
//! These are the caps a caller may ask for. They are not Stop budgets. A test
//! can build a smaller [`PhaseBudgets`]; zero and any value above the cap are
//! rejected. Nothing here sleeps or starts work.

use std::time::Duration;

/// One list, status, inventory, or ledger call.
pub const QUERY_BUDGET: Duration = Duration::from_secs(5);
/// One clone.
pub const CLONE_BUDGET: Duration = Duration::from_secs(60);
/// One start, including boot ready.
pub const START_BUDGET: Duration = Duration::from_secs(120);
/// One guest probe.
pub const PROBE_BUDGET: Duration = Duration::from_secs(2);
/// The whole probe group.
pub const PROBE_GROUP_BUDGET: Duration = Duration::from_secs(60);
/// One bootstrap or ACK stage.
pub const STAGE_BUDGET: Duration = Duration::from_secs(5);
/// One fixed login call. The call takes no secret.
pub const LOGIN_BUDGET: Duration = Duration::from_secs(5 * 60);
/// One guest run. Callers pass `min(task, this cap)`.
pub const RUN_BUDGET: Duration = Duration::from_secs(15 * 60);
/// One guest lease. A late lease is not a revive.
pub const LEASE_BUDGET: Duration = Duration::from_secs(3);
/// One revoke or guest drain.
pub const REVOKE_BUDGET: Duration = Duration::from_secs(10);
/// One stop plus the stop confirmation.
pub const STOP_BUDGET: Duration = Duration::from_secs(20);
/// One delete plus the absence confirmation.
pub const DELETE_BUDGET: Duration = Duration::from_secs(20);
/// Sealing one ledger row.
pub const SEAL_BUDGET: Duration = Duration::from_secs(10);
/// Revoke, stop, delete, and seal together.
pub const CLEANUP_BUDGET: Duration = Duration::from_secs(60);
/// Reserve through ready.
pub const PREP_BUDGET: Duration = Duration::from_secs(10 * 60);
/// Prep plus task plus cleanup. Retries cannot extend it.
pub const ATTEMPT_BUDGET: Duration = Duration::from_secs(10 * 60 + 15 * 60 + 60);

/// Which budget [`PhaseBudgets::shrink`] replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetField {
    /// [`QUERY_BUDGET`].
    Query,
    /// [`CLONE_BUDGET`].
    Clone,
    /// [`START_BUDGET`].
    Start,
    /// [`PROBE_BUDGET`].
    Probe,
    /// [`PROBE_GROUP_BUDGET`].
    ProbeGroup,
    /// [`STAGE_BUDGET`].
    Stage,
    /// [`LOGIN_BUDGET`].
    Login,
    /// [`RUN_BUDGET`].
    Run,
    /// [`LEASE_BUDGET`].
    Lease,
    /// [`REVOKE_BUDGET`].
    Revoke,
    /// [`STOP_BUDGET`].
    Stop,
    /// [`DELETE_BUDGET`].
    Delete,
    /// [`SEAL_BUDGET`].
    Seal,
    /// [`CLEANUP_BUDGET`].
    Cleanup,
    /// [`PREP_BUDGET`].
    Prep,
    /// [`ATTEMPT_BUDGET`].
    Attempt,
}

impl BudgetField {
    const fn cap(self) -> Duration {
        match self {
            Self::Query => QUERY_BUDGET,
            Self::Clone => CLONE_BUDGET,
            Self::Start => START_BUDGET,
            Self::Probe => PROBE_BUDGET,
            Self::ProbeGroup => PROBE_GROUP_BUDGET,
            Self::Stage => STAGE_BUDGET,
            Self::Login => LOGIN_BUDGET,
            Self::Run => RUN_BUDGET,
            Self::Lease => LEASE_BUDGET,
            Self::Revoke => REVOKE_BUDGET,
            Self::Stop => STOP_BUDGET,
            Self::Delete => DELETE_BUDGET,
            Self::Seal => SEAL_BUDGET,
            Self::Cleanup => CLEANUP_BUDGET,
            Self::Prep => PREP_BUDGET,
            Self::Attempt => ATTEMPT_BUDGET,
        }
    }
}

/// Why a shrunk budget was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BudgetError {
    /// Zero does not bound the step.
    #[error("phase budget is zero")]
    Zero,
    /// Larger than the ADR cap for that step.
    #[error("phase budget is above the ADR cap")]
    AboveCap,
}

/// Per-step limits for one machine.
///
/// [`Self::adr`] is the §2 table. [`Self::shrink`] only reduces a field.
/// Dropping this value does not cancel work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseBudgets {
    pub(super) query: Duration,
    pub(super) clone_limit: Duration,
    pub(super) start: Duration,
    pub(super) probe: Duration,
    pub(super) probe_group: Duration,
    pub(super) stage: Duration,
    pub(super) login: Duration,
    pub(super) run: Duration,
    pub(super) lease: Duration,
    pub(super) revoke: Duration,
    pub(super) stop: Duration,
    pub(super) delete: Duration,
    pub(super) seal: Duration,
    pub(super) cleanup: Duration,
    pub(super) prep: Duration,
    pub(super) attempt: Duration,
}

impl PhaseBudgets {
    /// The ADR 0018 §2 caps. Not a Stop budget and not a default deadline.
    #[must_use]
    pub const fn adr() -> Self {
        Self {
            query: QUERY_BUDGET,
            clone_limit: CLONE_BUDGET,
            start: START_BUDGET,
            probe: PROBE_BUDGET,
            probe_group: PROBE_GROUP_BUDGET,
            stage: STAGE_BUDGET,
            login: LOGIN_BUDGET,
            run: RUN_BUDGET,
            lease: LEASE_BUDGET,
            revoke: REVOKE_BUDGET,
            stop: STOP_BUDGET,
            delete: DELETE_BUDGET,
            seal: SEAL_BUDGET,
            cleanup: CLEANUP_BUDGET,
            prep: PREP_BUDGET,
            attempt: ATTEMPT_BUDGET,
        }
    }

    /// Replace `field` with `limit`.
    ///
    /// `limit` must be greater than zero and at most that field's ADR cap.
    /// Other fields stay as they are. Does not block.
    pub fn shrink(mut self, field: BudgetField, limit: Duration) -> Result<Self, BudgetError> {
        if limit.is_zero() {
            return Err(BudgetError::Zero);
        }
        if limit > field.cap() {
            return Err(BudgetError::AboveCap);
        }
        match field {
            BudgetField::Query => self.query = limit,
            BudgetField::Clone => self.clone_limit = limit,
            BudgetField::Start => self.start = limit,
            BudgetField::Probe => self.probe = limit,
            BudgetField::ProbeGroup => self.probe_group = limit,
            BudgetField::Stage => self.stage = limit,
            BudgetField::Login => self.login = limit,
            BudgetField::Run => self.run = limit,
            BudgetField::Lease => self.lease = limit,
            BudgetField::Revoke => self.revoke = limit,
            BudgetField::Stop => self.stop = limit,
            BudgetField::Delete => self.delete = limit,
            BudgetField::Seal => self.seal = limit,
            BudgetField::Cleanup => self.cleanup = limit,
            BudgetField::Prep => self.prep = limit,
            BudgetField::Attempt => self.attempt = limit,
        }
        Ok(self)
    }
}
