//! Closed mailbox messages. Strings are enums or short ASCII identities.

use serde::{Deserialize, Serialize};

use super::super::guest::GUEST_CHANNEL_VERSION;
use super::super::manifest::Digest;
use super::ids::{AsciiId, AttemptNonce};
use super::CodecFault;
use crate::disposable::{Check, Probe};

/// Largest frame, including the 4-byte length prefix.
pub const FRAME_MAX: usize = 4096;

/// JSON containers may nest this deep, counting the outermost object.
pub const MAX_JSON_DEPTH: usize = 3;

/// One JSON array may contain this many elements.
pub const MAX_JSON_ARRAY: usize = 16;

/// Inventory and lease requests allowed in one attempt.
pub const MAX_POLLS: u64 = 2048;

/// Guest lease window. `until_ms` is `sample_ms` plus this, with no wrap.
pub const LEASE_WINDOW_MS: u64 = 2000;

/// The seven guest operations. Names are the file stems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Startup and sampling snapshot.
    Inventory,
    /// One closed probe.
    Probe,
    /// Fixed bootstrap.
    Bootstrap,
    /// Fixed login metadata.
    Login,
    /// Fixed preinstalled task.
    Run,
    /// One lease grant.
    Lease,
    /// Revoke.
    Revoke,
}

impl Op {
    /// Guest and host message file.
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Inventory => "inventory.msg",
            Self::Probe => "probe.msg",
            Self::Bootstrap => "bootstrap.msg",
            Self::Login => "login.msg",
            Self::Run => "run.msg",
            Self::Lease => "lease.msg",
            Self::Revoke => "revoke.msg",
        }
    }

    /// Staging name beside the message.
    #[must_use]
    pub const fn tmp_name(self) -> &'static str {
        match self {
            Self::Inventory => "inventory.msg.tmp",
            Self::Probe => "probe.msg.tmp",
            Self::Bootstrap => "bootstrap.msg.tmp",
            Self::Login => "login.msg.tmp",
            Self::Run => "run.msg.tmp",
            Self::Lease => "lease.msg.tmp",
            Self::Revoke => "revoke.msg.tmp",
        }
    }

    /// Slot index. Inventory is 0.
    pub(super) const fn index(self) -> usize {
        match self {
            Self::Inventory => 0,
            Self::Probe => 1,
            Self::Bootstrap => 2,
            Self::Login => 3,
            Self::Run => 4,
            Self::Lease => 5,
            Self::Revoke => 6,
        }
    }
}

/// Wire status. Pending is local and is not on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MsgStatus {
    /// Host to guest.
    Request,
    /// Guest accepted the ordinary op. Not completion.
    Accepted,
    /// Guest finished this slot's message.
    Terminal,
}

/// `outcome` on bootstrap, login, run, and revoke terminals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The value fields are present.
    Ok,
    /// Value fields are null.
    Failed,
    /// Value fields are null.
    Unknown,
    /// Value fields are null. Includes revoke interrupting an ordinary op.
    Cancelled,
}

/// Ordinary side effect named by an inventory snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideOp {
    /// Bootstrap.
    Bootstrap,
    /// Login.
    Login,
    /// Run.
    Run,
}

/// `none`, `pending`, or `terminal` inside an inventory snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapStatus {
    /// No row. Sequence is 0 and the result is null.
    None,
    /// Accepted or still running. The result is null.
    Pending,
    /// Finished. The result uses that op's terminal schema.
    Terminal,
}

/// Login terminal value when `outcome` is `ok`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginState {
    /// The fixed login metadata is present.
    Ready,
    /// The fixed login metadata is absent.
    Missing,
}

/// Run terminal value when `outcome` is `ok`. Provisional; not a sealed score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunScore {
    /// Pass.
    Pass,
    /// Fail.
    Fail,
    /// Partial.
    Partial,
    /// Timeout.
    Timeout,
    /// Crash.
    Crash,
}

/// A terminal value that is present only when [`Outcome::Ok`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueOutcome<T> {
    /// Closed outcome.
    pub outcome: Outcome,
    /// Set only for [`Outcome::Ok`].
    pub value: Option<T>,
}

impl<T> ValueOutcome<T> {
    /// Reject `ok` without a value and a value with any other outcome.
    pub fn new(outcome: Outcome, value: Option<T>) -> Result<Self, CodecFault> {
        let built = Self { outcome, value };
        built.validate()?;
        Ok(built)
    }

    pub(super) fn validate(&self) -> Result<(), CodecFault> {
        let ok = matches!(
            (self.outcome, self.value.is_some()),
            (Outcome::Ok, true)
                | (
                    Outcome::Failed | Outcome::Unknown | Outcome::Cancelled,
                    false
                )
        );
        if ok {
            Ok(())
        } else {
            Err(CodecFault::Shape)
        }
    }
}

/// Revoke terminal. All three flags are true only when `outcome` is `ok`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevokeOutcome {
    /// Closed outcome.
    pub outcome: Outcome,
    /// `scope_revoked`, `input_released`, and `tree_exited` are all true.
    pub done: bool,
}

impl RevokeOutcome {
    /// `flags` is `Some([true, true, true])` only together with [`Outcome::Ok`].
    pub fn new(outcome: Outcome, flags: Option<[bool; 3]>) -> Result<Self, CodecFault> {
        let done = match (outcome, flags) {
            (Outcome::Ok, Some([true, true, true])) => true,
            (Outcome::Failed | Outcome::Unknown | Outcome::Cancelled, None) => false,
            _ => return Err(CodecFault::Shape),
        };
        Ok(Self { outcome, done })
    }
}

/// One ordinary row of an inventory snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    /// `none`, `pending`, or `terminal`.
    pub status: SnapStatus,
    /// Null when [`SnapStatus::None`]. Otherwise bootstrap, login, or run.
    pub op: Option<SideOp>,
    /// Zero when there is no row.
    pub request_seq: u64,
    /// Set only for [`SnapStatus::Terminal`].
    pub result: Option<SideResult>,
}

/// Terminal body of the ordinary row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideResult {
    /// Bootstrap terminal.
    Bootstrap(ValueOutcome<u64>),
    /// Login terminal.
    Login(ValueOutcome<LoginState>),
    /// Run terminal.
    Run(ValueOutcome<RunScore>),
}

impl Activity {
    /// Enforce none/pending/terminal pairing.
    pub fn new(
        status: SnapStatus,
        op: Option<SideOp>,
        request_seq: u64,
        result: Option<SideResult>,
    ) -> Result<Self, CodecFault> {
        let built = Self {
            status,
            op,
            request_seq,
            result,
        };
        built.validate()?;
        Ok(built)
    }

    pub(super) fn validate(&self) -> Result<(), CodecFault> {
        let paired = match self.status {
            SnapStatus::None => self.op.is_none() && self.request_seq == 0 && self.result.is_none(),
            SnapStatus::Pending => {
                self.op.is_some() && self.request_seq >= 1 && self.result.is_none()
            }
            SnapStatus::Terminal => self.request_seq >= 1 && side_matches(self.op, &self.result),
        };
        if !paired {
            return Err(CodecFault::Shape);
        }
        if let Some(result) = &self.result {
            result.validate()?;
        }
        Ok(())
    }
}

fn side_matches(op: Option<SideOp>, result: &Option<SideResult>) -> bool {
    matches!(
        (op, result),
        (Some(SideOp::Bootstrap), Some(SideResult::Bootstrap(_)))
            | (Some(SideOp::Login), Some(SideResult::Login(_)))
            | (Some(SideOp::Run), Some(SideResult::Run(_)))
    )
}

impl SideResult {
    fn validate(&self) -> Result<(), CodecFault> {
        match self {
            Self::Bootstrap(value) => value.validate(),
            Self::Login(value) => value.validate(),
            Self::Run(value) => value.validate(),
        }
    }
}

/// The revoke row of an inventory snapshot. Separate from the ordinary row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevokeSlot {
    /// `none`, `pending`, or `terminal`.
    pub status: SnapStatus,
    /// Zero when there is no revoke row.
    pub request_seq: u64,
    /// Set only for [`SnapStatus::Terminal`].
    pub result: Option<RevokeOutcome>,
}

impl RevokeSlot {
    /// Enforce none/pending/terminal pairing.
    pub fn new(
        status: SnapStatus,
        request_seq: u64,
        result: Option<RevokeOutcome>,
    ) -> Result<Self, CodecFault> {
        let built = Self {
            status,
            request_seq,
            result,
        };
        built.validate()?;
        Ok(built)
    }

    pub(super) fn validate(&self) -> Result<(), CodecFault> {
        let ok = match self.status {
            SnapStatus::None => self.request_seq == 0 && self.result.is_none(),
            SnapStatus::Pending => self.request_seq >= 1 && self.result.is_none(),
            SnapStatus::Terminal => self.request_seq >= 1 && self.result.is_some(),
        };
        if ok {
            Ok(())
        } else {
            Err(CodecFault::Shape)
        }
    }
}

/// Inventory request. Null op and sequence 0 mean "do not consume".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryConsume {
    /// Ordinary op to consume, or null.
    pub op: Option<SideOp>,
    /// Zero when `op` is null.
    pub request_seq: u64,
    /// Zero when the revoke row is not consumed.
    pub revoke_seq: u64,
}

impl InventoryConsume {
    /// Reject an op without a sequence, and a sequence without an op.
    pub fn new(op: Option<SideOp>, request_seq: u64, revoke_seq: u64) -> Result<Self, CodecFault> {
        let ordinary = match op {
            None => request_seq == 0,
            Some(_) => request_seq >= 1,
        };
        if !ordinary {
            return Err(CodecFault::Shape);
        }
        Ok(Self {
            op,
            request_seq,
            revoke_seq,
        })
    }
}

/// Fields the host stores for one sent lease and requires back on the ACK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseRequest {
    /// Inventory request the sample came from.
    pub inventory_request_seq: u64,
    /// Guest sample, in monotonic milliseconds.
    pub sample_ms: u64,
}

/// Lease terminal. `until_ms` must be the sent `sample_ms + 2000`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseTerminal {
    /// Echo of the grant.
    pub inventory_request_seq: u64,
    /// Echo of the grant.
    pub sample_ms: u64,
    /// Echo of the host-computed deadline.
    pub until_ms: u64,
}

/// Inventory terminal snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventorySnapshot {
    /// Guest uid.
    pub uid: u32,
    /// Desktop identity.
    pub desktop: AsciiId,
    /// Config identity token, not a path.
    pub config: AsciiId,
    /// Sample time on the guest boot clock.
    pub sample_ms: u64,
    /// The one ordinary side effect, if any.
    pub active: Activity,
    /// The revoke row.
    pub revoke: RevokeSlot,
}

impl InventorySnapshot {
    pub(super) fn validate(&self) -> Result<(), CodecFault> {
        self.active.validate()?;
        self.revoke.validate()
    }
}

/// Closed payload for one op and status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// Inventory request.
    InventoryRequest(InventoryConsume),
    /// Inventory terminal snapshot.
    InventoryTerminal(InventorySnapshot),
    /// Probe request.
    ProbeRequest(Probe),
    /// Probe terminal. There is no accepted state.
    ProbeTerminal(Check),
    /// Empty object for bootstrap, login, run, and revoke request or accepted.
    Empty,
    /// Bootstrap terminal.
    BootstrapTerminal(ValueOutcome<u64>),
    /// Login terminal.
    LoginTerminal(ValueOutcome<LoginState>),
    /// Run terminal.
    RunTerminal(ValueOutcome<RunScore>),
    /// Lease request. `until_ms` is not on this request.
    LeaseRequest(LeaseRequest),
    /// Lease terminal.
    LeaseTerminal(LeaseTerminal),
    /// Revoke terminal.
    RevokeTerminal(RevokeOutcome),
}

/// One framed mailbox message before it is normalized for the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// [`GUEST_CHANNEL_VERSION`].
    pub v: u16,
    /// Run identity.
    pub run_id: AsciiId,
    /// Public attempt nonce.
    pub attempt_nonce: AttemptNonce,
    /// Pinned VM config digest.
    pub vm_config_digest: Digest,
    /// Null only on the first inventory request.
    pub boot_id: Option<AsciiId>,
    /// Which slot this message belongs to.
    pub op: Op,
    /// Per-op sequence.
    pub seq: u64,
    /// Echo of the request sequence. Equal to `seq` on a request.
    pub request_seq: u64,
    /// Request, accepted, or terminal.
    pub status: MsgStatus,
    /// Closed body.
    pub payload: Payload,
}

impl Envelope {
    /// Reject a status/payload pair this op does not have.
    pub fn validate(&self) -> Result<(), CodecFault> {
        if self.v != GUEST_CHANNEL_VERSION {
            return Err(CodecFault::Version);
        }
        let matched = matches!(
            (self.op, self.status, &self.payload),
            (
                Op::Inventory,
                MsgStatus::Request,
                Payload::InventoryRequest(_)
            ) | (
                Op::Inventory,
                MsgStatus::Terminal,
                Payload::InventoryTerminal(_)
            ) | (Op::Probe, MsgStatus::Request, Payload::ProbeRequest(_))
                | (Op::Probe, MsgStatus::Terminal, Payload::ProbeTerminal(_))
                | (
                    Op::Bootstrap | Op::Login | Op::Run | Op::Revoke,
                    MsgStatus::Request | MsgStatus::Accepted,
                    Payload::Empty,
                )
                | (
                    Op::Bootstrap,
                    MsgStatus::Terminal,
                    Payload::BootstrapTerminal(_)
                )
                | (Op::Login, MsgStatus::Terminal, Payload::LoginTerminal(_))
                | (Op::Run, MsgStatus::Terminal, Payload::RunTerminal(_))
                | (Op::Revoke, MsgStatus::Terminal, Payload::RevokeTerminal(_))
                | (Op::Lease, MsgStatus::Request, Payload::LeaseRequest(_))
                | (Op::Lease, MsgStatus::Terminal, Payload::LeaseTerminal(_))
        );
        if !matched {
            return Err(CodecFault::Shape);
        }
        match &self.payload {
            Payload::InventoryTerminal(snapshot) => snapshot.validate(),
            Payload::BootstrapTerminal(value) => value.validate(),
            Payload::LoginTerminal(value) => value.validate(),
            Payload::RunTerminal(value) => value.validate(),
            Payload::InventoryRequest(_)
            | Payload::ProbeRequest(_)
            | Payload::ProbeTerminal(_)
            | Payload::Empty
            | Payload::LeaseRequest(_)
            | Payload::LeaseTerminal(_)
            | Payload::RevokeTerminal(_) => Ok(()),
        }
    }
}

/// Caller-facing state. Pending is local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyState {
    /// The slot is empty, unchanged, or not the current reply yet.
    Pending,
    /// The guest accepted the op.
    Accepted,
    /// The guest finished the op's own slot message.
    Terminal,
}

/// Normalized result carried in [`MailboxReply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyResult {
    /// JSON null. Only with [`ReplyState::Pending`].
    Pending,
    /// JSON `{}`. Only with [`ReplyState::Accepted`].
    Empty,
    /// Inventory snapshot.
    Inventory(InventorySnapshot),
    /// Probe check.
    Probe(Check),
    /// Bootstrap generation.
    Bootstrap(ValueOutcome<u64>),
    /// Login metadata.
    Login(ValueOutcome<LoginState>),
    /// Provisional run score.
    Run(ValueOutcome<RunScore>),
    /// Confirmed lease grant.
    Lease(LeaseTerminal),
    /// Revoke flags.
    Revoke(RevokeOutcome),
}

/// Normalized body M4 stores in [`super::super::output::BoundedOutput`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxReply {
    /// Pending, accepted, or terminal.
    pub state: ReplyState,
    /// Slot this reply belongs to.
    pub op: Op,
    /// Request this reply answers. Zero when nothing has been sent.
    pub request_seq: u64,
    /// Closed result for `op` and `state`.
    pub result: ReplyResult,
}

impl MailboxReply {
    /// Local pending reply. Not an error.
    #[must_use]
    pub fn pending(op: Op, request_seq: u64) -> Self {
        Self {
            state: ReplyState::Pending,
            op,
            request_seq,
            result: ReplyResult::Pending,
        }
    }

    pub(super) fn validate(&self) -> Result<(), CodecFault> {
        let ok = match (&self.state, &self.result) {
            (ReplyState::Pending, ReplyResult::Pending) => true,
            (ReplyState::Accepted, ReplyResult::Empty)
                if matches!(self.op, Op::Bootstrap | Op::Login | Op::Run | Op::Revoke) =>
            {
                true
            }
            (ReplyState::Terminal, ReplyResult::Inventory(_)) => self.op == Op::Inventory,
            (ReplyState::Terminal, ReplyResult::Probe(_)) => self.op == Op::Probe,
            (ReplyState::Terminal, ReplyResult::Bootstrap(value)) => {
                self.op == Op::Bootstrap && value.validate().is_ok()
            }
            (ReplyState::Terminal, ReplyResult::Login(value)) => {
                self.op == Op::Login && value.validate().is_ok()
            }
            (ReplyState::Terminal, ReplyResult::Run(value)) => {
                self.op == Op::Run && value.validate().is_ok()
            }
            (ReplyState::Terminal, ReplyResult::Lease(_)) => self.op == Op::Lease,
            (ReplyState::Terminal, ReplyResult::Revoke(_)) => self.op == Op::Revoke,
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            Err(CodecFault::Shape)
        }
    }
}
