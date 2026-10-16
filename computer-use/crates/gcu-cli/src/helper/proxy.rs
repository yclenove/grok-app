//! Helper socket, audit trait, and accept loop.
//!
//! Platform signature calls sit behind [`AuditSource`]. [`FixedAudit`] returns
//! scripted facts and does not read the socket. Authentication runs before any
//! frame is parsed. A new connection is a new MCP session.

use super::auth::{
    directory_is_private, socket_is_private, CodeSignature, EndpointStamp, IdleClock,
    HANDSHAKE_TIMEOUT,
};
use super::error::HelperError;
use super::serve::{self, HandleResult, ServeCtx};
use async_trait::async_trait;
use gcu_core::driver::Driver;
use gcu_core::session::Session;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::sync::{mpsc, Notify};

/// What one audit of a connected peer returned.
#[derive(Clone, Debug)]
pub struct PeerObservation {
    /// Effective uid from the audit token.
    pub uid: u32,
    /// Peer pid. Not an authorization key.
    pub pid: u32,
    /// Code signature, or `None` when unsigned or unreadable.
    pub signature: Option<CodeSignature>,
}

/// Reads the peer code signature. Implementations must not trust the uid alone.
#[async_trait]
pub trait AuditSource: Send + Sync {
    /// Observe `stream`'s peer.
    ///
    /// Waits at most [`HANDSHAKE_TIMEOUT`] for a platform call. Dropping the
    /// future cancels the wait. The socket is not moved to a blocking thread.
    /// A platform failure is [`HelperError::Audit`], not an anonymous success.
    async fn peer(&self, stream: &tokio::net::UnixStream) -> Result<PeerObservation, HelperError>;

    /// Signature of this process.
    ///
    /// Same timeout and cancellation as [`Self::peer`]. [`HelperError::UnsignedSelf`]
    /// means this process must not listen or connect as a helper.
    async fn local(&self) -> Result<CodeSignature, HelperError>;
}

/// Scripted audit. Ignores socket bytes so tests do not depend on peer creds.
pub struct FixedAudit {
    local: CodeSignature,
    peer: PeerObservation,
}

impl FixedAudit {
    /// Always return `local` and `peer`.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// value is `Send + Sync` and does not change.
    #[must_use]
    pub fn new(local: CodeSignature, peer: PeerObservation) -> Self {
        Self { local, peer }
    }
}

#[async_trait]
impl AuditSource for FixedAudit {
    async fn peer(&self, _stream: &tokio::net::UnixStream) -> Result<PeerObservation, HelperError> {
        Ok(self.peer.clone())
    }

    async fn local(&self) -> Result<CodeSignature, HelperError> {
        Ok(self.local.clone())
    }
}

/// Sessions opened by admitted connections. Rejects are not inserted.
#[derive(Default)]
pub struct SessionRegistry {
    sessions: Mutex<Vec<Arc<Session>>>,
}

impl SessionRegistry {
    /// Empty registry.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// value is `Send + Sync`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn insert(&self, session: Arc<Session>) {
        let mut guard = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        guard.push(session);
    }

    /// Sessions in admission order.
    ///
    /// Blocking: waits on the mutex. Timeout: none. Cancellation: none.
    /// Thread safety: poisoned mutexes are recovered.
    #[must_use]
    pub fn sessions(&self) -> Vec<Arc<Session>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// How many sessions were admitted.
    ///
    /// Blocking: waits on the mutex. Timeout: none. Cancellation: none.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// No admitted session.
    ///
    /// Blocking: waits on the mutex. Timeout: none. Cancellation: none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Why the accept loop returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenStop {
    /// No connection for the idle limit.
    Idle,
    /// [`Running::stop`] was notified.
    Stopped,
    /// An admitted peer asked the process to exit.
    Shutdown,
    /// The bound socket's device or inode changed. The new file is left in place.
    Replaced,
    /// Accept failed for a reason other than replacement.
    Failed,
}

/// A running listener. Drop does not stop the task; notify [`Self::stop`].
#[derive(Debug)]
pub struct Running {
    /// Notifying this asks the loop to exit with [`ListenStop::Stopped`].
    pub stop: Arc<Notify>,
    /// Completes when the loop has unlinked its own socket, if it still owns it.
    pub done: tokio::task::JoinHandle<ListenStop>,
}

/// What [`start_listener`] needs. The socket path is explicit so tests never
/// use the production path.
pub struct ListenConfig {
    /// Socket path. The parent directory is created `0700` when it is absent.
    pub path: PathBuf,
    /// Peer and self signatures.
    pub audit: Arc<dyn AuditSource>,
    /// Exact version string advertised in hello.
    pub version: String,
    /// Uid that must own the socket. Tests pass the real euid for real files.
    pub uid: u32,
    /// Host shared by admitted sessions. Each session has its own grants.
    pub host: gcu_mcp::Host,
    /// Driver shared by admitted sessions.
    pub driver: Arc<dyn Driver>,
    /// Filled only after admission, before the server hello is written.
    pub registry: Arc<SessionRegistry>,
    /// Hello deadline. Production uses [`HANDSHAKE_TIMEOUT`].
    pub handshake_timeout: Duration,
    /// Exit when no connection has been open for this long. Production uses [`super::auth::IDLE_LIMIT`].
    pub idle_limit: Duration,
}

pub(crate) enum LoopCmd {
    Closed,
    Shutdown(tokio::sync::oneshot::Sender<()>),
    Replaced,
}

/// Bind `config.path` and accept until idle, stop, shutdown, or replacement.
///
/// The directory is created mode `0700` before the socket so a chmod race is
/// not cross-user. A foreign socket is left untouched. A connectable socket
/// returns [`HelperError::Busy`]. Binding waits at most five seconds in a
/// blocking thread; dropping the future cancels the wait. The returned task
/// unlinks the socket on exit only when the stamp still matches.
pub async fn start_listener(config: ListenConfig) -> Result<Running, HelperError> {
    let local = config.audit.local().await?;
    let bound = bind_socket(&config.path, config.uid).await?;
    let listener = bound.listener;
    let stamp = bound.stamp;
    let stop = Arc::new(Notify::new());
    let stop_task = Arc::clone(&stop);
    let path = config.path.clone();
    let uid = config.uid;
    let ctx = Arc::new(ServeCtx {
        audit: config.audit,
        local,
        version: config.version,
        uid,
        path: path.clone(),
        bound: stamp,
        host: config.host,
        driver: config.driver,
        registry: config.registry,
        handshake_timeout: config.handshake_timeout,
        idle_limit: config.idle_limit,
    });
    let done = tokio::spawn(async move {
        let stop_reason = accept_loop(listener, stop_task, ctx).await;
        unlink_if_same(&path, stamp, uid);
        stop_reason
    });
    tokio::task::yield_now().await;
    Ok(Running { stop, done })
}

struct BoundSocket {
    listener: UnixListener,
    stamp: EndpointStamp,
}

async fn bind_socket(path: &Path, uid: u32) -> Result<BoundSocket, HelperError> {
    let path = path.to_path_buf();
    let (listener, stamp) = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        tokio::task::spawn_blocking(move || bind_blocking(&path, uid)),
    )
    .await
    .map_err(|_| HelperError::Timeout)?
    .map_err(|_| HelperError::Io("join"))??;
    let listener =
        UnixListener::from_std(listener).map_err(|error| HelperError::from_io(&error))?;
    Ok(BoundSocket { listener, stamp })
}

fn bind_blocking(
    path: &Path,
    uid: u32,
) -> Result<(std::os::unix::net::UnixListener, EndpointStamp), HelperError> {
    let parent = path.parent().ok_or(HelperError::Foreign)?;
    match endpoint_stamp(parent) {
        Err(HelperError::Absent) => create_private_dir(parent)?,
        Ok(stamp) if directory_is_private(stamp, uid) => {}
        Ok(_) => return Err(HelperError::Foreign),
        Err(error) => return Err(error),
    }
    match classify_existing(path, uid)? {
        Existing::Create => {}
        Existing::Foreign => return Err(HelperError::Foreign),
        Existing::Busy => return Err(HelperError::Busy),
        Existing::ReplaceStale => {
            std::fs::remove_file(path).map_err(|error| HelperError::from_io(&error))?;
        }
    }
    let listener = std::os::unix::net::UnixListener::bind(path)
        .map_err(|error| HelperError::from_io(&error))?;
    set_mode(path, 0o600)?;
    let stamp = endpoint_stamp(path)?;
    if !socket_is_private(stamp, uid) {
        let _ = std::fs::remove_file(path);
        return Err(HelperError::Foreign);
    }
    listener
        .set_nonblocking(true)
        .map_err(|error| HelperError::from_io(&error))?;
    Ok((listener, stamp))
}

enum Existing {
    Create,
    Foreign,
    Busy,
    ReplaceStale,
}

fn classify_existing(path: &Path, uid: u32) -> Result<Existing, HelperError> {
    let stamp = match endpoint_stamp(path) {
        Err(HelperError::Absent) => return Ok(Existing::Create),
        Ok(stamp) => stamp,
        Err(error) => return Err(error),
    };
    if !socket_is_private(stamp, uid) {
        return Ok(Existing::Foreign);
    }
    match std::os::unix::net::UnixStream::connect(path) {
        Ok(stream) => {
            drop(stream);
            Ok(Existing::Busy)
        }
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
            Ok(Existing::ReplaceStale)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Existing::Create),
        Err(_) => Ok(Existing::Foreign),
    }
}

fn create_private_dir(path: &Path) -> Result<(), HelperError> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|error| HelperError::from_io(&error))?;
    set_mode(path, 0o700)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), HelperError> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::symlink_metadata(path)
        .map_err(|error| HelperError::from_io(&error))?
        .permissions();
    permissions.set_mode(mode);
    std::fs::set_permissions(path, permissions).map_err(|error| HelperError::from_io(&error))
}

/// Read an endpoint stamp without following symlinks.
///
/// [`HelperError::Absent`] means the path is missing. Other failures are
/// categories, not paths. Blocking: one `stat`. Timeout: none. Cancellation:
/// none. Thread safety: the path is only read.
pub fn endpoint_stamp(path: &Path) -> Result<EndpointStamp, HelperError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(HelperError::Absent)
        }
        Err(error) => return Err(HelperError::from_io(&error)),
    };
    let kind = metadata.file_type();
    Ok(EndpointStamp {
        dev: metadata.dev(),
        ino: metadata.ino(),
        uid: metadata.uid(),
        mode: metadata.permissions().mode() & 0o777,
        socket: kind.is_socket(),
        directory: kind.is_dir(),
        symlink: kind.is_symlink(),
    })
}

fn unlink_if_same(path: &Path, stamp: EndpointStamp, uid: u32) {
    if let Ok(now) = endpoint_stamp(path) {
        if now.dev == stamp.dev && now.ino == stamp.ino && socket_is_private(now, uid) {
            let _ = std::fs::remove_file(path);
        }
    }
}

async fn accept_loop(listener: UnixListener, stop: Arc<Notify>, ctx: Arc<ServeCtx>) -> ListenStop {
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    let mut clock = IdleClock::new(ctx.idle_limit);
    let mut last = tokio::time::Instant::now();
    loop {
        let sleep = tokio::time::sleep(Duration::from_millis(200));
        tokio::pin!(sleep);
        tokio::select! {
            _ = stop.notified() => return ListenStop::Stopped,
            accepted = listener.accept() => {
                let _ = take_idle(&mut clock, &mut last);
                match accepted {
                    Ok((stream, _)) => {
                        clock.connect();
                        let ctx = Arc::clone(&ctx);
                        let cmd_tx = cmd_tx.clone();
                        tokio::spawn(async move {
                            let mut guard = CloseGuard {
                                tx: cmd_tx.clone(),
                                armed: true,
                            };
                            let result = serve::handle_connection(stream, &ctx, &cmd_tx).await;
                            if result != HandleResult::Closed {
                                guard.armed = false;
                            }
                        });
                    }
                    Err(_) => {
                        return if stamp_matches(&ctx) {
                            ListenStop::Failed
                        } else {
                            ListenStop::Replaced
                        };
                    }
                }
            }
            cmd = cmd_rx.recv() => {
                if take_idle(&mut clock, &mut last) {
                    return ListenStop::Idle;
                }
                match cmd {
                    Some(LoopCmd::Closed) => clock.disconnect(),
                    Some(LoopCmd::Shutdown(ack)) => {
                        unlink_if_same(&ctx.path, ctx.bound, ctx.uid);
                        let _ = ack.send(());
                        return ListenStop::Shutdown;
                    }
                    Some(LoopCmd::Replaced) => return ListenStop::Replaced,
                    None => return ListenStop::Stopped,
                }
            }
            _ = &mut sleep => {
                if !stamp_matches(&ctx) {
                    return ListenStop::Replaced;
                }
                if take_idle(&mut clock, &mut last) {
                    return ListenStop::Idle;
                }
            }
        }
    }
}

struct CloseGuard {
    tx: mpsc::UnboundedSender<LoopCmd>,
    armed: bool,
}

impl Drop for CloseGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.tx.send(LoopCmd::Closed);
        }
    }
}

fn take_idle(clock: &mut IdleClock, last: &mut tokio::time::Instant) -> bool {
    let now = tokio::time::Instant::now();
    let elapsed = now.saturating_duration_since(*last);
    *last = now;
    clock.tick(elapsed)
}

fn stamp_matches(ctx: &ServeCtx) -> bool {
    match endpoint_stamp(&ctx.path) {
        Ok(now) => {
            now.dev == ctx.bound.dev && now.ino == ctx.bound.ino && socket_is_private(now, ctx.uid)
        }
        Err(_) => false,
    }
}
