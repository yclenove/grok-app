//! Offline Tart interface and caller-driven fake state machine (ADR 0018).
//!
//! S11a types describe fixed operations, an explicit deadline, and a bounded
//! body. S11b adds a synchronous attempt machine that runs only against those
//! fakes. Nothing here starts Tart, opens a real guest channel, admits a
//! launch, or reads the environment. H1 (the host-to-guest channel) and H2
//! (the egress guard) stay undecided until S11-0 measures them.
//!
//! Fakes record structured calls and return scripted results. They do not
//! sleep, take a blocking lock, or start a process. A real adapter is not
//! compiled here, and there is no feature, environment variable, or flag that
//! turns one on. Dropping an attempt does not stop, delete, or seal.

mod budgets;
mod command;
mod deadline;
mod guest;
mod identity;
mod ledger;
mod lock;
mod machine;
mod manifest;
mod output;
mod parse;
mod policy;
mod receipt;
mod runner;

pub use budgets::{BudgetError, BudgetField, PhaseBudgets};
pub use command::TartCommand;
pub use deadline::{
    AttemptDeadline, ClockError, CloneDeadline, DeadlineError, DeleteDeadline, FakeClock,
    GroupDeadline, HostClock, LeaseDeadline, LoginDeadline, MonoInstant, ProbeDeadline,
    QueryDeadline, RevokeDeadline, RunDeadline, SealDeadline, StageDeadline, StartDeadline,
    StopDeadline,
};
pub use guest::{FakeGuestChannel, GuestChannel, GuestCommand, GUEST_CHANNEL_VERSION};
pub use identity::{CloneId, GoldenImage, IdentityError, RunId, MAX_IDENTITY_LEN};
pub use ledger::{
    AttemptLedger, ClonePermit, FakeLedger, LedgerError, LedgerFact, LedgerFault, Reservation,
    VmIdentity,
};
pub use lock::{CleanupLock, FakeCleanupLock, LockError};
pub use machine::{Attempt, CleanupDeadlines, ReserveInput, RuntimeLoss, StepError};
pub use manifest::{Digest, InstallManifest, ManifestError, DIGEST_LEN};
pub use output::{
    BoundedOutput, CallError, OutputError, ScriptOutcome, ScriptedReply, TraceEntry,
    MAX_OUTPUT_BYTES,
};
pub use policy::{PolicyError, SoftnetFlags, MAX_ALLOW_CIDRS, SOFTNET_BLOCK_CIDR};
pub use receipt::{AttemptOutcome, AttemptReceipt, NotRunReason, Phase, TaskScore};
pub use runner::{FakeTartRunner, TartRunner};
