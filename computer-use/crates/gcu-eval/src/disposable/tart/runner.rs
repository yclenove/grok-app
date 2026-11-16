//! Tart operations as begin/poll handles. The fake never starts a process.

use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use super::command::TartCommand;
use super::deadline::{
    charge_quantum, CloneDeadline, DeleteDeadline, FakeClock, HostClock, MonoInstant,
    QueryDeadline, StartDeadline, StopDeadline,
};
use super::identity::{CloneId, GoldenImage};
use super::ledger::ClonePermit;
use super::output::{
    dispatch_script, script_outcome, BoundedOutput, CallError, ScriptOutcome, ScriptedReply,
    TraceEntry, MAX_OUTPUT_BYTES,
};
use super::policy::SoftnetFlags;

/// Kind of one fixed Tart command. Not an argv vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// `clone`.
    Clone,
    /// `start`.
    Start,
    /// `stop`.
    Stop,
    /// `delete`.
    Delete,
    /// `list`.
    List,
    /// `status`.
    Status,
}

/// One in-flight Tart command.
///
/// Copying the handle copies the id. It does not copy a permit or a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandHandle {
    id: u64,
    kind: CommandKind,
    end: MonoInstant,
    cap: usize,
}

impl CommandHandle {
    /// Id of this command instance.
    #[must_use]
    pub const fn id(self) -> u64 {
        self.id
    }

    /// Which fixed operation this is.
    #[must_use]
    pub const fn kind(self) -> CommandKind {
        self.kind
    }

    /// Deadline captured at begin. Later polls do not move it.
    #[must_use]
    pub const fn end(self) -> MonoInstant {
        self.end
    }

    /// Output cap captured at begin.
    #[must_use]
    pub const fn output_cap(self) -> usize {
        self.cap
    }
}

/// Result of polling a command that has not failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandPoll {
    /// The scripted cost is not paid yet. The same handle stays open.
    Pending,
    /// Bounded body. The handle is finished.
    Ready(BoundedOutput),
}

/// Why a reap body is not an exit proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReapFault {
    /// The reap stays pending until the caller stops.
    PendingForever,
    /// Empty body.
    Empty,
    /// Body that is not a strict exit record.
    Malformed,
    /// Explicit unknown.
    Unknown,
    /// Exit record for a different handle.
    WrongHandle,
    /// A terminal that arrives at or after the handle end.
    Late,
}

/// One reap observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReapPoll {
    /// Kill was started and the process is not reaped yet.
    Pending,
    /// Strict exit for this handle id, already reaped, and still in time.
    Exited {
        /// Handle id that exited.
        id: u64,
    },
    /// A body that must not be treated as exit.
    NotProof,
}

/// Fixed Tart operations with begin/poll and cancel/reap.
///
/// There is no method that takes an argv. `begin_clone` consumes a
/// [`ClonePermit`]. An already-due deadline does not consume a script.
/// There is no blocking default.
pub trait TartRunner {
    /// Begin clone. The permit is consumed on every path, including expiry.
    fn begin_clone(
        &mut self,
        permit: ClonePermit,
        golden: GoldenImage,
        deadline: CloneDeadline,
    ) -> Result<CommandHandle, CallError>;

    /// Begin start. There is no start without Softnet flags.
    fn begin_start(
        &mut self,
        clone: &CloneId,
        softnet: &SoftnetFlags,
        deadline: StartDeadline,
    ) -> Result<CommandHandle, CallError>;

    /// Begin stop.
    fn begin_stop(
        &mut self,
        clone: &CloneId,
        deadline: StopDeadline,
    ) -> Result<CommandHandle, CallError>;

    /// Begin delete.
    fn begin_delete(
        &mut self,
        clone: &CloneId,
        deadline: DeleteDeadline,
    ) -> Result<CommandHandle, CallError>;

    /// Begin list. No name filter.
    fn begin_list(&mut self, deadline: QueryDeadline) -> Result<CommandHandle, CallError>;

    /// Begin status of one exact clone id.
    fn begin_status(
        &mut self,
        clone: &CloneId,
        deadline: QueryDeadline,
    ) -> Result<CommandHandle, CallError>;

    /// Poll a handle from [`Self::begin_clone`] or the other begin methods.
    ///
    /// A second poll of a finished handle returns the same result and does not
    /// record another trace entry.
    fn poll_command(&mut self, handle: &CommandHandle) -> Result<CommandPoll, CallError>;

    /// Begin kill once for `handle`. A second call fails and does not kill again.
    fn begin_kill(&mut self, handle: &CommandHandle) -> Result<(), CallError>;

    /// Poll reap. Only a strict, matching, in-time exit is [`ReapPoll::Exited`].
    fn poll_reap(&mut self, handle: &CommandHandle) -> Result<ReapPoll, CallError>;
}

struct Live {
    id: u64,
    end: MonoInstant,
    command: TartCommand,
    remaining: Duration,
    result: Option<Result<BoundedOutput, CallError>>,
    traced: bool,
    script_pending: bool,
    kill_begun: bool,
    reap_remaining: Duration,
    reap: Option<ReapPoll>,
}

/// Fake runner. One command is in flight. Scripted replies are consumed when
/// that command finishes, not when a poll is still pending.
pub struct FakeTartRunner {
    clock: FakeClock,
    script: VecDeque<ScriptedReply>,
    trace: Vec<TraceEntry<TartCommand>>,
    advance_after_stop: Option<Duration>,
    advance_after_delete: Option<Duration>,
    io_cost: Duration,
    reap_cost: Duration,
    reap_fault: Option<ReapFault>,
    overrun: Option<Duration>,
    live: Option<Live>,
    next_id: u64,
    begins: u64,
    kills: u64,
}

impl FakeTartRunner {
    /// Empty script. The next begin fails closed until a reply is pushed.
    #[must_use]
    pub fn new(clock: FakeClock) -> Self {
        Self {
            clock,
            script: VecDeque::new(),
            trace: Vec::new(),
            advance_after_stop: None,
            advance_after_delete: None,
            io_cost: Duration::ZERO,
            reap_cost: Duration::ZERO,
            reap_fault: None,
            overrun: None,
            live: None,
            next_id: 1,
            begins: 0,
            kills: 0,
        }
    }

    /// After each finished `stop`, move the shared clock forward by `step`.
    ///
    /// Does not sleep. The jump is not capped by the step room.
    pub fn advance_after_stop(&mut self, step: Duration) {
        self.advance_after_stop = Some(step);
    }

    /// After each finished `delete`, move the shared clock forward by `step`.
    pub fn advance_after_delete(&mut self, step: Duration) {
        self.advance_after_delete = Some(step);
    }

    /// Charge `cost` across polls before a script is consumed.
    ///
    /// Zero keeps the one-shot path. Does not sleep.
    pub fn set_io_cost(&mut self, cost: Duration) {
        self.io_cost = cost;
    }

    /// Charge `cost` across reap polls before a strict exit.
    pub fn set_reap_cost(&mut self, cost: Duration) {
        self.reap_cost = cost;
    }

    /// Next reap returns [`ReapPoll::NotProof`] or stays pending.
    pub fn set_reap_fault(&mut self, fault: Option<ReapFault>) {
        self.reap_fault = fault;
    }

    /// After the next finished command, jump the clock by `step` with no room cap.
    pub fn set_overrun(&mut self, step: Option<Duration>) {
        self.overrun = step;
    }

    /// Queue one reply.
    pub fn push(&mut self, reply: ScriptedReply) {
        self.script.push_back(reply);
    }

    /// Recorded commands, in call order.
    #[must_use]
    pub fn trace(&self) -> &[TraceEntry<TartCommand>] {
        &self.trace
    }

    /// How many begins created a handle.
    #[must_use]
    pub fn begin_count(&self) -> u64 {
        self.begins
    }

    /// How many times kill was begun.
    #[must_use]
    pub fn kill_count(&self) -> u64 {
        self.kills
    }

    /// Id of the command still in flight.
    #[must_use]
    pub fn inflight_id(&self) -> Option<u64> {
        self.live.as_ref().map(|live| live.id)
    }

    /// Clone, then poll once. A pending cost is [`CallError::TimedOut`].
    pub fn clone_vm(
        &mut self,
        permit: ClonePermit,
        golden: GoldenImage,
        deadline: CloneDeadline,
    ) -> Result<BoundedOutput, CallError> {
        let handle = TartRunner::begin_clone(self, permit, golden, deadline)?;
        ready(TartRunner::poll_command(self, &handle)?)
    }

    /// Start, then poll once.
    pub fn start(
        &mut self,
        clone: &CloneId,
        softnet: &SoftnetFlags,
        deadline: StartDeadline,
    ) -> Result<BoundedOutput, CallError> {
        let handle = TartRunner::begin_start(self, clone, softnet, deadline)?;
        ready(TartRunner::poll_command(self, &handle)?)
    }

    /// Stop, then poll once.
    pub fn stop(
        &mut self,
        clone: &CloneId,
        deadline: StopDeadline,
    ) -> Result<BoundedOutput, CallError> {
        let handle = TartRunner::begin_stop(self, clone, deadline)?;
        ready(TartRunner::poll_command(self, &handle)?)
    }

    /// Delete, then poll once.
    pub fn delete(
        &mut self,
        clone: &CloneId,
        deadline: DeleteDeadline,
    ) -> Result<BoundedOutput, CallError> {
        let handle = TartRunner::begin_delete(self, clone, deadline)?;
        ready(TartRunner::poll_command(self, &handle)?)
    }

    /// List, then poll once.
    pub fn list(&mut self, deadline: QueryDeadline) -> Result<BoundedOutput, CallError> {
        let handle = TartRunner::begin_list(self, deadline)?;
        ready(TartRunner::poll_command(self, &handle)?)
    }

    /// Status, then poll once.
    pub fn status(
        &mut self,
        clone: &CloneId,
        deadline: QueryDeadline,
    ) -> Result<BoundedOutput, CallError> {
        let handle = TartRunner::begin_status(self, clone, deadline)?;
        ready(TartRunner::poll_command(self, &handle)?)
    }

    fn reject_due(&mut self, command: TartCommand, end: MonoInstant) -> Result<(), CallError> {
        if self.clock.now() < end {
            return Ok(());
        }
        self.trace.push(TraceEntry {
            command,
            deadline: end,
            outcome: ScriptOutcome::Expired,
        });
        Err(CallError::Expired)
    }

    fn open(
        &mut self,
        kind: CommandKind,
        command: TartCommand,
        end: MonoInstant,
    ) -> Result<CommandHandle, CallError> {
        self.reject_due(command.clone(), end)?;
        if self.live.is_some() {
            return Err(CallError::ScriptExhausted);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.begins = self.begins.saturating_add(1);
        self.live = Some(Live {
            id,
            end,
            command,
            remaining: self.io_cost,
            result: None,
            traced: false,
            script_pending: true,
            kill_begun: false,
            reap_remaining: self.reap_cost,
            reap: None,
        });
        self.make_progress();
        Ok(CommandHandle {
            id,
            kind,
            end,
            cap: MAX_OUTPUT_BYTES,
        })
    }

    fn make_progress(&mut self) {
        let due = {
            let Some(live) = self.live.as_mut() else {
                return;
            };
            if live.result.is_some() || live.kill_begun {
                return;
            }
            if !live.remaining.is_zero() {
                let paid = charge_quantum(&self.clock, live.remaining, live.end);
                live.remaining = live.remaining.saturating_sub(paid);
            }
            live.remaining.is_zero() && self.clock.now() < live.end
        };
        if !due {
            if self.clock.now()
                >= self
                    .live
                    .as_ref()
                    .map(|live| live.end)
                    .unwrap_or(MonoInstant::EPOCH)
                && self.live.as_ref().is_some_and(|live| live.result.is_none())
            {
                self.note_timeout();
            }
            return;
        }
        let end = self
            .live
            .as_ref()
            .map(|live| live.end)
            .unwrap_or(MonoInstant::EPOCH);
        let result = dispatch_script(&mut self.script, self.clock.now(), end);
        let outcome = script_outcome(&result);
        let kind_jump = self.live.as_ref().map(|live| live.command.clone());
        let jump = match &kind_jump {
            Some(TartCommand::Stop { .. }) => self.advance_after_stop,
            Some(TartCommand::Delete { .. }) => self.advance_after_delete,
            _ => None,
        };
        let overrun = self.overrun.take();
        if let Some(live) = self.live.as_mut() {
            live.result = Some(result);
            live.script_pending = false;
            live.traced = true;
        }
        if let Some(command) = kind_jump {
            self.trace.push(TraceEntry {
                command,
                deadline: end,
                outcome,
            });
        }
        if let Some(step) = jump {
            let _ = self.clock.advance(step);
        }
        if let Some(step) = overrun {
            let _ = self.clock.advance(step);
        }
    }

    fn note_timeout(&mut self) {
        let noted = {
            let Some(live) = self.live.as_mut() else {
                return;
            };
            if live.traced {
                return;
            }
            let command = live.command.clone();
            let end = live.end;
            let pending = live.script_pending;
            live.script_pending = false;
            live.result = Some(Err(CallError::TimedOut));
            live.traced = true;
            (command, end, pending)
        };
        if noted.2 {
            let _ = self.script.pop_front();
        }
        self.trace.push(TraceEntry {
            command: noted.0,
            deadline: noted.1,
            outcome: ScriptOutcome::TimedOut,
        });
    }
}

fn ready(polled: CommandPoll) -> Result<BoundedOutput, CallError> {
    match polled {
        CommandPoll::Ready(body) => Ok(body),
        CommandPoll::Pending => Err(CallError::TimedOut),
    }
}

impl TartRunner for FakeTartRunner {
    fn begin_clone(
        &mut self,
        permit: ClonePermit,
        golden: GoldenImage,
        deadline: CloneDeadline,
    ) -> Result<CommandHandle, CallError> {
        let effective = deadline.at().min(permit.absolute_deadline());
        let command = TartCommand::Clone {
            golden,
            clone: permit.clone_id().clone(),
        };
        // `permit` is dropped on every path, including the due-deadline path.
        self.open(CommandKind::Clone, command, effective)
    }

    fn begin_start(
        &mut self,
        clone: &CloneId,
        softnet: &SoftnetFlags,
        deadline: StartDeadline,
    ) -> Result<CommandHandle, CallError> {
        self.open(
            CommandKind::Start,
            TartCommand::Start {
                clone: clone.clone(),
                softnet: *softnet,
            },
            deadline.at(),
        )
    }

    fn begin_stop(
        &mut self,
        clone: &CloneId,
        deadline: StopDeadline,
    ) -> Result<CommandHandle, CallError> {
        self.open(
            CommandKind::Stop,
            TartCommand::Stop {
                clone: clone.clone(),
            },
            deadline.at(),
        )
    }

    fn begin_delete(
        &mut self,
        clone: &CloneId,
        deadline: DeleteDeadline,
    ) -> Result<CommandHandle, CallError> {
        self.open(
            CommandKind::Delete,
            TartCommand::Delete {
                clone: clone.clone(),
            },
            deadline.at(),
        )
    }

    fn begin_list(&mut self, deadline: QueryDeadline) -> Result<CommandHandle, CallError> {
        self.open(CommandKind::List, TartCommand::List, deadline.at())
    }

    fn begin_status(
        &mut self,
        clone: &CloneId,
        deadline: QueryDeadline,
    ) -> Result<CommandHandle, CallError> {
        self.open(
            CommandKind::Status,
            TartCommand::Status {
                clone: clone.clone(),
            },
            deadline.at(),
        )
    }

    fn poll_command(&mut self, handle: &CommandHandle) -> Result<CommandPoll, CallError> {
        if self.live.as_ref().is_some_and(|live| live.id != handle.id) || self.live.is_none() {
            return Err(CallError::ScriptExhausted);
        }
        self.make_progress();
        let live = self.live.as_mut().ok_or(CallError::ScriptExhausted)?;
        if live.kill_begun {
            return Err(CallError::TimedOut);
        }
        match &live.result {
            None => Ok(CommandPoll::Pending),
            Some(Ok(body)) => {
                let body = body.clone();
                self.live = None;
                Ok(CommandPoll::Ready(body))
            }
            Some(Err(err)) => {
                let err = err.clone();
                self.live = None;
                Err(err)
            }
        }
    }

    fn begin_kill(&mut self, handle: &CommandHandle) -> Result<(), CallError> {
        let live = self.live.as_mut().ok_or(CallError::ScriptExhausted)?;
        if live.id != handle.id {
            return Err(CallError::ScriptExhausted);
        }
        if live.kill_begun {
            return Err(CallError::Failed(
                BoundedOutput::try_new(b"kill".to_vec()).map_err(|_| CallError::OutputTooLong)?,
            ));
        }
        live.kill_begun = true;
        self.kills = self.kills.saturating_add(1);
        if live.script_pending {
            let _ = self.script.pop_front();
            live.script_pending = false;
        }
        live.reap_remaining = self.reap_cost;
        Ok(())
    }

    fn poll_reap(&mut self, handle: &CommandHandle) -> Result<ReapPoll, CallError> {
        let fault = self.reap_fault;
        let live = self.live.as_mut().ok_or(CallError::ScriptExhausted)?;
        if live.id != handle.id || !live.kill_begun {
            return Ok(ReapPoll::NotProof);
        }
        if let Some(done) = live.reap {
            return Ok(done);
        }
        if matches!(fault, Some(ReapFault::PendingForever)) {
            return Ok(ReapPoll::Pending);
        }
        if fault.is_some() {
            self.live = None;
            return Ok(ReapPoll::NotProof);
        }
        if !live.reap_remaining.is_zero() {
            let paid = charge_quantum(&self.clock, live.reap_remaining, live.end);
            live.reap_remaining = live.reap_remaining.saturating_sub(paid);
            if !live.reap_remaining.is_zero() && self.clock.now() < live.end {
                return Ok(ReapPoll::Pending);
            }
        }
        let id = live.id;
        let late = self.clock.now() >= live.end;
        self.live = None;
        if late {
            Ok(ReapPoll::NotProof)
        } else {
            Ok(ReapPoll::Exited { id })
        }
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
