//! S11a offline Tart interface (ADR 0018 §8, §9, §11).
//!
//! The types describe fixed operations, an explicit deadline, and a bounded
//! body. They do not start Tart, open a guest channel, admit a launch, or
//! read the environment. H1 (the host-to-guest channel) and H2 (the egress
//! guard) stay undecided until S11-0 measures them. The state machine is S11b.
//!
//! Fakes record structured calls and return scripted results. They do not
//! sleep, take a lock, or start a process. A real adapter is not compiled here,
//! and there is no feature, environment variable, or flag that turns one on.

mod command;
mod deadline;
mod guest;
mod identity;
mod ledger;
mod manifest;
mod output;
mod policy;
mod runner;

pub use command::TartCommand;
pub use deadline::{
    AttemptDeadline, ClockError, CloneDeadline, DeadlineError, DeleteDeadline, FakeClock,
    HostClock, LeaseDeadline, LoginDeadline, MonoInstant, ProbeDeadline, QueryDeadline,
    RevokeDeadline, RunDeadline, SealDeadline, StageDeadline, StartDeadline, StopDeadline,
};
pub use guest::{FakeGuestChannel, GuestChannel, GuestCommand, GUEST_CHANNEL_VERSION};
pub use identity::{CloneId, GoldenImage, IdentityError, RunId, MAX_IDENTITY_LEN};
pub use ledger::{AttemptLedger, ClonePermit, FakeLedger, LedgerError, LedgerFact, Reservation};
pub use manifest::{Digest, InstallManifest, ManifestError, DIGEST_LEN};
pub use output::{
    BoundedOutput, CallError, OutputError, ScriptOutcome, ScriptedReply, TraceEntry,
    MAX_OUTPUT_BYTES,
};
pub use policy::{PolicyError, SoftnetFlags, MAX_ALLOW_CIDRS, SOFTNET_BLOCK_CIDR};
pub use runner::{FakeTartRunner, TartRunner};
