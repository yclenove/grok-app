//! Forward stages. One ledger, runner, guest, or lock call per step.

use std::time::Duration;

use super::super::super::deadline::{CloneDeadline, HostClock, QueryDeadline};
use super::super::super::guest::GuestChannel;
use super::super::super::ledger::{AttemptLedger, LedgerError, LedgerFact, LedgerPoll};
use super::super::super::lock::CleanupLock;
use super::super::super::output::CallError;
use super::super::super::owner::OwnerSource;
use super::super::super::parse;
use super::super::super::receipt::{NotRunReason, Phase};
use super::super::super::runner::{CommandKind, TartRunner};
use super::super::{Attempt, CleanupDeadlines, CompletedStep, Scope, StepError};
use super::{
    share_long, share_short, CloneCur, Cursor, DriveKind, Io, ReserveCur, StartCur, StepOut,
};

impl<'a, R, G, L, K, O, C> Attempt<'a, R, G, L, K, O, C>
where
    R: TartRunner,
    G: GuestChannel,
    L: AttemptLedger,
    K: CleanupLock,
    O: OwnerSource,
    C: HostClock,
{
    pub(super) fn precheck(
        &mut self,
        kind: &DriveKind,
        caller: super::super::super::deadline::MonoInstant,
    ) -> Result<(), StepError> {
        match kind {
            DriveKind::Reserve(_) => self.pre_open(&[Phase::Idle], caller, self.budgets.query),
            DriveKind::Clone(_) => {
                self.pre_open(&[Phase::Reserved], caller, self.budgets.clone_limit)
            }
            DriveKind::Start(_) => self.pre_open(&[Phase::Cloned], caller, self.budgets.start),
            DriveKind::Isolate => {
                self.pre_open(&[Phase::Booting], caller, self.budgets.probe_group)
            }
            DriveKind::Bootstrap => self.pre_open(&[Phase::Isolated], caller, self.budgets.stage),
            DriveKind::Login => self.pre_open(&[Phase::Bootstrapped], caller, self.budgets.login),
            DriveKind::Ready => {
                self.pre_open(&[Phase::CredentialReady], caller, self.budgets.query)
            }
            DriveKind::Run => {
                self.pre_open(&[Phase::Ready], caller, self.budgets.run)?;
                if self.client_starts > 0 {
                    self.fail(NotRunReason::Unexpected);
                    return Err(StepError::Unexpected);
                }
                Ok(())
            }
            DriveKind::Lease => self.pre_lease(caller),
            DriveKind::Revoke => self.pre_revoke(caller),
            DriveKind::Destroy(deadlines) => self.pre_destroy(deadlines),
            DriveKind::Seal(deadline) => self.pre_seal(deadline.at()),
            DriveKind::Recover(deadlines) => self.pre_recover(deadlines),
            DriveKind::Cancel | DriveKind::Provider | DriveKind::Runtime(_) => {
                self.pre_loss(caller)
            }
        }
    }

    fn pre_open(
        &mut self,
        expected: &[Phase],
        caller: super::super::super::deadline::MonoInstant,
        budget: Duration,
    ) -> Result<(), StepError> {
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        if !expected.contains(&self.phase) {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        }
        self.above_budget(caller, budget)
    }

    fn pre_lease(
        &mut self,
        caller: super::super::super::deadline::MonoInstant,
    ) -> Result<(), StepError> {
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        if self.lease_expired() {
            let _ = self.suspend_owner(caller);
            return Err(StepError::LeaseExpired);
        }
        self.pre_open(&[Phase::Running], caller, self.budgets.lease)
    }

    fn pre_revoke(
        &mut self,
        caller: super::super::super::deadline::MonoInstant,
    ) -> Result<(), StepError> {
        self.pre_open(
            &[Phase::Running, Phase::Revoking],
            caller,
            self.budgets.revoke,
        )?;
        if self.cleanup_origin.is_none() {
            self.cleanup_origin = Some(self.clock.now());
        }
        Ok(())
    }

    fn pre_destroy(&mut self, deadlines: &CleanupDeadlines) -> Result<(), StepError> {
        if self.delete_forbidden {
            return Err(StepError::DeleteForbidden);
        }
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        if self.phase != Phase::Revoking {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        }
        self.fit(deadlines)?;
        if self.clock.now() >= deadlines.query.at() {
            self.fail(NotRunReason::Deadline);
            self.vm_cleanup_ok = false;
            self.set_open(Phase::Destroying);
            return Err(StepError::Expired);
        }
        Ok(())
    }

    fn pre_seal(
        &mut self,
        caller: super::super::super::deadline::MonoInstant,
    ) -> Result<(), StepError> {
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        if self.phase != Phase::Destroying {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        }
        self.above_budget(caller, self.budgets.seal)
    }

    fn pre_recover(&mut self, deadlines: &CleanupDeadlines) -> Result<(), StepError> {
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
        Ok(())
    }

    fn pre_loss(
        &mut self,
        caller: super::super::super::deadline::MonoInstant,
    ) -> Result<(), StepError> {
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        self.above_budget(caller, self.budgets.query)
    }

    pub(super) fn destroy_cursor(
        &mut self,
        deadlines: CleanupDeadlines,
        _caller: super::super::super::deadline::MonoInstant,
    ) -> Result<Cursor, StepError> {
        let Some(run) = self.run.clone() else {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        };
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        };
        if self.cleanup_origin.is_none() {
            self.cleanup_origin = Some(self.clock.now());
        }
        let mut reclaim = super::empty_reclaim(deadlines, Scope::Cleanup);
        reclaim.run = Some(run);
        reclaim.clone = Some(clone);
        reclaim.started = self.vm_started;
        reclaim.identity = self.identity.clone();
        Ok(Cursor::Destroy(super::DestroyCur { deadlines, reclaim }))
    }

    pub(super) fn append_unknown(
        &mut self,
        clone: &super::super::super::identity::CloneId,
        caller: super::super::super::deadline::MonoInstant,
    ) -> Result<(), LedgerError> {
        let end = match self.bound(caller, self.budgets.query, self.scope_for_phase(), None) {
            Ok(end) => end,
            Err(_) => return Err(LedgerError::Expired),
        };
        if self.clock.now() >= end {
            return Err(LedgerError::Expired);
        }
        let before = self.clock.now();
        self.arm_room(end);
        let begun = self.ledger.begin_append(
            clone,
            LedgerFact::CleanupUnknown,
            QueryDeadline::from_instant(end),
        );
        self.clear_room();
        let handle = begun?;
        if self.clock.now() != before || self.clock.now() >= end {
            return Ok(());
        }
        self.arm_room(end);
        let polled = self.ledger.poll_ledger(&handle);
        self.clear_room();
        match polled? {
            LedgerPoll::Wrote | LedgerPoll::Pending => Ok(()),
            _ => Err(LedgerError::Failed),
        }
    }

    pub(super) fn tick_reserve(
        &mut self,
        cur: ReserveCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let absolute = Some(cur.input.absolute.at());
        let op = match self.save_op(cur.caller, self.budgets.query, Scope::Prep, absolute) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        match cur.stage {
            0 => self.reserve_lock(cur, call_end, op),
            1 => self.reserve_rows(cur, call_end, op),
            2 => self.reserve_list(cur, poll_caller, step_end),
            3 => self.reserve_write(cur, call_end, op),
            _ => self.reserve_readback(cur, call_end, op),
        }
    }

    fn reserve_lock(
        &mut self,
        mut cur: ReserveCur,
        call_end: super::super::super::deadline::MonoInstant,
        op: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        match self.lock_slice(call_end, op) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Reserve(cur))
            }
            Io::Stop(err) => StepOut::Stop(err),
            Io::Ready(()) => match self.reserve_policy(&cur) {
                Ok(()) => {
                    self.clear_slot();
                    cur.stage = 1;
                    StepOut::Stay(Cursor::Reserve(cur))
                }
                Err(err) => StepOut::Stop(err),
            },
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
            Io::Discard => StepOut::Stay(Cursor::Reserve(cur)),
        }
    }

    fn reserve_policy(&mut self, cur: &ReserveCur) -> Result<(), StepError> {
        let attempt_end = self
            .clock
            .now()
            .checked_add(self.budgets.attempt)
            .map_err(|_| StepError::Overflow)?;
        if cur.input.absolute.at() > attempt_end {
            return Err(StepError::AboveBudget);
        }
        if self.stop_latched_flag {
            self.halted = true;
            self.fail(NotRunReason::Refused);
            return Err(StepError::StopLatched);
        }
        if cur.input.manifest != cur.input.approved {
            self.fail(NotRunReason::Refused);
            return Err(StepError::ManifestMismatch);
        }
        if !self.proof_matches(cur.input.run.as_str()) {
            self.fail(NotRunReason::Refused);
            return Err(StepError::Refused);
        }
        Ok(())
    }

    fn proof_matches(&self, run: &str) -> bool {
        self.owner_proof
            .as_ref()
            .is_some_and(|proof| proof.binding.run_id.as_str() == run)
    }

    fn reserve_rows(
        &mut self,
        mut cur: ReserveCur,
        call_end: super::super::super::deadline::MonoInstant,
        op: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let input_clone = cur.input.clone_id.clone();
        match self.ledger_rows(call_end, op) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Reserve(cur))
            }
            Io::Ready(LedgerPoll::Rows(rows)) => {
                self.clear_slot();
                if rows.iter().any(|row| !row.is_sealed()) {
                    return StepOut::Stop(StepError::MustRecover);
                }
                if rows
                    .iter()
                    .any(|row| row.facts().contains(&LedgerFact::Quarantined))
                {
                    self.halted = true;
                    return StepOut::Stop(StepError::Halted);
                }
                let _ = input_clone;
                cur.stage = 2;
                StepOut::Stay(Cursor::Reserve(cur))
            }
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Ready(_) | Io::Discard => StepOut::Stop(StepError::Unexpected),
        }
    }

    fn reserve_list(
        &mut self,
        mut cur: ReserveCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        match self.list_names(
            cur.caller,
            Scope::Prep,
            Some(cur.input.absolute.at()),
            poll_caller,
            step_end,
        ) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Reserve(cur))
            }
            Io::Ready(names) => {
                self.clear_slot();
                if names.iter().any(|name| name == cur.input.clone_id.as_str()) {
                    return StepOut::Stop(StepError::DestinationExists);
                }
                cur.stage = 3;
                StepOut::Stay(Cursor::Reserve(cur))
            }
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Reserve(cur)),
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
        }
    }

    fn reserve_write(
        &mut self,
        mut cur: ReserveCur,
        call_end: super::super::super::deadline::MonoInstant,
        op: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let run = cur.input.run.clone();
        let clone = cur.input.clone_id.clone();
        let absolute = cur.input.absolute;
        match self.ledger_io(call_end, op, |machine, end| {
            machine.ledger.begin_reserve(
                run.clone(),
                clone.clone(),
                absolute,
                QueryDeadline::from_instant(end),
            )
        }) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Reserve(cur))
            }
            Io::Ready(LedgerPoll::Permit(permit)) => {
                self.absolute = Some(permit.absolute_deadline());
                self.run = Some(cur.input.run.clone());
                self.clone_id = Some(cur.input.clone_id.clone());
                self.permit = Some(permit);
                self.phase = Phase::Reserved;
                self.clear_slot();
                self.note_resolved();
                StepOut::Finish(CompletedStep::Reserved)
            }
            Io::Fault(LedgerError::ReplyLost) => {
                self.clear_slot();
                cur.stage = 4;
                StepOut::Stay(Cursor::Reserve(cur))
            }
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Ready(_) | Io::Discard => StepOut::Stop(StepError::Unexpected),
        }
    }

    fn reserve_readback(
        &mut self,
        cur: ReserveCur,
        call_end: super::super::super::deadline::MonoInstant,
        op: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let clone = cur.input.clone_id.clone();
        match self.ledger_io(call_end, op, |machine, end| {
            machine
                .ledger
                .begin_query(&clone, QueryDeadline::from_instant(end))
        }) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                StepOut::Stay(Cursor::Reserve(cur))
            }
            Io::Fault(LedgerError::NotFound) => {
                StepOut::Stop(self.ledger_err(LedgerError::ReplyLost))
            }
            Io::Ready(_) | Io::Fault(_) => StepOut::Stop(self.hold_unacked(cur)),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Reserve(cur)),
        }
    }

    fn hold_unacked(&mut self, cur: ReserveCur) -> StepError {
        self.run = Some(cur.input.run);
        self.clone_id = Some(cur.input.clone_id);
        self.absolute = Some(cur.input.absolute.at());
        self.trigger = Some(NotRunReason::ReplyLost);
        self.phase = Phase::Revoking;
        if self.cleanup_origin.is_none() {
            self.cleanup_origin = Some(self.clock.now());
        }
        self.receipt = Some(super::super::super::receipt::AttemptReceipt::new(
            self.run.clone(),
            self.clone_id.clone(),
            super::super::super::receipt::AttemptOutcome::NotRun {
                reason: NotRunReason::ReplyLost,
                client_started: false,
            },
            false,
            crate::disposable::CleanupDisposition::Allocated,
            false,
            true,
        ));
        self.clear_slot();
        StepError::Ledger(LedgerError::ReplyLost)
    }

    pub(super) fn gate_lock(
        &mut self,
        saved: super::super::super::deadline::MonoInstant,
        budget: Duration,
        scope: Scope,
        absolute: Option<super::super::super::deadline::MonoInstant>,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> Result<bool, StepError> {
        let op = self.save_op(saved, budget, scope, absolute)?;
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.lock_slice(call_end, op) {
            Io::Pending { issued } => {
                self.stay_pending(issued);
                Ok(false)
            }
            Io::Ready(()) => {
                self.clear_slot();
                Ok(true)
            }
            Io::Stop(err) => Err(err),
            Io::Discard => Ok(false),
            Io::Fault(err) => Err(self.ledger_err(err)),
        }
    }

    pub(super) fn stay_cur(&mut self, issued: bool, cursor: Cursor) -> StepOut {
        self.stay_pending(issued);
        StepOut::Stay(cursor)
    }

    pub(super) fn tick_clone(
        &mut self,
        mut cur: CloneCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        if cur.stage == 0 {
            return match self.gate_lock(
                cur.caller,
                self.budgets.clone_limit,
                Scope::Prep,
                None,
                poll_caller,
                step_end,
            ) {
                Ok(false) => StepOut::Stay(Cursor::Clone(cur)),
                Ok(true) => {
                    cur.stage = 1;
                    StepOut::Stay(Cursor::Clone(cur))
                }
                Err(err) => StepOut::Stop(err),
            };
        }
        match cur.stage {
            1 => self.clone_list(cur, poll_caller, step_end),
            2 => self.clone_note(cur, poll_caller, step_end),
            3 => self.clone_note_read(cur, poll_caller, step_end),
            4 => self.clone_run(cur, poll_caller, step_end),
            5 => self.clone_fact(cur, poll_caller, step_end),
            _ => self.clone_fact_read(cur, poll_caller, step_end),
        }
    }

    fn clone_list(
        &mut self,
        mut cur: CloneCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return StepOut::Stop(StepError::Unexpected);
        };
        match self.list_names(cur.caller, Scope::Prep, None, poll_caller, step_end) {
            Io::Pending { issued } => self.stay_cur(issued, Cursor::Clone(cur)),
            Io::Ready(names) => {
                self.clear_slot();
                if names.iter().any(|name| name == clone.as_str()) {
                    return self.stop_reason(NotRunReason::IdentityConflict);
                }
                cur.stage = 2;
                StepOut::Stay(Cursor::Clone(cur))
            }
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Clone(cur)),
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
        }
    }

    fn clone_note(
        &mut self,
        mut cur: CloneCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return StepOut::Stop(StepError::Unexpected);
        };
        let expected = Self::fake_allocation();
        let op = match self.save_op(cur.caller, self.budgets.query, Scope::Prep, None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.ledger_io(call_end, op, |machine, end| {
            machine
                .ledger
                .begin_note_identity(&clone, &expected, QueryDeadline::from_instant(end))
        }) {
            Io::Pending { issued } => self.stay_cur(issued, Cursor::Clone(cur)),
            Io::Ready(LedgerPoll::Wrote) => {
                self.identity = Some(expected);
                self.clear_slot();
                cur.stage = 4;
                StepOut::Stay(Cursor::Clone(cur))
            }
            Io::Fault(LedgerError::ReplyLost) => {
                self.clear_slot();
                cur.stage = 3;
                StepOut::Stay(Cursor::Clone(cur))
            }
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Ready(_) | Io::Discard => StepOut::Stop(StepError::Unexpected),
        }
    }

    fn clone_note_read(
        &mut self,
        mut cur: CloneCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return StepOut::Stop(StepError::Unexpected);
        };
        let expected = Self::fake_allocation();
        let op = match self.save_op(cur.caller, self.budgets.query, Scope::Prep, None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.ledger_io(call_end, op, |machine, end| {
            machine
                .ledger
                .begin_query(&clone, QueryDeadline::from_instant(end))
        }) {
            Io::Pending { issued } => self.stay_cur(issued, Cursor::Clone(cur)),
            Io::Ready(LedgerPoll::Read(row)) => {
                self.clear_slot();
                match row.expected() {
                    Some(got) if got == &expected => {
                        self.identity = Some(expected);
                        cur.stage = 4;
                        StepOut::Stay(Cursor::Clone(cur))
                    }
                    Some(_) => self.stop_reason(NotRunReason::IdentityConflict),
                    None => StepOut::Stop(self.ledger_err(LedgerError::ReplyLost)),
                }
            }
            Io::Fault(LedgerError::NotFound) => {
                StepOut::Stop(self.ledger_err(LedgerError::ReplyLost))
            }
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Ready(_) | Io::Discard => StepOut::Stop(StepError::Unexpected),
        }
    }

    fn clone_run(
        &mut self,
        mut cur: CloneCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let golden = cur.golden;
        let expected = self.identity.clone().unwrap_or_else(Self::fake_allocation);
        let op = match self.save_op(cur.caller, self.budgets.clone_limit, Scope::Prep, None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        if let Err(err) = self.arm_share(op, share_long()) {
            return StepOut::Stop(err);
        }
        let call_end = self.call_end(step_end, poll_caller, op);
        if self.active_command.is_none() && self.clock.now() >= call_end {
            return StepOut::Stay(Cursor::Clone(cur));
        }
        let permit = if self.active_command.is_none() {
            let Some(permit) = self.permit.take() else {
                self.fail(NotRunReason::Unexpected);
                return StepOut::Stop(StepError::Unexpected);
            };
            Some(permit)
        } else {
            None
        };
        match self.runner_io(call_end, op, "clone", CommandKind::Clone, |machine, end| {
            let Some(permit) = permit else {
                return Err(CallError::ScriptExhausted);
            };
            machine
                .runner
                .begin_clone(permit, golden, CloneDeadline::from_instant(end))
        }) {
            Io::Pending { issued } => self.stay_cur(issued, Cursor::Clone(cur)),
            Io::Ready(body) => match parse::parse_clone(body.as_bytes()) {
                Ok(mark) => {
                    let got = super::super::super::ledger::VmIdentity::from_parts(
                        mark.incarnation,
                        mark.config,
                    );
                    if got != expected {
                        self.stop_reason(NotRunReason::IdentityConflict)
                    } else {
                        self.clear_slot();
                        cur.stage = 5;
                        StepOut::Stay(Cursor::Clone(cur))
                    }
                }
                Err(_) => self.stop_reason(NotRunReason::Malformed),
            },
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Clone(cur)),
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
        }
    }

    fn clone_fact(
        &mut self,
        mut cur: CloneCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return StepOut::Stop(StepError::Unexpected);
        };
        match self.append_fact(
            &clone,
            LedgerFact::CloneIssued,
            cur.caller,
            Scope::Prep,
            poll_caller,
            step_end,
        ) {
            Io::Pending { issued } => self.stay_cur(issued, Cursor::Clone(cur)),
            Io::Ready(()) => {
                self.phase = Phase::Cloned;
                self.note_resolved();
                self.clear_slot();
                StepOut::Finish(CompletedStep::Cloned)
            }
            Io::Fault(LedgerError::ReplyLost) => {
                self.clear_slot();
                cur.stage = 6;
                StepOut::Stay(Cursor::Clone(cur))
            }
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Clone(cur)),
        }
    }

    fn clone_fact_read(
        &mut self,
        cur: CloneCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return StepOut::Stop(StepError::Unexpected);
        };
        match self.read_fact(
            &clone,
            LedgerFact::CloneIssued,
            cur.caller,
            Scope::Prep,
            poll_caller,
            step_end,
        ) {
            Io::Pending { issued } => self.stay_cur(issued, Cursor::Clone(cur)),
            Io::Ready(true) => {
                self.phase = Phase::Cloned;
                self.clear_slot();
                StepOut::Finish(CompletedStep::Cloned)
            }
            Io::Ready(false) => StepOut::Stop(self.ledger_err(LedgerError::ReplyLost)),
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Clone(cur)),
        }
    }

    pub(super) fn tick_start(
        &mut self,
        mut cur: StartCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        if cur.stage == 0 {
            return match self.gate_lock(
                cur.caller,
                self.budgets.start,
                Scope::Prep,
                None,
                poll_caller,
                step_end,
            ) {
                Ok(false) => StepOut::Stay(Cursor::Start(cur)),
                Ok(true) => {
                    cur.stage = 1;
                    StepOut::Stay(Cursor::Start(cur))
                }
                Err(err) => StepOut::Stop(err),
            };
        }
        match cur.stage {
            1 => self.start_list(cur, poll_caller, step_end),
            2 => self.start_status(cur, poll_caller, step_end),
            3 => self.start_fact(cur, poll_caller, step_end),
            4 => self.start_fact_read(cur, poll_caller, step_end),
            _ => self.start_run(cur, poll_caller, step_end),
        }
    }

    fn start_list(
        &mut self,
        mut cur: StartCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return StepOut::Stop(StepError::Unexpected);
        };
        match self.list_names(cur.caller, Scope::Prep, None, poll_caller, step_end) {
            Io::Pending { issued } => self.stay_cur(issued, Cursor::Start(cur)),
            Io::Ready(names) => {
                self.clear_slot();
                if names.iter().any(|name| name == clone.as_str()) {
                    cur.stage = 2;
                    StepOut::Stay(Cursor::Start(cur))
                } else {
                    self.stop_reason(NotRunReason::IdentityConflict)
                }
            }
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Start(cur)),
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
        }
    }

    fn start_status(
        &mut self,
        mut cur: StartCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return StepOut::Stop(StepError::Unexpected);
        };
        let Some(run) = self.run.clone() else {
            self.fail(NotRunReason::Unexpected);
            return StepOut::Stop(StepError::Unexpected);
        };
        let Some(identity) = self.identity.clone() else {
            return self.stop_reason(NotRunReason::IdentityConflict);
        };
        match self.vm_status(&clone, cur.caller, Scope::Prep, None, poll_caller, step_end) {
            Io::Pending { issued } => self.stay_cur(issued, Cursor::Start(cur)),
            Io::Ready(status) => {
                self.clear_slot();
                if super::super::cleanup::owns(&status, &identity, &run) {
                    cur.stage = 3;
                    StepOut::Stay(Cursor::Start(cur))
                } else {
                    self.stop_reason(NotRunReason::IdentityConflict)
                }
            }
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Start(cur)),
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
        }
    }

    fn start_fact(
        &mut self,
        mut cur: StartCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return StepOut::Stop(StepError::Unexpected);
        };
        match self.append_fact(
            &clone,
            LedgerFact::StartIssued,
            cur.caller,
            Scope::Prep,
            poll_caller,
            step_end,
        ) {
            Io::Pending { issued } => self.stay_cur(issued, Cursor::Start(cur)),
            Io::Ready(()) => {
                self.vm_started = true;
                self.clear_slot();
                cur.stage = 5;
                StepOut::Stay(Cursor::Start(cur))
            }
            Io::Fault(LedgerError::ReplyLost) => {
                self.clear_slot();
                cur.stage = 4;
                StepOut::Stay(Cursor::Start(cur))
            }
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Start(cur)),
        }
    }

    fn start_fact_read(
        &mut self,
        mut cur: StartCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return StepOut::Stop(StepError::Unexpected);
        };
        match self.read_fact(
            &clone,
            LedgerFact::StartIssued,
            cur.caller,
            Scope::Prep,
            poll_caller,
            step_end,
        ) {
            Io::Pending { issued } => self.stay_cur(issued, Cursor::Start(cur)),
            Io::Ready(true) => {
                self.vm_started = true;
                self.clear_slot();
                cur.stage = 5;
                StepOut::Stay(Cursor::Start(cur))
            }
            Io::Ready(false) => StepOut::Stop(self.ledger_err(LedgerError::ReplyLost)),
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Start(cur)),
        }
    }

    fn start_run(
        &mut self,
        cur: StartCur,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> StepOut {
        let Some(clone) = self.clone_id.clone() else {
            self.fail(NotRunReason::Unexpected);
            return StepOut::Stop(StepError::Unexpected);
        };
        let flags = cur.flags;
        let op = match self.save_op(cur.caller, self.budgets.start, Scope::Prep, None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        if let Err(err) = self.arm_share(op, share_long()) {
            return StepOut::Stop(err);
        }
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.runner_io(call_end, op, "start", CommandKind::Start, |machine, end| {
            machine.runner.begin_start(
                &clone,
                &flags,
                super::super::super::deadline::StartDeadline::from_instant(end),
            )
        }) {
            Io::Pending { issued } => self.stay_cur(issued, Cursor::Start(cur)),
            Io::Ready(_) => {
                self.phase = Phase::Booting;
                self.note_resolved();
                self.clear_slot();
                StepOut::Finish(CompletedStep::Started)
            }
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard => StepOut::Stay(Cursor::Start(cur)),
            Io::Fault(err) => StepOut::Stop(self.ledger_err(err)),
        }
    }

    pub(super) fn stop_reason(&mut self, reason: NotRunReason) -> StepOut {
        self.fail(reason);
        StepOut::Stop(reason_error(reason))
    }

    pub(super) fn list_names(
        &mut self,
        saved: super::super::super::deadline::MonoInstant,
        scope: Scope,
        absolute: Option<super::super::super::deadline::MonoInstant>,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> Io<Vec<String>> {
        let op = match self.save_op(saved, self.budgets.query, scope, absolute) {
            Ok(end) => end,
            Err(err) => return Io::Stop(err),
        };
        if let Err(err) = self.arm_share(op, share_short()) {
            return Io::Stop(err);
        }
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.runner_io(call_end, op, "list", CommandKind::List, |machine, end| {
            machine.runner.begin_list(QueryDeadline::from_instant(end))
        }) {
            Io::Ready(body) => match parse::parse_list(body.as_bytes()) {
                Ok(names) => Io::Ready(names),
                Err(_) => {
                    self.fail(NotRunReason::Malformed);
                    Io::Stop(StepError::MalformedBody)
                }
            },
            other => other.map_fault(),
        }
    }

    pub(super) fn vm_status(
        &mut self,
        clone: &super::super::super::identity::CloneId,
        saved: super::super::super::deadline::MonoInstant,
        scope: Scope,
        absolute: Option<super::super::super::deadline::MonoInstant>,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> Io<parse::VmStatus> {
        let op = match self.save_op(saved, self.budgets.query, scope, absolute) {
            Ok(end) => end,
            Err(err) => return Io::Stop(err),
        };
        if let Err(err) = self.arm_share(op, share_short()) {
            return Io::Stop(err);
        }
        let call_end = self.call_end(step_end, poll_caller, op);
        let clone = clone.clone();
        match self.runner_io(
            call_end,
            op,
            "status",
            CommandKind::Status,
            |machine, end| {
                machine
                    .runner
                    .begin_status(&clone, QueryDeadline::from_instant(end))
            },
        ) {
            Io::Ready(body) => match parse::parse_status(body.as_bytes()) {
                Ok(status) => Io::Ready(status),
                Err(_) => {
                    self.fail(NotRunReason::Malformed);
                    Io::Stop(StepError::MalformedBody)
                }
            },
            other => other.map_fault(),
        }
    }

    pub(super) fn append_fact(
        &mut self,
        clone: &super::super::super::identity::CloneId,
        fact: LedgerFact,
        saved: super::super::super::deadline::MonoInstant,
        scope: Scope,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> Io<()> {
        let op = match self.save_op(saved, self.budgets.query, scope, None) {
            Ok(end) => end,
            Err(err) => return Io::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        let clone = clone.clone();
        match self.ledger_io(call_end, op, |machine, end| {
            machine
                .ledger
                .begin_append(&clone, fact, QueryDeadline::from_instant(end))
        }) {
            Io::Ready(LedgerPoll::Wrote) => Io::Ready(()),
            Io::Ready(_) => Io::Stop(StepError::Unexpected),
            Io::Pending { issued } => Io::Pending { issued },
            Io::Fault(err) => Io::Fault(err),
            Io::Stop(err) => Io::Stop(err),
            Io::Discard => Io::Discard,
        }
    }

    pub(super) fn read_fact(
        &mut self,
        clone: &super::super::super::identity::CloneId,
        fact: LedgerFact,
        saved: super::super::super::deadline::MonoInstant,
        scope: Scope,
        poll_caller: super::super::super::deadline::MonoInstant,
        step_end: super::super::super::deadline::MonoInstant,
    ) -> Io<bool> {
        let op = match self.save_op(saved, self.budgets.query, scope, None) {
            Ok(end) => end,
            Err(err) => return Io::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        let clone = clone.clone();
        match self.ledger_io(call_end, op, |machine, end| {
            machine
                .ledger
                .begin_query(&clone, QueryDeadline::from_instant(end))
        }) {
            Io::Ready(LedgerPoll::Read(row)) => Io::Ready(row.facts().contains(&fact)),
            Io::Ready(_) => Io::Stop(StepError::Unexpected),
            Io::Pending { issued } => Io::Pending { issued },
            Io::Fault(err) => Io::Fault(err),
            Io::Stop(err) => Io::Stop(err),
            Io::Discard => Io::Discard,
        }
    }

    pub(super) fn ledger_rows(
        &mut self,
        call_end: super::super::super::deadline::MonoInstant,
        op: super::super::super::deadline::MonoInstant,
    ) -> Io<LedgerPoll> {
        self.ledger_io(call_end, op, |machine, end| {
            machine.ledger.begin_rows(QueryDeadline::from_instant(end))
        })
    }
}

fn reason_error(reason: NotRunReason) -> StepError {
    match reason {
        NotRunReason::Deadline => StepError::Expired,
        NotRunReason::Malformed => StepError::MalformedBody,
        other => StepError::Cleanup(other),
    }
}

trait MapFault<T> {
    fn map_fault(self) -> Io<T>;
}

impl<T> MapFault<T> for Io<super::super::super::output::BoundedOutput> {
    fn map_fault(self) -> Io<T> {
        match self {
            Io::Pending { issued } => Io::Pending { issued },
            Io::Stop(err) => Io::Stop(err),
            Io::Fault(err) => Io::Fault(err),
            Io::Discard => Io::Discard,
            Io::Ready(_) => Io::Stop(StepError::Unexpected),
        }
    }
}
