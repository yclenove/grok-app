//! Guest operation inventory. No transport is named or assumed.
//!
//! ADR 0018 H1 is undecided. This trait has no address, path, socket, or
//! shell field. S11-0 has to measure a real channel before an adapter exists.

use std::collections::VecDeque;
use std::fmt;
use std::num::NonZeroU64;
use std::time::Duration;

use super::super::Probe;
use super::deadline::{
    charge_quantum, FakeClock, HostClock, LeaseDeadline, LoginDeadline, MonoInstant, ProbeDeadline,
    QueryDeadline, RevokeDeadline, RunDeadline, StageDeadline,
};
use super::mailbox::{encode_reply, MailboxReply, Op, ReplyResult, ReplyState};
use super::output::{script_outcome, BoundedOutput, CallError, ScriptedReply, TraceEntry};

/// The only guest operation version this interface speaks.
///
/// There is no negotiation method and no transport version.
pub const GUEST_CHANNEL_VERSION: u16 = 1;

/// Fixed guest operations. A variant does not carry a path, address, or secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestCommand {
    /// Inventory with no filter string.
    Inventory,
    /// One named probe from the closed probe set.
    Probe(Probe),
    /// Fixed bootstrap. No program path.
    Bootstrap,
    /// Fixed login. No secret, account, or URL.
    Login,
    /// Fixed run. No prompt and no argv.
    Run,
    /// One lease frame. The sequence is non-zero. A late frame is not a revive API.
    Lease {
        /// Monotonic sequence the caller supplies. Zero cannot be represented.
        seq: NonZeroU64,
    },
    /// Revoke. No reason string.
    Revoke,
}

/// Versioned guest operations with an explicit deadline and a bounded body.
///
/// This fake returns immediately and does not open a connection. An
/// already-due deadline is rejected before dispatch only. That refusal is
/// not cancellation of an operation already in progress. An expired call
/// does not consume a scripted reply.
pub trait GuestChannel {
    /// Version this channel speaks. Must be [`GUEST_CHANNEL_VERSION`].
    ///
    /// Does not block.
    fn protocol_version(&self) -> u16;

    /// Inventory. No path or filter argument.
    fn inventory(&mut self, deadline: QueryDeadline) -> Result<BoundedOutput, CallError>;

    /// One probe from the closed set.
    fn probe(&mut self, probe: Probe, deadline: ProbeDeadline) -> Result<BoundedOutput, CallError>;

    /// Fixed bootstrap.
    fn bootstrap(&mut self, deadline: StageDeadline) -> Result<BoundedOutput, CallError>;

    /// Fixed login. The method takes no credential.
    ///
    /// The deadline is checked before dispatch only. It does not cancel a
    /// login that has already started, and it does not mean the caller waits
    /// out the cap.
    fn login(&mut self, deadline: LoginDeadline) -> Result<BoundedOutput, CallError>;

    /// Fixed run. The method takes no prompt.
    ///
    /// The deadline is checked before dispatch only. Waiting until the run
    /// finishes is not this method's meaning.
    fn run(&mut self, deadline: RunDeadline) -> Result<BoundedOutput, CallError>;

    /// One lease. `seq` cannot be zero.
    fn lease(
        &mut self,
        seq: NonZeroU64,
        deadline: LeaseDeadline,
    ) -> Result<BoundedOutput, CallError>;

    /// Revoke.
    fn revoke(&mut self, deadline: RevokeDeadline) -> Result<BoundedOutput, CallError>;
}

/// Machine-side consumption note. Not a [`GuestChannel`] method.
///
/// `active_kind` is 1 bootstrap, 2 login, 3 run, or 0 when only revoke is consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsumeNote {
    /// Ordinary op, or 0.
    pub active_kind: u8,
    /// Nested active sequence. Zero when no active row is consumed.
    pub active_seq: u64,
    /// Nested revoke sequence. Zero when revoke is not consumed.
    pub revoke_seq: u64,
}

/// Private accounting beside [`GuestChannel`]. Version 1 gains no method.
///
/// Scripted fakes ignore consumption. The mailbox adapter records it.
pub trait GuestAccounting {
    /// Boot from the first verified inventory envelope, when this channel has one.
    fn envelope_boot(&self) -> Option<String> {
        None
    }

    /// Queue consume only after the machine has recorded that handoff.
    fn note_consumed(&mut self, note: ConsumeNote) -> Result<(), CallError> {
        let _ = note;
        Ok(())
    }
}

/// Fake channel. It records operations and does not bind a transport.
enum Piece {
    State(ReplyState),
    Reply(ScriptedReply),
}

struct OpenSlot {
    command: GuestCommand,
    deadline: MonoInstant,
    remaining: Duration,
    seq: u64,
    traced: bool,
}

/// Scripted guest channel. Replies are queued. Nothing here opens a transport.
pub struct FakeGuestChannel {
    clock: FakeClock,
    script: VecDeque<Piece>,
    trace: Vec<TraceEntry<GuestCommand>>,
    io_cost: Duration,
    envelope: Option<String>,
    open: Option<OpenSlot>,
    next_seq: u64,
    consumes: u64,
}

impl FakeGuestChannel {
    /// Empty script. The next call fails closed until a reply is pushed.
    #[must_use]
    pub fn new(clock: FakeClock) -> Self {
        Self {
            clock,
            script: VecDeque::new(),
            trace: Vec::new(),
            io_cost: Duration::ZERO,
            envelope: None,
            open: None,
            next_seq: 1,
            consumes: 0,
        }
    }

    /// Boot the machine should treat as the first verified inventory envelope.
    pub fn set_envelope_boot(&mut self, boot: impl Into<String>) {
        self.envelope = Some(boot.into());
    }

    /// Charge `cost` before each scripted piece of the open command.
    ///
    /// Cost left after one poll is paid by the next poll of the same command.
    /// Does not sleep. Zero keeps the one-shot path.
    pub fn set_io_cost(&mut self, cost: Duration) {
        self.io_cost = cost;
    }

    /// How many times [`GuestAccounting::note_consumed`] ran.
    #[must_use]
    pub fn consume_count(&self) -> u64 {
        self.consumes
    }

    /// Queue a normalized pending or accepted observation. It is not a trace row.
    pub fn push_state(&mut self, state: ReplyState) {
        self.script.push_back(Piece::State(state));
    }

    /// Queue one reply.
    pub fn push(&mut self, reply: ScriptedReply) {
        self.script.push_back(Piece::Reply(reply));
    }

    /// Recorded operations, in call order.
    #[must_use]
    pub fn trace(&self) -> &[TraceEntry<GuestCommand>] {
        &self.trace
    }

    fn exchange(
        &mut self,
        command: GuestCommand,
        deadline: MonoInstant,
    ) -> Result<BoundedOutput, CallError> {
        let same = self
            .open
            .as_ref()
            .is_some_and(|slot| slot.command == command);
        if !same {
            // A new operation abandons the open slot. Cleanup must be able to
            // revoke after a guest call that already returned pending.
            self.open = None;
            if self.clock.now() >= deadline {
                let result = Err(CallError::Expired);
                self.trace.push(TraceEntry {
                    command,
                    deadline,
                    outcome: script_outcome(&result),
                });
                return result;
            }
            let seq = self.next_seq;
            self.next_seq = self.next_seq.saturating_add(1);
            self.open = Some(OpenSlot {
                command: command.clone(),
                deadline,
                remaining: self.io_cost,
                seq,
                traced: false,
            });
        }
        let (deadline, seq, remaining) = {
            let slot = self.open.as_mut().ok_or(CallError::ScriptExhausted)?;
            if !slot.remaining.is_zero() {
                let paid = charge_quantum(&self.clock, slot.remaining, slot.deadline);
                slot.remaining = slot.remaining.saturating_sub(paid);
            }
            (slot.deadline, slot.seq, slot.remaining)
        };
        if !remaining.is_zero() {
            if self.clock.now() >= deadline {
                return self.fail_open(CallError::TimedOut);
            }
            return pending_body(op_of(&command), seq);
        }
        if self.clock.now() >= deadline {
            return self.fail_open(CallError::TimedOut);
        }
        let piece = self.script.pop_front();
        match piece {
            Some(Piece::State(ReplyState::Pending)) => {
                if let Some(slot) = self.open.as_mut() {
                    slot.remaining = self.io_cost;
                }
                pending_body(op_of(&command), seq)
            }
            Some(Piece::State(ReplyState::Accepted)) => {
                if let Some(slot) = self.open.as_mut() {
                    slot.remaining = self.io_cost;
                }
                accepted_body(op_of(&command), seq)
            }
            Some(Piece::State(ReplyState::Terminal)) | None => {
                self.fail_open(CallError::ScriptExhausted)
            }
            Some(Piece::Reply(reply)) => {
                let result = reply.into_result();
                let outcome = script_outcome(&result);
                if let Some(slot) = self.open.as_mut() {
                    slot.traced = true;
                }
                let command = self
                    .open
                    .as_ref()
                    .map(|slot| slot.command.clone())
                    .unwrap_or(command);
                self.trace.push(TraceEntry {
                    command,
                    deadline,
                    outcome,
                });
                self.open = None;
                result
            }
        }
    }

    fn fail_open(&mut self, err: CallError) -> Result<BoundedOutput, CallError> {
        if let Some(slot) = self.open.take() {
            if !slot.traced {
                self.trace.push(TraceEntry {
                    command: slot.command,
                    deadline: slot.deadline,
                    outcome: script_outcome(&Err(err.clone())),
                });
            }
        }
        Err(err)
    }
}

fn op_of(command: &GuestCommand) -> Op {
    match command {
        GuestCommand::Inventory => Op::Inventory,
        GuestCommand::Probe(_) => Op::Probe,
        GuestCommand::Bootstrap => Op::Bootstrap,
        GuestCommand::Login => Op::Login,
        GuestCommand::Run => Op::Run,
        GuestCommand::Lease { .. } => Op::Lease,
        GuestCommand::Revoke => Op::Revoke,
    }
}

fn pending_body(op: Op, seq: u64) -> Result<BoundedOutput, CallError> {
    encode_reply(&MailboxReply::pending(op, seq)).map_err(|_| CallError::Malformed(empty_body()))
}

fn accepted_body(op: Op, seq: u64) -> Result<BoundedOutput, CallError> {
    let reply = MailboxReply {
        state: ReplyState::Accepted,
        op,
        request_seq: seq,
        result: ReplyResult::Empty,
    };
    encode_reply(&reply).map_err(|_| CallError::Malformed(empty_body()))
}

fn empty_body() -> BoundedOutput {
    match BoundedOutput::try_new(Vec::new()) {
        Ok(body) => body,
        Err(_) => match BoundedOutput::try_new(vec![b'x']) {
            Ok(body) => body,
            Err(_) => empty_body(),
        },
    }
}

impl GuestChannel for FakeGuestChannel {
    fn protocol_version(&self) -> u16 {
        GUEST_CHANNEL_VERSION
    }

    fn inventory(&mut self, deadline: QueryDeadline) -> Result<BoundedOutput, CallError> {
        self.exchange(GuestCommand::Inventory, deadline.at())
    }

    fn probe(&mut self, probe: Probe, deadline: ProbeDeadline) -> Result<BoundedOutput, CallError> {
        self.exchange(GuestCommand::Probe(probe), deadline.at())
    }

    fn bootstrap(&mut self, deadline: StageDeadline) -> Result<BoundedOutput, CallError> {
        self.exchange(GuestCommand::Bootstrap, deadline.at())
    }

    fn login(&mut self, deadline: LoginDeadline) -> Result<BoundedOutput, CallError> {
        self.exchange(GuestCommand::Login, deadline.at())
    }

    fn run(&mut self, deadline: RunDeadline) -> Result<BoundedOutput, CallError> {
        self.exchange(GuestCommand::Run, deadline.at())
    }

    fn lease(
        &mut self,
        seq: NonZeroU64,
        deadline: LeaseDeadline,
    ) -> Result<BoundedOutput, CallError> {
        self.exchange(GuestCommand::Lease { seq }, deadline.at())
    }

    fn revoke(&mut self, deadline: RevokeDeadline) -> Result<BoundedOutput, CallError> {
        self.exchange(GuestCommand::Revoke, deadline.at())
    }
}

impl fmt::Debug for FakeGuestChannel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FakeGuestChannel")
            .field("queued", &self.script.len())
            .field("traced", &self.trace.len())
            .finish()
    }
}

impl GuestAccounting for FakeGuestChannel {
    fn envelope_boot(&self) -> Option<String> {
        self.envelope.clone()
    }

    fn note_consumed(&mut self, note: ConsumeNote) -> Result<(), CallError> {
        let _ = note;
        self.consumes = self.consumes.saturating_add(1);
        Ok(())
    }
}
