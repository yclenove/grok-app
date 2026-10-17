//! App approval desk. A grant exists only after a real `host.approval.requestApp`.
//!
//! The binding is the request id, the App session, the OS image, the authenticated
//! engine instance, and the policy version shown to the user. Always is written
//! once, then `engine.policy.changed` is published. A missing request, a mismatched
//! binding, or a second decision fails. Closing a session denies its pending cards.

mod binding;
mod disk;
mod host;
mod office;
mod text;

pub use host::host_callbacks;
pub use office::Office;
pub use text::sanitize_plain;

use crate::auth::ProcessInstance;
use crate::messages::{Decision, Platform, Risk, Signature};
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
