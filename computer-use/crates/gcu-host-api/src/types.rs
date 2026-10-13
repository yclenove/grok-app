//! Values the engine passes into Host trait methods.
//!
//! These are in-process types. The versioned wire format lives in `gcu-host-protocol`.
//! [`AppIdentity`] is richer than `gcu_protocol::AppIdentity`: the protocol value is
//! an optional observation field, while this one is the OS-verified authorization key.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Operating system that produced an [`AppIdentity`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    /// macOS. [`AppIdentity::key`] is the bundle id.
    Macos,
    /// Windows. [`AppIdentity::key`] is the normalized executable path.
    Windows,
    /// Linux. [`AppIdentity::key`] is the desktop id, or the canonical exe path.
    Linux,
}

/// Result of the OS signature check. The model does not choose this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureState {
    /// The OS accepted the signature.
    Valid,
    /// A signature was present and the OS rejected it.
    Invalid,
    /// The image has no signature.
    Unsigned,
    /// The OS check did not produce a result.
    Unknown,
}

/// OS-verified application identity.
///
/// The engine fills this from the operating system. Model text is not copied
/// into these fields. [`AppIdentity::key`] is the primary key: a macOS bundle
/// id, a Windows canonical executable path, or a Linux desktop id (or the
/// canonical exe path when the app has no desktop id). Callers normalize
/// Windows paths before storing them. This type compares `key` exactly and
/// does not case-fold, so `C:\App.exe` and `c:\app.exe` differ.
///
/// # Equality and hashing
///
/// [`Eq`] and [`Hash`] include every field. A different digest, team id,
/// publisher, signature state, or image path is a different value. Equal
/// values hash equally and are valid `HashMap` keys.
///
/// Do not use that map to look up grants. Authorization uses
/// [`AppIdentity::grant_eq`], which is a different relation: two identities
/// can grant-match and still hash apart. [`crate::PolicyStore::always_allowed`]
/// scans with `grant_eq`.
///
/// # Grant equality
///
/// - macOS [`SignatureState::Valid`] with a team id matches the same bundle id
///   and team id. Image path, content digest, and publisher are ignored, so a
///   moved or updated bundle keeps the grant.
/// - Windows [`SignatureState::Valid`] with a publisher matches the same `key`
///   and publisher. Image path, content digest, and team id are ignored.
/// - Every other identity matches only with structural [`Eq`]. That includes
///   `Invalid`, `Unsigned`, `Unknown`, Linux, and a `Valid` identity that is
///   missing the team id or publisher it would otherwise bind. Unsigned grants
///   therefore bind path and digest.
///
/// A `Valid` grant never matches `Invalid`, `Unsigned`, or `Unknown`.
#[derive(Debug, Clone, Eq)]
pub struct AppIdentity {
    /// Platform that verified this identity.
    pub platform: Platform,
    /// Bundle id, normalized executable path, or desktop id.
    pub key: String,
    /// macOS Team ID, when the signature carries one.
    pub team_id: Option<String>,
    /// Windows Authenticode publisher subject, when present.
    pub publisher: Option<String>,
    /// SHA-256 of the image contents. `None` means the Host did not hash it.
    pub sha256: Option<[u8; 32]>,
    /// OS signature check.
    pub signature: SignatureState,
    /// Canonical path of the executable image.
    pub image_path: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SignedBinding<'a> {
    Macos { key: &'a str, team_id: &'a str },
    Windows { key: &'a str, publisher: &'a str },
}

impl AppIdentity {
    fn signed_binding(&self) -> Option<SignedBinding<'_>> {
        match self.platform {
            Platform::Macos if self.signature == SignatureState::Valid => {
                self.team_id.as_deref().map(|team_id| SignedBinding::Macos {
                    key: self.key.as_str(),
                    team_id,
                })
            }
            Platform::Windows if self.signature == SignatureState::Valid => self
                .publisher
                .as_deref()
                .map(|publisher| SignedBinding::Windows {
                    key: self.key.as_str(),
                    publisher,
                }),
            Platform::Macos | Platform::Windows | Platform::Linux => None,
        }
    }

    /// Whether `other` satisfies the same persistent grant as `self`.
    ///
    /// See the type-level docs for the platform rules. This is not [`Eq`].
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure;
    /// the value is not locked.
    #[must_use]
    pub fn grant_eq(&self, other: &Self) -> bool {
        match (self.signed_binding(), other.signed_binding()) {
            (Some(left), Some(right)) => left == right,
            (None, None) => self == other,
            (Some(_), None) | (None, Some(_)) => false,
        }
    }
}

impl PartialEq for AppIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.platform == other.platform
            && self.key == other.key
            && self.team_id == other.team_id
            && self.publisher == other.publisher
            && self.sha256 == other.sha256
            && self.signature == other.signature
            && self.image_path == other.image_path
    }
}

impl Hash for AppIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.platform.hash(state);
        self.key.hash(state);
        self.team_id.hash(state);
        self.publisher.hash(state);
        self.sha256.hash(state);
        self.signature.hash(state);
        self.image_path.hash(state);
    }
}

/// Application the engine wants the user to authorize or watch.
///
/// `identity` is required and is the OS-verified value. `name` and `pid` are
/// display context and are not part of [`AppIdentity`] equality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    /// Display name from the OS, not from the model.
    pub name: String,
    /// Running process id, when the app is running.
    pub pid: Option<u32>,
    /// OS-verified authorization identity.
    pub identity: AppIdentity,
    /// Small in-memory icon. There is no path to open or write.
    pub icon: Option<ImageRef>,
}

/// Plain text the model wrote for an approval card.
///
/// The caller guarantees the contents are plain text. This type does not
/// inspect or sanitize them. A Host that shows the string must label it as
/// model-provided. It is not an OS fact and it is not a [`HandoffReason`](crate::HandoffReason).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelReason(String);

impl ModelReason {
    /// Store model-provided plain text.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// value is owned and immutable after construction.
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// Borrow the plain text.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: shared
    /// references are enough; the string does not change.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Session the engine is acting in. Host UI binds cards to `session_id`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionCtx {
    /// Opaque session id from the engine. This crate does not parse it.
    pub session_id: String,
}

/// Coarse risk the engine attaches to an approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Risk {
    /// Ordinary authorization or a low-risk action.
    Low,
    /// Sensitive target (send, pay, delete, password, blocked chord).
    High,
}

/// In-memory image. Callers pass bytes, never a temporary file path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    /// Image media type, such as `image/png`.
    pub mime: String,
    /// Pixel width. The Host trusts this; it is not checked against `bytes`.
    pub width: u32,
    /// Pixel height. The Host trusts this; it is not checked against `bytes`.
    pub height: u32,
    /// Image bytes. Cloning shares the allocation.
    pub bytes: Arc<[u8]>,
}

impl ImageRef {
    /// Build an image the Host can show without touching the filesystem.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// returned value is immutable and `Send`.
    pub fn from_bytes(
        mime: impl Into<String>,
        width: u32,
        height: u32,
        bytes: impl Into<Vec<u8>>,
    ) -> Self {
        let bytes = bytes.into();
        Self {
            mime: mime.into(),
            width,
            height,
            bytes: Arc::from(bytes.into_boxed_slice()),
        }
    }

    /// Borrow the image bytes.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// bytes are immutable.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Sensitive action the user is asked to confirm.
///
/// `snapshot_id` and `action_digest` bind the dialog to the action the engine
/// will run. `action_digest` is a 32-byte digest of the canonical action
/// parameters, conventionally SHA-256. This crate does not compute it (`sha2`
/// is not an allowed dependency). The Host shows this summary and must not
/// treat a confirmation as covering a different digest or snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionSummary {
    /// Engine-written description, for example `click "Send"`. Not model text.
    pub text: String,
    /// Snapshot the action was planned against.
    pub snapshot_id: String,
    /// Digest of the action parameters.
    pub action_digest: [u8; 32],
}

/// Point in the Host's screen coordinate space, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPoint {
    /// Horizontal physical pixels.
    pub x: f64,
    /// Vertical physical pixels.
    pub y: f64,
}

/// Window the overlay should track.
///
/// `frame` is the window's screen rectangle in physical pixels. It is not the
/// window-relative frame inside `gcu_protocol::AppState`.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    /// Engine window id.
    pub id: String,
    /// Window title from the OS.
    pub title: String,
    /// Screen-space physical-pixel rectangle.
    pub frame: gcu_protocol::Rect,
    /// Window backing scale. `2.0` is a 2x display.
    pub scale: f64,
}
