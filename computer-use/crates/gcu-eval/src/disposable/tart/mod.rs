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
//!
//! [`mailbox`] is the offline S11-M3 codec, slot, and sequence layer.
//! [`MailboxGuestChannel`] speaks [`GuestChannel`] over those slots. It does
//! not start a VM, mount a volume, or read the environment. There is no
//! feature, environment variable, or flag that selects a real provider.

mod budgets;
mod command;
mod deadline;
mod guest;
mod identity;
mod ledger;
mod lock;
mod machine;
mod mailbox;
mod mailbox_guest;
mod manifest;
mod output;
mod parse;
mod policy;
mod receipt;
mod runner;
mod schedule;

pub use budgets::{BudgetError, BudgetField, PhaseBudgets};
pub use command::TartCommand;
pub use deadline::{
    AttemptDeadline, ClockError, CloneDeadline, DeadlineError, DeleteDeadline, FakeClock,
    GroupDeadline, HostClock, LeaseDeadline, LoginDeadline, MonoInstant, ProbeDeadline,
    QueryDeadline, RevokeDeadline, RunDeadline, SealDeadline, StageDeadline, StartDeadline,
    StopDeadline,
};
pub use guest::{
    ConsumeNote, FakeGuestChannel, GuestAccounting, GuestChannel, GuestCommand,
    GUEST_CHANNEL_VERSION,
};
pub use identity::{CloneId, GoldenImage, IdentityError, RunId, MAX_IDENTITY_LEN};
pub use ledger::{
    AttemptLedger, ClonePermit, FakeLedger, LedgerError, LedgerFact, LedgerFault, LedgerOp,
    Reservation, VmIdentity,
};
pub use lock::{CleanupLock, FakeCleanupLock, LockError};
pub use machine::{Attempt, CleanupDeadlines, ReserveInput, RuntimeLoss, StepError};
pub use mailbox::{
    decode_frame, decode_reply, encode_frame, encode_reply, poll_guest, prepare_slots, publish,
    read_checked, read_slot, Activity, AsciiId, AttemptBinding, AttemptNonce, Breach, CheckedRead,
    CodecFault, Envelope, FailedRead, GuestPoll, HostSlot, InventoryConsume, InventorySnapshot,
    Layout, LeaseRequest, LeaseTerminal, LoginState, MailboxError, MailboxReply, MsgStatus,
    Observe, Op, Outcome, Payload, ReplyResult, ReplyState, RevokeOutcome, RevokeSlot, RunScore,
    SeqFault, SeqTracker, SideOp, SideResult, SlotBody, SlotRead, SlotRef, SnapStatus,
    ValueOutcome, FRAME_MAX, GUEST_WRITE_DIR, HOST_WRITE_DIR, LEASE_WINDOW_MS, MAILBOX_DIR,
    MAX_JSON_ARRAY, MAX_JSON_DEPTH, MAX_POLLS,
};
pub use mailbox_guest::MailboxGuestChannel;
pub use manifest::{Digest, InstallManifest, ManifestError, DIGEST_LEN};
pub use output::{
    BoundedOutput, CallError, OutputError, ScriptOutcome, ScriptedReply, TraceEntry,
    MAX_OUTPUT_BYTES,
};
pub use policy::{PolicyError, SoftnetFlags, MAX_ALLOW_CIDRS, SOFTNET_BLOCK_CIDR};
pub use receipt::{AttemptOutcome, AttemptReceipt, NotRunReason, Phase, TaskScore};
pub use runner::{FakeTartRunner, TartRunner};
pub use schedule::{
    attempt_window_ms, due, prove, step_end, ScheduleError, ScheduleProof, ACCEPT_AGE_MS,
    EXTRA_SAMPLES, GRANT_GAP_MS, ONE_WAY_MS, POLL_BYTE_BUDGET, RENEW_MS, ROUND_TRIP_MAX_MS,
    STEP_MS,
};
