//! Synchronous Tart operations. The fake records commands and never starts a process.

use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

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
/// This fake returns immediately, sleeps nowhere, and starts nothing.
/// An already-due deadline is rejected before dispatch. That is not
/// cancellation of work already in progress, and there is no second cancel
/// channel. An expired call does not consume a scripted reply.
pub trait TartRunner {
    /// Clone the fixed golden image to the permit's clone id.
    ///
    /// The effective deadline is the earlier of `deadline` and the permit's
    /// absolute deadline. The permit is consumed even when that instant is
    /// already due or the call fails, so it cannot be retried. At or after
    /// the effective deadline the call expires and does not consume a
    /// scripted reply. Does not accept a destination string.
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
/// enforced deadline is already due. `clone_vm` enforces the earlier of the
/// call deadline and the permit's absolute deadline, and records that instant.
pub struct FakeTartRunner {
    clock: FakeClock,
    script: VecDeque<ScriptedReply>,
    trace: Vec<TraceEntry<TartCommand>>,
    advance_after_stop: Option<Duration>,
    advance_after_delete: Option<Duration>,
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
            advance_after_stop: None,
            advance_after_delete: None,
        }
    }

    /// After each `stop`, move the shared fake clock forward by `step`.
    ///
    /// Does not sleep. The next call in the same machine step sees the new
    /// instant. A zero step is ignored.
    pub fn advance_after_stop(&mut self, step: Duration) {
        self.advance_after_stop = Some(step);
    }

    /// After each `delete`, move the shared fake clock forward by `step`.
    ///
    /// Does not sleep. See [`Self::advance_after_stop`].
    pub fn advance_after_delete(&mut self, step: Duration) {
        self.advance_after_delete = Some(step);
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
        let step = match &command {
            TartCommand::Stop { .. } => self.advance_after_stop,
            TartCommand::Delete { .. } => self.advance_after_delete,
            _ => None,
        };
        self.trace.push(TraceEntry {
            command,
            deadline,
            outcome: script_outcome(&result),
        });
        if let Some(step) = step {
            let _ = self.clock.advance(step);
        }
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
        // Taking `permit` by value consumes it on every path, including expiry.
        let effective = deadline.at().min(permit.absolute_deadline());
        let command = TartCommand::Clone {
            golden,
            clone: permit.clone_id().clone(),
        };
        self.record(command, effective)
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
