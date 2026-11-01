//! Startup recovery for unsealed ledger rows.
//!
//! The caller drives this once before reserve. It does not clone, start, or
//! restart a VM. It does not use the dead row's absolute deadline: that
//! instant already passed does not block the deletion a restart still owes,
//! and the old attempt stays `not_run` with no score.

use super::super::deadline::{HostClock, QueryDeadline, SealDeadline};
use super::super::guest::GuestChannel;
use super::super::identity::{CloneId, RunId};
use super::super::ledger::{AttemptLedger, LedgerFact};
use super::super::lock::CleanupLock;
use super::super::receipt::{AttemptOutcome, AttemptReceipt, NotRunReason, Phase};
use super::super::runner::TartRunner;
use super::{Attempt, CleanupDeadlines, StepError};

impl<'a, R, G, L, K, C> Attempt<'a, R, G, L, K, C>
where
    R: TartRunner,
    G: GuestChannel,
    L: AttemptLedger,
    K: CleanupLock,
    C: HostClock,
{
    /// Reclaim every unsealed row, then leave this machine [`Phase::Idle`].
    ///
    /// Does not clone or start. Appends [`LedgerFact::RecoveryNoted`] before
    /// stop or delete. An append failure deletes nothing. A row with
    /// [`LedgerFact::CleanupUnknown`] is [`NotRunReason::OwnerDeath`]; any
    /// other unsealed row is [`NotRunReason::HostCrash`]. Neither keeps a
    /// score. Does not block. The returned receipts are not kept on the
    /// machine, so a later reserve is a new attempt.
    pub fn recover(
        &mut self,
        deadlines: &CleanupDeadlines,
    ) -> Result<Vec<AttemptReceipt>, StepError> {
        if self.delete_forbidden {
            return Err(StepError::DeleteForbidden);
        }
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        if self.phase != Phase::Idle {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        }
        self.fit(deadlines)?;
        if self.clock.now() >= deadlines.query.at() {
            return Err(StepError::Expired);
        }
        let locked = self
            .earlier(deadlines.query.at(), self.budgets.query, None)
            .map_err(|_| StepError::Expired)?;
        if let Err(err) = self.lock.try_acquire(locked) {
            return Err(self.on_lock(err));
        }
        let listed = self
            .earlier(deadlines.query.at(), self.budgets.query, None)
            .map_err(|_| StepError::Expired)?;
        let rows = self
            .ledger
            .rows(QueryDeadline::from_instant(listed))
            .map_err(StepError::Ledger)?;
        let mut receipts = Vec::new();
        for row in rows {
            if row.is_sealed() {
                continue;
            }
            let run = row.run().clone();
            let clone = row.clone_id().clone();
            let reason = if row.facts().contains(&LedgerFact::CleanupUnknown) {
                NotRunReason::OwnerDeath
            } else {
                NotRunReason::HostCrash
            };
            let note_at = self
                .earlier(deadlines.query.at(), self.budgets.query, None)
                .map_err(|_| StepError::Expired)?;
            if let Err(err) = self.ledger.append(
                &clone,
                LedgerFact::RecoveryNoted,
                QueryDeadline::from_instant(note_at),
            ) {
                return self.stop_recovery(&run, &clone, NotRunReason::ReceiptFailed, true, err);
            }
            if let Err(reason) = self.reclaim(&run, &clone, deadlines, None) {
                self.mark_quarantine(&clone, deadlines);
                return self.stop_recovery(&run, &clone, reason, true, reason);
            }
            let seal_at = self
                .earlier(deadlines.seal.at(), self.budgets.seal, None)
                .map_err(|_| StepError::Expired)?;
            if let Err(err) = self
                .ledger
                .seal(&clone, SealDeadline::from_instant(seal_at))
            {
                self.mark_quarantine(&clone, deadlines);
                return self.stop_recovery(&run, &clone, NotRunReason::ReceiptFailed, true, err);
            }
            receipts.push(AttemptReceipt::new(
                Some(run),
                Some(clone),
                AttemptOutcome::NotRun {
                    reason,
                    client_started: false,
                },
                true,
                super::super::super::CleanupDisposition::Allocated,
                true,
                false,
            ));
        }
        self.clear_for_reserve();
        Ok(receipts)
    }

    fn mark_quarantine(&mut self, clone: &CloneId, deadlines: &CleanupDeadlines) {
        let Ok(at) = self.earlier(deadlines.query.at(), self.budgets.query, None) else {
            return;
        };
        let _ = self.ledger.append(
            clone,
            LedgerFact::Quarantined,
            QueryDeadline::from_instant(at),
        );
    }

    fn stop_recovery<E>(
        &mut self,
        run: &RunId,
        clone: &CloneId,
        reason: NotRunReason,
        still_open: bool,
        error: E,
    ) -> Result<Vec<AttemptReceipt>, StepError>
    where
        E: IntoRecoveryError,
    {
        self.halted = true;
        self.run = Some(run.clone());
        self.clone_id = Some(clone.clone());
        if self.trigger.is_none() {
            self.trigger = Some(reason);
        }
        self.phase = Phase::Quarantined;
        self.receipt = Some(AttemptReceipt::new(
            Some(run.clone()),
            Some(clone.clone()),
            AttemptOutcome::Quarantined {
                reason,
                client_started: false,
            },
            false,
            super::super::super::CleanupDisposition::Allocated,
            false,
            still_open,
        ));
        Err(error.into_step())
    }

    fn clear_for_reserve(&mut self) {
        self.phase = Phase::Idle;
        self.run = None;
        self.clone_id = None;
        self.permit = None;
        self.absolute = None;
        self.origin = None;
        self.cleanup_origin = None;
        self.evidence = super::super::receipt::Evidence::new();
        self.trigger = None;
        self.revoke_ok = false;
        self.revoke_attempted = false;
        self.vm_cleanup_ok = false;
        self.no_allocation = false;
        self.delete_forbidden = false;
        self.halted = false;
        self.client_starts = 0;
        self.lease_until = None;
        self.lease_seq = 0;
        self.receipt = None;
    }
}

/// Maps a recovery failure into [`StepError`] without a second copy of the row.
trait IntoRecoveryError {
    fn into_step(self) -> StepError;
}

impl IntoRecoveryError for super::super::ledger::LedgerError {
    fn into_step(self) -> StepError {
        StepError::Ledger(self)
    }
}

impl IntoRecoveryError for NotRunReason {
    fn into_step(self) -> StepError {
        StepError::Cleanup(self)
    }
}
