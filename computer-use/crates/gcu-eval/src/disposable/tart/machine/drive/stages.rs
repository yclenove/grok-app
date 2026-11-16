//! Isolate, guest, ready, and lease stages. One guest call per step.

use std::num::NonZeroU64;

use super::super::super::deadline::{
    HostClock, LeaseDeadline, LoginDeadline, MonoInstant, ProbeDeadline, QueryDeadline,
    RevokeDeadline, RunDeadline, StageDeadline,
};
use super::super::super::guest::{GuestChannel, GUEST_CHANNEL_VERSION};
use super::super::super::ledger::AttemptLedger;
use super::super::super::lock::CleanupLock;
use super::super::super::mailbox::{decode_reply, ReplyState};
use super::super::super::output::{BoundedOutput, CallError};
use super::super::super::owner::OwnerSource;
use super::super::super::parse::{self, LoginMark};
use super::super::super::receipt::{NotRunReason, Phase};
use super::super::super::runner::TartRunner;
use super::super::{score_of, Attempt, CompletedStep, Scope, StepError};
use super::{Cursor, GuestCur, Io, IsoCur, LeaseCur, ReadyCur, StepOut};
use crate::disposable::{Check, Probe};

#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyClass {
    Pending,
    Accepted,
    Terminal,
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
    pub(super) fn tick_isolate(
        &mut self,
        cur: IsoCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        if cur.stage == 0 {
            return match self.gate_lock(
                cur.caller,
                self.budgets.probe_group,
                Scope::Prep,
                None,
                poll_caller,
                step_end,
            ) {
                Ok(false) => StepOut::Stay(Cursor::Isolate(cur)),
                Ok(true) => self.isolate_version(cur),
                Err(err) => StepOut::Stop(err),
            };
        }
        if cur.stage == 1 {
            return self.isolate_inventory(cur, poll_caller, step_end);
        }
        self.isolate_probe(cur, poll_caller, step_end)
    }

    fn isolate_version(&mut self, mut cur: IsoCur) -> StepOut {
        if self.guest.protocol_version() != GUEST_CHANNEL_VERSION {
            self.fail(NotRunReason::MissingProof);
            return StepOut::Stop(StepError::Protocol);
        }
        cur.stage = 1;
        StepOut::Stay(Cursor::Isolate(cur))
    }

    fn isolate_inventory(
        &mut self,
        mut cur: IsoCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        let op = match self.save_op(cur.caller, self.budgets.query, Scope::Prep, None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.guest_exchange(call_end, op, false, |machine, end| {
            machine.guest.inventory(QueryDeadline::from_instant(end))
        }) {
            Io::Pending { issued } => {
                cur.seen = true;
                self.stay_cur(issued, Cursor::Isolate(cur))
            }
            Io::Ready(body) => match parse::parse_guest(body.as_bytes()) {
                Ok(view) => {
                    self.evidence.guest = Some(view);
                    self.guest_open = false;
                    self.clear_slot();
                    cur.stage = 2;
                    cur.probe_at = 0;
                    StepOut::Stay(Cursor::Isolate(cur))
                }
                Err(_) => self.guest_parse(cur.seen, NotRunReason::Malformed),
            },
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard | Io::Fault(_) => StepOut::Stay(Cursor::Isolate(cur)),
        }
    }

    fn isolate_probe(
        &mut self,
        mut cur: IsoCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        let Some(probe) = Probe::ALL.get(cur.probe_at).copied() else {
            self.phase = Phase::Isolated;
            self.clear_slot();
            return StepOut::Finish(CompletedStep::Isolated);
        };
        let op = match self.save_op(cur.caller, self.budgets.probe, Scope::Prep, None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.guest_exchange(call_end, op, false, |machine, end| {
            machine.guest.probe(probe, ProbeDeadline::from_instant(end))
        }) {
            Io::Pending { issued } => {
                cur.seen = true;
                self.stay_cur(issued, Cursor::Isolate(cur))
            }
            Io::Ready(body) => self.accept_probe(cur, body),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard | Io::Fault(_) => StepOut::Stay(Cursor::Isolate(cur)),
        }
    }

    fn accept_probe(&mut self, mut cur: IsoCur, body: BoundedOutput) -> StepOut {
        let Some(probe) = Probe::ALL.get(cur.probe_at).copied() else {
            return StepOut::Stop(StepError::Unexpected);
        };
        let check = match parse::parse_probe(body.as_bytes()) {
            Ok(check) => check,
            Err(_) => return self.guest_parse(cur.seen, NotRunReason::Malformed),
        };
        self.evidence.probes.push((probe, check));
        self.guest_open = false;
        self.clear_slot();
        if check == Check::Failed {
            self.fail(NotRunReason::EnvironmentBreach);
            return StepOut::Stop(StepError::Breach);
        }
        cur.probe_at = cur.probe_at.saturating_add(1);
        cur.seen = false;
        if cur.probe_at >= Probe::ALL.len() {
            self.phase = Phase::Isolated;
            StepOut::Finish(CompletedStep::Isolated)
        } else {
            StepOut::Stay(Cursor::Isolate(cur))
        }
    }

    pub(super) fn tick_guest(
        &mut self,
        cur: GuestCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        if cur.stage == 0 {
            let budget = self.guest_budget(cur.which);
            let scope = if cur.which == 4 {
                Scope::Cleanup
            } else if cur.which == 3 {
                Scope::Task
            } else {
                Scope::Prep
            };
            return match self.gate_lock(cur.caller, budget, scope, None, poll_caller, step_end) {
                Ok(false) => StepOut::Stay(Cursor::Guest(cur)),
                Ok(true) => self.guest_after_lock(cur),
                Err(err) => self.guest_lock_err(cur.which, err),
            };
        }
        self.guest_body(cur, poll_caller, step_end)
    }

    fn guest_after_lock(&mut self, mut cur: GuestCur) -> StepOut {
        if cur.which == 4 {
            self.revoke_attempted = true;
        }
        cur.stage = 1;
        StepOut::Stay(Cursor::Guest(cur))
    }

    fn guest_lock_err(&mut self, which: u8, err: StepError) -> StepOut {
        if which == 4 && !matches!(err, StepError::AboveBudget) {
            self.revoke_attempted = true;
            self.set_open(Phase::Revoking);
        }
        StepOut::Stop(err)
    }

    fn guest_body(
        &mut self,
        cur: GuestCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        if cur.which == 1 {
            let Some(run) = self.run.clone() else {
                self.fail(NotRunReason::Unexpected);
                return StepOut::Stop(StepError::Unexpected);
            };
            return self.guest_op(cur, poll_caller, step_end, true, move |machine, end| {
                let _ = &run;
                machine.guest.bootstrap(StageDeadline::from_instant(end))
            });
        }
        if cur.which == 2 {
            return self.guest_op(cur, poll_caller, step_end, true, |machine, end| {
                machine.guest.login(LoginDeadline::from_instant(end))
            });
        }
        if cur.which == 3 {
            return self.guest_op(cur, poll_caller, step_end, true, |machine, end| {
                machine.guest.run(RunDeadline::from_instant(end))
            });
        }
        self.guest_op(cur, poll_caller, step_end, true, |machine, end| {
            machine.guest.revoke(RevokeDeadline::from_instant(end))
        })
    }

    fn guest_op<F>(
        &mut self,
        cur: GuestCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
        effect: bool,
        call: F,
    ) -> StepOut
    where
        F: FnOnce(&mut Self, MonoInstant) -> Result<BoundedOutput, CallError>,
    {
        let budget = self.guest_budget(cur.which);
        let scope = self.guest_scope(cur.which);
        let op = match self.save_op(cur.caller, budget, scope, None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        if cur.which == 3 && self.active_command.is_none() {
            // Run dispatch is counted from the guest result below.
        }
        let counted = cur.which == 3;
        match self.guest_exchange(call_end, op, counted, call) {
            Io::Pending { issued } => {
                let mut cur = cur;
                cur.seen = true;
                self.stay_cur(issued, Cursor::Guest(cur))
            }
            Io::Ready(body) => self.accept_guest(cur, body, effect),
            Io::Stop(err) => {
                if cur.which == 4 {
                    self.revoke_attempted = true;
                    self.set_open(Phase::Revoking);
                }
                StepOut::Stop(err)
            }
            Io::Discard | Io::Fault(_) => StepOut::Stay(Cursor::Guest(cur)),
        }
    }

    fn accept_guest(&mut self, cur: GuestCur, body: BoundedOutput, _effect: bool) -> StepOut {
        let class = classify(&body);
        if matches!(class, BodyClass::Pending | BodyClass::Accepted) {
            // Accepted on a command that has no accepted shape stays open.
            // Revoke, bootstrap, login, and run all accept that shape.
            if matches!(class, BodyClass::Accepted) && !accepts_ack(cur.which) {
                self.guest_open = true;
                self.issued_seen = true;
                return StepOut::Stay(Cursor::Guest(cur));
            }
            if matches!(class, BodyClass::Pending)
                || (matches!(class, BodyClass::Accepted) && accepts_ack(cur.which))
            {
                self.guest_open = true;
                self.issued_seen = true;
                let mut cur = cur;
                cur.seen = true;
                return StepOut::Stay(Cursor::Guest(cur));
            }
        }
        self.guest_open = false;
        match cur.which {
            1 => self.accept_bootstrap(body, cur.seen),
            2 => self.accept_login(body, cur.seen),
            3 => self.accept_run(body, cur.seen),
            _ => self.accept_revoke(body, cur.seen),
        }
    }

    fn accept_bootstrap(&mut self, body: BoundedOutput, seen: bool) -> StepOut {
        let Some(run) = self.run.clone() else {
            self.fail(NotRunReason::Unexpected);
            return StepOut::Stop(StepError::Unexpected);
        };
        match parse::parse_ack(body.as_bytes(), run.as_str()) {
            Ok(generation) => {
                self.evidence.ack_generation = Some(generation);
                self.phase = Phase::Bootstrapped;
                self.note_resolved();
                self.clear_slot();
                StepOut::Finish(CompletedStep::Bootstrapped)
            }
            Err(_) => self.guest_parse(seen, NotRunReason::MissingProof),
        }
    }

    fn accept_login(&mut self, body: BoundedOutput, seen: bool) -> StepOut {
        match parse::parse_login(body.as_bytes()) {
            Ok(LoginMark::Ready) => {
                self.evidence.login_ready = true;
                self.phase = Phase::CredentialReady;
                self.note_resolved();
                self.clear_slot();
                StepOut::Finish(CompletedStep::LoggedIn)
            }
            Ok(LoginMark::Missing) => self.guest_parse(seen, NotRunReason::MissingProof),
            Err(_) => self.guest_parse(seen, NotRunReason::Malformed),
        }
    }

    fn accept_run(&mut self, body: BoundedOutput, seen: bool) -> StepOut {
        match parse::parse_score(body.as_bytes()) {
            Ok(mark) => {
                self.evidence.provisional = Some(score_of(mark));
                self.phase = Phase::Running;
                self.note_resolved();
                self.clear_slot();
                StepOut::Finish(CompletedStep::Ran)
            }
            Err(_) => self.guest_parse(seen, NotRunReason::Malformed),
        }
    }

    fn accept_revoke(&mut self, body: BoundedOutput, seen: bool) -> StepOut {
        self.revoke_attempted = true;
        if classify(&body) == BodyClass::Terminal || legacy_ok(&body) {
            self.revoke_ok = true;
            self.set_open(Phase::Revoking);
            self.note_resolved();
            self.clear_slot();
            return StepOut::Finish(CompletedStep::Revoked);
        }
        self.set_open(Phase::Revoking);
        self.guest_parse(seen, NotRunReason::Malformed)
    }

    pub(super) fn tick_ready(
        &mut self,
        mut cur: ReadyCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        if cur.stage == 0 {
            return match self.gate_lock(
                cur.caller,
                self.budgets.query,
                Scope::Prep,
                None,
                poll_caller,
                step_end,
            ) {
                Ok(false) => StepOut::Stay(Cursor::Ready(cur)),
                Ok(true) => {
                    cur.stage = 1;
                    StepOut::Stay(Cursor::Ready(cur))
                }
                Err(err) => StepOut::Stop(err),
            };
        }
        let op = match self.save_op(cur.caller, self.budgets.query, Scope::Prep, None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.guest_exchange(call_end, op, false, |machine, end| {
            machine.guest.inventory(QueryDeadline::from_instant(end))
        }) {
            Io::Pending { issued } => {
                cur.seen = true;
                self.stay_cur(issued, Cursor::Ready(cur))
            }
            Io::Ready(body) => self.accept_ready(cur, body),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard | Io::Fault(_) => StepOut::Stay(Cursor::Ready(cur)),
        }
    }

    fn accept_ready(&mut self, mut cur: ReadyCur, body: BoundedOutput) -> StepOut {
        let seen = cur.seen;
        if matches!(classify(&body), BodyClass::Pending | BodyClass::Accepted) {
            self.guest_open = true;
            self.issued_seen = true;
            cur.seen = true;
            return StepOut::Stay(Cursor::Ready(cur));
        }
        let view = match parse::parse_guest(body.as_bytes()) {
            Ok(view) => view,
            Err(_) => return self.guest_parse(seen, NotRunReason::Malformed),
        };
        let Some(previous) = self.evidence.guest.clone() else {
            return self.guest_parse(seen, NotRunReason::MissingProof);
        };
        if previous.boot != view.boot {
            self.fail(NotRunReason::BootChanged);
            return StepOut::Stop(StepError::Breach);
        }
        if previous.uid != view.uid
            || previous.desktop != view.desktop
            || previous.config != view.config
        {
            self.fail(NotRunReason::EnvironmentBreach);
            return StepOut::Stop(StepError::Breach);
        }
        if !self.evidence.complete() {
            return self.guest_parse(seen, NotRunReason::MissingProof);
        }
        self.phase = Phase::Ready;
        self.clear_slot();
        StepOut::Finish(CompletedStep::Ready)
    }

    pub(super) fn tick_lease(
        &mut self,
        mut cur: LeaseCur,
        poll_caller: MonoInstant,
        step_end: MonoInstant,
    ) -> StepOut {
        if cur.stage == 0 {
            return match self.gate_lock(
                cur.caller,
                self.budgets.lease,
                Scope::Task,
                None,
                poll_caller,
                step_end,
            ) {
                Ok(false) => StepOut::Stay(Cursor::Lease(cur)),
                Ok(true) => {
                    cur.stage = 1;
                    StepOut::Stay(Cursor::Lease(cur))
                }
                Err(err) => StepOut::Stop(err),
            };
        }
        if self.lease_expired() {
            let _ = self.suspend_owner(cur.caller);
            return StepOut::Stop(StepError::LeaseExpired);
        }
        let op = match self.save_op(cur.caller, self.budgets.lease, Scope::Task, None) {
            Ok(end) => end,
            Err(err) => return StepOut::Stop(err),
        };
        let seq = match cur.seq {
            Some(seq) => seq,
            None => {
                let next = self.lease_seq.checked_add(1).ok_or(StepError::Overflow);
                let next = match next {
                    Ok(next) => next,
                    Err(err) => return StepOut::Stop(err),
                };
                match NonZeroU64::new(next) {
                    Some(seq) => {
                        cur.seq = Some(seq);
                        seq
                    }
                    None => return StepOut::Stop(StepError::Overflow),
                }
            }
        };
        let call_end = self.call_end(step_end, poll_caller, op);
        match self.guest_exchange(call_end, op, false, |machine, end| {
            machine.guest.lease(seq, LeaseDeadline::from_instant(end))
        }) {
            Io::Pending { issued } => {
                cur.seen = true;
                self.stay_cur(issued, Cursor::Lease(cur))
            }
            Io::Ready(body) => self.accept_lease(cur, op, body),
            Io::Stop(err) => StepOut::Stop(err),
            Io::Discard | Io::Fault(_) => StepOut::Stay(Cursor::Lease(cur)),
        }
    }

    fn accept_lease(&mut self, cur: LeaseCur, op: MonoInstant, body: BoundedOutput) -> StepOut {
        if matches!(classify(&body), BodyClass::Pending | BodyClass::Accepted) {
            self.guest_open = true;
            self.issued_seen = true;
            return StepOut::Stay(Cursor::Lease(cur));
        }
        if self.lease_expired() {
            let _ = self.suspend_owner(cur.caller);
            return StepOut::Stop(StepError::LeaseExpired);
        }
        if let Some(seq) = cur.seq {
            self.lease_seq = seq.get();
        }
        self.lease_until = Some(op);
        self.note_resolved();
        self.clear_slot();
        StepOut::Finish(CompletedStep::OwnerLeaseObserved)
    }

    pub(super) fn priority_lease_slice(
        &mut self,
        caller: MonoInstant,
        step_end: MonoInstant,
    ) -> Io<()> {
        let end = match self.bound(caller, self.budgets.lease, Scope::Prep, None) {
            Ok(end) => end,
            Err(err) => return Io::Stop(self.map_trip(err)),
        };
        let call_end = step_end.min(caller).min(end);
        if self.clock.now() >= call_end {
            return Io::Pending { issued: false };
        }
        if self.priority_seq == 0 {
            self.priority_seq = 1;
        }
        let Some(seq) = NonZeroU64::new(self.priority_seq) else {
            return Io::Stop(StepError::Overflow);
        };
        self.arm_room(call_end);
        let result = self.guest.lease(seq, LeaseDeadline::from_instant(end));
        self.clear_room();
        match result {
            Ok(body) if matches!(classify(&body), BodyClass::Pending | BodyClass::Accepted) => {
                self.guest_open = true;
                Io::Pending { issued: true }
            }
            Ok(_) => {
                self.priority_lease = false;
                self.priority_seq = 0;
                self.guest_open = false;
                Io::Ready(())
            }
            Err(CallError::Expired) => {
                self.priority_lease = false;
                Io::Stop(StepError::Expired)
            }
            Err(err) => {
                self.priority_lease = false;
                self.fail(super::super::call_reason(&err));
                Io::Stop(StepError::Call(err))
            }
        }
    }

    fn guest_exchange<F>(
        &mut self,
        call_end: MonoInstant,
        op_end: MonoInstant,
        count_run: bool,
        call: F,
    ) -> Io<BoundedOutput>
    where
        F: FnOnce(&mut Self, MonoInstant) -> Result<BoundedOutput, CallError>,
    {
        if !self.guest_open && self.clock.now() >= op_end {
            self.note_dispatch(
                "guest",
                super::super::progress::DispatchState::NotDispatched,
            );
            self.fail(NotRunReason::Deadline);
            return Io::Stop(StepError::Expired);
        }
        if self.clock.now() >= call_end {
            return Io::Pending {
                issued: self.guest_open,
            };
        }
        self.arm_room(call_end.min(op_end));
        let result = call(self, op_end);
        self.clear_room();
        if count_run {
            self.count_run(&result);
        }
        match result {
            Ok(body) => match classify(&body) {
                BodyClass::Pending | BodyClass::Accepted => {
                    self.guest_open = true;
                    self.issued_seen = true;
                    Io::Pending { issued: true }
                }
                BodyClass::Terminal => {
                    if self.jumped() {
                        self.mark_unknown("guest");
                        self.fail(NotRunReason::Timeout);
                        Io::Stop(StepError::Expired)
                    } else {
                        self.note_issued("guest");
                        Io::Ready(body)
                    }
                }
            },
            Err(err) => Io::Stop(self.call_stop(err, "guest")),
        }
    }

    fn count_run(&mut self, result: &Result<BoundedOutput, CallError>) {
        if self.run_counted {
            return;
        }
        if matches!(result, Err(CallError::Expired)) && !self.guest_open {
            return;
        }
        self.client_starts = self.client_starts.saturating_add(1);
        self.run_counted = true;
    }

    fn guest_parse(&mut self, seen: bool, reason: NotRunReason) -> StepOut {
        if seen || self.issued_seen {
            self.mark_unknown("guest");
        }
        self.fail(reason);
        StepOut::Stop(if reason == NotRunReason::MissingProof {
            StepError::MalformedBody
        } else if reason == NotRunReason::EnvironmentBreach || reason == NotRunReason::BootChanged {
            StepError::Breach
        } else {
            StepError::MalformedBody
        })
    }

    fn guest_budget(&self, which: u8) -> std::time::Duration {
        match which {
            1 => self.budgets.stage,
            2 => self.budgets.login,
            3 => self.budgets.run,
            _ => self.budgets.revoke,
        }
    }

    fn guest_scope(&self, which: u8) -> Scope {
        match which {
            3 => Scope::Task,
            4 => Scope::Cleanup,
            _ => Scope::Prep,
        }
    }
}

fn classify(body: &BoundedOutput) -> BodyClass {
    match decode_reply(body.as_bytes()) {
        Ok(reply) => match reply.state {
            ReplyState::Pending => BodyClass::Pending,
            ReplyState::Accepted => BodyClass::Accepted,
            ReplyState::Terminal => BodyClass::Terminal,
        },
        Err(_) => BodyClass::Terminal,
    }
}

fn accepts_ack(which: u8) -> bool {
    matches!(which, 1..=4)
}

fn legacy_ok(body: &BoundedOutput) -> bool {
    !body.as_bytes().is_empty()
}
