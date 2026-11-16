//! One caller step and the value it may finish.
//!
//! A step is 100ms on the monotonic clock. `Pending` is not a finished phase.
//! `Done` moves the finished value once.

use super::super::deadline::MonoInstant;
use super::super::identity::{CloneId, RunId};
use super::super::receipt::AttemptReceipt;
use super::super::runner::CommandHandle;

/// Outcome of one public step.
#[derive(Debug)]
pub enum StepProgress<T> {
    /// The cursor is still open. This is not phase completion.
    Pending,
    /// The phase finished. The value is delivered once.
    Done(T),
}

/// Closed set of phase completions. Each entry returns only its own variant.
#[derive(Debug)]
pub enum CompletedStep {
    /// The reservation and its only permit are stored.
    Reserved,
    /// Clone was issued and the incarnation matched.
    Cloned,
    /// Start was issued.
    Started,
    /// Inventory and every probe were recorded.
    Isolated,
    /// Bootstrap evidence was recorded.
    Bootstrapped,
    /// Login evidence was recorded.
    LoggedIn,
    /// Evidence is complete for the fake run.
    Ready,
    /// The fake run body was recorded.
    Ran,
    /// Revoke was attempted.
    Revoked,
    /// Stop and delete finished, or absence was already true.
    Destroyed,
    /// The caller cancelled.
    Cancelled,
    /// Provider EOF was recorded.
    ProviderCrashNoted,
    /// Guest, Tart, or Softnet loss was recorded.
    RuntimeLossNoted,
    /// The owner lease call finished.
    OwnerLeaseObserved,
    /// The receipt was sealed.
    Sealed(AttemptReceipt),
    /// Recovery finished every unsealed row it could.
    Recovered(Vec<AttemptReceipt>),
}

/// Clock interval of one public entry or poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepSpan {
    /// Instant when the entry or poll began.
    pub start: MonoInstant,
    /// Instant when it returned. An injected jump can pass `start + 100ms`.
    pub end: MonoInstant,
}

impl StepSpan {
    /// Elapsed time. Zero when the clock did not move.
    #[must_use]
    pub fn elapsed(self) -> std::time::Duration {
        self.end.saturating_duration_since(self.start)
    }
}

/// How a command left the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchState {
    /// Refused before any handle or guest slot existed.
    NotDispatched,
    /// A handle or guest slot was opened.
    Issued,
    /// The outcome cannot be placed.
    Unknown,
}

/// One recorded effect. Sticky unknown is separate and is not cleared by cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRecord {
    /// Short operation name.
    pub op: &'static str,
    /// Dispatch evidence.
    pub dispatch: DispatchState,
    /// A terminal result was accepted.
    pub resolved: bool,
}

/// Exact ledger identity for one reclaim row. Not an index and not a prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowKey {
    /// Run id on the row.
    pub run: RunId,
    /// Clone id on the row.
    pub clone_id: CloneId,
    /// Absolute instant stored at reserve.
    pub absolute: MonoInstant,
    /// Expected incarnation, when the row has one.
    pub incarnation: Option<String>,
    /// Expected config, when the row has one.
    pub config: Option<String>,
    /// Start was issued for this row.
    pub started: bool,
}

/// In-flight Tart command, when one is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveCommand {
    /// Runner handle.
    pub handle: CommandHandle,
}
