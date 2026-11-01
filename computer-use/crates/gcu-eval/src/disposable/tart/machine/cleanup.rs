//! Revoke, destroy, and seal. Confirmation failure is not absence.

use super::super::deadline::{
    DeleteDeadline, HostClock, MonoInstant, QueryDeadline, RevokeDeadline, SealDeadline,
    StopDeadline,
};
use super::super::guest::GuestChannel;
use super::super::identity::{CloneId, RunId};
use super::super::ledger::{AttemptLedger, LedgerFact};
use super::super::lock::CleanupLock;
use super::super::parse::{self, VmState};
use super::super::receipt::{NotRunReason, Phase};
use super::super::runner::TartRunner;
use super::{absent_reply, call_reason, Attempt, RuntimeLoss, StepError};

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
    /// destroy. Does not block. Does not delete the VM.
    pub fn revoke(&mut self, deadline: RevokeDeadline) -> Result<(), StepError> {
        self.ensure_phase(&[Phase::Running, Phase::Revoking])?;
        if self.revoke_attempted {
            self.phase = Phase::Revoking;
            return if self.revoke_ok {
                Ok(())
            } else {
                Err(StepError::Cleanup(NotRunReason::CleanupFailed))
            };
        }
        let effective = match self.admit(deadline.at(), self.budgets.revoke, false) {
            Ok(at) => at,
            Err(StepError::AboveBudget) => return Err(StepError::AboveBudget),
            Err(err) => {
                self.revoke_attempted = true;
                self.phase = Phase::Revoking;
                return Err(err);
            }
        };
        self.revoke_attempted = true;
        if self.cleanup_origin.is_none() {
            self.cleanup_origin = Some(self.clock.now());
        }
        let call = RevokeDeadline::from_instant(effective);
        match self.guest.revoke(call) {
            Ok(_) => {
                self.revoke_ok = true;
                self.phase = Phase::Revoking;
                Ok(())
            }
            Err(err) => {
                if self.trigger.is_none() {
                    self.trigger = Some(call_reason(&err));
                }
                self.phase = Phase::Revoking;
                Err(StepError::Call(err))
            }
        }
    }

    /// Stop the owned clone, delete it, and require a complete list that omits it.
    ///
    /// A failed or incomplete list is not absence. A run-id mismatch does not
    /// stop or delete. Golden and other names in the list are left alone.
    /// Does not block. Does not start a VM.
    pub fn destroy(&mut self, deadlines: &CleanupDeadlines) -> Result<(), StepError> {
        if self.delete_forbidden {
            return Err(StepError::DeleteForbidden);
        }
        self.ensure_phase(&[Phase::Revoking])?;
        self.fit(deadlines)?;
        if let Err(err) = self.admit(deadlines.query.at(), self.budgets.query, false) {
            if !matches!(err, StepError::AboveBudget) {
                self.vm_cleanup_ok = false;
                self.phase = Phase::Destroying;
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
        match self.reclaim(&run, &clone, deadlines, self.absolute) {
            Ok(()) => {
                self.vm_cleanup_ok = true;
                self.phase = Phase::Destroying;
                Ok(())
            }
            Err(reason) => {
                if self.trigger.is_none() {
                    self.trigger = Some(reason);
                }
                self.vm_cleanup_ok = false;
                self.phase = Phase::Destroying;
                Err(StepError::Cleanup(reason))
            }
        }
    }

    /// Seal the receipt.
    ///
    /// A confirmed cleanup with no fault and complete evidence keeps the
    /// provisional score. Any fault keeps `not_run` even if that score was
    /// pass. A failed seal or an unconfirmed cleanup is `Quarantined` and
    /// stops later task, arm, and repeat. Does not block.
    pub fn seal(
        &mut self,
        deadline: SealDeadline,
    ) -> Result<super::super::receipt::AttemptReceipt, StepError> {
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        if self.phase != Phase::Destroying {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        }
        if self.halted && self.trigger == Some(NotRunReason::LockBusy) {
            return self.seal_lock_busy(deadline);
        }
        let effective = match self.admit(deadline.at(), self.budgets.seal, false) {
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
            let query = match self.query_deadline(effective) {
                Ok(query) => query,
                Err(err) => {
                    self.store_quarantine(NotRunReason::ReceiptFailed);
                    return Err(err);
                }
            };
            if let Err(err) = self.ledger.append(&clone, LedgerFact::Quarantined, query) {
                self.store_quarantine(NotRunReason::ReceiptFailed);
                return Err(StepError::Ledger(err));
            }
        }
        match self
            .ledger
            .seal(&clone, SealDeadline::from_instant(effective))
        {
            Ok(()) => {
                let receipt = self.make_receipt(confirmed, None);
                self.phase = if confirmed {
                    Phase::Final
                } else {
                    Phase::Quarantined
                };
                self.receipt = Some(receipt.clone());
                Ok(receipt)
            }
            Err(err) => {
                self.halted = true;
                self.store_quarantine(NotRunReason::ReceiptFailed);
                Err(StepError::Ledger(err))
            }
        }
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
        self.ensure_phase(&[Phase::Running])?;
        let Some(until) = self.lease_until else {
            return Err(StepError::LeaseLive);
        };
        if self.clock.now() < until {
            return Err(StepError::LeaseLive);
        }
        if self.clock.now() >= deadline.at() {
            return Err(StepError::Expired);
        }
        let effective = match self.earlier(deadline.at(), self.budgets.revoke, None) {
            Ok(at) => at,
            Err(_) => return Err(StepError::Expired),
        };
        if let Err(err) = self.lock.try_acquire(effective) {
            return Err(self.on_lock(err));
        }
        self.delete_forbidden = true;
        if self.trigger.is_none() {
            self.trigger = Some(NotRunReason::OwnerDeath);
        }
        let _ = self.guest.revoke(RevokeDeadline::from_instant(effective));
        if let Some(clone) = self.clone_id.clone() {
            if let Ok(query) = self.query_deadline(deadline.at()) {
                let _ = self
                    .ledger
                    .append(&clone, LedgerFact::CleanupUnknown, query);
            }
        }
        let receipt = super::super::receipt::AttemptReceipt::new(
            self.run.clone(),
            self.clone_id.clone(),
            super::super::receipt::AttemptOutcome::NotRun {
                reason: NotRunReason::OwnerDeath,
                client_started: self.client_starts > 0,
            },
            false,
            super::super::super::CleanupDisposition::Unknown,
            false,
            true,
        );
        self.phase = Phase::Suspended;
        self.receipt = Some(receipt);
        Err(StepError::OwnerDead)
    }

    /// Record a lock-busy refusal without taking the lock again.
    ///
    /// The mutex is still held. Waiting here would block the caller. No Tart
    /// command is issued.
    fn seal_lock_busy(
        &mut self,
        deadline: SealDeadline,
    ) -> Result<super::super::receipt::AttemptReceipt, StepError> {
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

    /// Stop and delete one ledger id. Never starts. Never deletes a different id.
    pub(super) fn reclaim(
        &mut self,
        run: &RunId,
        clone: &CloneId,
        deadlines: &CleanupDeadlines,
        absolute: Option<MonoInstant>,
    ) -> Result<(), NotRunReason> {
        let list_at = self.earlier(deadlines.query.at(), self.budgets.query, absolute)?;
        let listed = self
            .runner
            .list(QueryDeadline::from_instant(list_at))
            .map_err(|err| call_reason(&err))?;
        let names = parse::parse_list(listed.as_bytes()).map_err(|_| NotRunReason::Malformed)?;
        let present = names.iter().any(|name| name == clone.as_str());
        if present {
            let status = self.read_status(clone, deadlines.query.at(), absolute)?;
            if status.state == VmState::Absent || status.run.as_deref() != Some(run.as_str()) {
                return Err(NotRunReason::IdentityConflict);
            }
        }
        let stop_at = self.earlier(deadlines.stop.at(), self.budgets.stop, absolute)?;
        if let Err(err) = self.runner.stop(clone, StopDeadline::from_instant(stop_at)) {
            if !absent_reply(&err) {
                return Err(call_reason(&err));
            }
        }
        let status = self.read_status(clone, deadlines.query.at(), absolute)?;
        if !parse::stopped_clean(&status) {
            return Err(NotRunReason::CleanupFailed);
        }
        if status.state != VmState::Absent && status.run.as_deref() != Some(run.as_str()) {
            return Err(NotRunReason::IdentityConflict);
        }
        let delete_at = self.earlier(deadlines.delete.at(), self.budgets.delete, absolute)?;
        if let Err(err) = self
            .runner
            .delete(clone, DeleteDeadline::from_instant(delete_at))
        {
            if !absent_reply(&err) {
                return Err(call_reason(&err));
            }
        }
        let confirm_at = self.earlier(deadlines.query.at(), self.budgets.query, absolute)?;
        let confirm = self
            .runner
            .list(QueryDeadline::from_instant(confirm_at))
            .map_err(|err| call_reason(&err))?;
        let names = parse::parse_list(confirm.as_bytes()).map_err(|_| NotRunReason::Malformed)?;
        if names.iter().any(|name| name == clone.as_str()) {
            return Err(NotRunReason::CleanupFailed);
        }
        Ok(())
    }

    fn read_status(
        &mut self,
        clone: &CloneId,
        requested: MonoInstant,
        absolute: Option<MonoInstant>,
    ) -> Result<parse::VmStatus, NotRunReason> {
        let at = self.earlier(requested, self.budgets.query, absolute)?;
        let body = self
            .runner
            .status(clone, QueryDeadline::from_instant(at))
            .map_err(|err| call_reason(&err))?;
        parse::parse_status(body.as_bytes()).map_err(|_| NotRunReason::Malformed)
    }
}
