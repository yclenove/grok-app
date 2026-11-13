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
    FakeClock, HostClock, LeaseDeadline, LoginDeadline, MonoInstant, ProbeDeadline, QueryDeadline,
    RevokeDeadline, RunDeadline, StageDeadline,
};
use super::output::{
    dispatch_script, script_outcome, BoundedOutput, CallError, ScriptedReply, TraceEntry,
};

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
pub struct FakeGuestChannel {
    clock: FakeClock,
    script: VecDeque<ScriptedReply>,
    trace: Vec<TraceEntry<GuestCommand>>,
    io_cost: Option<Duration>,
    envelope: Option<String>,
}

impl FakeGuestChannel {
    /// Empty script. The next call fails closed until a reply is pushed.
    #[must_use]
    pub fn new(clock: FakeClock) -> Self {
        Self {
            clock,
            script: VecDeque::new(),
            trace: Vec::new(),
            io_cost: None,
            envelope: None,
        }
    }

    /// Boot the machine should treat as the first verified inventory envelope.
    pub fn set_envelope_boot(&mut self, boot: impl Into<String>) {
        self.envelope = Some(boot.into());
    }

    /// Charge `cost` on the next calls that fit in their deadline.
    ///
    /// A cost that would pass the deadline returns [`CallError::Expired`]
    /// without moving the clock and without taking a script. Does not sleep.
    pub fn set_io_cost(&mut self, cost: Duration) {
        self.io_cost = Some(cost);
    }

    /// Queue one reply.
    pub fn push(&mut self, reply: ScriptedReply) {
        self.script.push_back(reply);
    }

    /// Recorded operations, in call order.
    #[must_use]
    pub fn trace(&self) -> &[TraceEntry<GuestCommand>] {
        &self.trace
    }

    fn record(
        &mut self,
        command: GuestCommand,
        deadline: MonoInstant,
    ) -> Result<BoundedOutput, CallError> {
        if let Some(cost) = self.io_cost.filter(|cost| !cost.is_zero()) {
            let now = self.clock.now();
            let fits = now < deadline && now.checked_add(cost).is_ok_and(|next| next <= deadline);
            if !fits {
                let result = Err(CallError::Expired);
                self.trace.push(TraceEntry {
                    command,
                    deadline,
                    outcome: script_outcome(&result),
                });
                return result;
            }
            let result = dispatch_script(&mut self.script, now, deadline);
            self.trace.push(TraceEntry {
                command,
                deadline,
                outcome: script_outcome(&result),
            });
            if result.is_ok() {
                let _ = self.clock.advance(cost);
            }
            return result;
        }
        let result = dispatch_script(&mut self.script, self.clock.now(), deadline);
        self.trace.push(TraceEntry {
            command,
            deadline,
            outcome: script_outcome(&result),
        });
        result
    }
}

impl GuestChannel for FakeGuestChannel {
    fn protocol_version(&self) -> u16 {
        GUEST_CHANNEL_VERSION
    }

    fn inventory(&mut self, deadline: QueryDeadline) -> Result<BoundedOutput, CallError> {
        self.record(GuestCommand::Inventory, deadline.at())
    }

    fn probe(&mut self, probe: Probe, deadline: ProbeDeadline) -> Result<BoundedOutput, CallError> {
        self.record(GuestCommand::Probe(probe), deadline.at())
    }

    fn bootstrap(&mut self, deadline: StageDeadline) -> Result<BoundedOutput, CallError> {
        self.record(GuestCommand::Bootstrap, deadline.at())
    }

    fn login(&mut self, deadline: LoginDeadline) -> Result<BoundedOutput, CallError> {
        self.record(GuestCommand::Login, deadline.at())
    }

    fn run(&mut self, deadline: RunDeadline) -> Result<BoundedOutput, CallError> {
        self.record(GuestCommand::Run, deadline.at())
    }

    fn lease(
        &mut self,
        seq: NonZeroU64,
        deadline: LeaseDeadline,
    ) -> Result<BoundedOutput, CallError> {
        self.record(GuestCommand::Lease { seq }, deadline.at())
    }

    fn revoke(&mut self, deadline: RevokeDeadline) -> Result<BoundedOutput, CallError> {
        self.record(GuestCommand::Revoke, deadline.at())
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
}
