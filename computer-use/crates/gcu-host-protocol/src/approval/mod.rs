//! App approval desk. A grant exists only after a real `host.approval.requestApp`.
//!
//! The binding is the request id, the App session, the OS image, the authenticated
//! engine instance, and the policy version shown to the user. Always is written
//! once, then `engine.policy.changed` is published. A missing request, a mismatched
//! binding, or a second decision fails. Closing a session denies its pending cards.
//!
//! Sensitive confirmation and handoff share that desk. Confirmation also binds the
//! snapshot and action digest. Handoff never grants and never clears Stop.

mod binding;
mod disk;
mod host;
mod office;
mod text;

pub use host::host_callbacks;
pub use office::{Approval, Office, Wait};
pub use text::sanitize_plain;

use crate::auth::ProcessInstance;
use crate::messages::{Decision, GrantProof, Platform, Risk, Signature};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Why a decision was refused. Display text is chosen by the App, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// No live approval has this request id.
    Absent,
    /// Session, image, engine instance, or policy version does not match.
    Identity,
    /// This request id was already consumed.
    Replay,
    /// Persistence or `engine.policy.changed` failed. The card stays pending.
    Failed,
    /// The confirm binding no longer matches the action shown to the user.
    Stale,
}

/// Why a real request was not shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindFault {
    /// No authenticated engine is attached.
    NoEngine,
    /// The OS image key or path is empty.
    Unverified,
    /// This request id was already used.
    Replay,
}

/// User act. Timeout is not a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Act {
    /// This turn only.
    Turn,
    /// Until the App session closes.
    Session,
    /// Persist, then publish a new policy version.
    Always,
    /// Refuse.
    Deny,
    /// The countdown elapsed.
    Timeout,
}

/// OS image the card is bound to. Name and icon are not part of this value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageBinding {
    /// Verifying platform.
    pub platform: Platform,
    /// Canonical authorization key.
    pub key: String,
    /// macOS team, when the OS reported one.
    pub team_id: Option<String>,
    /// Windows publisher, when the OS reported one.
    pub publisher: Option<String>,
    /// Lowercase hex digest, when the OS reported one.
    pub sha256_hex: Option<String>,
    /// OS verification result.
    pub signature: Signature,
    /// Canonical executable path.
    pub image_path: String,
    /// Running process. A reused pid with a new image does not match.
    pub pid: Option<u32>,
    /// §7.2 facts. Absent on a card that has not echoed them yet.
    #[serde(default, skip_serializing_if = "GrantProof::is_empty")]
    pub proof: GrantProof,
}

/// What the user presented. Every field must match the bound request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// JSON-RPC id of `host.approval.requestApp`.
    pub request_id: u64,
    /// App session that owns the card.
    pub session_id: String,
    /// OS image shown on the card.
    pub image: ImageBinding,
    /// Engine instance that asked.
    pub engine: ProcessInstance,
    /// Policy version shown on the card.
    pub policy_version: u64,
    /// The button or the countdown.
    pub act: Act,
}

/// Result delivered to the waiting Host callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A real decision. Timeout is not represented here.
    Decision(Decision),
    /// Reply with JSON-RPC `-32001` / `approval_timeout`.
    TimedOut,
    /// Sensitive-action confirmation. `allowed` is true only for an exact match.
    Confirmed {
        /// Whether the action may be delivered.
        allowed: bool,
        /// Why the card left the pending set.
        end: ConfirmEnd,
    },
    /// User takeover. Never a grant and never a cleared stop.
    HandedOff {
        /// Why the handoff card left the pending set.
        end: HandoffEnd,
    },
}

/// How a sensitive confirmation ended. Allowed is the only grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmEnd {
    /// The bound snapshot, digest, and policy version still matched.
    Allowed,
    /// The user or a closed session refused.
    Denied,
    /// The countdown elapsed. This is not a grant.
    TimedOut,
    /// The screen, digest, or policy version changed after the card was shown.
    Stale,
    /// The engine disconnected.
    Closed,
}

/// How a handoff ended. Neither variant approves an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffEnd {
    /// The user said they handled it. The caller may send a new observation.
    Handled,
    /// The session closed or the wait was dropped. Stop stays latched.
    Closed,
}

/// User act on a sensitive confirmation. Timeout is not a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmAct {
    /// Allow this exact action.
    Allow,
    /// Refuse.
    Deny,
    /// The countdown elapsed.
    Timeout,
}

/// User act on a handoff. There is no timeout grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffAct {
    /// The user handled the prompt. Do not replay the action.
    Handled,
}

/// Redacted crop the card may show. Missing and unlocated carry no pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum CropView {
    /// PNG bytes that passed the redaction checks.
    Redacted {
        /// Encoded media type. Only `image/png` is accepted.
        mime: String,
        /// Physical pixel width.
        width: u32,
        /// Physical pixel height.
        height: u32,
        /// PNG bytes. The UI checks the signature again before display.
        bytes: Vec<u8>,
    },
    /// The engine attached no crop.
    Missing,
    /// A crop arrived, but it was not a safe redacted PNG. Bytes are dropped.
    Unlocated,
}

/// What the user presented for one sensitive action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmClaim {
    /// JSON-RPC id of `host.approval.confirmAction`.
    pub request_id: u64,
    /// App session that owns the card.
    pub session_id: String,
    /// OS image shown on the card.
    pub image: ImageBinding,
    /// Engine instance that asked.
    pub engine: ProcessInstance,
    /// Policy version shown on the card.
    pub policy_version: u64,
    /// Snapshot shown on the card.
    pub snapshot_id: String,
    /// Digest shown on the card.
    pub action_digest: [u8; 32],
    /// The button or the countdown.
    pub act: ConfirmAct,
}

/// What the user presented for one handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffClaim {
    /// JSON-RPC id of `host.approval.handoff`.
    pub request_id: u64,
    /// App session that owns the card.
    pub session_id: String,
    /// OS image shown on the card.
    pub image: ImageBinding,
    /// Engine instance that asked.
    pub engine: ProcessInstance,
    /// Policy version shown on the card.
    pub policy_version: u64,
    /// The only handoff button.
    pub act: HandoffAct,
}

/// Sensitive-action card. `summary` is engine text, not a model reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmView {
    /// JSON-RPC id.
    pub request_id: u64,
    /// Owning App session.
    pub session_id: String,
    /// Authenticated engine pid.
    pub engine_pid: u32,
    /// Authenticated engine start token.
    pub engine_started: u64,
    /// Policy version captured when the request arrived.
    pub policy_version: u64,
    /// OS image. A later decision must repeat it.
    pub image: ImageBinding,
    /// Sanitized display name. Not an identity.
    pub name: String,
    /// Sanitized engine action text. Not model-provided.
    pub summary: String,
    /// Sanitized model reason. Empty: this wire message has none.
    pub reason: String,
    /// State shown to the user.
    pub snapshot_id: String,
    /// Lowercase hex digest of the exact action.
    pub action_digest: String,
    /// Redacted crop, or an explicit gap. Never an unredacted frame.
    pub crop: CropView,
    /// Countdown length. The clock starts at [`Self::opened_ms`].
    pub timeout_secs: u64,
    /// Wall-clock milliseconds when the request was bound.
    pub opened_ms: u64,
    /// An icon was attached. It is not verified.
    pub has_icon: bool,
}

/// Handoff card. It has no countdown and does not approve the action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffView {
    /// JSON-RPC id.
    pub request_id: u64,
    /// Owning App session.
    pub session_id: String,
    /// Authenticated engine pid.
    pub engine_pid: u32,
    /// Authenticated engine start token.
    pub engine_started: u64,
    /// Policy version captured when the request arrived.
    pub policy_version: u64,
    /// OS image. A later decision must repeat it.
    pub image: ImageBinding,
    /// Sanitized display name. Not an identity.
    pub name: String,
    /// `login`, `captcha`, `system_prompt`, or `other`.
    pub code: String,
    /// Sanitized engine detail for `other`. Empty for the known codes.
    pub detail: String,
    /// Wall-clock milliseconds when the request was bound.
    pub opened_ms: u64,
    /// An icon was attached. It is not verified.
    pub has_icon: bool,
}

/// How a settled card ended. The UI removes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Settled {
    /// Turn, session, or always was accepted.
    Grant,
    /// The user or a closed session refused.
    Deny,
    /// The countdown elapsed.
    Timeout,
    /// The engine disconnected.
    Closed,
}

/// Card the App may render. `name` and `reason` are already plain text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardView {
    /// JSON-RPC id.
    pub request_id: u64,
    /// Owning App session.
    pub session_id: String,
    /// Authenticated engine pid.
    pub engine_pid: u32,
    /// Authenticated engine start token.
    pub engine_started: u64,
    /// Policy version captured when the request arrived.
    pub policy_version: u64,
    /// OS image. This is what a later decision must repeat.
    pub image: ImageBinding,
    /// Sanitized display name. Not an identity.
    pub name: String,
    /// Sanitized model reason. Empty when the model sent none.
    pub reason: String,
    /// Engine risk.
    pub risk: Risk,
    /// Countdown length. The clock starts at [`Self::opened_ms`].
    pub timeout_secs: u64,
    /// Wall-clock milliseconds when the request was bound.
    pub opened_ms: u64,
    /// An icon was attached. It is not verified.
    pub has_icon: bool,
    /// When false the Always button must not grant.
    pub always_enabled: bool,
}

/// One UI notification. Pending cards survive a session switch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Notice {
    /// A real request is waiting.
    Pending {
        /// Card to render for its session. Boxed so a settled notice stays small.
        card: Box<CardView>,
    },
    /// The request was consumed.
    Settled {
        /// JSON-RPC id.
        request_id: u64,
        /// Owning session.
        session_id: String,
        /// Why it left the pending set.
        outcome: Settled,
    },
    /// A sensitive action is waiting.
    ConfirmPending {
        /// Card to render for its session.
        card: Box<ConfirmView>,
    },
    /// The confirmation was consumed.
    ConfirmSettled {
        /// JSON-RPC id.
        request_id: u64,
        /// Owning session.
        session_id: String,
        /// Why it left the pending set.
        end: ConfirmEnd,
    },
    /// The user must take over. This is not an approval.
    HandoffPending {
        /// Card to render for its session.
        card: Box<HandoffView>,
    },
    /// The handoff was consumed. Stop is unchanged.
    HandoffSettled {
        /// JSON-RPC id.
        request_id: u64,
        /// Owning session.
        session_id: String,
        /// Why it left the pending set.
        end: HandoffEnd,
    },
}

/// Default approval wait when the loaded policy says zero.
pub const MIN_TIMEOUT: Duration = Duration::from_secs(1);

pub(crate) fn timeout_secs(raw: u64) -> u64 {
    raw.max(MIN_TIMEOUT.as_secs())
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

/// Parse the hex digest shown on a confirm card. Bad hex is [`Fault::Stale`].
pub fn action_digest_from_hex(text: &str) -> Result<[u8; 32], Fault> {
    hex_decode_32(text).ok_or(Fault::Stale)
}

pub(crate) fn hex_decode_32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0_u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(out)
}
