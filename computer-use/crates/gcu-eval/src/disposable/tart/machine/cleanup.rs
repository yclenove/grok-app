//! Revoke, destroy, and seal. Confirmation failure is not absence.

use super::super::deadline::{
    DeleteDeadline, HostClock, QueryDeadline, RevokeDeadline, SealDeadline, StopDeadline,
};
use super::super::guest::GuestChannel;
use super::super::identity::RunId;
use super::super::ledger::{AttemptLedger, VmIdentity};
use super::super::lock::CleanupLock;
use super::super::owner::OwnerSource;
use super::super::parse::{self, VmState};
use super::super::receipt::{AttemptOutcome, AttemptReceipt, NotRunReason};
use super::super::runner::TartRunner;
use super::drive::DriveKind;
use super::{Attempt, CompletedStep, RuntimeLoss, StepError, StepProgress};

/// Deadlines for one cleanup or recovery pass.
///
/// Each field is a caller deadline. The machine also clips to the phase budget
/// and, for an in-attempt destroy, to the absolute attempt deadline. Recovery
/// does not reuse the dead row's absolute deadline.
#[derive(Debug, Clone, Copy)]
pub struct CleanupDeadlines {
    /// List and status calls.
    pub query: QueryDeadline,
    /// Stop plus the stopped check.
    pub stop: StopDeadline,
    /// Delete plus the absence check.
    pub delete: DeleteDeadline,
    /// Ledger seal.
    pub seal: SealDeadline,
    /// Guest revoke. Recovery does not send one.
    pub revoke: RevokeDeadline,
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
    /// Revoke the guest once. A repeat does not send a second revoke.
    ///
    /// Failure leaves the attempt in `Revoking` so the caller can still
    /// destroy. The cleanup aggregate includes this first revoke. Does not
    /// block. Does not delete the VM. `Pending` is not a finished revoke.
    pub fn revoke(
        &mut self,
        deadline: RevokeDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Revoke, deadline.at(), true)
    }

    /// Stop the owned clone, delete it, and require a complete list that omits it.
    ///
    /// A failed or incomplete list is not absence. List absence after start is
    /// not process-exit proof. Does not block. One stage per call.
    pub fn destroy(
        &mut self,
        deadlines: &CleanupDeadlines,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Destroy(*deadlines), deadlines.query.at(), true)
    }

    /// Seal the receipt. The entry returns `Pending` when the receipt is held.
    ///
    /// [`Attempt::poll`] delivers [`CompletedStep::Sealed`] once. A confirmed
    /// cleanup with no fault and complete evidence keeps the provisional score.
    /// Does not block.
    pub fn seal(
        &mut self,
        deadline: SealDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Seal(deadline), deadline.at(), true)
    }

    /// Cancel. Moves into cleanup when no command is in flight.
    ///
    /// An in-flight command is killed inside its frozen cancel end. Does not
    /// block. An already closed attempt is unchanged.
    pub fn cancel(
        &mut self,
        deadline: QueryDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Cancel, deadline.at(), true)
    }

    /// Provider EOF. Later cleanup can still delete the VM.
    ///
    /// The outcome stays `not_run` even when that cleanup succeeds. Does not block.
    pub fn note_provider_crash(
        &mut self,
        deadline: QueryDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Provider, deadline.at(), true)
    }

    /// Guest, Tart, or Softnet loss. Does not restart or adopt a new boot.
    ///
    /// Does not block.
    pub fn note_runtime_loss(
        &mut self,
        kind: RuntimeLoss,
        deadline: QueryDeadline,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        self.open_and_tick(DriveKind::Runtime(kind), deadline.at(), true)
    }

    /// Apply an expired owner lease: do not delete the VM.
    ///
    /// A live lease returns [`StepError::LeaseLive`] and changes nothing.
    /// The row stays unsealed with [`LedgerFact::CleanupUnknown`] when the
    /// caller is still ahead. Does not block and does not claim absence.
    pub fn observe_owner_lease(&mut self, deadline: QueryDeadline) -> Result<(), StepError> {
        self.open_observe(deadline.at())
    }

    pub(super) fn fit(&self, deadlines: &CleanupDeadlines) -> Result<(), StepError> {
        let now = self.clock.now();
        let pairs = [
            (deadlines.query.at(), self.budgets.query),
            (deadlines.stop.at(), self.budgets.stop),
            (deadlines.delete.at(), self.budgets.delete),
            (deadlines.seal.at(), self.budgets.seal),
            (deadlines.revoke.at(), self.budgets.revoke),
        ];
        for (at, budget) in pairs {
            let end = now.checked_add(budget).map_err(|_| StepError::Overflow)?;
            if at > end {
                return Err(StepError::AboveBudget);
            }
        }
        Ok(())
    }

    /// Stop and delete one owned id.
    ///
    /// A complete list that omits a never-started name is absence and does not
    /// authorize stop or delete. The same list after start was issued is
    /// [`NotRunReason::CleanupFailed`]. A present name is destroyed only when
    /// its status matches `identity`. Never starts. Never deletes a different id.
    pub(super) fn note_session_boot(&mut self, status: &parse::VmStatus) {
        let Some(guest) = &self.evidence.guest else {
            return;
        };
        let Some(boot) = status.boot.as_deref() else {
            return;
        };
        if boot != guest.boot && self.trigger.is_none() {
            self.trigger = Some(NotRunReason::BootChanged);
        }
    }

    pub(super) fn make_receipt(
        &self,
        confirmed: bool,
        quarantine: Option<NotRunReason>,
    ) -> AttemptReceipt {
        let client_started = self.client_starts > 0;
        let confirmed = if self.effect_unknown_flag {
            false
        } else {
            confirmed
        };
        let quarantine = if self.effect_unknown_flag {
            Some(
                quarantine
                    .or(self.trigger)
                    .unwrap_or(NotRunReason::CleanupFailed),
            )
        } else {
            quarantine
        };
        let quarantine_reason = quarantine.or(if confirmed {
            None
        } else if self.trigger == Some(NotRunReason::IdentityConflict) {
            Some(NotRunReason::IdentityConflict)
        } else if self.trigger == Some(NotRunReason::LockBusy) {
            Some(NotRunReason::LockBusy)
        } else if self.trigger == Some(NotRunReason::ReceiptFailed) {
            Some(NotRunReason::ReceiptFailed)
        } else {
            Some(NotRunReason::CleanupFailed)
        });
        let outcome = if let Some(reason) = quarantine_reason {
            AttemptOutcome::Quarantined {
                reason,
                client_started,
            }
        } else if let Some(reason) = self.trigger {
            AttemptOutcome::NotRun {
                reason,
                client_started,
            }
        } else if self.evidence.complete() {
            match self.evidence.provisional {
                Some(score) => AttemptOutcome::Scored(score),
                None => AttemptOutcome::NotRun {
                    reason: NotRunReason::MissingProof,
                    client_started,
                },
            }
        } else {
            AttemptOutcome::NotRun {
                reason: NotRunReason::MissingProof,
                client_started,
            }
        };
        let quarantined = matches!(outcome, AttemptOutcome::Quarantined { .. });
        let followups = !quarantined && !self.halted;
        let disposition = if self.no_allocation {
            super::super::super::CleanupDisposition::NoAllocation
        } else {
            super::super::super::CleanupDisposition::Allocated
        };
        AttemptReceipt::new(
            self.run.clone(),
            self.clone_id.clone(),
            outcome,
            followups,
            disposition,
            confirmed && !quarantined,
            self.clone_id.is_some() && !self.sealed,
        )
    }
}

pub(super) fn owns(status: &parse::VmStatus, identity: &VmIdentity, run: &RunId) -> bool {
    status.state != VmState::Absent
        && status.run.as_deref() == Some(run.as_str())
        && status.incarnation.as_deref() == Some(identity.incarnation())
        && status.config.as_deref() == Some(identity.config())
}
