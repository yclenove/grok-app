//! Client half of the helper link.
//!
//! The socket is stat'd before and after connect. No frame is written until
//! [`super::auth::admit`] returns serve or replace. A rejected peer produces
//! zero payload bytes.

use super::auth::{
    admit, directory_is_private, socket_is_private, valid_version, versions_match, Admit,
    AuthInput, HANDSHAKE_TIMEOUT,
};
use super::error::HelperError;
use super::frame::{
    read_frame, write_frame, Frame, Role, ShutdownReason, WireProfile, CONTROL_MAX,
};
use super::proxy::{endpoint_stamp, AuditSource};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;

/// Asks a replaced helper to exit when it ignores the shutdown frame.
#[async_trait]
pub trait ProcessControl: Send + Sync {
    /// Signal `pid`. Must not signal pid 0 or this process.
    ///
    /// Blocking: the real implementation issues one `kill`. Timeout: none.
    /// Cancellation: dropping the future before the call returns skips the signal.
    async fn terminate(&self, pid: u32);
}

/// Records terminate calls and does not signal a process.
#[derive(Default)]
pub struct RecordingTerminate {
    /// Pids that would have been signaled, in order.
    pub pids: Mutex<Vec<u32>>,
}

impl RecordingTerminate {
    /// Empty record.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ProcessControl for RecordingTerminate {
    async fn terminate(&self, pid: u32) {
        self.pids
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(pid);
    }
}

/// Sends `SIGTERM` to one positive pid that is not this process.
pub struct Sigterm;

#[async_trait]
impl ProcessControl for Sigterm {
    async fn terminate(&self, pid: u32) {
        if pid == 0 || pid == std::process::id() {
            return;
        }
        let Ok(pid) = i32::try_from(pid) else {
            return;
        };
        if pid <= 0 {
            return;
        }
        // SAFETY: `pid` is a single positive process id, not a process group.
        // `SIGTERM` only requests that process to exit. No pointer is dereferenced.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}

/// Inputs for one client attach. The path is explicit.
pub struct ClientConfig {
    /// Helper socket.
    pub path: PathBuf,
    /// How this process sees the helper.
    pub audit: Arc<dyn AuditSource>,
    /// Uid that must own the socket.
    pub uid: u32,
    /// Exact version this process speaks.
    pub version: String,
    /// Profile requested for this MCP session.
    pub profile: WireProfile,
    /// Turn idle advertised to the helper, in seconds.
    pub turn_idle_secs: u64,
    /// Fallback signal when a replaced helper stays up.
    pub control: Arc<dyn ProcessControl>,
    /// Handshake deadline. Production uses [`HANDSHAKE_TIMEOUT`].
    pub handshake_timeout: Duration,
}

/// Result of a client handshake.
#[derive(Debug)]
pub enum ClientAttach {
    /// Hello succeeded. The stream is ready for MCP frames.
    Ready(UnixStream),
    /// The helper was asked to exit. No MCP session was opened.
    Replaced {
        /// Pid observed for the helper. Not an authorization key.
        pid: u32,
    },
}

/// Connect, admit, and complete the hello.
///
/// Writes nothing when admission rejects. Waits at most
/// `config.handshake_timeout` on connect and on the hello. Dropping the future
/// drops the socket. A cancelled handshake is not retried on the same stream.
pub async fn client_attach(config: ClientConfig) -> Result<ClientAttach, HelperError> {
    if let Some(parent) = config.path.parent() {
        match endpoint_stamp(parent) {
            Ok(stamp) if directory_is_private(stamp, config.uid) => {}
            Ok(_) => return Err(HelperError::Foreign),
            Err(error) => return Err(error),
        }
    }
    let before = endpoint_stamp(&config.path)?;
    if !socket_is_private(before, config.uid) {
        return Err(HelperError::Foreign);
    }
    let mut stream =
        tokio::time::timeout(config.handshake_timeout, UnixStream::connect(&config.path))
            .await
            .map_err(|_| HelperError::Timeout)?
            .map_err(|error| HelperError::from_io(&error))?;
    let after = endpoint_stamp(&config.path)?;
    let local = config.audit.local().await?;
    let peer = config.audit.peer(&stream).await?;
    let decision = admit(AuthInput {
        local_uid: config.uid,
        local: &local,
        peer_uid: peer.uid,
        peer: peer.signature.as_ref(),
        bound: before,
        observed: after,
        directory: None,
    });
    match decision {
        Admit::Reject(reason) => {
            drop(stream);
            Err(HelperError::Refused(reason))
        }
        Admit::Replace => {
            write_frame(
                &mut stream,
                &Frame::Shutdown {
                    reason: ShutdownReason::Replace,
                },
                CONTROL_MAX,
            )
            .await?;
            let eof = tokio::time::timeout(config.handshake_timeout, wait_eof(&mut stream)).await;
            if eof.is_err() && still_connectable(&config.path, config.handshake_timeout).await {
                config.control.terminate(peer.pid).await;
            }
            Ok(ClientAttach::Replaced { pid: peer.pid })
        }
        Admit::Serve => finish_hello(&config, stream, peer.pid).await,
    }
}

async fn finish_hello(
    config: &ClientConfig,
    mut stream: UnixStream,
    pid: u32,
) -> Result<ClientAttach, HelperError> {
    write_frame(
        &mut stream,
        &Frame::Hello {
            version: config.version.clone(),
            role: Role::Cli,
            profile: config.profile,
            turn_idle_secs: config.turn_idle_secs,
        },
        CONTROL_MAX,
    )
    .await?;
    let frame = match tokio::time::timeout(
        config.handshake_timeout,
        read_frame(&mut stream, CONTROL_MAX),
    )
    .await
    {
        Ok(Ok(Some(frame))) => frame,
        Ok(Ok(None)) => return Ok(ClientAttach::Replaced { pid }),
        Ok(Err(error)) => return Err(error),
        Err(_) => return Err(HelperError::Timeout),
    };
    match frame {
        Frame::Shutdown { .. } => Ok(ClientAttach::Replaced { pid }),
        Frame::Hello {
            version,
            role,
            profile,
            turn_idle_secs,
        } if role == Role::Agent
            && profile == config.profile
            && turn_idle_secs == config.turn_idle_secs
            && versions_match(&config.version, &version) =>
        {
            Ok(ClientAttach::Ready(stream))
        }
        Frame::Hello { version, .. } if valid_version(&version) => {
            let _ = write_frame(
                &mut stream,
                &Frame::Shutdown {
                    reason: ShutdownReason::Version,
                },
                CONTROL_MAX,
            )
            .await;
            Ok(ClientAttach::Replaced { pid })
        }
        Frame::Hello { .. } | Frame::Mcp { .. } => Err(HelperError::Frame),
    }
}

async fn wait_eof(stream: &mut UnixStream) -> Result<(), HelperError> {
    let mut buffer = [0u8; 256];
    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(error) => return Err(HelperError::from_io(&error)),
        }
    }
}

async fn still_connectable(path: &std::path::Path, timeout: Duration) -> bool {
    match tokio::time::timeout(timeout, UnixStream::connect(path)).await {
        Ok(Ok(stream)) => {
            drop(stream);
            true
        }
        Ok(Err(_)) | Err(_) => false,
    }
}

/// Production handshake timeout re-exported for callers that build [`ClientConfig`].
pub const CLIENT_HANDSHAKE_TIMEOUT: Duration = HANDSHAKE_TIMEOUT;
