//! Synchronous Tart operations. The fake records commands and never starts a process.

use std::collections::VecDeque;
use std::fmt;

use super::command::TartCommand;
use super::deadline::{
    CloneDeadline, DeleteDeadline, FakeClock, HostClock, MonoInstant, QueryDeadline, StartDeadline,
    StopDeadline,
};
use super::identity::{CloneId, GoldenImage};
use super::ledger::ClonePermit;
use super::output::{
    dispatch_script, script_outcome, BoundedOutput, CallError, ScriptedReply, TraceEntry,
};
use super::policy::SoftnetFlags;

/// Fixed Tart operations with an explicit deadline and a bounded body.
///
/// There is no method that takes an argv or shell string. `clone_vm` consumes
/// a [`ClonePermit`], which exists only after the ledger has written the row.
/// The other methods name a [`CloneId`] and do not themselves check ownership.
/// A future adapter must return by the deadline, including any kill and reap.
/// This fake returns immediately, sleeps nowhere, and starts nothing.
///
/// Cancellation is the deadline already being due. There is no second cancel
/// channel. An expired call does not consume a scripted reply.
pub trait TartRunner {
    /// Clone the fixed golden image to the permit's clone id.
    ///
    /// The permit is consumed even when the call fails, so the same permit
    /// cannot be retried. Does not accept a destination string.
    fn clone_vm(
        &mut self,
        permit: ClonePermit,
        golden: GoldenImage,
        deadline: CloneDeadline,
    ) -> Result<BoundedOutput, CallError>;

    /// Start one clone with required Softnet flags.
    ///
    /// There is no start without those flags.
    fn start(
        &mut self,
        clone: &CloneId,
        softnet: &SoftnetFlags,
        deadline: StartDeadline,
    ) -> Result<BoundedOutput, CallError>;

    /// Stop one clone.
    fn stop(&mut self, clone: &CloneId, deadline: StopDeadline)
        -> Result<BoundedOutput, CallError>;

    /// Delete one clone. A golden name is not a [`CloneId`].
    fn delete(
        &mut self,
        clone: &CloneId,
        deadline: DeleteDeadline,
    ) -> Result<BoundedOutput, CallError>;

    /// List with no name filter.
    fn list(&mut self, deadline: QueryDeadline) -> Result<BoundedOutput, CallError>;

    /// Status of one exact clone id.
    fn status(
        &mut self,
        clone: &CloneId,
        deadline: QueryDeadline,
    ) -> Result<BoundedOutput, CallError>;
}

/// Fake runner. Scripted replies are consumed in order, except when the
/// deadline is already due.
pub struct FakeTartRunner {
    clock: FakeClock,
    script: VecDeque<ScriptedReply>,
    trace: Vec<TraceEntry<TartCommand>>,
}

impl FakeTartRunner {
    /// Empty script. The next call fails closed until a reply is pushed.
    ///
    /// Does not look up a Tart binary.
    #[must_use]
    pub fn new(clock: FakeClock) -> Self {
        Self {
            clock,
            script: VecDeque::new(),
            trace: Vec::new(),
        }
    }

    /// Queue one reply. Overlong success bodies are rejected before they are queued.
    pub fn push(&mut self, reply: ScriptedReply) {
        self.script.push_back(reply);
    }

    /// Recorded commands, in call order.
    #[must_use]
    pub fn trace(&self) -> &[TraceEntry<TartCommand>] {
        &self.trace
    }

    fn record(
        &mut self,
        command: TartCommand,
        deadline: MonoInstant,
    ) -> Result<BoundedOutput, CallError> {
        let result = dispatch_script(&mut self.script, self.clock.now(), deadline);
        self.trace.push(TraceEntry {
            command,
            deadline,
            outcome: script_outcome(&result),
        });
        result
    }
}

impl TartRunner for FakeTartRunner {
    fn clone_vm(
        &mut self,
        permit: ClonePermit,
        golden: GoldenImage,
        deadline: CloneDeadline,
    ) -> Result<BoundedOutput, CallError> {
        let command = TartCommand::Clone {
            golden,
            clone: permit.clone_id().clone(),
        };
        self.record(command, deadline.at())
    }

    fn start(
        &mut self,
        clone: &CloneId,
        softnet: &SoftnetFlags,
        deadline: StartDeadline,
    ) -> Result<BoundedOutput, CallError> {
        let command = TartCommand::Start {
            clone: clone.clone(),
            softnet: *softnet,
        };
        self.record(command, deadline.at())
    }

    fn stop(
        &mut self,
        clone: &CloneId,
        deadline: StopDeadline,
    ) -> Result<BoundedOutput, CallError> {
        self.record(
            TartCommand::Stop {
                clone: clone.clone(),
            },
            deadline.at(),
        )
    }

    fn delete(
        &mut self,
        clone: &CloneId,
        deadline: DeleteDeadline,
    ) -> Result<BoundedOutput, CallError> {
        self.record(
            TartCommand::Delete {
                clone: clone.clone(),
            },
            deadline.at(),
        )
    }

    fn list(&mut self, deadline: QueryDeadline) -> Result<BoundedOutput, CallError> {
        self.record(TartCommand::List, deadline.at())
    }

    fn status(
        &mut self,
        clone: &CloneId,
        deadline: QueryDeadline,
    ) -> Result<BoundedOutput, CallError> {
        self.record(
            TartCommand::Status {
                clone: clone.clone(),
            },
            deadline.at(),
        )
    }
}

impl fmt::Debug for FakeTartRunner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FakeTartRunner")
            .field("queued", &self.script.len())
            .field("traced", &self.trace.len())
            .finish()
    }
}
