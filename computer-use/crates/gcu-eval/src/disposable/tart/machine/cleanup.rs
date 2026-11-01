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
                self.set_open(Phase::Revoking);
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
                self.set_open(Phase::Revoking);
                Ok(())
            }
            Err(err) => {
                if self.trigger.is_none() {
                    self.trigger = Some(call_reason(&err));
                }
                self.set_open(Phase::Revoking);
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
            self.absolute,
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
    /// pass. A failed seal or an unconfirmed cleanup is `Quarantined` and
    /// stops later task, arm, and repeat. Does not block.
    pub fn seal(
        &mut self,
        deadline: SealDeadline,
    ) -> Result<super::super::receipt::AttemptReceipt, StepError> {
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        if self.lease_expired() {
            self.expire_owner(Some(deadline.at()));
            return Err(StepError::OwnerDead);
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
            Err(err) => {
                self.sealed = self.row_is_sealed();
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
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        // An expired lease is decided here even when this call's own deadline
        // is already due. The note may be skipped; the hold is not.
        if self.lease_expired() {
            self.expire_owner(Some(deadline.at()));
            return Err(StepError::OwnerDead);
        }
        self.ensure_phase(&[Phase::Running])?;
        Err(StepError::LeaseLive)
    }

    /// Enter the owner-death hold. Does not delete, does not renew, does not score.
    ///
    /// Revoke and the ledger note are best-effort. A missed reply still leaves
    /// `Suspended` and forbids delete. Does not block and does not arm a timer.
    pub(super) fn expire_owner(&mut self, note: Option<MonoInstant>) {
        if self.phase.closed() {
            return;
        }
        self.delete_forbidden = true;
        if self.trigger.is_none() {
            self.trigger = Some(NotRunReason::OwnerDeath);
        }
        let now = self.clock.now();
        let bound = |budget: std::time::Duration| -> Option<MonoInstant> {
            let cap = now.checked_add(budget).ok()?;
            let at = note.map_or(cap, |requested| requested.min(cap));
            (now < at).then_some(at)
        };
        if let Some(at) = bound(self.budgets.revoke) {
            let _ = self.guest.revoke(RevokeDeadline::from_instant(at));
        }
        if let (Some(at), Some(clone)) = (bound(self.budgets.query), self.clone_id.clone()) {
            let _ = self.ledger.append(
                &clone,
                LedgerFact::CleanupUnknown,
                QueryDeadline::from_instant(at),
            );
        }
        self.phase = Phase::Suspended;
        self.receipt = Some(AttemptReceipt::new(
            self.run.clone(),
            self.clone_id.clone(),
            AttemptOutcome::NotRun {
                reason: NotRunReason::OwnerDeath,
                client_started: self.client_starts > 0,
            },
            false,
            super::super::super::CleanupDisposition::Unknown,
            false,
            self.clone_id.is_some(),
        ));
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

    /// Stop and delete one owned id. A complete list that omits the name is absence.
    ///
    /// That absence does not authorize a later stop or delete of the same name.
    /// A present name is destroyed only when its status matches `identity`.
    /// A different guest boot drops the score and still allows that delete.
    /// Never starts. Never deletes a different id.
    pub(super) fn reclaim(
        &mut self,
        run: &RunId,
        clone: &CloneId,
        deadlines: &CleanupDeadlines,
        absolute: Option<MonoInstant>,
        identity: Option<VmIdentity>,
    ) -> Result<(), NotRunReason> {
        let names = self.names_at(deadlines.query.at(), absolute)?;
        if !names.iter().any(|name| name == clone.as_str()) {
            return Ok(());
        }
        let Some(identity) = identity else {
            return Err(NotRunReason::IdentityConflict);
        };
        let status = self.read_status(clone, deadlines.query.at(), absolute)?;
        if !owns(&status, &identity, run) {
            return Err(NotRunReason::IdentityConflict);
        }
        self.note_session_boot(&status);
        let stop_at = self.earlier(deadlines.stop.at(), self.budgets.stop, absolute)?;
        if let Err(err) = self.runner.stop(clone, StopDeadline::from_instant(stop_at)) {
            if !absent_reply(&err) {
                return Err(call_reason(&err));
            }
        }
        let status = self.read_status(clone, deadlines.query.at().min(stop_at), absolute)?;
        if !parse::stopped_clean(&status) {
            return Err(NotRunReason::CleanupFailed);
        }
        if !owns(&status, &identity, run) {
            return Err(NotRunReason::IdentityConflict);
        }
        self.note_session_boot(&status);
        let delete_at = self.earlier(deadlines.delete.at(), self.budgets.delete, absolute)?;
        if let Err(err) = self
            .runner
            .delete(clone, DeleteDeadline::from_instant(delete_at))
        {
            if !absent_reply(&err) {
                return Err(call_reason(&err));
            }
        }
        let names = self.names_at(deadlines.query.at().min(delete_at), absolute)?;
        if names.iter().any(|name| name == clone.as_str()) {
            return Err(NotRunReason::CleanupFailed);
        }
        Ok(())
    }

    /// List and status must name this exact incarnation before start.
    pub(super) fn confirm_owned(
        &mut self,
        clone: &CloneId,
        run: &RunId,
        identity: &VmIdentity,
        requested: MonoInstant,
    ) -> Result<(), NotRunReason> {
        let names = self.names_at(requested, self.absolute)?;
        if !names.iter().any(|name| name == clone.as_str()) {
            return Err(NotRunReason::IdentityConflict);
        }
        let status = self.read_status(clone, requested, self.absolute)?;
        if owns(&status, identity, run) {
            Ok(())
        } else {
            Err(NotRunReason::IdentityConflict)
        }
    }

    pub(super) fn names_at(
        &mut self,
        requested: MonoInstant,
        absolute: Option<MonoInstant>,
    ) -> Result<Vec<String>, NotRunReason> {
        let at = self.earlier(requested, self.budgets.query, absolute)?;
        let listed = self
            .runner
            .list(QueryDeadline::from_instant(at))
            .map_err(|err| call_reason(&err))?;
        parse::parse_list(listed.as_bytes()).map_err(|_| NotRunReason::Malformed)
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

    /// Read whether the reserved row is sealed. Unknown means it still needs recovery.
    pub(super) fn row_is_sealed(&mut self) -> bool {
        let Some(clone) = self.clone_id.clone() else {
            return false;
        };
        let Ok(call) = self.query_deadline_from_now() else {
            return false;
        };
        self.ledger
            .query(&clone, call)
            .map(|row| row.is_sealed())
            .unwrap_or(false)
    }

    fn query_deadline_from_now(&self) -> Result<QueryDeadline, StepError> {
        let at = self
            .clock
            .now()
            .checked_add(self.budgets.query)
            .map_err(|_| StepError::Overflow)?;
        Ok(QueryDeadline::from_instant(at))
    }
}

fn owns(status: &parse::VmStatus, identity: &VmIdentity, run: &RunId) -> bool {
    status.state != VmState::Absent
        && status.run.as_deref() == Some(run.as_str())
        && status.incarnation.as_deref() == Some(identity.incarnation())
        && status.config.as_deref() == Some(identity.config())
}
