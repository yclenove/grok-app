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
                return self.stop_recovery(
                    &run,
                    &clone,
                    NotRunReason::ReceiptFailed,
                    StepError::Ledger(err),
                );
            }
            // Identity stays on the row. Copying it onto this Idle attempt would
            // leak the dead reservation into the next reserve.
            if let Err(reason) =
                self.reclaim(&run, &clone, deadlines, None, row.expected().cloned())
            {
                self.mark_quarantine(&clone, deadlines);
                return self.stop_recovery(&run, &clone, reason, StepError::Cleanup(reason));
            }
            let seal_at = self
                .earlier(deadlines.seal.at(), self.budgets.seal, None)
                .map_err(|_| StepError::Expired)?;
            if let Err(err) = self
                .ledger
                .seal(&clone, SealDeadline::from_instant(seal_at))
            {
                self.mark_quarantine(&clone, deadlines);
                return self.stop_recovery(
                    &run,
                    &clone,
                    NotRunReason::ReceiptFailed,
                    StepError::Ledger(err),
                );
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

    fn stop_recovery(
        &mut self,
        run: &RunId,
        clone: &CloneId,
        reason: NotRunReason,
        error: StepError,
    ) -> Result<Vec<AttemptReceipt>, StepError> {
        self.halted = true;
        self.run = Some(run.clone());
        self.clone_id = Some(clone.clone());
        self.sealed = self.row_is_sealed();
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
            !self.sealed,
        ));
        Err(error)
    }
}
