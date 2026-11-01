//! Revoke, destroy, and seal. Confirmation failure is not absence.

use super::super::deadline::{
    DeleteDeadline, HostClock, MonoInstant, QueryDeadline, RevokeDeadline, SealDeadline,
    StopDeadline,
};
use super::super::guest::GuestChannel;
use super::super::identity::{CloneId, RunId};
use super::super::ledger::{AttemptLedger, LedgerFact, VmIdentity};
use super::super::lock::CleanupLock;
use super::super::parse::{self, VmState};
use super::super::receipt::{AttemptOutcome, AttemptReceipt, NotRunReason, Phase};
use super::super::runner::TartRunner;
use super::{Attempt, RuntimeLoss, Scope, StepError};

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

impl<'a, R, G, L, K, C> Attempt<'a, R, G, L, K, C>
where
    R: TartRunner,
    G: GuestChannel,
    L: AttemptLedger,
    K: CleanupLock,
    C: HostClock,
{
    /// Revoke the guest once. A repeat does not send a second revoke.
    ///
    /// Failure leaves the attempt in `Revoking` so the caller can still
    /// destroy. The cleanup aggregate includes this first revoke. Does not
    /// block. Does not delete the VM.
    pub fn revoke(&mut self, deadline: RevokeDeadline) -> Result<(), StepError> {
        let caller = deadline.at();
        self.ensure_phase(
            &[Phase::Running, Phase::Revoking],
            Some(caller),
            Scope::Cleanup,
        )?;
        if self.revoke_attempted {
            self.phase = Phase::Revoking;
            return if self.revoke_ok {
                Ok(())
            } else {
                Err(StepError::Cleanup(NotRunReason::CleanupFailed))
            };
        }
        let now = self.clock.now();
        let budget_end = now
            .checked_add(self.budgets.revoke)
            .map_err(|_| StepError::Overflow)?;
        if caller > budget_end {
            return Err(StepError::AboveBudget);
        }
        if self.cleanup_origin.is_none() {
            self.cleanup_origin = Some(now);
        }
        if let Err(err) = self.admit(caller, self.budgets.revoke, Scope::Cleanup, None) {
            if !matches!(err, StepError::AboveBudget) {
                self.revoke_attempted = true;
                self.set_open(Phase::Revoking);
            }
            return Err(err);
        }
        self.revoke_attempted = true;
        match self.guest_revoke(caller, Scope::Cleanup) {
            Ok(_) => {
                self.revoke_ok = true;
                self.set_open(Phase::Revoking);
                Ok(())
            }
            Err(err) => {
                self.set_open(Phase::Revoking);
                Err(err)
            }
        }
    }

    /// Stop the owned clone, delete it, and require a complete list that omits it.
    ///
    /// A failed or incomplete list is not absence. List absence after start is
    /// not process-exit proof. The owner lease is rechecked before stop and
    /// before delete. A run-id mismatch does not stop or delete. Does not block.
    pub fn destroy(&mut self, deadlines: &CleanupDeadlines) -> Result<(), StepError> {
        if self.delete_forbidden {
            return Err(StepError::DeleteForbidden);
        }
        self.ensure_phase(
            &[Phase::Revoking],
            Some(deadlines.query.at()),
            Scope::Cleanup,
        )?;
        self.fit(deadlines)?;
        if let Err(err) = self.admit(
            deadlines.query.at(),
            self.budgets.query,
            Scope::Cleanup,
            None,
        ) {
            if !matches!(err, StepError::AboveBudget) {
                self.vm_cleanup_ok = false;
                self.set_open(Phase::Destroying);
            }
            return Err(err);
        }
        let Some(run) = self.run.clone() else {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        };
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        };
        match self.reclaim(
            &run,
            &clone,
            deadlines,
            Scope::Cleanup,
            self.vm_started,
            self.identity.clone(),
        ) {
            Ok(()) => {
                self.vm_cleanup_ok = true;
                self.set_open(Phase::Destroying);
                Ok(())
            }
            Err(reason) => {
                if self.trigger.is_none() {
                    self.trigger = Some(reason);
                }
                self.vm_cleanup_ok = false;
                self.set_open(Phase::Destroying);
                Err(StepError::Cleanup(reason))
            }
        }
    }

    /// Seal the receipt.
    ///
    /// A confirmed cleanup with no fault and complete evidence keeps the
    /// provisional score. Any fault keeps `not_run` even if that score was
    /// pass. A seal whose readback shows the row already sealed is final,
    /// including when the acknowledgement was lost. An unconfirmed cleanup is
    /// `Quarantined`. Does not block.
    pub fn seal(&mut self, deadline: SealDeadline) -> Result<AttemptReceipt, StepError> {
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        if self.lease_expired() {
            self.expire_owner(Some(deadline.at()), Scope::Cleanup);
            return Err(StepError::OwnerDead);
        }
        if self.phase != Phase::Destroying {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        }
        if self.halted && self.trigger == Some(NotRunReason::LockBusy) {
            return self.seal_lock_busy(deadline);
        }
        let caller = deadline.at();
        let effective = match self.admit(caller, self.budgets.seal, Scope::Cleanup, None) {
            Ok(at) => at,
            Err(StepError::AboveBudget) => return Err(StepError::AboveBudget),
            Err(StepError::LockBusy) => {
                if self.receipt.is_none() {
                    self.store_quarantine(NotRunReason::LockBusy);
                }
                return Err(StepError::LockBusy);
            }
            Err(err) => {
                self.store_quarantine(NotRunReason::ReceiptFailed);
                return Err(err);
            }
        };
        if self.no_allocation || self.clone_id.is_none() {
            let quarantine = self.trigger == Some(NotRunReason::LockBusy);
            let receipt =
                self.make_receipt(!quarantine, quarantine.then_some(NotRunReason::LockBusy));
            self.phase = if quarantine {
                Phase::Quarantined
            } else {
                Phase::Final
            };
            self.receipt = Some(receipt.clone());
            return Ok(receipt);
        }
        let confirmed = self.revoke_ok && self.vm_cleanup_ok && !self.halted;
        let Some(clone) = self.clone_id.clone() else {
            self.store_quarantine(NotRunReason::CleanupFailed);
            return Err(StepError::Cleanup(NotRunReason::CleanupFailed));
        };
        if !confirmed {
            if let Err(err) = self.ledger_append(
                &clone,
                LedgerFact::Quarantined,
                effective,
                Scope::Cleanup,
                None,
            ) {
                self.store_quarantine(NotRunReason::ReceiptFailed);
                return Err(StepError::Ledger(err));
            }
        }
        match self.ledger_seal(&clone, caller, Scope::Cleanup) {
            Ok(()) => self.finish_sealed(confirmed),
            Err(err) => {
                if self.row_sealed(caller, Scope::Cleanup) {
                    return self.finish_sealed(confirmed);
                }
                self.sealed = false;
                self.halted = true;
                self.store_quarantine(NotRunReason::ReceiptFailed);
                Err(StepError::Ledger(err))
            }
        }
    }

    fn finish_sealed(&mut self, confirmed: bool) -> Result<AttemptReceipt, StepError> {
        self.sealed = true;
        let receipt = self.make_receipt(confirmed, None);
        self.phase = if confirmed {
            Phase::Final
        } else {
            Phase::Quarantined
        };
        self.receipt = Some(receipt.clone());
        Ok(receipt)
    }

    /// Cancel. Moves into cleanup and does not dispatch a Tart command.
    ///
    /// Does not block. An already closed attempt is unchanged.
    pub fn cancel(&mut self, deadline: QueryDeadline) -> Result<(), StepError> {
        self.note_loss(NotRunReason::Cancelled, deadline.at(), self.budgets.query)
    }

    /// Provider EOF. Later cleanup can still delete the VM.
    ///
    /// The outcome stays `not_run` even when that cleanup succeeds. Does not block.
    pub fn note_provider_crash(&mut self, deadline: QueryDeadline) -> Result<(), StepError> {
        self.note_loss(
            NotRunReason::ProviderCrash,
            deadline.at(),
            self.budgets.query,
        )
    }

    /// Guest, Tart, or Softnet loss. Does not restart or adopt a new boot.
    ///
    /// Does not block.
    pub fn note_runtime_loss(
        &mut self,
        kind: RuntimeLoss,
        deadline: QueryDeadline,
    ) -> Result<(), StepError> {
        self.note_loss(kind.reason(), deadline.at(), self.budgets.query)
    }

    /// Apply an expired owner lease: revoke scope, do not delete the VM.
    ///
    /// A live lease returns [`StepError::LeaseLive`] and changes nothing.
    /// The row stays unsealed with [`LedgerFact::CleanupUnknown`]. The next
    /// startup must recover it. Does not block and does not claim absence.
    pub fn observe_owner_lease(&mut self, deadline: QueryDeadline) -> Result<(), StepError> {
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        // An expired lease is decided here even when this call's own deadline
        // is already due. The note may be skipped; the hold is not.
        let scope = self.scope_for_phase();
        if self.lease_expired() {
            self.expire_owner(Some(deadline.at()), scope);
            return Err(StepError::OwnerDead);
        }
        self.ensure_phase(&[Phase::Running], Some(deadline.at()), scope)?;
        Err(StepError::LeaseLive)
    }

    /// Enter the owner-death hold before any optional IO.
    ///
    /// The phase change does not dispatch. Revoke and the ledger note run only
    /// when `caller` is still ahead; a due or missing caller skips them and
    /// still leaves `Suspended`. Does not delete, renew, score, or arm a timer.
    pub(super) fn expire_owner(&mut self, caller: Option<MonoInstant>, scope: Scope) {
        if self.phase.closed() {
            return;
        }
        self.delete_forbidden = true;
        if self.trigger.is_none() {
            self.trigger = Some(NotRunReason::OwnerDeath);
        }
        let run = self.run.clone();
        let clone = self.clone_id.clone();
        self.phase = Phase::Suspended;
        self.receipt = Some(AttemptReceipt::new(
            run,
            clone.clone(),
            AttemptOutcome::NotRun {
                reason: NotRunReason::OwnerDeath,
                client_started: self.client_starts > 0,
            },
            false,
            super::super::super::CleanupDisposition::Unknown,
            false,
            clone.is_some(),
        ));
        let Some(caller) = caller else {
            return;
        };
        if self.clock.now() >= caller {
            return;
        }
        if !self.revoke_attempted {
            self.revoke_attempted = true;
            if self.guest_revoke(caller, scope).is_ok() {
                self.revoke_ok = true;
            }
        }
        if let Some(clone) = self.clone_id.clone() {
            let _ = self.ledger_append(&clone, LedgerFact::CleanupUnknown, caller, scope, None);
        }
    }

    /// Record a lock-busy refusal without taking the lock again.
    ///
    /// The mutex is still held. Waiting here would block the caller. No Tart
    /// command is issued.
    fn seal_lock_busy(&mut self, deadline: SealDeadline) -> Result<AttemptReceipt, StepError> {
        if self.clock.now() >= deadline.at() {
            return Err(StepError::Expired);
        }
        let end = self
            .clock
            .now()
            .checked_add(self.budgets.seal)
            .map_err(|_| StepError::Overflow)?;
        if deadline.at() > end {
            return Err(StepError::AboveBudget);
        }
        let receipt = self.make_receipt(false, Some(NotRunReason::LockBusy));
        self.phase = Phase::Quarantined;
        self.receipt = Some(receipt.clone());
        Ok(receipt)
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
    pub(super) fn reclaim(
        &mut self,
        run: &RunId,
        clone: &CloneId,
        deadlines: &CleanupDeadlines,
        scope: Scope,
        started: bool,
        identity: Option<VmIdentity>,
    ) -> Result<(), NotRunReason> {
        let names = self.listed(deadlines.query.at(), scope, None)?;
        if !names.iter().any(|name| name == clone.as_str()) {
            return if started {
                Err(NotRunReason::CleanupFailed)
            } else {
                Ok(())
            };
        }
        let Some(identity) = identity else {
            return Err(NotRunReason::IdentityConflict);
        };
        let status = self.vm_status(clone, deadlines.query.at(), scope, None)?;
        if !owns(&status, &identity, run) {
            return Err(NotRunReason::IdentityConflict);
        }
        self.note_session_boot(&status);
        let stop_at = self.stop_owned(clone, deadlines.stop.at(), scope)?;
        let status = self.vm_status(clone, deadlines.query.at().min(stop_at), scope, None)?;
        if !parse::stopped_clean(&status) {
            return Err(NotRunReason::CleanupFailed);
        }
        if !owns(&status, &identity, run) {
            return Err(NotRunReason::IdentityConflict);
        }
        self.note_session_boot(&status);
        let delete_at = self.delete_owned(clone, deadlines.delete.at(), scope)?;
        let names = self.listed(deadlines.query.at().min(delete_at), scope, None)?;
        if names.iter().any(|name| name == clone.as_str()) {
            Err(NotRunReason::CleanupFailed)
        } else {
            Ok(())
        }
    }

    /// List and status must name this exact incarnation before start.
    pub(super) fn confirm_owned(
        &mut self,
        clone: &CloneId,
        run: &RunId,
        identity: &VmIdentity,
        requested: MonoInstant,
    ) -> Result<(), NotRunReason> {
        let names = self.listed(requested, Scope::Prep, None)?;
        if !names.iter().any(|name| name == clone.as_str()) {
            return Err(NotRunReason::IdentityConflict);
        }
        let status = self.vm_status(clone, requested, Scope::Prep, None)?;
        if owns(&status, identity, run) {
            Ok(())
        } else {
            Err(NotRunReason::IdentityConflict)
        }
    }

    fn note_session_boot(&mut self, status: &parse::VmStatus) {
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

fn owns(status: &parse::VmStatus, identity: &VmIdentity, run: &RunId) -> bool {
    status.state != VmState::Absent
        && status.run.as_deref() == Some(run.as_str())
        && status.incarnation.as_deref() == Some(identity.incarnation())
        && status.config.as_deref() == Some(identity.config())
}
