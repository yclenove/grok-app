//! Destroy, seal, recover, loss, and kill. One command per step.

use super::super::super::deadline::{
    DeleteDeadline, HostClock, MonoInstant, QueryDeadline, SealDeadline, StopDeadline,
};
use super::super::super::guest::GuestChannel;
use super::super::super::identity::RunId;
use super::super::super::ledger::{
    AttemptLedger, LedgerError, LedgerFact, LedgerPoll, Reservation,
};
use super::super::super::lock::CleanupLock;
use super::super::super::owner::OwnerSource;
use super::super::super::parse;
use super::super::super::receipt::{AttemptOutcome, AttemptReceipt, NotRunReason, Phase};
use super::super::super::runner::{CommandKind, ReapPoll, TartRunner};
use super::super::{Attempt, CleanupDeadlines, CompletedStep, Scope, StepError};
use super::{
    share_short, Cursor, DestroyCur, Io, KillCur, LossCur, ReclaimCur, RecoverCur, SealCur, StepOut,
};
use crate::disposable::CleanupDisposition;

enum ReclaimOut {
    Stay(ReclaimCur),
    Done,
    Fail(NotRunReason),
    Stop(StepError),
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
    pub(super) fn tick_destroy(
        &mut self,
        mut cur: DestroyCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        if cur.reclaim.stage == 5 && self.stop_fresh {
            return StepOut::Stay(Cursor::Destroy(cur));
        }
        let deadlines = cur.deadlines;
        match self.tick_reclaim(&mut cur.reclaim, deadlines, poll_caller, step_end) {
            ReclaimOut::Stay(reclaim) => {
                cur.reclaim = reclaim;
                StepOut::Stay(Cursor::Destroy(cur))
            }
            ReclaimOut::Done => {
                self.vm_cleanup_ok = true;
                self.set_open(Phase::Destroying);
                self.clear_slot();
                StepOut::Finish(CompletedStep::Destroyed)
            }
            ReclaimOut::Fail(reason) => self.destroy_fail(reason),
            ReclaimOut::Stop(err) => {
                if !matches!(err, StepError::AboveBudget) {
                    self.vm_cleanup_ok = false;
                    self.set_open(Phase::Destroying);
                }
                StepOut::Stop(err)
            }
        }
    }

    fn destroy_fail(&mut self, reason: NotRunReason) -> StepOut {
        if self.trigger.is_none() {
            self.trigger = Some(reason);
        }
        self.vm_cleanup_ok = false;
        self.set_open(Phase::Destroying);
        let err = match reason {
            NotRunReason::Deadline => StepError::Expired,
            other => StepError::Cleanup(other),
        };
        StepOut::Stop(err)
    }

    pub(super) fn tick_seal(
        &mut self,
        mut cur: SealCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        if cur.stage == 0 {
            if self.halted && self.trigger == Some(NotRunReason::LockBusy) {
                return self.finish_lock_busy(cur.caller);
            }
            return match self.gate_lock(
                cur.caller,
                self.budgets.seal,
                Scope::Cleanup,
                None,
                poll_caller,
                step_end,
            ) {
                Ok(false) => StepOut::Stay(Cursor::Seal(cur)),
                Ok(true) => {
                    cur.confirmed = self.revoke_ok && self.vm_cleanup_ok && !self.halted;
                    cur.stage = 1;
                    StepOut::Stay(Cursor::Seal(cur))
                }
                Err(StepError::LockBusy) => {
                    if self.receipt.is_none() {
                        self.store_quarantine(NotRunReason::LockBusy);
                    }
                    StepOut::Stop(StepError::LockBusy)
                }
                Err(StepError::AboveBudget) => StepOut::Stop(StepError::AboveBudget),
                Err(err) => {
                    self.store_quarantine(NotRunReason::ReceiptFailed);
                    StepOut::Stop(err)
                }
            };
        }
        if self.no_allocation || self.clone_id.is_none() {
            return self.finish_no_allocation();
        }
        match cur.stage {
            1 => self.seal_fact(cur, poll_caller, step_end),
            2 => self.seal_write(cur, poll_caller, step_end),
            _ => self.seal_read(cur, poll_caller, step_end),
        }
    }

    fn finish_lock_busy(&mut self, caller: MonoInstant) -> StepOut {
        if self.clock.now() >= caller {
            return StepOut::Stop(StepError::Expired);
        }
        let receipt = self.make_receipt(false, Some(NotRunReason::LockBusy));
        self.phase = Phase::Quarantined;
        self.receipt = Some(receipt.clone());
        StepOut::Finish(CompletedStep::Sealed(receipt))
    }

    fn finish_no_allocation(&mut self) -> StepOut {
        let quarantine = self.trigger == Some(NotRunReason::LockBusy);
        let receipt = self.make_receipt(!quarantine, quarantine.then_some(NotRunReason::LockBusy));
        self.phase = if quarantine {
            Phase::Quarantined
        } else {
            Phase::Final
        };
        self.receipt = Some(receipt.clone());
        StepOut::Finish(CompletedStep::Sealed(receipt))
    }

    fn seal_fact(
        &mut self,
        mut cur: SealCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        if cur.confirmed {
            cur.stage = 2;
            return StepOut::Stay(Cursor::Seal(cur));
        }
        let Some(clone) = self.clone_id.clone() else {
            return self.finish_no_allocation();
        };
        match self.append_fact(
            &clone,
            LedgerFact::Quarantined,
            cur.caller,
            Scope::Cleanup,
            poll_caller,
            step_end,
        ) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Seal(cur))
            }
            Io::Ready(()) => {
                self.clear_slot();
                cur.stage = 2;
                StepOut::Stay(Cursor::Seal(cur))
            }
            Io::Fault(_) | Io::Stop(_) => {
                self.store_quarantine(NotRunReason::ReceiptFailed);
                StepOut::Stop(StepError::Ledger(LedgerError::Failed))
            }
            Io::Discard => StepOut::Stay(Cursor::Seal(cur)),
        }
    }

    fn seal_write(
        &mut self,
        mut cur: SealCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        let Some(clone) = self.clone_id.clone() else {
            return self.finish_no_allocation();
        };
        let op = match self.save_op(cur.caller, self.budgets.seal, Scope::Cleanup, None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.ledger_io(call_end, op, |machine, end| {
            machine
                .ledger
                .begin_seal(&clone, SealDeadline::from_instant(end))
        }) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Seal(cur))
            }
            Io::Ready(_) => self.finish_sealed(cur.confirmed),
            Io::Fault(LedgerError::ReplyLost) => {
                cur.stage = 3;
                StepOut::Stay(Cursor::Seal(cur))
            }
            Io::Fault(err) => {
                cur.stage = 3;
                let _ = err;
                StepOut::Stay(Cursor::Seal(cur))
            }
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Seal(cur)),
        }
    }

    fn seal_read(
        &mut self,
        cur: SealCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        let Some(clone) = self.clone_id.clone() else {
            self.store_quarantine(NotRunReason::ReceiptFailed);
            return StepOut::Stop(StepError::Ledger(LedgerError::NotFound));
        };
        let op = match self.save_op(cur.caller, self.budgets.seal, Scope::Cleanup, None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.ledger_io(call_end, op, |machine, end| {
            machine
                .ledger
                .begin_query(&clone, QueryDeadline::from_instant(end))
        }) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Seal(cur))
            }
            Io::Ready(LedgerPoll::Read(row)) if row.is_sealed() => {
                self.finish_sealed(cur.confirmed)
            }
            Io::Ready(_) | Io::Fault(_) => {
                self.sealed = false;
                self.halted = true;
                self.store_quarantine(NotRunReason::ReceiptFailed);
                StepOut::Stop(StepError::Ledger(LedgerError::ReplyLost))
            }
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Seal(cur)),
        }
    }

    fn finish_sealed(&mut self, confirmed: bool) -> StepOut {
        self.sealed = true;
        let receipt = self.make_receipt(confirmed, None);
        self.phase = if confirmed {
            Phase::Final
        } else {
            Phase::Quarantined
        };
        self.receipt = Some(receipt.clone());
        self.clear_slot();
        StepOut::Finish(CompletedStep::Sealed(receipt))
    }

    pub(super) fn tick_recover(
        &mut self,
        mut cur: RecoverCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        if cur.phase == 0 {
            return match self.gate_lock(
                cur.caller,
                self.budgets.query,
                Scope::Recovery,
                None,
                poll_caller,
                step_end,
            ) {
                Ok(false) => StepOut::Stay(Cursor::Recover(cur)),
                Ok(true) => {
                    cur.phase = 1;
                    StepOut::Stay(Cursor::Recover(cur))
                }
                Err(err) => StepOut::Stop(err),
            };
        }
        if cur.phase == 1 {
            return self.recover_rows(cur, poll_caller, step_end);
        }
        if cur.phase == 2 {
            return self.recover_note(cur, poll_caller, step_end);
        }
        if cur.phase == 3 {
            return self.recover_reclaim(cur, poll_caller, step_end);
        }
        if cur.phase == 4 {
            return self.recover_seal(cur, poll_caller, step_end);
        }
        if cur.phase == 5 {
            return self.recover_seal_read(cur, poll_caller, step_end);
        }
        self.recover_quarantine(cur, poll_caller, step_end)
    }

    fn recover_rows(
        &mut self,
        mut cur: RecoverCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        let op = match self.save_op(cur.caller, self.budgets.query, Scope::Recovery, None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.ledger_rows(call_end, op) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Recover(cur))
            }
            Io::Ready(LedgerPoll::Rows(rows)) => {
                self.clear_slot();
                cur.rows = rows;
                cur.index = 0;
                cur.phase = 2;
                StepOut::Stay(Cursor::Recover(cur))
            }
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Ready(_) | Io::Discard => StepOut::Stop(StepError::Unexpected),
        }
    }

    fn recover_note(
        &mut self,
        mut cur: RecoverCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        let Some(index) = next_open(&cur.rows, cur.index) else {
            self.phase = Phase::Idle;
            return StepOut::Finish(CompletedStep::Recovered(cur.receipts));
        };
        cur.index = index;
        let row = &cur.rows[index];
        if self.needs_rebind(row.run()) {
            self.owner_proof = None;
            self.owner_until = None;
            self.allow_rebind = true;
            return StepOut::Stay(Cursor::Recover(cur));
        }
        let clone = row.clone_id().clone();
        cur.reason = if row.facts().contains(&LedgerFact::CleanupUnknown) {
            NotRunReason::OwnerDeath
        } else {
            NotRunReason::HostCrash
        };
        self.prepare_row(&mut cur, index);
        match self.append_fact(
            &clone,
            LedgerFact::RecoveryNoted,
            cur.caller,
            Scope::Recovery,
            poll_caller,
            step_end,
        ) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Recover(cur))
            }
            Io::Ready(()) => {
                self.clear_slot();
                cur.phase = 3;
                cur.reclaim.stage = 0;
                StepOut::Stay(Cursor::Recover(cur))
            }
            Io::Fault(err) => self.fail_row(
                &cur,
                index,
                NotRunReason::ReceiptFailed,
                StepError::Ledger(err),
            ),
            Io::Stop(err) => self.fail_row(&cur, index, NotRunReason::ReceiptFailed, err),
            Io::Discard => StepOut::Stay(Cursor::Recover(cur)),
        }
    }

    fn prepare_row(&mut self, cur: &mut RecoverCur, index: usize) {
        let run = cur.rows[index].run().clone();
        let clone = cur.rows[index].clone_id().clone();
        let started = cur.rows[index].facts().contains(&LedgerFact::StartIssued);
        let identity = cur.rows[index].expected().cloned();
        let absolute = cur.rows[index].absolute_deadline();
        let incarnation = identity.as_ref().map(|item| item.incarnation().to_owned());
        let config = identity.as_ref().map(|item| item.config().to_owned());
        cur.reclaim = super::empty_reclaim(cur.deadlines, Scope::Recovery);
        cur.reclaim.run = Some(run.clone());
        cur.reclaim.clone = Some(clone.clone());
        cur.reclaim.started = started;
        cur.reclaim.identity = identity;
        cur.reclaim.absolute = Some(absolute);
        self.reclaim_key = Some(super::super::progress::RowKey {
            run,
            clone_id: clone,
            absolute,
            incarnation,
            config,
            started,
        });
    }

    fn needs_rebind(&self, run: &RunId) -> bool {
        self.owner_proof
            .as_ref()
            .is_some_and(|proof| proof.binding.run_id.as_str() != run.as_str())
    }

    fn recover_reclaim(
        &mut self,
        mut cur: RecoverCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        let deadlines = cur.deadlines;
        match self.tick_reclaim(&mut cur.reclaim, deadlines, poll_caller, step_end) {
            ReclaimOut::Stay(reclaim) => {
                cur.reclaim = reclaim;
                StepOut::Stay(Cursor::Recover(cur))
            }
            ReclaimOut::Done => {
                self.reclaim_stage_name = None;
                self.reclaim_key = None;
                self.clear_slot();
                cur.phase = 4;
                StepOut::Stay(Cursor::Recover(cur))
            }
            ReclaimOut::Fail(reason) => {
                cur.reason = reason;
                cur.phase = 6;
                StepOut::Stay(Cursor::Recover(cur))
            }
            ReclaimOut::Stop(err) => StepOut::Stop(err),
        }
    }

    fn recover_seal(
        &mut self,
        mut cur: RecoverCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        let Some(clone) = cur.reclaim.clone.clone() else {
            return StepOut::Stop(StepError::Unexpected);
        };
        let op = match self.save_op(
            cur.deadlines.seal.at(),
            self.budgets.seal,
            Scope::Recovery,
            None,
        ) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.ledger_io(call_end, op, |machine, end| {
            machine
                .ledger
                .begin_seal(&clone, SealDeadline::from_instant(end))
        }) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Recover(cur))
            }
            Io::Ready(_) => self.recover_pushed(cur),
            Io::Fault(_) => {
                cur.phase = 5;
                StepOut::Stay(Cursor::Recover(cur))
            }
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Recover(cur)),
        }
    }

    fn recover_seal_read(
        &mut self,
        cur: RecoverCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        let Some(clone) = cur.reclaim.clone.clone() else {
            return StepOut::Stop(StepError::Unexpected);
        };
        let op = match self.save_op(
            cur.deadlines.seal.at(),
            self.budgets.seal,
            Scope::Recovery,
            None,
        ) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.ledger_io(call_end, op, |machine, end| {
            machine
                .ledger
                .begin_query(&clone, QueryDeadline::from_instant(end))
        }) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Recover(cur))
            }
            Io::Ready(LedgerPoll::Read(row)) if row.is_sealed() => self.recover_pushed(cur),
            Io::Fault(err) => self.fail_row(
                &cur,
                cur.index,
                NotRunReason::ReceiptFailed,
                StepError::Ledger(err),
            ),
            Io::Ready(_) => self.fail_row(
                &cur,
                cur.index,
                NotRunReason::ReceiptFailed,
                StepError::Ledger(LedgerError::ReplyLost),
            ),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Recover(cur)),
        }
    }

    fn recover_pushed(&mut self, mut cur: RecoverCur) -> StepOut {
        let Some(run) = cur.reclaim.run.clone() else {
            return StepOut::Stop(StepError::Unexpected);
        };
        let Some(clone) = cur.reclaim.clone.clone() else {
            return StepOut::Stop(StepError::Unexpected);
        };
        cur.receipts.push(AttemptReceipt::new(
            Some(run),
            Some(clone),
            AttemptOutcome::NotRun {
                reason: cur.reason,
                client_started: false,
            },
            true,
            CleanupDisposition::Allocated,
            true,
            false,
        ));
        if self
            .owner_proof
            .as_ref()
            .map(|proof| proof.binding.run_id.as_str().to_owned())
            != cur.reclaim.run.as_ref().map(|run| run.as_str().to_owned())
        {
            self.owner_proof = None;
            self.owner_until = None;
            self.allow_rebind = true;
        }
        cur.index = cur.index.saturating_add(1);
        cur.phase = 2;
        cur.reclaim.stage = 0;
        self.reclaim_stage_name = None;
        self.reclaim_key = None;
        self.clear_slot();
        StepOut::Stay(Cursor::Recover(cur))
    }

    fn recover_quarantine(
        &mut self,
        cur: RecoverCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        let Some(clone) = cur.reclaim.clone.clone() else {
            return self.fail_row(&cur, cur.index, cur.reason, StepError::Cleanup(cur.reason));
        };
        match self.append_fact(
            &clone,
            LedgerFact::Quarantined,
            cur.caller,
            Scope::Recovery,
            poll_caller,
            step_end,
        ) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Recover(cur))
            }
            _ => self.fail_row(&cur, cur.index, cur.reason, StepError::Cleanup(cur.reason)),
        }
    }

    fn fail_row(
        &mut self,
        cur: &RecoverCur,
        index: usize,
        reason: NotRunReason,
        error: StepError,
    ) -> StepOut {
        let Some(row) = cur.rows.get(index) else {
            return StepOut::Stop(error);
        };
        let run = row.run().clone();
        let clone = row.clone_id().clone();
        self.halted = true;
        self.run = Some(run.clone());
        self.clone_id = Some(clone.clone());
        if self.trigger.is_none() {
            self.trigger = Some(reason);
        }
        self.phase = Phase::Quarantined;
        self.receipt = Some(AttemptReceipt::new(
            Some(run),
            Some(clone),
            AttemptOutcome::Quarantined {
                reason,
                client_started: false,
            },
            false,
            CleanupDisposition::Allocated,
            false,
            true,
        ));
        StepOut::Stop(error)
    }

    fn tick_reclaim(
        &mut self,
        cur: &mut ReclaimCur,
        deadlines: CleanupDeadlines,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> ReclaimOut {
        self.show_reclaim(cur);
        if cur.stage == 0 {
            return match self.gate_lock(
                deadlines.query.at(),
                self.budgets.query,
                cur.scope,
                None,
                poll_caller,
                step_end,
            ) {
                Ok(false) => ReclaimOut::Stay(cur.clone_row()),
                Ok(true) => {
                    cur.stage = 1;
                    ReclaimOut::Stay(cur.clone_row())
                }
                Err(err) => ReclaimOut::Stop(err),
            };
        }
        match cur.stage {
            1 => self.reclaim_list(cur, poll_caller, step_end),
            2 => self.reclaim_status(cur, deadlines, poll_caller, step_end, false),
            3 => self.reclaim_stop(cur, deadlines, poll_caller, step_end),
            4 => self.reclaim_status(cur, deadlines, poll_caller, step_end, true),
            5 => self.reclaim_delete(cur, deadlines, poll_caller, step_end),
            _ => self.reclaim_absent(cur, poll_caller, step_end),
        }
    }

    fn reclaim_list(
        &mut self,
        cur: &mut ReclaimCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> ReclaimOut {
        let Some(clone) = cur.clone.clone() else {
            return ReclaimOut::Fail(NotRunReason::Unexpected);
        };
        match self.list_names(cur.entry_query, cur.scope, None, poll_caller, step_end) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                ReclaimOut::Stay(cur.clone_row())
            }
            Io::Ready(names) => {
                self.clear_slot();
                if names.iter().any(|name| name == clone.as_str()) {
                    cur.stage = 2;
                    ReclaimOut::Stay(cur.clone_row())
                } else if cur.started {
                    ReclaimOut::Fail(NotRunReason::CleanupFailed)
                } else {
                    ReclaimOut::Done
                }
            }
            Io::Stop(StepError::Expired) => ReclaimOut::Fail(NotRunReason::Deadline),
            Io::Stop(StepError::MalformedBody) => ReclaimOut::Fail(NotRunReason::Malformed),
            Io::Discard => ReclaimOut::Stay(cur.clone_row()),
            Io::Stop(_) | Io::Fault(_) => ReclaimOut::Fail(NotRunReason::CleanupFailed),
        }
    }

    fn reclaim_status(
        &mut self,
        cur: &mut ReclaimCur,
        deadlines: CleanupDeadlines,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
        after_stop: bool,
    ) -> ReclaimOut {
        let Some(clone) = cur.clone.clone() else {
            return ReclaimOut::Fail(NotRunReason::Unexpected);
        };
        let Some(run) = cur.run.clone() else {
            return ReclaimOut::Fail(NotRunReason::Unexpected);
        };
        let Some(identity) = cur.identity.clone() else {
            return ReclaimOut::Fail(NotRunReason::IdentityConflict);
        };
        let saved = if after_stop {
            cur.stop_at
                .map_or(cur.entry_query, |stop| cur.entry_query.min(stop))
        } else {
            deadlines.query.at()
        };
        match self.vm_status(&clone, saved, cur.scope, None, poll_caller, step_end) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                ReclaimOut::Stay(cur.clone_row())
            }
            Io::Ready(status) => {
                self.clear_slot();
                if status.state == parse::VmState::Absent {
                    return ReclaimOut::Fail(NotRunReason::CleanupFailed);
                }
                if !super::super::cleanup::owns(&status, &identity, &run) {
                    return ReclaimOut::Fail(NotRunReason::IdentityConflict);
                }
                self.note_session_boot(&status);
                if after_stop && !parse::stopped_clean(&status) {
                    return ReclaimOut::Fail(NotRunReason::CleanupFailed);
                }
                cur.stage = if after_stop { 5 } else { 3 };
                ReclaimOut::Stay(cur.clone_row())
            }
            Io::Stop(StepError::Expired) => ReclaimOut::Fail(NotRunReason::Deadline),
            Io::Stop(StepError::MalformedBody) => ReclaimOut::Fail(NotRunReason::Malformed),
            Io::Discard => ReclaimOut::Stay(cur.clone_row()),
            Io::Stop(_) | Io::Fault(_) => ReclaimOut::Fail(NotRunReason::CleanupFailed),
        }
    }

    fn reclaim_stop(
        &mut self,
        cur: &mut ReclaimCur,
        _deadlines: CleanupDeadlines,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> ReclaimOut {
        self.owned_command(cur, poll_caller, step_end, true)
    }

    fn reclaim_delete(
        &mut self,
        cur: &mut ReclaimCur,
        _deadlines: CleanupDeadlines,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> ReclaimOut {
        if self.stop_fresh {
            return ReclaimOut::Stay(cur.clone_row());
        }
        self.owned_command(cur, poll_caller, step_end, false)
    }

    fn owned_command(
        &mut self,
        cur: &mut ReclaimCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
        stop: bool,
    ) -> ReclaimOut {
        let Some(clone) = cur.clone.clone() else {
            return ReclaimOut::Fail(NotRunReason::Unexpected);
        };
        let saved = if stop {
            cur.entry_stop
        } else {
            cur.entry_delete
        };
        let budget = if stop {
            self.budgets.stop
        } else {
            self.budgets.delete
        };
        let op = match self.save_op(saved, budget, cur.scope, None) {
            Ok(end) => end,
            Err(_) => return ReclaimOut::Fail(NotRunReason::Deadline),
        };
        if self.lease_expired() && cur.scope != Scope::Recovery {
            let _ = self.suspend_owner(saved);
            return ReclaimOut::Fail(NotRunReason::OwnerDeath);
        }
        if self.arm_share(op, share_short()).is_err() {
            return ReclaimOut::Fail(NotRunReason::Deadline);
        }
        let call_end = self.call_end(step_end, poll_caller, op);
        let kind = if stop {
            CommandKind::Stop
        } else {
            CommandKind::Delete
        };
        let op_name = if stop { "stop" } else { "delete" };
        let polled = self.runner_io(call_end, op, op_name, kind, |machine, end| {
            if stop {
                machine
                    .runner
                    .begin_stop(&clone, StopDeadline::from_instant(end))
            } else {
                machine
                    .runner
                    .begin_delete(&clone, DeleteDeadline::from_instant(end))
            }
        });
        match polled {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                ReclaimOut::Stay(cur.clone_row())
            }
            Io::Ready(_) => {
                if stop {
                    cur.stop_at = self.op_end.or(Some(op));
                    cur.stage = 4;
                } else {
                    cur.delete_at = self.op_end.or(Some(op));
                    cur.stage = 6;
                }
                self.clear_slot();
                ReclaimOut::Stay(cur.clone_row())
            }
            Io::Stop(StepError::Call(err)) if super::super::absent_reply(&err) => {
                if stop {
                    cur.stop_at = Some(op);
                    cur.stage = 4;
                } else {
                    cur.delete_at = Some(op);
                    cur.stage = 6;
                }
                self.clear_slot();
                ReclaimOut::Stay(cur.clone_row())
            }
            Io::Stop(StepError::Expired) => ReclaimOut::Fail(NotRunReason::Deadline),
            Io::Discard => ReclaimOut::Stay(cur.clone_row()),
            Io::Stop(_) | Io::Fault(_) => ReclaimOut::Fail(NotRunReason::CommandFailed),
        }
    }

    fn reclaim_absent(
        &mut self,
        cur: &mut ReclaimCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> ReclaimOut {
        let Some(clone) = cur.clone.clone() else {
            return ReclaimOut::Fail(NotRunReason::Unexpected);
        };
        let saved = cur
            .delete_at
            .map_or(cur.entry_query, |delete| cur.entry_query.min(delete));
        match self.list_names(saved, cur.scope, None, poll_caller, step_end) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                ReclaimOut::Stay(cur.clone_row())
            }
            Io::Ready(names) => {
                self.clear_slot();
                if names.iter().any(|name| name == clone.as_str()) {
                    ReclaimOut::Fail(NotRunReason::CleanupFailed)
                } else {
                    cur.stage = 7;
                    self.reclaim_stage_name = Some("row_done");
                    ReclaimOut::Done
                }
            }
            Io::Stop(StepError::Expired) => ReclaimOut::Fail(NotRunReason::Deadline),
            Io::Discard => ReclaimOut::Stay(cur.clone_row()),
            Io::Stop(_) | Io::Fault(_) => ReclaimOut::Fail(NotRunReason::CleanupFailed),
        }
    }

    fn show_reclaim(&mut self, cur: &ReclaimCur) {
        self.reclaim_stage_name = match cur.stage {
            1 => Some("list"),
            2 => Some("owned_status"),
            3 => Some("stop"),
            4 => Some("stopped_status"),
            5 => Some("delete"),
            6 => Some("absent_list"),
            7 => Some("row_done"),
            _ => None,
        };
    }

    pub(super) fn tick_loss(
        &mut self,
        mut cur: LossCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        if !cur.locked {
            return match self.gate_lock(
                cur.caller,
                self.budgets.query,
                self.scope_for_phase(),
                None,
                poll_caller,
                step_end,
            ) {
                Ok(false) => StepOut::Stay(Cursor::Loss(cur)),
                Ok(true) => {
                    cur.locked = true;
                    StepOut::Stay(Cursor::Loss(cur))
                }
                Err(err) => StepOut::Stop(err),
            };
        }
        let op = match self.save_op(cur.caller, self.budgets.query, self.scope_for_phase(), None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        if self.clock.now() >= op {
            self.fail(NotRunReason::Deadline);
            return StepOut::Stop(StepError::Expired);
        }
        self.fail(cur.reason);
        self.clear_slot();
        StepOut::Finish(cur.done)
    }

    pub(super) fn tick_kill(
        &mut self,
        cur: KillCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        let end = self.cancel_end.unwrap_or(cur.caller);
        if self.clock.now() >= end {
            self.mark_unknown("reap");
            self.fail(cur.reason);
            return self.kill_stop(cur.reason);
        }
        let call_end = self.call_end(step_end, poll_caller, end);
        if self.clock.now() >= call_end {
            return StepOut::Stay(Cursor::Kill(cur));
        }
        let Some(handle) = self.active_command else {
            self.mark_unknown("reap");
            self.fail(cur.reason);
            return self.kill_stop(cur.reason);
        };
        if !self.kill_started {
            self.arm_room(call_end);
            let begun = self.runner.begin_kill(&handle);
            self.clear_room();
            self.kill_started = true;
            if begun.is_err() {
                self.mark_unknown("reap");
                self.active_command = None;
                self.fail(cur.reason);
                return self.kill_stop(cur.reason);
            }
            return StepOut::Stay(Cursor::Kill(cur));
        }
        self.arm_room(call_end.min(end));
        let polled = self.runner.poll_reap(&handle);
        self.clear_room();
        if self.clock.now() >= end {
            self.mark_unknown("reap");
            self.active_command = None;
            self.fail(cur.reason);
            return self.kill_stop(cur.reason);
        }
        match polled {
            Ok(ReapPoll::Exited { .. }) => {
                self.active_command = None;
                self.kill_started = false;
                if cur.reason == NotRunReason::Deadline {
                    self.fail(NotRunReason::Deadline);
                    return StepOut::Stop(StepError::Expired);
                }
                self.fail(cur.reason);
                StepOut::Finish(cur.done)
            }
            Ok(ReapPoll::Pending) => StepOut::Stay(Cursor::Kill(cur)),
            _ => {
                self.mark_unknown("reap");
                self.active_command = None;
                self.fail(cur.reason);
                self.kill_stop(cur.reason)
            }
        }
    }

    fn kill_stop(&mut self, reason: NotRunReason) -> StepOut {
        let err = if reason == NotRunReason::Deadline {
            StepError::Expired
        } else {
            StepError::Cleanup(reason)
        };
        StepOut::Stop(err)
    }
}

impl ReclaimCur {
    fn clone_row(&self) -> Self {
        Self {
            stage: self.stage,
            started: self.started,
            run: self.run.clone(),
            clone: self.clone.clone(),
            identity: self.identity.clone(),
            scope: self.scope,
            stop_at: self.stop_at,
            delete_at: self.delete_at,
            entry_query: self.entry_query,
            entry_stop: self.entry_stop,
            entry_delete: self.entry_delete,
            absolute: self.absolute,
        }
    }
}

fn next_open(rows: &[Reservation], index: usize) -> Option<usize> {
    rows.iter()
        .enumerate()
        .skip(index)
        .find(|(_, row)| !row.is_sealed())
        .map(|(index, _)| index)
}
