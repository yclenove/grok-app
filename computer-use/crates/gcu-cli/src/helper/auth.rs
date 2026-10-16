//! Pure helper admission. No sockets, signatures APIs, or process control.
//!
//! A peer is served only when the endpoint is still the bound socket, the UID
//! matches, and the code signature has the same signing identity and cdhash.
//! The same identity with a different cdhash may only ask the helper to exit.
//! UID checks and token files are not substitutes for this decision.

use std::time::Duration;

/// How long a helper with no connections stays up.
pub const IDLE_LIMIT: Duration = Duration::from_secs(10 * 60);
/// Audit and handshake ceiling. Callers must not shorten this for a real helper.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Release bundle id. Developer ID signing is a later phase; the id is stable.
pub const RELEASE_BUNDLE_ID: &str = "ai.grok.computer-use.helper";
/// Dev bundle id. Ad-hoc signing uses an identifier-only designated requirement.
pub const DEV_BUNDLE_ID: &str = "ai.grok.computer-use.helper.dev";
/// Release app name shown by TCC.
pub const RELEASE_APP_NAME: &str = "Grok Computer Use";
/// Dev app name. The `(Dev)` suffix keeps it distinct from a signed release app.
pub const DEV_APP_NAME: &str = "Grok Computer Use (Dev)";

/// Bundle id of this build.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn current_bundle_id() -> &'static str {
    if cfg!(debug_assertions) {
        DEV_BUNDLE_ID
    } else {
        RELEASE_BUNDLE_ID
    }
}

/// App name of this build.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn current_app_name() -> &'static str {
    if cfg!(debug_assertions) {
        DEV_APP_NAME
    } else {
        RELEASE_APP_NAME
    }
}

/// Bundle id selected by `debug`.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn bundle_id(debug: bool) -> &'static str {
    if debug {
        DEV_BUNDLE_ID
    } else {
        RELEASE_BUNDLE_ID
    }
}

/// App name selected by `debug`.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn app_name(debug: bool) -> &'static str {
    if debug {
        DEV_APP_NAME
    } else {
        RELEASE_APP_NAME
    }
}

/// Signing identity: codesign identifier plus optional Team ID.
///
/// `None` and `Some` team ids never match. Ad-hoc dev signatures have no team.
#[derive(Clone, Debug)]
pub struct SigningIdentity {
    identifier: String,
    team_id: Option<String>,
}

/// Code signature facts the audit trait observed. No path is stored.
#[derive(Clone, Debug)]
pub struct CodeSignature {
    identity: SigningIdentity,
    cdhash: String,
}

impl CodeSignature {
    /// Build a signature from already-read facts.
    ///
    /// Returns `None` when the identifier, team id, or cdhash is not usable.
    /// Hex is stored lowercase. Blocking: none. Timeout: none. Cancellation:
    /// none. Thread safety: pure.
    #[must_use]
    pub fn new(identifier: &str, team_id: Option<&str>, cdhash: &str) -> Option<Self> {
        if !valid_identifier(identifier) {
            return None;
        }
        let team_id = match team_id {
            None => None,
            Some(team) if valid_team_id(team) => Some(team.to_string()),
            Some(_) => return None,
        };
        let cdhash = normalize_cdhash(cdhash)?;
        Some(Self {
            identity: SigningIdentity {
                identifier: identifier.to_string(),
                team_id,
            },
            cdhash,
        })
    }

    /// Codesign identifier.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: shared
    /// references are enough.
    #[must_use]
    pub fn identifier(&self) -> &str {
        &self.identity.identifier
    }

    /// Team ID, when the signature has one.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: shared
    /// references are enough.
    #[must_use]
    pub fn team_id(&self) -> Option<&str> {
        self.identity.team_id.as_deref()
    }

    /// Lowercase cdhash hex.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: shared
    /// references are enough.
    #[must_use]
    pub fn cdhash(&self) -> &str {
        &self.cdhash
    }

    /// Same identifier and team id. Byte compare is constant time for equal lengths.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
    #[must_use]
    pub fn same_identity(&self, other: &Self) -> bool {
        self.identity.identifier.len() == other.identity.identifier.len()
            && constant_time_eq(
                self.identity.identifier.as_bytes(),
                other.identity.identifier.as_bytes(),
            )
            && option_eq(&self.identity.team_id, &other.identity.team_id)
    }

    /// Same cdhash. Constant time when both sides are the same length.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
    #[must_use]
    pub fn same_cdhash(&self, other: &Self) -> bool {
        self.cdhash.len() == other.cdhash.len()
            && constant_time_eq(self.cdhash.as_bytes(), other.cdhash.as_bytes())
    }
}

/// `stat` facts for one directory entry. Mode is masked to permission bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndpointStamp {
    /// Device id.
    pub dev: u64,
    /// Inode.
    pub ino: u64,
    /// Owner uid.
    pub uid: u32,
    /// Permission bits (`0o777`).
    pub mode: u32,
    /// The entry is a socket.
    pub socket: bool,
    /// The entry is a directory.
    pub directory: bool,
    /// The entry is a symlink. Symlinks are never private.
    pub symlink: bool,
}

/// Why a peer or endpoint is not served.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// Symlink, wrong owner, wrong mode, or not a socket/directory.
    EndpointForeign,
    /// Device or inode changed after the socket was bound.
    EndpointReplaced,
    /// Peer uid is not the helper uid. Checked before the signature.
    UidMismatch,
    /// Same uid and no code signature. This is impersonation, not a session.
    UnsignedPeer,
    /// Same uid, signature present, signing identity differs. Do not signal it.
    SigningIdentityMismatch,
}

/// What the helper may do with one peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    /// Same uid, identity, and cdhash. Serve one MCP session.
    Serve,
    /// Same identity, different cdhash. Accept only a shutdown frame.
    Replace,
    /// Close. No session and no shutdown of this process.
    Reject(RejectReason),
}

impl Admit {
    /// Whether this decision may carry MCP.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
    #[must_use]
    pub fn serves_mcp(self) -> bool {
        matches!(self, Self::Serve)
    }
}

/// Facts needed to admit one peer. The caller has already read them.
pub struct AuthInput<'a> {
    /// Uid of this helper.
    pub local_uid: u32,
    /// This helper's own signature.
    pub local: &'a CodeSignature,
    /// Peer uid from the audit token.
    pub peer_uid: u32,
    /// Peer signature. `None` means unsigned or unreadable.
    pub peer: Option<&'a CodeSignature>,
    /// Stamp captured when the socket was bound.
    pub bound: EndpointStamp,
    /// Stamp captured after connect, before any frame.
    pub observed: EndpointStamp,
    /// Directory stamp. `None` skips the directory check.
    pub directory: Option<EndpointStamp>,
}

/// Admit `input`.
///
/// Endpoint checks run first, then uid, then signature. A cdhash mismatch with
/// the same signing identity is [`Admit::Replace`], not a session. Blocking:
/// none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn admit(input: AuthInput<'_>) -> Admit {
    if let Some(directory) = input.directory {
        if !directory_is_private(directory, input.local_uid) {
            return Admit::Reject(RejectReason::EndpointForeign);
        }
    }
    if !socket_is_private(input.observed, input.local_uid) {
        return Admit::Reject(RejectReason::EndpointForeign);
    }
    if input.bound.dev != input.observed.dev || input.bound.ino != input.observed.ino {
        return Admit::Reject(RejectReason::EndpointReplaced);
    }
    if input.peer_uid != input.local_uid {
        return Admit::Reject(RejectReason::UidMismatch);
    }
    let Some(peer) = input.peer else {
        return Admit::Reject(RejectReason::UnsignedPeer);
    };
    if !input.local.same_identity(peer) {
        return Admit::Reject(RejectReason::SigningIdentityMismatch);
    }
    if !input.local.same_cdhash(peer) {
        return Admit::Replace;
    }
    Admit::Serve
}

/// Whether a socket stamp is a private socket owned by `uid`.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn socket_is_private(stamp: EndpointStamp, uid: u32) -> bool {
    stamp.socket && !stamp.symlink && !stamp.directory && stamp.uid == uid && stamp.mode == 0o600
}

/// Whether a directory stamp is a private directory owned by `uid`.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn directory_is_private(stamp: EndpointStamp, uid: u32) -> bool {
    stamp.directory && !stamp.symlink && !stamp.socket && stamp.uid == uid && stamp.mode == 0o700
}

/// Exact non-empty version match. Empty strings never match.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn versions_match(local: &str, peer: &str) -> bool {
    valid_version(local)
        && local.len() == peer.len()
        && constant_time_eq(local.as_bytes(), peer.as_bytes())
}

/// A version token that may appear in a hello.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'"' && byte != b'\\')
}

/// Turn-idle range accepted in a hello, in seconds.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn valid_idle_secs(seconds: u64) -> bool {
    (1..=86_400).contains(&seconds)
}

/// Idle clock for the listener. Time advances only while nothing is connected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdleClock {
    connections: u32,
    idle_for: Duration,
    limit: Duration,
}

impl IdleClock {
    /// No connections and no elapsed idle time. `limit` is the idle exit threshold.
    ///
    /// Production uses [`IDLE_LIMIT`]. Blocking: none. Timeout: none.
    /// Cancellation: none. Thread safety: the value is not shared.
    #[must_use]
    pub fn new(limit: Duration) -> Self {
        Self {
            connections: 0,
            idle_for: Duration::ZERO,
            limit,
        }
    }

    /// A connection began. The idle window starts over.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: `&mut self`.
    pub fn connect(&mut self) {
        self.connections = self.connections.saturating_add(1);
        self.idle_for = Duration::ZERO;
    }

    /// A connection ended. The idle window starts over at zero connections.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: `&mut self`.
    pub fn disconnect(&mut self) {
        self.connections = self.connections.saturating_sub(1);
        if self.connections == 0 {
            self.idle_for = Duration::ZERO;
        }
    }

    /// Advance by `elapsed`. Returns whether the helper should exit.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: `&mut self`.
    pub fn tick(&mut self, elapsed: Duration) -> bool {
        if self.connections == 0 {
            self.idle_for = self.idle_for.saturating_add(elapsed);
        }
        self.connections == 0 && self.idle_for >= self.limit
    }

    /// Open connections.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: `&self`.
    #[must_use]
    pub fn connections(&self) -> u32 {
        self.connections
    }
}

impl Default for IdleClock {
    fn default() -> Self {
        Self::new(IDLE_LIMIT)
    }
}

/// `true` when the helper has had no connection for [`IDLE_LIMIT`].
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn idle_expired(idle_for: Duration, connections: u32) -> bool {
    connections == 0 && idle_for >= IDLE_LIMIT
}

/// Identifier-only designated requirement.
///
/// Returns `None` when `identifier` could escape the requirement string.
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn designated_requirement(identifier: &str) -> Option<String> {
    if !valid_identifier(identifier) {
        return None;
    }
    Some(format!("designated => identifier \"{identifier}\""))
}

/// `open` argv that Launch Services uses to start the helper.
///
/// The binary is not executed directly: TCC follows the responsible process,
/// and an `exec` from a shell does not receive the helper's grant. Returns
/// `None` when `bundle` is not a safe identifier. Blocking: none. Timeout:
/// none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn launch_command(bundle: &str) -> Option<Vec<String>> {
    if !valid_identifier(bundle) {
        return None;
    }
    Some(vec![
        "open".to_string(),
        "-g".to_string(),
        "-j".to_string(),
        "-b".to_string(),
        bundle.to_string(),
        "--args".to_string(),
        "agent".to_string(),
    ])
}

/// Identifier safe to embed in a codesign requirement and a bundle id.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn valid_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && value.len() <= 255
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

fn valid_team_id(value: &str) -> bool {
    (1..=20).contains(&value.len()) && value.chars().all(|c| c.is_ascii_alphanumeric())
}

fn normalize_cdhash(value: &str) -> Option<String> {
    if value.len() != 40 && value.len() != 64 {
        return None;
    }
    if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(value.to_ascii_lowercase())
}

/// Constant-time equality. Lengths must already match.
pub(crate) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in left.iter().zip(right.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

fn option_eq(left: &Option<String>, right: &Option<String>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.len() == right.len() && constant_time_eq(left.as_bytes(), right.as_bytes())
        }
        (None, Some(_)) | (Some(_), None) => false,
    }
}
