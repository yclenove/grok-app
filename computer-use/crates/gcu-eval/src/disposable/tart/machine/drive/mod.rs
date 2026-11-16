//! One 100ms step. Each public entry or poll runs owner service, then one stage.
//!
//! A stage may begin and poll one command when that command costs nothing and
//! the clock stays inside the step. It does not start the next stage's I/O.

mod forward;
mod reclaim;
mod stages;

use std::time::Duration;

use super::super::deadline::{HostClock, MonoInstant};
use super::super::guest::GuestChannel;
use super::super::identity::GoldenImage;
use super::super::ledger::{AttemptLedger, LedgerError, LedgerHandle, LedgerPoll};
use super::super::lock::{CleanupLock, LockError};
use super::super::output::{BoundedOutput, CallError};
use super::super::owner::{OwnerProof, OwnerRead, OwnerSource, OWNER_PROOF_LIFE};
use super::super::policy::SoftnetFlags;
use super::super::receipt::{AttemptOutcome, NotRunReason, Phase};
use super::super::runner::{CommandHandle, CommandKind, CommandPoll, TartRunner};
use super::cleanup::CleanupDeadlines;
use super::progress::{CompletedStep, DispatchState, EffectRecord, StepSpan};
use super::{
    call_reason, ledger_reason, step_budget, Attempt, ReserveInput, RuntimeLoss, Scope, StepError,
    StepProgress,
};

/// Termination share for clone and start. A shorter clipped window still begins.
const SHARE_LONG_MS: u64 = 1_200;
/// Termination share for stop, delete, list, and status.
const SHARE_SHORT_MS: u64 = 200;

pub(super) enum DriveKind {
    Reserve(ReserveInput),
    Clone(GoldenImage),
    Start(SoftnetFlags),
    Isolate,
    Bootstrap,
    Login,
    Ready,
    Run,
    Lease,
    Revoke,
    Destroy(CleanupDeadlines),
    Seal(super::super::deadline::SealDeadline),
    Recover(CleanupDeadlines),
    Cancel,
    Provider,
    Runtime(RuntimeLoss),
}

pub(super) enum Cursor {
    Reserve(ReserveCur),
    Clone(CloneCur),
    Start(StartCur),
    Isolate(IsoCur),
    Guest(GuestCur),
    Ready(ReadyCur),
    Lease(LeaseCur),
    Destroy(DestroyCur),
    Seal(SealCur),
    Recover(RecoverCur),
    Loss(LossCur),
    Kill(KillCur),
}

pub(super) struct ReserveCur {
    pub input: ReserveInput,
    pub caller: MonoInstant,
    pub stage: u8,
}

pub(super) struct CloneCur {
    pub golden: GoldenImage,
    pub caller: MonoInstant,
    pub stage: u8,
}

pub(super) struct StartCur {
    pub flags: SoftnetFlags,
    pub caller: MonoInstant,
    pub stage: u8,
}

pub(super) struct IsoCur {
    pub caller: MonoInstant,
    pub stage: u8,
    pub probe_at: usize,
    pub seen: bool,
}

pub(super) struct GuestCur {
    pub which: u8,
    pub caller: MonoInstant,
    pub seen: bool,
    pub stage: u8,
}

pub(super) struct ReadyCur {
    pub caller: MonoInstant,
    pub stage: u8,
    pub seen: bool,
}

pub(super) struct LeaseCur {
    pub caller: MonoInstant,
    pub seq: Option<std::num::NonZeroU64>,
    pub seen: bool,
    pub stage: u8,
}

pub(super) struct DestroyCur {
    pub deadlines: CleanupDeadlines,
    pub reclaim: ReclaimCur,
}

pub(super) struct SealCur {
    pub caller: MonoInstant,
    pub stage: u8,
    pub confirmed: bool,
}

pub(super) struct RecoverCur {
    pub deadlines: CleanupDeadlines,
    pub caller: MonoInstant,
    pub rows: Vec<super::super::ledger::Reservation>,
    pub index: usize,
    pub receipts: Vec<super::super::receipt::AttemptReceipt>,
    pub phase: u8,
    pub reclaim: ReclaimCur,
    pub reason: NotRunReason,
}

pub(super) struct ReclaimCur {
    pub stage: u8,
    pub started: bool,
    pub run: Option<super::super::identity::RunId>,
    pub clone: Option<super::super::identity::CloneId>,
    pub identity: Option<super::super::ledger::VmIdentity>,
    pub scope: Scope,
    pub stop_at: Option<MonoInstant>,
    pub delete_at: Option<MonoInstant>,
    pub entry_query: MonoInstant,
    pub entry_stop: MonoInstant,
    pub entry_delete: MonoInstant,
    pub absolute: Option<MonoInstant>,
}

pub(super) struct LossCur {
    pub reason: NotRunReason,
    pub done: CompletedStep,
    pub caller: MonoInstant,
    pub locked: bool,
}

pub(super) struct KillCur {
    pub reason: NotRunReason,
    pub done: CompletedStep,
    pub caller: MonoInstant,
}

pub(super) enum Io<T> {
    Pending { issued: bool },
    Ready(T),
    Fault(LedgerError),
    Stop(StepError),
    Discard,
}

/// One step result. `Stay` carries the cursor by value so the next poll
/// resumes it. Boxing that cursor would allocate on every yield.
#[allow(clippy::large_enum_variant)]
pub(super) enum StepOut {
    Stay(Cursor),
    Finish(CompletedStep),
    Stop(StepError),
}

impl DriveKind {
    fn holdable(&self) -> bool {
        matches!(self, Self::Seal(_) | Self::Recover(_))
    }

    fn priority(&self) -> bool {
        matches!(self, Self::Cancel | Self::Provider | Self::Runtime(_))
    }

    fn same_family(&self, cursor: &Cursor) -> bool {
        match (self, cursor) {
            (Self::Reserve(_), Cursor::Reserve(_))
            | (Self::Clone(_), Cursor::Clone(_))
            | (Self::Start(_), Cursor::Start(_))
            | (Self::Isolate, Cursor::Isolate(_))
            | (Self::Ready, Cursor::Ready(_))
            | (Self::Lease, Cursor::Lease(_))
            | (Self::Destroy(_), Cursor::Destroy(_))
            | (Self::Seal(_), Cursor::Seal(_))
            | (Self::Recover(_), Cursor::Recover(_)) => true,
            (Self::Bootstrap, Cursor::Guest(guest)) => guest.which == 1,
            (Self::Login, Cursor::Guest(guest)) => guest.which == 2,
            (Self::Run, Cursor::Guest(guest)) => guest.which == 3,
            (Self::Revoke, Cursor::Guest(guest)) => guest.which == 4,
            (
                Self::Cancel | Self::Provider | Self::Runtime(_),
                Cursor::Loss(_) | Cursor::Kill(_),
            ) => true,
            _ => false,
        }
    }
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
    pub(super) fn open_and_tick(
        &mut self,
        kind: DriveKind,
        caller: MonoInstant,
        entry: bool,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        let start = self.clock.now();
        self.step_start = Some(start);
        if self.held.is_some() && kind.holdable() {
            self.finish_span(start);
            return Ok(StepProgress::Pending);
        }
        let result = self.drive_inner(Some(kind), caller, entry);
        self.finish_span(start);
        result
    }

    pub(super) fn open_poll(
        &mut self,
        caller: MonoInstant,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        let start = self.clock.now();
        self.step_start = Some(start);
        if let Some(done) = self.held.take() {
            self.finish_span(start);
            return Ok(StepProgress::Done(done));
        }
        if self.cursor.is_none() {
            self.finish_span(start);
            return Err(StepError::NoActiveStep);
        }
        let result = self.drive_inner(None, caller, false);
        self.finish_span(start);
        result
    }

    pub(super) fn open_sync(&mut self, caller: MonoInstant) -> Result<(), StepError> {
        let start = self.clock.now();
        self.step_start = Some(start);
        if self.owner_dead || self.phase == Phase::Suspended {
            self.finish_span(start);
            return Err(StepError::OwnerDead);
        }
        let step_end = match start.checked_add(step_budget()) {
            Ok(end) => end,
            Err(_) => {
                self.finish_span(start);
                return Err(StepError::Overflow);
            }
        };
        let result = self.service(caller, step_end);
        self.finish_span(start);
        result.map(|_| ())
    }

    pub(super) fn open_observe(&mut self, caller: MonoInstant) -> Result<(), StepError> {
        let start = self.clock.now();
        self.step_start = Some(start);
        let result = self.observe_at(caller);
        self.finish_span(start);
        result
    }

    fn finish_span(&mut self, start: MonoInstant) {
        self.spans.push(StepSpan {
            start,
            end: self.clock.now(),
        });
        self.clear_room();
        self.step_start = None;
        self.yield_used = false;
    }

    fn drive_inner(
        &mut self,
        kind: Option<DriveKind>,
        caller: MonoInstant,
        entry: bool,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        let step_end = self
            .step_start
            .unwrap_or(self.clock.now())
            .checked_add(step_budget())
            .map_err(|_| StepError::Overflow)?;
        if let Some(kind) = kind.as_ref() {
            match self.prepare_kind(kind, caller) {
                Ok(Some(out)) => return self.step_result(out, false),
                Ok(None) => {}
                Err(err) => return Err(err),
            }
        }
        if self.cursor.is_none() {
            let Some(kind) = kind else {
                return Err(StepError::NoActiveStep);
            };
            let cursor = self.fresh_cursor(kind, caller)?;
            self.cursor = Some(cursor);
        }
        if let Err(err) = self.service(caller, step_end) {
            self.cursor = None;
            return Err(err);
        }
        if self.lease_expired() {
            self.cursor = None;
            self.suspend_owner(caller)?;
            return Err(StepError::LeaseExpired);
        }
        if self.yield_used {
            return Ok(StepProgress::Pending);
        }
        if self.priority_lease && self.honors_priority() {
            let other_slot = self.guest_open && self.priority_seq == 0;
            if other_slot {
                self.priority_lease = false;
            } else {
                match self.priority_lease_slice(caller, step_end) {
                    Io::Pending { .. } | Io::Ready(()) | Io::Fault(_) | Io::Discard => {
                        return Ok(StepProgress::Pending);
                    }
                    Io::Stop(err) => {
                        self.cursor = None;
                        return Err(err);
                    }
                }
            }
        }
        if let Some(pre) = self.preempt_stop() {
            return self.step_result(pre, entry);
        }
        if self.in_kill_tail() && !matches!(self.cursor, Some(Cursor::Kill(_))) {
            self.freeze_cancel(self.op_end.unwrap_or(caller));
            self.cursor = Some(Cursor::Kill(KillCur {
                reason: NotRunReason::Deadline,
                done: CompletedStep::Cancelled,
                caller,
            }));
        }
        let cursor = self.cursor.take();
        let Some(cursor) = cursor else {
            return Err(StepError::NoActiveStep);
        };
        let out = self.tick(cursor, caller, step_end);
        let hold = entry
            && matches!(
                &out,
                StepOut::Finish(CompletedStep::Sealed(_) | CompletedStep::Recovered(_))
            );
        self.step_result(out, hold)
    }

    fn step_result(
        &mut self,
        out: StepOut,
        hold: bool,
    ) -> Result<StepProgress<CompletedStep>, StepError> {
        match out {
            StepOut::Stay(cursor) => {
                self.cursor = Some(cursor);
                Ok(StepProgress::Pending)
            }
            StepOut::Stop(err) => {
                self.cursor = None;
                Err(err)
            }
            StepOut::Finish(done) => {
                self.cursor = None;
                self.clear_slot();
                if hold {
                    self.held = Some(done);
                    Ok(StepProgress::Pending)
                } else {
                    Ok(StepProgress::Done(done))
                }
            }
        }
    }

    fn prepare_kind(
        &mut self,
        kind: &DriveKind,
        caller: MonoInstant,
    ) -> Result<Option<StepOut>, StepError> {
        if let Some(cursor) = self.cursor.as_ref() {
            if kind.same_family(cursor) {
                return Ok(None);
            }
            if !kind.priority() {
                return Err(StepError::Unexpected);
            }
        }
        if matches!(kind, DriveKind::Revoke) && self.revoke_attempted && self.cursor.is_none() {
            if self.phase.closed() {
                return Err(StepError::Terminal);
            }
            self.set_open(Phase::Revoking);
            if self.revoke_ok {
                return Ok(Some(StepOut::Finish(CompletedStep::Revoked)));
            }
            return Err(StepError::Cleanup(NotRunReason::CleanupFailed));
        }
        if self.cursor.is_some() && kind.priority() {
            let (reason, done) = loss_pair(kind);
            if matches!(kind, DriveKind::Provider | DriveKind::Runtime(_))
                && (self.issued_seen || self.guest_open)
            {
                self.mark_unknown("guest");
            }
            if self.active_command.is_some() {
                self.freeze_cancel(self.op_end.unwrap_or(caller));
                self.cursor = Some(Cursor::Kill(KillCur {
                    reason,
                    done,
                    caller,
                }));
            } else {
                self.clear_slot();
                self.cursor = Some(loss_cursor(reason, done, caller));
            }
            return Ok(None);
        }
        self.precheck(kind, caller).map(|()| None)
    }

    fn fresh_cursor(&mut self, kind: DriveKind, caller: MonoInstant) -> Result<Cursor, StepError> {
        if kind.priority() && self.active_command.is_some() {
            let (reason, done) = loss_pair(&kind);
            self.freeze_cancel(self.op_end.unwrap_or(caller));
            return Ok(Cursor::Kill(KillCur {
                reason,
                done,
                caller,
            }));
        }
        self.clear_slot();
        if matches!(kind, DriveKind::Reserve(_)) && self.phase == Phase::Idle {
            // The previous recovery binding must not reject this reserve's proof.
            self.allow_rebind = true;
        }
        Ok(match kind {
            DriveKind::Reserve(input) => Cursor::Reserve(ReserveCur {
                input,
                caller,
                stage: 0,
            }),
            DriveKind::Clone(golden) => Cursor::Clone(CloneCur {
                golden,
                caller,
                stage: 0,
            }),
            DriveKind::Start(flags) => Cursor::Start(StartCur {
                flags,
                caller,
                stage: 0,
            }),
            DriveKind::Isolate => Cursor::Isolate(IsoCur {
                caller,
                stage: 0,
                probe_at: 0,
                seen: false,
            }),
            DriveKind::Bootstrap => Cursor::Guest(GuestCur {
                which: 1,
                caller,
                seen: false,
                stage: 0,
            }),
            DriveKind::Login => Cursor::Guest(GuestCur {
                which: 2,
                caller,
                seen: false,
                stage: 0,
            }),
            DriveKind::Run => Cursor::Guest(GuestCur {
                which: 3,
                caller,
                seen: false,
                stage: 0,
            }),
            DriveKind::Ready => Cursor::Ready(ReadyCur {
                caller,
                stage: 0,
                seen: false,
            }),
            DriveKind::Lease => Cursor::Lease(LeaseCur {
                caller,
                seq: None,
                seen: false,
                stage: 0,
            }),
            DriveKind::Revoke => Cursor::Guest(GuestCur {
                which: 4,
                caller,
                seen: false,
                stage: 0,
            }),
            DriveKind::Destroy(deadlines) => self.destroy_cursor(deadlines, caller)?,
            DriveKind::Seal(_) => Cursor::Seal(SealCur {
                caller,
                stage: 0,
                confirmed: false,
            }),
            DriveKind::Recover(deadlines) => Cursor::Recover(RecoverCur {
                deadlines,
                caller,
                rows: Vec::new(),
                index: 0,
                receipts: Vec::new(),
                phase: 0,
                reclaim: empty_reclaim(deadlines, Scope::Recovery),
                reason: NotRunReason::HostCrash,
            }),
            DriveKind::Cancel => {
                loss_cursor(NotRunReason::Cancelled, CompletedStep::Cancelled, caller)
            }
            DriveKind::Provider => loss_cursor(
                NotRunReason::ProviderCrash,
                CompletedStep::ProviderCrashNoted,
                caller,
            ),
            DriveKind::Runtime(kind) => {
                let reason = kind.reason();
                loss_cursor(reason, CompletedStep::RuntimeLossNoted, caller)
            }
        })
    }

    fn tick(&mut self, cursor: Cursor, poll_caller: MonoInstant, step_end: MonoInstant) -> StepOut {
        match cursor {
            Cursor::Reserve(cur) => self.tick_reserve(cur, poll_caller, step_end),
            Cursor::Clone(cur) => self.tick_clone(cur, poll_caller, step_end),
            Cursor::Start(cur) => self.tick_start(cur, poll_caller, step_end),
            Cursor::Isolate(cur) => self.tick_isolate(cur, poll_caller, step_end),
            Cursor::Guest(cur) => self.tick_guest(cur, poll_caller, step_end),
            Cursor::Ready(cur) => self.tick_ready(cur, poll_caller, step_end),
            Cursor::Lease(cur) => self.tick_lease(cur, poll_caller, step_end),
            Cursor::Destroy(cur) => self.tick_destroy(cur, poll_caller, step_end),
            Cursor::Seal(cur) => self.tick_seal(cur, poll_caller, step_end),
            Cursor::Recover(cur) => self.tick_recover(cur, poll_caller, step_end),
            Cursor::Loss(cur) => self.tick_loss(cur, poll_caller, step_end),
            Cursor::Kill(cur) => self.tick_kill(cur, poll_caller, step_end),
        }
    }

    fn service(&mut self, caller: MonoInstant, step_end: MonoInstant) -> Result<(), StepError> {
        self.yield_used = false;
        self.stop_fresh = false;
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        if self.owner_dead {
            return Err(StepError::OwnerDead);
        }
        if let Some(until) = self.owner_until {
            if self.clock.now() >= until {
                return self.suspend_owner(caller);
            }
        }
        if self.clock.now() >= step_end {
            return Ok(());
        }
        let read_end = step_end.min(caller);
        if self.clock.now() >= read_end {
            return Ok(());
        }
        let before = self.clock.now();
        let old_until = self.owner_until;
        self.arm_room(read_end);
        let read = self.owner.poll_owner(self.clock.now(), read_end);
        self.clear_room();
        if self.clock.now() != before {
            self.yield_used = true;
        }
        match read {
            OwnerRead::Pending => {
                self.yield_used = true;
                Ok(())
            }
            OwnerRead::Ready { stop, proof, lease } => {
                if let Some(until) = old_until {
                    if self.clock.now() >= until {
                        return self.suspend_owner(caller);
                    }
                }
                if stop && !self.stop_latched_flag {
                    self.stop_latched_flag = true;
                    self.stop_fresh = true;
                    self.halted = true;
                    if self.clone_id.is_some() && self.cleanup_origin.is_none() {
                        self.cleanup_origin = Some(self.clock.now());
                    }
                }
                self.consider_proof(proof)?;
                if lease && self.honors_priority() {
                    self.priority_lease = true;
                }
                Ok(())
            }
        }
    }

    fn consider_proof(&mut self, proof: Option<OwnerProof>) -> Result<(), StepError> {
        let rebind = self.allow_rebind;
        let Some(proof) = proof else {
            if rebind {
                self.allow_rebind = false;
            }
            return Ok(());
        };
        if proof.seq == 0 || proof.issued_at > self.clock.now() || self.clock.now() >= proof.until {
            return self.bad_proof();
        }
        let life = proof
            .issued_at
            .checked_add(OWNER_PROOF_LIFE)
            .map_err(|_| StepError::Overflow)?;
        if proof.until > life {
            return self.bad_proof();
        }
        if let Some(standing) = self.owner_proof.clone() {
            if proof.seq < standing.seq {
                return self.bad_proof();
            }
            if proof.seq == standing.seq {
                if proof.binding == standing.binding
                    && proof.issued_at == standing.issued_at
                    && proof.until == standing.until
                {
                    return Ok(());
                }
                return self.bad_proof();
            }
            if proof.binding != standing.binding && !rebind {
                return self.bad_proof();
            }
        }
        self.owner_until = Some(proof.until);
        self.owner_proof = Some(proof);
        self.allow_rebind = false;
        Ok(())
    }

    fn bad_proof(&mut self) -> Result<(), StepError> {
        self.fail(NotRunReason::Refused);
        Err(StepError::Refused)
    }

    fn suspend_owner(&mut self, caller: MonoInstant) -> Result<(), StepError> {
        if self.owner_dead {
            return Err(StepError::OwnerDead);
        }
        self.owner_dead = true;
        self.delete_forbidden = true;
        if self.trigger.is_none() {
            self.trigger = Some(NotRunReason::OwnerDeath);
        }
        let run = self.run.clone();
        let clone = self.clone_id.clone();
        self.phase = Phase::Suspended;
        self.receipt = Some(super::super::receipt::AttemptReceipt::new(
            run,
            clone.clone(),
            AttemptOutcome::NotRun {
                reason: NotRunReason::OwnerDeath,
                client_started: self.client_starts > 0,
            },
            false,
            crate::disposable::CleanupDisposition::Unknown,
            false,
            clone.is_some(),
        ));
        self.cursor = None;
        self.held = None;
        if self.clock.now() < caller && !self.yield_used {
            if let Some(clone) = clone {
                let _ = self.append_unknown(&clone, caller);
            }
        }
        Err(StepError::OwnerDead)
    }

    fn observe_at(&mut self, caller: MonoInstant) -> Result<(), StepError> {
        if self.phase.closed() {
            return Err(StepError::Terminal);
        }
        let step_end = self
            .clock
            .now()
            .checked_add(step_budget())
            .map_err(|_| StepError::Overflow)?;
        self.service(caller, step_end)?;
        if self.lease_expired() {
            return self.suspend_owner(caller).map(|_| ());
        }
        if self.phase != Phase::Running {
            self.fail(NotRunReason::Unexpected);
            return Err(StepError::Unexpected);
        }
        Err(StepError::LeaseLive)
    }

    fn honors_priority(&self) -> bool {
        matches!(
            self.phase,
            Phase::Booting
                | Phase::Isolated
                | Phase::Bootstrapped
                | Phase::CredentialReady
                | Phase::Ready
                | Phase::Running
        )
    }

    fn preempt_stop(&mut self) -> Option<StepOut> {
        if !self.stop_fresh {
            return None;
        }
        let cleanup = matches!(
            self.cursor,
            Some(
                Cursor::Destroy(_)
                    | Cursor::Seal(_)
                    | Cursor::Recover(_)
                    | Cursor::Loss(_)
                    | Cursor::Kill(_)
                    | Cursor::Guest(GuestCur { which: 4, .. })
            )
        );
        if cleanup {
            return None;
        }
        if self.active_command.is_some() {
            self.freeze_cancel(self.op_end.unwrap_or(self.clock.now()));
            self.cursor = Some(Cursor::Kill(KillCur {
                reason: NotRunReason::Refused,
                done: CompletedStep::Cancelled,
                caller: self.clock.now(),
            }));
            return None;
        }
        self.fail(NotRunReason::Refused);
        self.cursor = None;
        Some(StepOut::Stop(StepError::StopLatched))
    }

    pub(super) fn clear_slot(&mut self) {
        self.active_command = None;
        self.ledger_handle = None;
        self.op_end = None;
        self.work_end = None;
        self.issued_seen = false;
        self.kill_started = false;
        self.cancel_end = None;
        self.issue_note_clear();
    }

    fn issue_note_clear(&mut self) {
        self.guest_open = false;
    }

    pub(super) fn jumped(&self) -> bool {
        self.step_start
            .is_some_and(|start| self.clock.now().saturating_duration_since(start) > step_budget())
    }

    pub(super) fn call_end(
        &self,
        step_end: MonoInstant,
        poll_caller: MonoInstant,
        op_end: MonoInstant,
    ) -> MonoInstant {
        step_end.min(poll_caller).min(op_end)
    }

    pub(super) fn save_op(
        &mut self,
        caller: MonoInstant,
        budget: Duration,
        scope: Scope,
        absolute: Option<MonoInstant>,
    ) -> Result<MonoInstant, StepError> {
        if let Some(end) = self.op_end {
            return Ok(end);
        }
        match self.bound(caller, budget, scope, absolute) {
            Ok(end) => {
                self.op_end = Some(end);
                Ok(end)
            }
            Err(err) => Err(self.map_trip(err)),
        }
    }

    pub(super) fn arm_share(
        &mut self,
        op_end: MonoInstant,
        share: Duration,
    ) -> Result<(), StepError> {
        if self.work_end.is_some() {
            return Ok(());
        }
        let now = self.clock.now();
        if now >= op_end {
            self.note_dispatch("cmd", DispatchState::NotDispatched);
            self.fail(NotRunReason::Deadline);
            return Err(StepError::Expired);
        }
        if let Ok(work) = op_end.checked_sub(share) {
            if now < work {
                self.work_end = Some(work);
                return Ok(());
            }
        }
        self.work_end = Some(op_end);
        Ok(())
    }

    pub(super) fn in_kill_tail(&self) -> bool {
        match (self.work_end, self.op_end, self.active_command) {
            (Some(work), Some(end), Some(_)) if work < end => {
                let now = self.clock.now();
                now >= work && now < end
            }
            _ => false,
        }
    }

    pub(super) fn note_dispatch(&mut self, op: &'static str, dispatch: DispatchState) {
        self.effects_log.push(EffectRecord {
            op,
            dispatch,
            resolved: false,
        });
    }

    pub(super) fn note_issued(&mut self, op: &'static str) {
        if self
            .effects_log
            .last()
            .is_some_and(|row| row.op == op && row.dispatch == DispatchState::Issued)
        {
            return;
        }
        self.note_dispatch(op, DispatchState::Issued);
    }

    pub(super) fn note_resolved(&mut self) {
        if let Some(last) = self.effects_log.last_mut() {
            if last.dispatch == DispatchState::Issued {
                last.resolved = true;
            }
        }
    }

    pub(super) fn mark_unknown(&mut self, op: &'static str) {
        self.effect_unknown_flag = true;
        self.halted = true;
        self.note_dispatch(op, DispatchState::Unknown);
    }

    pub(super) fn ledger_err(&mut self, err: LedgerError) -> StepError {
        if matches!(
            err,
            LedgerError::Expired | LedgerError::AttemptAlreadyExpired
        ) {
            self.fail(NotRunReason::Deadline);
            return StepError::Expired;
        }
        self.fail(ledger_reason(&err));
        StepError::Ledger(err)
    }

    pub(super) fn call_stop(&mut self, err: CallError, op: &'static str) -> StepError {
        self.active_command = None;
        if self.issued_seen {
            self.mark_unknown(op);
            self.fail(call_reason(&err));
        } else if !matches!(err, CallError::Expired) {
            self.fail(call_reason(&err));
        } else {
            self.fail(NotRunReason::Deadline);
        }
        if matches!(err, CallError::Expired) && !self.issued_seen {
            StepError::Expired
        } else {
            StepError::Call(err)
        }
    }

    pub(super) fn lock_slice(&mut self, call_end: MonoInstant, op_end: MonoInstant) -> Io<()> {
        if self.clock.now() >= op_end {
            self.fail(NotRunReason::Deadline);
            return Io::Stop(StepError::Expired);
        }
        if self.clock.now() >= call_end {
            return Io::Pending { issued: false };
        }
        self.arm_room(call_end.min(op_end));
        let result = self.lock.try_acquire(op_end);
        self.clear_room();
        match result {
            Ok(()) => Io::Ready(()),
            Err(LockError::Pending) => Io::Pending { issued: false },
            Err(err) => Io::Stop(self.on_lock(err)),
        }
    }

    pub(super) fn stay_pending(&mut self, issued: bool) {
        if issued {
            self.issued_seen = true;
        }
    }

    fn freeze_cancel(&mut self, op_end: MonoInstant) {
        let mut end = op_end;
        if let Some(limit) = self.absolute {
            end = end.min(limit);
        }
        if let Some(origin) = self.cleanup_origin {
            if let Ok(cleanup_end) = origin.checked_add(self.budgets.cleanup) {
                end = end.min(cleanup_end);
            }
        }
        self.cancel_end = Some(self.cancel_end.map_or(end, |prior| prior.min(end)));
    }
}

fn loss_pair(kind: &DriveKind) -> (NotRunReason, CompletedStep) {
    match kind {
        DriveKind::Provider => (
            NotRunReason::ProviderCrash,
            CompletedStep::ProviderCrashNoted,
        ),
        DriveKind::Runtime(loss) => (loss.reason(), CompletedStep::RuntimeLossNoted),
        _ => (NotRunReason::Cancelled, CompletedStep::Cancelled),
    }
}

fn loss_cursor(reason: NotRunReason, done: CompletedStep, caller: MonoInstant) -> Cursor {
    Cursor::Loss(LossCur {
        reason,
        done,
        caller,
        locked: false,
    })
}

pub(super) fn empty_reclaim(deadlines: CleanupDeadlines, scope: Scope) -> ReclaimCur {
    ReclaimCur {
        stage: 0,
        started: false,
        run: None,
        clone: None,
        identity: None,
        scope,
        stop_at: None,
        delete_at: None,
        entry_query: deadlines.query.at(),
        entry_stop: deadlines.stop.at(),
        entry_delete: deadlines.delete.at(),
        absolute: None,
    }
}

pub(super) fn share_long() -> Duration {
    Duration::from_millis(SHARE_LONG_MS)
}

pub(super) fn share_short() -> Duration {
    Duration::from_millis(SHARE_SHORT_MS)
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
    pub(super) fn runner_io<F>(
        &mut self,
        call_end: MonoInstant,
        op_end: MonoInstant,
        op: &'static str,
        kind: CommandKind,
        begin: F,
    ) -> Io<BoundedOutput>
    where
        F: FnOnce(&mut Self, MonoInstant) -> Result<CommandHandle, CallError>,
    {
        if self.active_command.is_none() {
            if self.clock.now() >= op_end {
                self.note_dispatch(op, DispatchState::NotDispatched);
                self.fail(NotRunReason::Deadline);
                return Io::Stop(StepError::Expired);
            }
            if self.clock.now() >= call_end {
                return Io::Pending { issued: false };
            }
            self.arm_room(call_end.min(op_end));
            let before = self.clock.now();
            let begun = begin(self, op_end);
            self.clear_room();
            match begun {
                Ok(handle) => {
                    self.active_command = Some(handle);
                    self.note_issued(op);
                    if self.clock.now() != before || self.clock.now() >= call_end {
                        if self.jumped() {
                            self.carry_jump = true;
                        }
                        return Io::Pending { issued: true };
                    }
                }
                Err(err) => return Io::Stop(self.call_stop(err, op)),
            }
        }
        self.poll_runner(call_end, op, kind)
    }

    fn poll_runner(
        &mut self,
        call_end: MonoInstant,
        op: &'static str,
        kind: CommandKind,
    ) -> Io<BoundedOutput> {
        let Some(handle) = self.active_command else {
            return Io::Stop(StepError::Unexpected);
        };
        if self.clock.now() >= call_end {
            return Io::Pending { issued: true };
        }
        self.arm_room(call_end);
        let polled = self.runner.poll_command(&handle);
        self.clear_room();
        match polled {
            Ok(CommandPoll::Pending) => Io::Pending { issued: true },
            Ok(CommandPoll::Ready(body)) => self.adopt_body(kind, op, body),
            Err(err) => Io::Stop(self.call_stop(err, op)),
        }
    }

    fn adopt_body(
        &mut self,
        kind: CommandKind,
        op: &'static str,
        body: BoundedOutput,
    ) -> Io<BoundedOutput> {
        self.active_command = None;
        let jumped = self.jumped() || self.carry_jump;
        self.carry_jump = false;
        if !jumped {
            return Io::Ready(body);
        }
        let read = matches!(kind, CommandKind::List | CommandKind::Status);
        let stop_like = matches!(kind, CommandKind::Stop | CommandKind::Delete);
        if stop_like {
            return Io::Ready(body);
        }
        if read {
            if self.op_end.is_some_and(|end| self.clock.now() >= end) {
                self.mark_unknown(op);
                self.fail(NotRunReason::Deadline);
                return Io::Stop(StepError::Expired);
            }
            self.op_end = None;
            self.work_end = None;
            self.issued_seen = false;
            return Io::Discard;
        }
        self.mark_unknown(op);
        self.fail(NotRunReason::Timeout);
        Io::Stop(StepError::Expired)
    }

    pub(super) fn ledger_io<F>(
        &mut self,
        call_end: MonoInstant,
        op_end: MonoInstant,
        begin: F,
    ) -> Io<LedgerPoll>
    where
        F: FnOnce(&mut Self, MonoInstant) -> Result<LedgerHandle, LedgerError>,
    {
        if self.ledger_handle.is_none() {
            if self.clock.now() >= op_end {
                self.fail(NotRunReason::Deadline);
                return Io::Stop(StepError::Expired);
            }
            if self.clock.now() >= call_end {
                return Io::Pending { issued: false };
            }
            self.arm_room(call_end.min(op_end));
            let before = self.clock.now();
            let begun = begin(self, op_end);
            self.clear_room();
            match begun {
                Ok(handle) => {
                    self.ledger_handle = Some(handle);
                    if self.clock.now() != before || self.clock.now() >= call_end {
                        return Io::Pending { issued: false };
                    }
                }
                Err(err) => return Io::Fault(err),
            }
        }
        self.poll_ledger(call_end)
    }

    fn poll_ledger(&mut self, call_end: MonoInstant) -> Io<LedgerPoll> {
        let Some(handle) = self.ledger_handle else {
            return Io::Stop(StepError::Unexpected);
        };
        if self.clock.now() >= call_end {
            return Io::Pending { issued: false };
        }
        self.arm_room(call_end);
        let polled = self.ledger.poll_ledger(&handle);
        self.clear_room();
        self.ledger_handle = None;
        match polled {
            Ok(LedgerPoll::Pending) => {
                self.ledger_handle = Some(handle);
                Io::Pending { issued: false }
            }
            Ok(other) => Io::Ready(other),
            Err(err) => Io::Fault(err),
        }
    }
}
