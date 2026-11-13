//! Offline [`GuestChannel`] on the M3 mailbox.
//!
//! The caller keeps the volume-root descriptor open. Nothing here mounts a
//! volume, starts Tart, or reads the environment. A repeated poll of an open
//! request does not publish again. Pending is a normal [`MailboxReply`].
//! IO and protocol breaches are [`CallError`]s and stick for the attempt.

use std::num::NonZeroU64;

use super::super::Probe;
use super::deadline::{
    HostClock, LeaseDeadline, LoginDeadline, MonoInstant, ProbeDeadline, QueryDeadline,
    RevokeDeadline, RunDeadline, StageDeadline,
};
use super::guest::{ConsumeNote, GuestAccounting, GuestChannel, GUEST_CHANNEL_VERSION};
use super::mailbox::{
    encode_frame, encode_reply, poll_guest_until, publish, Activity, AttemptBinding, Envelope,
    FailedRead, GuestPoll, HostSlot, InventoryConsume, InventorySnapshot, Layout, LeaseRequest,
    MailboxError, MailboxReply, MsgStatus, Op, Outcome, Payload, ReplyResult, ReplyState,
    RevokeSlot, SeqFault, SeqTracker, SideOp, SideResult, SnapStatus,
};
use super::output::{BoundedOutput, CallError};

/// Seven guest methods over pre-created mailbox slots.
///
/// `root` is the raw descriptor `prepare_slots` already checked. Dropping
/// this value does not close it and does not revoke the guest.
pub struct MailboxGuestChannel<C: HostClock> {
    clock: C,
    root: i32,
    layout: Layout,
    tracker: SeqTracker,
    binding: AttemptBinding,
    bytes_read: usize,
    stuck: Option<CallError>,
    open: [bool; 7],
    /// Grant reference fixed to the first inventory that reported this sample.
    ///
    /// A later inventory with the same `sample_ms` keeps this value. The host
    /// validity is `sample_ms + 2000` and does not move when the timestamp repeats.
    pinned: Option<LeaseRequest>,
    effect: Option<SideOp>,
    /// Side rows this attempt actually published.
    issued_active: Vec<(SideOp, u64)>,
    issued_revoke: Vec<u64>,
    /// Terminals retained until the machine records consumption.
    held_active: Option<Activity>,
    held_revoke: Option<RevokeSlot>,
    done_active: Option<Activity>,
    done_revoke: Option<RevokeSlot>,
    consumed_active: Vec<(SideOp, u64)>,
    consumed_revoke: Vec<u64>,
    queued: InventoryConsume,
    sent_consume: Option<InventoryConsume>,
}

impl<C: HostClock> MailboxGuestChannel<C> {
    /// Bind `layout` from [`super::mailbox::prepare_slots`] to `root`.
    ///
    /// Does not create slots and does not block.
    #[must_use]
    pub fn new(clock: C, root: i32, layout: Layout, binding: AttemptBinding) -> Self {
        Self {
            clock,
            root,
            layout,
            tracker: SeqTracker::new(binding.clone()),
            binding,
            bytes_read: 0,
            stuck: None,
            open: [false; 7],
            pinned: None,
            effect: None,
            issued_active: Vec::new(),
            issued_revoke: Vec::new(),
            held_active: None,
            held_revoke: None,
            done_active: None,
            done_revoke: None,
            consumed_active: Vec::new(),
            consumed_revoke: Vec::new(),
            queued: empty_consume(),
            sent_consume: None,
        }
    }

    /// Bytes read from guest slots in this attempt, including rejected frames.
    #[must_use]
    pub fn bytes_read(&self) -> usize {
        self.bytes_read
    }

    /// Lease request saved for the current sample. Repeated timestamps keep it.
    #[must_use]
    pub fn pinned_grant(&self) -> Option<LeaseRequest> {
        self.pinned
    }

    /// Host end of the latest published grant. An ACK does not move it.
    #[must_use]
    pub fn host_grant_end_ms(&self) -> Option<u64> {
        self.tracker.lease_deadline_ms()
    }

    fn pre(&mut self, at: super::deadline::MonoInstant) -> Result<(), CallError> {
        if let Some(err) = self.stuck.clone() {
            return Err(err);
        }
        if self.clock.now() >= at {
            return Err(CallError::Expired);
        }
        Ok(())
    }

    fn fail(&mut self, msg: &[u8]) -> CallError {
        let err = match BoundedOutput::try_new(msg.to_vec()) {
            Ok(body) => CallError::Failed(body),
            Err(_) => CallError::OutputTooLong,
        };
        self.stuck = Some(err.clone());
        err
    }

    fn map_err(&mut self, err: MailboxError) -> CallError {
        let msg: &[u8] = match err {
            MailboxError::Deadline => return CallError::Expired,
            MailboxError::Sequence(SeqFault::Boot) => b"boot-changed",
            MailboxError::Sequence(SeqFault::TooMany) => b"poll-cap",
            MailboxError::Sequence(SeqFault::Overflow) => b"lease-overflow",
            MailboxError::PollBudget => b"poll-budget",
            MailboxError::Io | MailboxError::Unsupported | MailboxError::Sticky => b"mailbox-io",
            MailboxError::Breach(_) | MailboxError::Malformed(_) | MailboxError::Sequence(_) => {
                b"mailbox"
            }
        };
        self.fail(msg)
    }

    /// Remember bytes from one poll. The 128 KiB cap is inside that read, not here.
    fn account(&mut self, bytes: usize) {
        self.bytes_read = self.bytes_read.saturating_add(bytes);
    }

    fn account_failed(&mut self, fail: FailedRead) -> CallError {
        self.account(fail.bytes);
        self.map_err(fail.error)
    }

    fn next_seq(&self, op: Op) -> u64 {
        self.tracker.outstanding(op).saturating_add(1)
    }

    fn publish_new(&mut self, op: Op, payload: Payload) -> Result<(), CallError> {
        let seq = self.next_seq(op);
        let env = Envelope {
            v: GUEST_CHANNEL_VERSION,
            run_id: self.binding.run_id.clone(),
            attempt_nonce: self.binding.attempt_nonce,
            vm_config_digest: self.binding.vm_config_digest,
            boot_id: self.tracker.boot_id().cloned(),
            op,
            seq,
            request_seq: seq,
            status: MsgStatus::Request,
            payload,
        };
        let frame = match encode_frame(&env) {
            Ok(frame) => frame,
            Err(err) => return Err(self.map_err(err)),
        };
        if let Err(err) = self.tracker.note_request(&frame) {
            return Err(self.map_err(err));
        }
        if let Err(err) = publish(self.root, &mut self.layout, HostSlot::Op(op), &frame) {
            return Err(self.map_err(err));
        }
        self.open[index(op)] = true;
        match op {
            Op::Bootstrap => self.issued_active.push((SideOp::Bootstrap, seq)),
            Op::Login => self.issued_active.push((SideOp::Login, seq)),
            Op::Run => self.issued_active.push((SideOp::Run, seq)),
            Op::Revoke => self.issued_revoke.push(seq),
            Op::Inventory | Op::Probe | Op::Lease => {}
        }
        Ok(())
    }

    fn poll_only(&mut self, op: Op, end: MonoInstant) -> Result<BoundedOutput, CallError> {
        if let Some(err) = self.stuck.clone() {
            return Err(err);
        }
        let root = self.root;
        let poll = poll_guest_until(
            root,
            &mut self.layout,
            op,
            &mut self.tracker,
            &self.clock,
            end,
        );
        match poll {
            Ok(poll) => self.finish_poll(op, poll),
            Err(fail) => Err(self.account_failed(fail)),
        }
    }

    fn exchange(
        &mut self,
        op: Op,
        end: MonoInstant,
        payload: impl FnOnce(&mut Self) -> Payload,
    ) -> Result<BoundedOutput, CallError> {
        if !self.open[index(op)] {
            let payload = payload(self);
            self.publish_new(op, payload)?;
        }
        self.poll_only(op, end)
    }

    fn finish_poll(&mut self, op: Op, poll: GuestPoll) -> Result<BoundedOutput, CallError> {
        let (reply, bytes) = match poll {
            GuestPoll::Pending {
                op,
                request_seq,
                bytes,
                ..
            } => (MailboxReply::pending(op, request_seq), bytes),
            GuestPoll::DiscardedLease { seq, bytes, .. } => {
                (MailboxReply::pending(Op::Lease, seq), bytes)
            }
            GuestPoll::Ready { reply, bytes, .. } => (reply, bytes),
        };
        self.account(bytes);
        if reply.state == ReplyState::Terminal {
            self.open[index(op)] = false;
            self.note_terminal(&reply)?;
        }
        match encode_reply(&reply) {
            Ok(body) => Ok(body),
            Err(err) => Err(self.map_err(err)),
        }
    }

    fn note_terminal(&mut self, reply: &MailboxReply) -> Result<(), CallError> {
        if reply.op != Op::Inventory {
            return Ok(());
        }
        let ReplyResult::Inventory(snap) = &reply.result else {
            return Err(self.fail(b"mailbox"));
        };
        let pinned_ms = self.pinned.map(|pin| pin.sample_ms);
        match pinned_ms {
            Some(ms) if snap.sample_ms == ms => {}
            Some(ms) if snap.sample_ms < ms => return Err(self.fail(b"sample")),
            _ => {
                self.pinned = Some(LeaseRequest {
                    inventory_request_seq: reply.request_seq,
                    sample_ms: snap.sample_ms,
                });
            }
        }
        self.confirm(snap)?;
        self.absorb(snap)?;
        Ok(())
    }

    fn confirm(&mut self, snap: &InventorySnapshot) -> Result<(), CallError> {
        let Some(asked) = self.sent_consume.take() else {
            return Ok(());
        };
        if let Some(op) = asked.op {
            self.confirm_active(snap, op, asked.request_seq)?;
        }
        if asked.revoke_seq >= 1 {
            self.confirm_revoke(snap, asked.revoke_seq)?;
        }
        Ok(())
    }

    fn confirm_active(
        &mut self,
        snap: &InventorySnapshot,
        op: SideOp,
        seq: u64,
    ) -> Result<(), CallError> {
        let row = &snap.active;
        let same_key = row.op == Some(op) && row.request_seq == seq;
        let exact = same_key && self.same_active(row, op, seq);
        let gone = row.status == SnapStatus::None;
        let next = self.other_issued_active(row, op, seq);
        let idempotent = self.consumed_active(op, seq)
            && (gone
                || self.held_active.as_ref() == Some(row)
                || self.done_active.as_ref() == Some(row));
        let held_this = self
            .held_active
            .as_ref()
            .is_some_and(|held| held.op == Some(op) && held.request_seq == seq);
        if exact || idempotent || (held_this && (gone || next)) {
            if held_this {
                self.done_active = self.held_active.clone();
                if gone || next {
                    self.held_active = None;
                    self.queued.op = None;
                    self.queued.request_seq = 0;
                }
            }
            return Ok(());
        }
        Err(self.fail(b"consume"))
    }

    fn confirm_revoke(&mut self, snap: &InventorySnapshot, seq: u64) -> Result<(), CallError> {
        let row = snap.revoke;
        let exact = row.status == SnapStatus::Terminal
            && row.request_seq == seq
            && (self.held_revoke == Some(row) || self.done_revoke == Some(row));
        let gone = row.status == SnapStatus::None;
        let idempotent = self.consumed_revoke.contains(&seq)
            && (gone || self.held_revoke == Some(row) || self.done_revoke == Some(row));
        let held_this = self.held_revoke.is_some_and(|held| held.request_seq == seq);
        if exact || idempotent || (held_this && gone) {
            if held_this {
                self.done_revoke = self.held_revoke;
                if gone {
                    self.held_revoke = None;
                    self.queued.revoke_seq = 0;
                }
            }
            return Ok(());
        }
        Err(self.fail(b"consume"))
    }

    fn same_active(&self, row: &Activity, op: SideOp, seq: u64) -> bool {
        let held = self.held_active.as_ref() == Some(row);
        let done = self.done_active.as_ref() == Some(row);
        row.status == SnapStatus::Terminal
            && row.op == Some(op)
            && row.request_seq == seq
            && (held || done)
    }

    fn other_issued_active(&self, row: &Activity, op: SideOp, seq: u64) -> bool {
        let Some(row_op) = row.op else {
            return false;
        };
        if row.status == SnapStatus::None || (row_op == op && row.request_seq == seq) {
            return false;
        }
        self.issued_active(row_op, row.request_seq)
    }

    fn issued_active(&self, op: SideOp, seq: u64) -> bool {
        self.issued_active
            .iter()
            .any(|row| row.0 == op && row.1 == seq)
    }

    fn consumed_active(&self, op: SideOp, seq: u64) -> bool {
        self.consumed_active
            .iter()
            .any(|row| row.0 == op && row.1 == seq)
    }

    fn absorb(&mut self, snap: &InventorySnapshot) -> Result<(), CallError> {
        self.absorb_active(&snap.active)?;
        self.absorb_revoke(snap.revoke)?;
        Ok(())
    }

    fn absorb_active(&mut self, row: &Activity) -> Result<(), CallError> {
        if row.status == SnapStatus::None {
            return Ok(());
        }
        let Some(op) = row.op else {
            return Err(self.fail(b"unissued"));
        };
        if !self.issued_active(op, row.request_seq) {
            return Err(self.fail(b"unissued"));
        }
        if row.status != SnapStatus::Terminal {
            return Ok(());
        }
        if let Some(prev) = &self.held_active {
            let prev_op = prev.op.unwrap_or(op);
            let prev_seq = prev.request_seq;
            let prev_open = !self.consumed_active(prev_op, prev_seq);
            if prev_open && (prev.op != Some(op) || prev_seq != row.request_seq) {
                return Err(self.fail(b"unissued"));
            }
            if prev.op == Some(op) && prev_seq == row.request_seq && prev != row {
                return Err(self.fail(b"consume"));
            }
        }
        self.held_active = Some(row.clone());
        if self.effect == Some(op) {
            self.effect = None;
        }
        Ok(())
    }

    fn absorb_revoke(&mut self, row: RevokeSlot) -> Result<(), CallError> {
        if row.status == SnapStatus::None {
            return Ok(());
        }
        if !self.issued_revoke.contains(&row.request_seq) {
            return Err(self.fail(b"unissued"));
        }
        if row.status != SnapStatus::Terminal {
            return Ok(());
        }
        if let Some(prev) = self.held_revoke {
            let prev_open = !self.consumed_revoke.contains(&prev.request_seq);
            if prev_open && prev.request_seq != row.request_seq {
                return Err(self.fail(b"unissued"));
            }
            if prev.request_seq == row.request_seq && prev != row {
                return Err(self.fail(b"consume"));
            }
        }
        self.held_revoke = Some(row);
        Ok(())
    }

    fn begin_side(&mut self, op: SideOp) -> Result<(), CallError> {
        if self.effect.is_some() {
            // Not sticky: the inventory terminal that retires the row clears
            // `effect`, and the next ordinary op is then legal.
            return Err(self.rejected(b"effect-busy"));
        }
        self.effect = Some(op);
        Ok(())
    }

    fn rejected(&self, msg: &[u8]) -> CallError {
        match BoundedOutput::try_new(msg.to_vec()) {
            Ok(body) => CallError::Failed(body),
            Err(_) => CallError::OutputTooLong,
        }
    }

    fn side(&mut self, op: Op, side: SideOp, end: MonoInstant) -> Result<BoundedOutput, CallError> {
        if !self.open[index(op)] {
            self.begin_side(side)?;
        }
        self.exchange(op, end, |_| Payload::Empty)
    }

    fn active_success(&self, op: SideOp, seq: u64) -> bool {
        if !self.issued_active(op, seq) {
            return false;
        }
        let Some(row) = &self.held_active else {
            return false;
        };
        if row.status != SnapStatus::Terminal || row.op != Some(op) || row.request_seq != seq {
            return false;
        }
        match &row.result {
            Some(SideResult::Bootstrap(value)) if op == SideOp::Bootstrap => {
                value.outcome == Outcome::Ok && value.value.is_some()
            }
            Some(SideResult::Login(value)) if op == SideOp::Login => {
                value.outcome == Outcome::Ok && value.value.is_some()
            }
            Some(SideResult::Run(value)) if op == SideOp::Run => {
                value.outcome == Outcome::Ok && value.value.is_some()
            }
            _ => false,
        }
    }

    fn revoke_success(&self, seq: u64) -> bool {
        if !self.issued_revoke.contains(&seq) {
            return false;
        }
        let Some(row) = self.held_revoke else {
            return false;
        };
        row.status == SnapStatus::Terminal
            && row.request_seq == seq
            && matches!(row.result, Some(value) if value.outcome == Outcome::Ok && value.done)
    }
}

impl<C: HostClock> GuestChannel for MailboxGuestChannel<C> {
    fn protocol_version(&self) -> u16 {
        GUEST_CHANNEL_VERSION
    }

    fn inventory(&mut self, deadline: QueryDeadline) -> Result<BoundedOutput, CallError> {
        let end = deadline.at();
        self.pre(end)?;
        let queued = self.queued;
        self.exchange(Op::Inventory, end, move |channel| {
            channel.sent_consume = Some(queued);
            Payload::InventoryRequest(queued)
        })
    }

    fn probe(&mut self, probe: Probe, deadline: ProbeDeadline) -> Result<BoundedOutput, CallError> {
        let end = deadline.at();
        self.pre(end)?;
        self.exchange(Op::Probe, end, move |_| Payload::ProbeRequest(probe))
    }

    fn bootstrap(&mut self, deadline: StageDeadline) -> Result<BoundedOutput, CallError> {
        let end = deadline.at();
        self.pre(end)?;
        self.side(Op::Bootstrap, SideOp::Bootstrap, end)
    }

    fn login(&mut self, deadline: LoginDeadline) -> Result<BoundedOutput, CallError> {
        let end = deadline.at();
        self.pre(end)?;
        self.side(Op::Login, SideOp::Login, end)
    }

    fn run(&mut self, deadline: RunDeadline) -> Result<BoundedOutput, CallError> {
        let end = deadline.at();
        self.pre(end)?;
        self.side(Op::Run, SideOp::Run, end)
    }

    fn lease(
        &mut self,
        seq: NonZeroU64,
        deadline: LeaseDeadline,
    ) -> Result<BoundedOutput, CallError> {
        let end = deadline.at();
        self.pre(end)?;
        let open = self.open[index(Op::Lease)];
        let outstanding = self.tracker.outstanding(Op::Lease);
        if !open || seq.get() != outstanding {
            let Some(pin) = self.pinned else {
                return Err(self.fail(b"lease-unsampled"));
            };
            if seq.get() != self.next_seq(Op::Lease) {
                return Err(self.fail(b"lease-seq"));
            }
            self.publish_new(Op::Lease, Payload::LeaseRequest(pin))?;
        }
        self.poll_only(Op::Lease, end)
    }

    fn revoke(&mut self, deadline: RevokeDeadline) -> Result<BoundedOutput, CallError> {
        let end = deadline.at();
        self.pre(end)?;
        self.exchange(Op::Revoke, end, |_| Payload::Empty)
    }
}

impl<C: HostClock> GuestAccounting for MailboxGuestChannel<C> {
    fn envelope_boot(&self) -> Option<String> {
        self.tracker.boot_id().map(|boot| boot.as_str().to_owned())
    }

    fn note_consumed(&mut self, note: ConsumeNote) -> Result<(), CallError> {
        if let Some(err) = self.stuck.clone() {
            return Err(err);
        }
        let active = if note.active_seq >= 1 {
            let op = match note.active_kind {
                1 => SideOp::Bootstrap,
                2 => SideOp::Login,
                3 => SideOp::Run,
                _ => return Err(self.fail(b"consume")),
            };
            if !self.active_success(op, note.active_seq) {
                return Err(self.fail(b"consume"));
            }
            Some((op, note.active_seq))
        } else if note.active_kind != 0 {
            return Err(self.fail(b"consume"));
        } else {
            None
        };
        let revoke = if note.revoke_seq >= 1 {
            if !self.revoke_success(note.revoke_seq) {
                return Err(self.fail(b"consume"));
            }
            Some(note.revoke_seq)
        } else {
            None
        };
        if active.is_none() && revoke.is_none() {
            return Err(self.fail(b"consume"));
        }
        if let Some((op, seq)) = active {
            if !self.consumed_active(op, seq) {
                self.consumed_active.push((op, seq));
            }
            self.queued.op = Some(op);
            self.queued.request_seq = seq;
        }
        if let Some(seq) = revoke {
            if !self.consumed_revoke.contains(&seq) {
                self.consumed_revoke.push(seq);
            }
            self.queued.revoke_seq = seq;
        }
        Ok(())
    }
}

fn index(op: Op) -> usize {
    match op {
        Op::Inventory => 0,
        Op::Probe => 1,
        Op::Bootstrap => 2,
        Op::Login => 3,
        Op::Run => 4,
        Op::Lease => 5,
        Op::Revoke => 6,
    }
}

fn empty_consume() -> InventoryConsume {
    match InventoryConsume::new(None, 0, 0) {
        Ok(consume) => consume,
        Err(_) => InventoryConsume {
            op: None,
            request_seq: 0,
            revoke_seq: 0,
        },
    }
}
