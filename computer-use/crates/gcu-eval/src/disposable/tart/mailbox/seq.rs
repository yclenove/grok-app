//! Per-attempt sequence, boot binding, and lease-grant checks. No I/O.

use super::super::manifest::Digest;
use super::codec::decode_frame;
use super::ids::{AsciiId, AttemptNonce};
use super::ops::{
    Envelope, LeaseTerminal, MailboxReply, MsgStatus, Op, Payload, ReplyResult, ReplyState,
    LEASE_WINDOW_MS, MAX_POLLS,
};
use super::{CodecFault, MailboxError, SeqFault};

/// Run, nonce, and VM digest fixed for one attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptBinding {
    /// Run identity.
    pub run_id: AsciiId,
    /// Public attempt nonce.
    pub attempt_nonce: AttemptNonce,
    /// Pinned VM config digest.
    pub vm_config_digest: Digest,
}

/// What a guest slot read means for the current attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observe {
    /// The slot is empty of a new reply. Not an error.
    Pending {
        /// Slot that was read.
        op: Op,
        /// Sequence the caller is still waiting on. Zero if none was sent.
        request_seq: u64,
    },
    /// An older lease acknowledgement matched a sent grant. The deadline stays.
    DiscardedLease {
        /// Lease sequence that was discarded.
        seq: u64,
    },
    /// A new accepted or terminal reply.
    Reply(MailboxReply),
}

/// Sequence state for one attempt. Fields are private so a caller cannot rewind them.
#[derive(Debug, Clone)]
pub struct SeqTracker {
    binding: AttemptBinding,
    boot: Option<AsciiId>,
    ops: [SlotSeq; 7],
    leases: Vec<SentLease>,
    deadline: Option<u64>,
}

#[derive(Debug, Clone)]
struct SlotSeq {
    expected: u64,
    sent: u64,
    polls: u64,
    phase: Phase,
    seen: Vec<Seen>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Open,
    Accepted,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    seq: u64,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SentLease {
    seq: u64,
    inventory_request_seq: u64,
    sample_ms: u64,
    until_ms: u64,
    /// First accepted ACK bytes for this grant, including a stale first sighting.
    ack: Option<Vec<u8>>,
}

impl SeqTracker {
    /// Start at sequence 1 with no boot and no deadline.
    #[must_use]
    pub fn new(binding: AttemptBinding) -> Self {
        Self {
            binding,
            boot: None,
            ops: std::array::from_fn(|_| SlotSeq {
                expected: 1,
                sent: 0,
                polls: 0,
                phase: Phase::Open,
                seen: Vec::new(),
            }),
            leases: Vec::new(),
            deadline: None,
        }
    }

    /// Record one host request. Does not keep the request bytes.
    pub fn note_request(&mut self, bytes: &[u8]) -> Result<u64, MailboxError> {
        let env = decode_frame(bytes)?;
        self.admit_request(&env)?;
        let advanced = succ(env.seq).map_err(MailboxError::Sequence)?;
        let grant = lease_grant(&env)?;
        {
            // Previous reply bytes stay so an exact stale frame is recognized.
            let slot = &mut self.ops[env.op.index()];
            slot.expected = advanced;
            slot.sent = env.seq;
            slot.phase = Phase::Open;
            if matches!(env.op, Op::Inventory | Op::Lease) {
                slot.polls = slot.polls.saturating_add(1);
            }
        }
        if let Some(grant) = grant {
            self.deadline = Some(grant.until_ms);
            self.leases.push(grant);
        }
        Ok(env.seq)
    }

    /// Interpret one guest frame for `slot`. Does not move the lease deadline.
    pub fn observe(&mut self, slot: Op, bytes: &[u8]) -> Result<Observe, MailboxError> {
        let env = decode_frame(bytes)?;
        self.precheck(slot, &env)?;
        let introduced = self
            .introducing_boot(&env)
            .map_err(MailboxError::Sequence)?;
        if slot == Op::Lease {
            return self.observe_lease(&env, bytes);
        }
        self.observe_ordinary(&env, bytes, introduced)
    }

    /// Last request sequence sent for `op`, or zero when none has been sent.
    #[must_use]
    pub fn outstanding(&self, op: Op) -> u64 {
        self.ops[op.index()].sent
    }

    /// Deadline of the latest sent lease. Observing an ACK does not change it.
    #[must_use]
    pub fn lease_deadline_ms(&self) -> Option<u64> {
        self.deadline
    }

    /// Boot bound by the first inventory terminal, if that reply has arrived.
    #[must_use]
    pub fn boot_id(&self) -> Option<&AsciiId> {
        self.boot.as_ref()
    }

    fn admit_request(&self, env: &Envelope) -> Result<(), MailboxError> {
        if env.status != MsgStatus::Request {
            return Err(MailboxError::Sequence(SeqFault::Status));
        }
        if env.seq != env.request_seq {
            return Err(MailboxError::Sequence(SeqFault::Echo));
        }
        if !self.same_binding(env) {
            return Err(MailboxError::Sequence(SeqFault::Binding));
        }
        let slot = &self.ops[env.op.index()];
        if env.seq != slot.expected {
            return Err(MailboxError::Sequence(SeqFault::NotConsecutive));
        }
        self.boot_for_request(env)?;
        if matches!(env.op, Op::Inventory | Op::Lease) && slot.polls >= MAX_POLLS {
            return Err(MailboxError::Sequence(SeqFault::TooMany));
        }
        // Fail before any state change when the following sequence cannot be named.
        let _advanced = succ(env.seq).map_err(MailboxError::Sequence)?;
        Ok(())
    }

    fn boot_for_request(&self, env: &Envelope) -> Result<(), MailboxError> {
        let ok = match &self.boot {
            None => env.op == Op::Inventory && env.seq == 1 && env.boot_id.is_none(),
            Some(boot) => env.boot_id.as_ref() == Some(boot),
        };
        if ok {
            Ok(())
        } else {
            Err(MailboxError::Sequence(SeqFault::Boot))
        }
    }

    fn precheck(&self, slot: Op, env: &Envelope) -> Result<(), MailboxError> {
        if env.op != slot {
            return Err(MailboxError::Sequence(SeqFault::WrongOp));
        }
        if env.status == MsgStatus::Request {
            return Err(MailboxError::Sequence(SeqFault::Status));
        }
        if !self.same_binding(env) {
            return Err(MailboxError::Sequence(SeqFault::Binding));
        }
        if env.seq != env.request_seq {
            return Err(MailboxError::Sequence(SeqFault::Echo));
        }
        if env.seq == 0 {
            return Err(MailboxError::Sequence(SeqFault::Unsent));
        }
        if env.seq > self.ops[slot.index()].sent {
            return Err(MailboxError::Sequence(SeqFault::Future));
        }
        Ok(())
    }

    /// `Some` when this reply is the inventory terminal that names the boot.
    fn introducing_boot(&self, env: &Envelope) -> Result<Option<AsciiId>, SeqFault> {
        match &self.boot {
            Some(boot) => {
                if env.boot_id.as_ref() == Some(boot) {
                    Ok(None)
                } else {
                    Err(SeqFault::Boot)
                }
            }
            None => {
                let first = env.op == Op::Inventory
                    && env.status == MsgStatus::Terminal
                    && env.seq == 1
                    && self.ops[Op::Inventory.index()].sent == 1;
                if !first {
                    return Err(SeqFault::Boot);
                }
                env.boot_id.clone().ok_or(SeqFault::Boot).map(Some)
            }
        }
    }

    fn observe_ordinary(
        &mut self,
        env: &Envelope,
        bytes: &[u8],
        introduced: Option<AsciiId>,
    ) -> Result<Observe, MailboxError> {
        // These four finish in an inventory row. M4 consumes that row. The
        // slot itself stays at accepted and never completes from its own file.
        if matches!(env.op, Op::Bootstrap | Op::Login | Op::Run | Op::Revoke)
            && env.status == MsgStatus::Terminal
        {
            return Err(MailboxError::Sequence(SeqFault::Status));
        }
        if self.replay(env.op, env.seq, bytes) {
            return Ok(self.pending_now(env.op));
        }
        let next = {
            let slot = &self.ops[env.op.index()];
            if env.seq != slot.sent {
                return Err(if slot.seen.iter().any(|seen| seen.seq == env.seq) {
                    MailboxError::Sequence(SeqFault::Changed)
                } else {
                    MailboxError::Sequence(SeqFault::WrongRequest)
                });
            }
            match (env.op, slot.phase, env.status) {
                (
                    Op::Bootstrap | Op::Login | Op::Run | Op::Revoke,
                    Phase::Open,
                    MsgStatus::Accepted,
                ) => Phase::Accepted,
                (Op::Inventory | Op::Probe, Phase::Open | Phase::Accepted, MsgStatus::Terminal) => {
                    Phase::Terminal
                }
                _ => return Err(MailboxError::Sequence(SeqFault::Changed)),
            }
        };
        let reply = into_reply(env)?;
        let slot = &mut self.ops[env.op.index()];
        slot.phase = next;
        slot.seen.push(Seen {
            seq: env.seq,
            bytes: bytes.to_vec(),
        });
        if let Some(boot) = introduced {
            self.boot = Some(boot);
        }
        Ok(Observe::Reply(reply))
    }

    fn observe_lease(&mut self, env: &Envelope, bytes: &[u8]) -> Result<Observe, MailboxError> {
        if env.status != MsgStatus::Terminal {
            return Err(MailboxError::Sequence(SeqFault::Status));
        }
        let Payload::LeaseTerminal(term) = &env.payload else {
            return Err(MailboxError::Malformed(CodecFault::Shape));
        };
        let Some(index) = self.leases.iter().position(|row| row.seq == env.seq) else {
            return Err(MailboxError::Sequence(SeqFault::LeaseMismatch));
        };
        if !same_grant(&self.leases[index], term) {
            return Err(MailboxError::Sequence(SeqFault::LeaseMismatch));
        }
        let ack_changed = self.leases[index]
            .ack
            .as_ref()
            .is_some_and(|seen| seen.as_slice() != bytes);
        if ack_changed {
            return Err(MailboxError::Sequence(SeqFault::Changed));
        }
        if self.leases[index].ack.is_none() {
            self.leases[index].ack = Some(bytes.to_vec());
        }
        if env.seq != self.ops[Op::Lease.index()].sent {
            return Ok(Observe::DiscardedLease { seq: env.seq });
        }
        if self.replay(Op::Lease, env.seq, bytes) {
            return Ok(self.pending_now(Op::Lease));
        }
        if self.ops[Op::Lease.index()].phase == Phase::Terminal {
            return Err(MailboxError::Sequence(SeqFault::Changed));
        }
        let reply = into_reply(env)?;
        let slot = &mut self.ops[Op::Lease.index()];
        slot.phase = Phase::Terminal;
        slot.seen.push(Seen {
            seq: env.seq,
            bytes: bytes.to_vec(),
        });
        Ok(Observe::Reply(reply))
    }

    fn replay(&self, op: Op, seq: u64, bytes: &[u8]) -> bool {
        self.ops[op.index()]
            .seen
            .iter()
            .any(|seen| seen.seq == seq && seen.bytes == bytes)
    }

    /// Old identical bytes keep waiting for the request that is in flight now.
    fn pending_now(&self, op: Op) -> Observe {
        Observe::Pending {
            op,
            request_seq: self.outstanding(op),
        }
    }

    fn same_binding(&self, env: &Envelope) -> bool {
        env.run_id == self.binding.run_id
            && env.attempt_nonce == self.binding.attempt_nonce
            && env.vm_config_digest == self.binding.vm_config_digest
    }
}

/// Next sequence, or [`SeqFault::Wrapped`] at `u64::MAX`.
pub(super) fn succ(seq: u64) -> Result<u64, SeqFault> {
    seq.checked_add(1).ok_or(SeqFault::Wrapped)
}

fn lease_grant(env: &Envelope) -> Result<Option<SentLease>, MailboxError> {
    if env.op != Op::Lease {
        return Ok(None);
    }
    let Payload::LeaseRequest(req) = &env.payload else {
        return Err(MailboxError::Malformed(CodecFault::Shape));
    };
    let until_ms = req
        .sample_ms
        .checked_add(LEASE_WINDOW_MS)
        .ok_or(MailboxError::Sequence(SeqFault::Overflow))?;
    Ok(Some(SentLease {
        seq: env.seq,
        inventory_request_seq: req.inventory_request_seq,
        sample_ms: req.sample_ms,
        until_ms,
        ack: None,
    }))
}

fn same_grant(grant: &SentLease, term: &LeaseTerminal) -> bool {
    grant.inventory_request_seq == term.inventory_request_seq
        && grant.sample_ms == term.sample_ms
        && grant.until_ms == term.until_ms
}

fn into_reply(env: &Envelope) -> Result<MailboxReply, MailboxError> {
    let (state, result) = match (env.status, &env.payload) {
        (MsgStatus::Accepted, Payload::Empty) => (ReplyState::Accepted, ReplyResult::Empty),
        (MsgStatus::Terminal, Payload::InventoryTerminal(body)) => {
            (ReplyState::Terminal, ReplyResult::Inventory(body.clone()))
        }
        (MsgStatus::Terminal, Payload::ProbeTerminal(check)) => {
            (ReplyState::Terminal, ReplyResult::Probe(*check))
        }
        (MsgStatus::Terminal, Payload::BootstrapTerminal(body)) => {
            (ReplyState::Terminal, ReplyResult::Bootstrap(*body))
        }
        (MsgStatus::Terminal, Payload::LoginTerminal(body)) => {
            (ReplyState::Terminal, ReplyResult::Login(*body))
        }
        (MsgStatus::Terminal, Payload::RunTerminal(body)) => {
            (ReplyState::Terminal, ReplyResult::Run(*body))
        }
        (MsgStatus::Terminal, Payload::LeaseTerminal(body)) => {
            (ReplyState::Terminal, ReplyResult::Lease(*body))
        }
        (MsgStatus::Terminal, Payload::RevokeTerminal(body)) => {
            (ReplyState::Terminal, ReplyResult::Revoke(*body))
        }
        _ => return Err(MailboxError::Malformed(CodecFault::Shape)),
    };
    let reply = MailboxReply {
        state,
        op: env.op,
        request_seq: env.request_seq,
        result,
    };
    reply.validate()?;
    Ok(reply)
}
