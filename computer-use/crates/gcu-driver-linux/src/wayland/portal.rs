//! Wayland `xdg-desktop-portal` ScreenCast session negotiation, stream parsing, and lifetime tracking.
//!
//! Exposes `PortalScreenCastClient` and `PortalScreenCastSession` traits so that ScreenCast
//! negotiation can be thoroughly tested on any platform with deterministic fake implementations.
//! Includes ports of v1 security checks: single source validation, PipeWire node verification,
//! source type consistency, and session cleanup.

use async_trait::async_trait;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use thiserror::Error;

/// Errors arising from portal ScreenCast interactions and stream validation.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PortalError {
    /// Portal D-Bus or IPC call failure.
    #[error("portal D-Bus error: {0}")]
    Dbus(String),

    /// User cancelled or dismissed the portal selection dialog.
    #[error("portal request was cancelled by user")]
    CancelledByUser,

    /// Portal granted zero or multiple streams (exactly one is required).
    #[error("portal must grant exactly one stream, got {0}")]
    InvalidStreamCount(usize),

    /// PipeWire node ID granted by portal is 0 or u32::MAX.
    #[error("invalid PipeWire node ID: {0}")]
    InvalidNodeId(u32),

    /// Source kind granted by portal does not match requested scope.
    #[error("granted source type {granted:?} does not match requested scope {requested:?}")]
    SourceMismatch {
        /// Requested source kind.
        requested: PortalSourceKind,
        /// Granted source kind.
        granted: PortalSourceKind,
    },

    /// PipeWire stream serial is non-positive or invalid.
    #[error("invalid PipeWire stream serial")]
    InvalidPipeWireSerial,

    /// Stream dimensions reported by portal are non-positive.
    #[error("invalid non-positive stream dimensions: {0}x{1}")]
    InvalidStreamDimensions(i32, i32),

    /// Portal session is already closed or retired.
    #[error("portal session is closed")]
    SessionClosed,

    /// Internal portal error.
    #[error("portal error: {0}")]
    Other(String),
}

/// Scope of screen capture source requested from or granted by the portal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortalSourceKind {
    /// Entire monitor or display.
    Monitor,
    /// Single application window.
    Window,
}

impl PortalSourceKind {
    /// Bitmask value used by `xdg-desktop-portal`.
    pub fn mask(&self) -> u32 {
        match self {
            Self::Monitor => 1,
            Self::Window => 2,
        }
    }

    /// Parse from portal numeric source type.
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            1 => Some(Self::Monitor),
            2 => Some(Self::Window),
            _ => None,
        }
    }
}

/// Cursor embedding mode for portal screencast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PortalCursorMode {
    /// Cursor is not embedded in the video stream.
    #[default]
    Hidden,
    /// Cursor is composited into the frame pixels.
    Embedded,
    /// Cursor shape and position are sent as PipeWire metadata.
    Metadata,
}

/// Information describing a single PipeWire video stream granted by the portal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalStream {
    /// PipeWire node ID to connect to for video buffer streaming.
    pub node_id: u32,
    /// Source type granted by the portal.
    pub source_type: PortalSourceKind,
    /// Stream position on the compositor display grid, if provided.
    pub position: Option<(i32, i32)>,
    /// Logical size of the stream, if provided.
    pub size: Option<(i32, i32)>,
    /// Mapping ID for input synchronization (e.g. libei), if provided.
    pub mapping_id: Option<String>,
    /// PipeWire stream sequence serial, if provided.
    pub pipewire_serial: Option<u64>,
}

impl PortalStream {
    /// Validate this stream against strict security and sanity requirements.
    pub fn validate(&self, expected_source: PortalSourceKind) -> Result<(), PortalError> {
        if self.node_id == 0 || self.node_id == u32::MAX {
            return Err(PortalError::InvalidNodeId(self.node_id));
        }
        if self.source_type != expected_source {
            return Err(PortalError::SourceMismatch {
                requested: expected_source,
                granted: self.source_type,
            });
        }
        if let Some(serial) = self.pipewire_serial {
            if serial == 0 {
                return Err(PortalError::InvalidPipeWireSerial);
            }
        }
        if let Some((w, h)) = self.size {
            if w <= 0 || h <= 0 {
                return Err(PortalError::InvalidStreamDimensions(w, h));
            }
        }
        Ok(())
    }
}

/// Outcome of a successful portal ScreenCast negotiation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalScreenCastResult {
    /// Portal session object path or handle.
    pub session_handle: String,
    /// The granted PipeWire video stream.
    pub stream: PortalStream,
    /// New restore token returned by the portal for future session continuity.
    pub restore_token: Option<String>,
}

/// Active portal ScreenCast session.
#[async_trait]
pub trait PortalScreenCastSession: std::fmt::Debug + Send + Sync {
    /// Close and retire this portal session.
    async fn close(&self) -> Result<(), PortalError>;

    /// Whether this session is currently active.
    fn is_active(&self) -> bool;

    /// Portal session handle or D-Bus path.
    fn handle(&self) -> &str;
}

/// Abstract client for negotiating ScreenCast sessions with `xdg-desktop-portal`.
#[async_trait]
pub trait PortalScreenCastClient: Send + Sync {
    /// Start a ScreenCast session, optionally supplying an existing restore token.
    async fn start_screencast(
        &self,
        source_kind: PortalSourceKind,
        restore_token: Option<&str>,
        cursor_mode: PortalCursorMode,
    ) -> Result<(Arc<dyn PortalScreenCastSession>, PortalScreenCastResult), PortalError>;
}

/// In-memory fake ScreenCast session for cross-platform unit and contract testing.
#[derive(Debug)]
pub struct FakePortalSession {
    handle: String,
    active: Arc<AtomicBool>,
}

impl FakePortalSession {
    /// Create a new fake session with the specified handle.
    pub fn new(handle: impl Into<String>) -> (Self, Arc<AtomicBool>) {
        let active = Arc::new(AtomicBool::new(true));
        (
            Self {
                handle: handle.into(),
                active: active.clone(),
            },
            active,
        )
    }
}

#[async_trait]
impl PortalScreenCastSession for FakePortalSession {
    async fn close(&self) -> Result<(), PortalError> {
        self.active.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    fn handle(&self) -> &str {
        &self.handle
    }
}

/// Scriptable fake portal client for deterministic testing across platforms.
#[derive(Debug, Clone, Default)]
pub struct FakePortalClient {
    streams: Arc<Mutex<Vec<PortalStream>>>,
    restore_token: Arc<Mutex<Option<String>>>,
    error_injection: Arc<Mutex<Option<PortalError>>>,
    created_sessions: Arc<Mutex<Vec<Arc<AtomicBool>>>>,
    last_requested_token: Arc<Mutex<Option<String>>>,
}

impl FakePortalClient {
    /// Create a new fake portal client configured with a default valid stream.
    pub fn new() -> Self {
        let default_stream = PortalStream {
            node_id: 42,
            source_type: PortalSourceKind::Window,
            position: Some((100, 100)),
            size: Some((800, 600)),
            mapping_id: Some("mapping-42".into()),
            pipewire_serial: Some(1001),
        };
        Self {
            streams: Arc::new(Mutex::new(vec![default_stream])),
            restore_token: Arc::new(Mutex::new(Some("test-restore-token-abc".into()))),
            error_injection: Arc::new(Mutex::new(None)),
            created_sessions: Arc::new(Mutex::new(Vec::new())),
            last_requested_token: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the streams to return upon screencast negotiation.
    pub fn set_streams(&self, streams: Vec<PortalStream>) {
        *self.streams.lock().unwrap_or_else(|e| e.into_inner()) = streams;
    }

    /// Set the restore token to return.
    pub fn set_restore_token(&self, token: Option<String>) {
        *self.restore_token.lock().unwrap_or_else(|e| e.into_inner()) = token;
    }

    /// Inject an error to be returned on next call.
    pub fn inject_error(&self, error: PortalError) {
        *self
            .error_injection
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(error);
    }

    /// Get the restore token that was passed into the most recent call.
    pub fn last_requested_token(&self) -> Option<String> {
        self.last_requested_token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Get all session active flags to observe closures.
    pub fn session_flags(&self) -> Vec<Arc<AtomicBool>> {
        self.created_sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

#[async_trait]
impl PortalScreenCastClient for FakePortalClient {
    async fn start_screencast(
        &self,
        source_kind: PortalSourceKind,
        restore_token: Option<&str>,
        _cursor_mode: PortalCursorMode,
    ) -> Result<(Arc<dyn PortalScreenCastSession>, PortalScreenCastResult), PortalError> {
        *self
            .last_requested_token
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = restore_token.map(|s| s.to_string());

        if let Some(err) = self
            .error_injection
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            return Err(err);
        }

        let streams = self
            .streams
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if streams.len() != 1 {
            return Err(PortalError::InvalidStreamCount(streams.len()));
        }

        let stream = streams[0].clone();
        stream.validate(source_kind)?;

        let handle = format!(
            "/org/freedesktop/portal/desktop/session/fake_{}",
            stream.node_id
        );
        let (session, flag) = FakePortalSession::new(&handle);
        self.created_sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(flag);

        let result = PortalScreenCastResult {
            session_handle: handle,
            stream,
            restore_token: self
                .restore_token
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        };

        Ok((Arc::new(session), result))
    }
}

#[cfg(target_os = "linux")]
/// Real portal ScreenCast client communicating with `xdg-desktop-portal` via `ashpd`.
pub struct AshpdPortalClient;

#[cfg(target_os = "linux")]
/// Ashpd-backed Screencast session wrapper.
pub struct AshpdSessionWrapper {
    /// Inner ashpd Screencast session.
    pub session: ashpd::desktop::Session<ashpd::desktop::screencast::Screencast>,
    /// Session D-Bus object path handle.
    pub handle: String,
    /// Whether the session is currently active.
    pub active: AtomicBool,
}

#[cfg(target_os = "linux")]
impl AshpdSessionWrapper {
    /// Open a PipeWire remote file descriptor for the screencast session.
    pub async fn open_pipewire_remote(&self) -> Result<std::os::fd::OwnedFd, PortalError> {
        let proxy = ashpd::desktop::screencast::Screencast::new()
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?;
        proxy
            .open_pipe_wire_remote(&self.session, Default::default())
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))
    }
}

#[cfg(target_os = "linux")]
impl std::fmt::Debug for AshpdSessionWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AshpdSessionWrapper")
            .field("handle", &self.handle)
            .field("active", &self.active)
            .finish()
    }
}

#[cfg(target_os = "linux")]
#[async_trait]
impl PortalScreenCastSession for AshpdSessionWrapper {
    async fn close(&self) -> Result<(), PortalError> {
        self.active.store(false, Ordering::SeqCst);
        self.session
            .close()
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    fn handle(&self) -> &str {
        &self.handle
    }
}

#[cfg(target_os = "linux")]
#[async_trait]
impl PortalScreenCastClient for AshpdPortalClient {
    async fn start_screencast(
        &self,
        source_kind: PortalSourceKind,
        restore_token: Option<&str>,
        cursor_mode: PortalCursorMode,
    ) -> Result<(Arc<dyn PortalScreenCastSession>, PortalScreenCastResult), PortalError> {
        use ashpd::desktop::{
            screencast::{
                CursorMode as AshpdCursorMode, Screencast, SelectSourcesOptions, SourceType,
            },
            CreateSessionOptions, PersistMode,
        };
        use ashpd::enumflags2::BitFlags;

        let proxy = Screencast::new()
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?;

        let session = proxy
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?;

        let handle_str = serde_json::to_string(&session)
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|_| {
                "/org/freedesktop/portal/desktop/session/wayland_screencast".into()
            });

        let ashpd_sources = match source_kind {
            PortalSourceKind::Monitor => BitFlags::from_flag(SourceType::Monitor),
            PortalSourceKind::Window => BitFlags::from_flag(SourceType::Window),
        };

        let ashpd_cursor = match cursor_mode {
            PortalCursorMode::Hidden => AshpdCursorMode::Hidden,
            PortalCursorMode::Embedded => AshpdCursorMode::Embedded,
            PortalCursorMode::Metadata => AshpdCursorMode::Metadata,
        };

        let mut options = SelectSourcesOptions::default()
            .set_sources(ashpd_sources)
            .set_cursor_mode(ashpd_cursor)
            .set_multiple(false)
            .set_persist_mode(PersistMode::ExplicitlyRevoked);

        if let Some(token) = restore_token {
            options = options.set_restore_token(token);
        }

        proxy
            .select_sources(&session, options)
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?;

        let request = proxy
            .start(&session, None, Default::default())
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?;

        let start_response = request
            .response()
            .map_err(|_| PortalError::CancelledByUser)?;

        let mut streams = Vec::new();
        for s in start_response.streams() {
            let src = match s.source_type() {
                Some(SourceType::Monitor) => PortalSourceKind::Monitor,
                Some(SourceType::Window) => PortalSourceKind::Window,
                _ => continue,
            };
            streams.push(PortalStream {
                node_id: s.pipe_wire_node_id(),
                source_type: src,
                position: s.position(),
                size: s.size(),
                mapping_id: s.mapping_id().map(|id| id.to_string()),
                pipewire_serial: None,
            });
        }

        if streams.len() != 1 {
            let _ = session.close().await;
            return Err(PortalError::InvalidStreamCount(streams.len()));
        }

        let stream = streams.remove(0);
        if let Err(e) = stream.validate(source_kind) {
            let _ = session.close().await;
            return Err(e);
        }

        let result = PortalScreenCastResult {
            session_handle: handle_str.clone(),
            stream,
            restore_token: start_response.restore_token().map(|t| t.to_string()),
        };

        let wrapper = AshpdSessionWrapper {
            session,
            handle: handle_str,
            active: AtomicBool::new(true),
        };

        Ok((Arc::new(wrapper), result))
    }
}
