//! Outcome of one fake attempt. A receipt is not a later authorization.
//!
//! Nothing in this module is serialized. Copying a receipt does not admit a
//! launch and does not revive a score.

use super::super::{Check, CleanupDisposition, Probe};
use super::identity::{CloneId, RunId};
use super::parse::GuestView;

/// Name of the current step.
///
/// This is not a capability. It has no private authority and cannot be turned
/// into a launch. `Suspended` is the owner-death hold: the VM is not deleted
/// and the row stays unsealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Nothing reserved.
    Idle,
    /// Ledger row written. Clone has not been issued.
    Reserved,
    /// Clone was issued.
    Cloned,
    /// Start was issued.
    Booting,
    /// Inventory and probes were recorded.
    Isolated,
    /// Bootstrap ACK was recorded. Not a real helper launch.
    Bootstrapped,
    /// Login evidence was recorded. Not a real login.
    CredentialReady,
    /// Recorded evidence is complete enough for the fake run. Not admission.
    Ready,
    /// The fake guest run was issued.
    Running,
    /// New work is refused. Cleanup has started.
    Revoking,
    /// Stop and delete are in progress or finished, receipt not sealed yet.
    Destroying,
    /// Receipt sealed. Score or `not_run`.
    Final,
    /// Cleanup or the receipt failed. Later task, arm, and repeat stay stopped.
    Quarantined,
    /// Owner lease expired. Cleanup is unknown and the VM was not deleted.
    Suspended,
}

impl Phase {
    pub(super) const fn closed(self) -> bool {
        matches!(self, Self::Final | Self::Quarantined | Self::Suspended)
    }
}

/// Task result the fake run reported.
///
/// Kept only when the receipt is [`AttemptOutcome::Scored`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskScore {
    /// Fake body `score pass`.
    Pass,
    /// Fake body `score fail`.
    Fail,
    /// Fake body `score partial`.
    Partial,
    /// Fake body `score timeout`.
    Timeout,
    /// Fake body `score crash`.
    Crash,
}

/// Why an attempt cannot keep a score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotRunReason {
    /// Install, owner, or Stop check refused before clone.
    Refused,
    /// The caller cancelled.
    Cancelled,
    /// The deadline was already due. Nothing was dispatched.
    Deadline,
    /// The fake reported a timeout. The machine did not sleep.
    Timeout,
    /// The command returned a failure body.
    CommandFailed,
    /// The reply could not be parsed, or the fake marked it malformed.
    Malformed,
    /// The reply was over the 1MiB cap. The body was not kept.
    Overlong,
    /// The command was issued and no reply came back.
    ReplyLost,
    /// A probe observed a breach.
    EnvironmentBreach,
    /// Evidence was missing, unknown, or not complete.
    MissingProof,
    /// Provider EOF. Cleanup may still succeed.
    ProviderCrash,
    /// Guest crashed. The restarted guest is not adopted.
    GuestCrash,
    /// Boot id changed. The new boot is not adopted.
    BootChanged,
    /// Tart left. The VM is not restarted.
    TartExited,
    /// Softnet left. The VM is not restarted.
    SoftnetExited,
    /// Owner lease expired. The VM is not claimed deleted.
    OwnerDeath,
    /// The previous process left an unsealed row. Recovery does not score it.
    HostCrash,
    /// Stop, delete, or absence could not be confirmed.
    CleanupFailed,
    /// The receipt could not be sealed.
    ReceiptFailed,
    /// Status run id did not match the ledger row. The VM was not deleted.
    IdentityConflict,
    /// The cleanup lock was busy. This call did not wait.
    LockBusy,
    /// The call was not legal for the current phase.
    Unexpected,
}

/// Final classification. `Quarantined` stops later task, arm, and repeat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// Environment and cleanup both confirmed. The task score stands.
    Scored(TaskScore),
    /// No score. Later work is allowed when the receipt says so.
    NotRun {
        /// First fault, or [`NotRunReason::MissingProof`] when evidence is short.
        reason: NotRunReason,
        /// The fake run command was issued.
        client_started: bool,
    },
    /// `not_run` plus cleanup failure. Later task, arm, and repeat stay stopped.
    Quarantined {
        /// Why cleanup could not be confirmed.
        reason: NotRunReason,
        /// The fake run command was issued.
        client_started: bool,
    },
}

impl AttemptOutcome {
    /// Score when this outcome kept one.
    #[must_use]
    pub fn score(&self) -> Option<TaskScore> {
        match self {
            Self::Scored(score) => Some(*score),
            Self::NotRun { .. } | Self::Quarantined { .. } => None,
        }
    }

    /// Reason when no score was kept.
    #[must_use]
    pub fn reason(&self) -> Option<NotRunReason> {
        match self {
            Self::Scored(_) => None,
            Self::NotRun { reason, .. } | Self::Quarantined { reason, .. } => Some(*reason),
        }
    }

    /// True when the fake run command was issued.
    #[must_use]
    pub fn client_started(&self) -> bool {
        match self {
            Self::Scored(_) => true,
            Self::NotRun { client_started, .. } | Self::Quarantined { client_started, .. } => {
                *client_started
            }
        }
    }
}

/// Sealed or abandoned result. Not an input to the next reserve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptReceipt {
    run: Option<RunId>,
    clone_id: Option<CloneId>,
    outcome: AttemptOutcome,
    followups_allowed: bool,
    disposition: CleanupDisposition,
    cleanup_confirmed: bool,
    recovery_required: bool,
}

impl AttemptReceipt {
    pub(super) fn new(
        run: Option<RunId>,
        clone_id: Option<CloneId>,
        outcome: AttemptOutcome,
        followups_allowed: bool,
        disposition: CleanupDisposition,
        cleanup_confirmed: bool,
        recovery_required: bool,
    ) -> Self {
        Self {
            run,
            clone_id,
            outcome,
            followups_allowed,
            disposition,
            cleanup_confirmed,
            recovery_required,
        }
    }

    /// Run id when one was reserved.
    #[must_use]
    pub fn run(&self) -> Option<&RunId> {
        self.run.as_ref()
    }

    /// Clone id when one was reserved.
    #[must_use]
    pub fn clone_id(&self) -> Option<&CloneId> {
        self.clone_id.as_ref()
    }

    /// Score, `not_run`, or quarantine.
    #[must_use]
    pub fn outcome(&self) -> &AttemptOutcome {
        &self.outcome
    }

    /// False after quarantine, a busy lock, a latched Stop, or an unsealed owner death.
    #[must_use]
    pub fn followups_allowed(&self) -> bool {
        self.followups_allowed
    }

    /// `Unknown` means the VM was not confirmed deleted.
    #[must_use]
    pub fn disposition(&self) -> CleanupDisposition {
        self.disposition
    }

    /// True only when absence was confirmed, or nothing was allocated.
    #[must_use]
    pub fn cleanup_confirmed(&self) -> bool {
        self.cleanup_confirmed
    }

    /// True when the next startup must recover this row before reserving.
    #[must_use]
    pub fn recovery_required(&self) -> bool {
        self.recovery_required
    }
}

pub(super) struct Evidence {
    pub probes: Vec<(Probe, Check)>,
    pub guest: Option<GuestView>,
    pub ack_generation: Option<u64>,
    pub login_ready: bool,
    pub provisional: Option<TaskScore>,
}

impl Evidence {
    pub(super) fn new() -> Self {
        Self {
            probes: Vec::new(),
            guest: None,
            ack_generation: None,
            login_ready: false,
            provisional: None,
        }
    }

    pub(super) fn complete(&self) -> bool {
        if self.guest.is_none() || self.ack_generation.is_none() || !self.login_ready {
            return false;
        }
        Probe::ALL.iter().all(|probe| {
            self.probes
                .iter()
                .filter(|(seen, check)| seen == probe && *check == Check::Passed)
                .count()
                == 1
        }) && self.probes.len() == Probe::ALL.len()
    }
}
