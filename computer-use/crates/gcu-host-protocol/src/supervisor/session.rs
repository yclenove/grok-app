//! Spawn `gcu engine` over an inherited private transport and keep it accountable.
//!
//! Unix prefers a socketpair, which has no filesystem path to replace. The
//! fallback binds a mode-0700 directory and the pinned listener from
//! [`crate::transport::PrivateListener`]. Windows creates a fresh duplex pipe
//! with `first_pipe_instance` and rejects remote clients; an existing name is
//! a refusal, not a connection to someone else's pipe.

#[cfg(unix)]
use crate::Error as ProtocolError;
use crate::{
    auth::{AuthConfig, Bootstrap, Channel, ProcessInstance, Role, Secret},
    codec::write_frame,
    messages::{Empty, Version, VERSION},
    server::{HostCallbacks, HostServer, Peer, RPC_TIMEOUT},
};
#[cfg(unix)]
use std::process::Stdio;
use std::{path::PathBuf, process::Child, sync::Arc};
use tokio::io::{AsyncRead, AsyncWrite};

use super::Error;

/// Arguments for one engine process. The stop flag is the Host latch, not a model bit.
#[derive(Clone, Debug)]
pub struct LaunchRequest {
    /// Canonical engine executable.
    pub path: PathBuf,
    /// Extra arguments placed after `engine` and before the transport flags.
    pub extra_args: Vec<String>,
    /// Copied into both handshakes. Restart does not clear it.
    pub stopped: bool,
    /// Host protocol version offered to the child.
    pub version: Version,
    /// Unix only: use the pinned socket instead of a socketpair.
    pub fallback: bool,
}

impl LaunchRequest {
    /// Inherited transport, current protocol version, latch copied from the caller.
    pub fn new(path: impl Into<PathBuf>, stopped: bool) -> Self {
        Self {
            path: path.into(),
            extra_args: Vec::new(),
            stopped,
            version: VERSION,
            fallback: false,
        }
    }
}

/// Result of a non-blocking poll of the engine child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollExit {
    /// Still running.
    Running,
    /// Exited after this host asked it to.
    Clean,
    /// Exited without a host request. Counts as a crash.
    Crash,
}

/// Token facts read from the live child. Unknown fails closed in release mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LivePrivilege {
    /// High integrity or an elevated token.
    pub elevated: bool,
    /// TokenUIAccess.
    pub ui_access: bool,
    /// The token could not be read.
    pub unknown: bool,
}

impl LivePrivilege {
    /// Unix and ordinary Windows tokens.
    pub const fn normal() -> Self {
        Self {
            elevated: false,
            ui_access: false,
            unknown: false,
        }
    }

    /// Probe failed.
    pub const fn unknown() -> Self {
        Self {
            elevated: false,
            ui_access: false,
            unknown: true,
        }
    }
}

/// Process the supervisor can shut down without knowing the transport.
pub trait EngineProcess: Send + Sync {
    /// Non-blocking exit poll. IO errors are crashes.
    fn poll_exit(&mut self) -> PollExit;
    /// Ask the engine to leave. The later exit is clean even if the RPC fails.
    fn begin_shutdown(&mut self);
    /// Kill the child. The following exit is clean so it does not trip the breaker.
    fn kill(&mut self);
    /// Live token, or [`LivePrivilege::normal`] where elevation does not apply.
    fn privilege(&self) -> LivePrivilege;
    /// Authenticated RPC peer. Test doubles and a child that is not up return None.
    fn peer(&self) -> Option<Peer> {
        None
    }
}

/// A connected engine. Drop kills the child and does not wait.
pub struct EngineSession {
    peer: Peer,
    stop: Peer,
    child: Child,
    callbacks: tokio::task::JoinHandle<()>,
    /// Same object passed to the RPC task. Drop denies pending approvals even
    /// when abort skips the task's own `disconnected` call.
    hooks: Arc<dyn HostCallbacks>,
    shutdown_requested: bool,
}

impl EngineSession {
    /// Spawn and handshake. Five-second handshake budget. Failure kills the child.
    ///
    /// Blocks only for the process spawn and the process-start metadata read,
    /// each bounded by the handshake deadline inside the protocol calls. Cancel
    /// by dropping the returned future; the guard kills a child already spawned.
    pub async fn spawn(
        request: &LaunchRequest,
        callbacks: Arc<dyn HostCallbacks>,
    ) -> Result<Self, Error> {
        if request.fallback {
            #[cfg(unix)]
            {
                return spawn_fallback(request, callbacks).await;
            }
            #[cfg(not(unix))]
            {
                let _ = callbacks;
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "pinned host socket fallback is unix-only",
                )));
            }
        }
        #[cfg(unix)]
        {
            spawn_inherited(request, callbacks).await
        }
        #[cfg(windows)]
        {
            windows::spawn_pipe(request, callbacks).await
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = callbacks;
            Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "engine supervision is not implemented on this platform",
            )))
        }
    }

    /// Authenticated ordinary RPC. Not for model tools.
    pub fn peer(&self) -> &Peer {
        &self.peer
    }

    /// Independent stop channel.
    pub fn stop(&self) -> &Peer {
        &self.stop
    }

    /// OS process id, if the child has not been waited.
    pub fn id(&self) -> Option<u32> {
        Some(self.child.id())
    }
}

impl EngineProcess for EngineSession {
    fn poll_exit(&mut self) -> PollExit {
        match self.child.try_wait() {
            Ok(None) => PollExit::Running,
            Ok(Some(_)) if self.shutdown_requested => PollExit::Clean,
            Ok(Some(_)) => PollExit::Crash,
            Err(_) => PollExit::Crash,
        }
    }

    fn begin_shutdown(&mut self) {
        self.shutdown_requested = true;
        let peer = self.peer.clone();
        tokio::spawn(async move {
            let _ = peer
                .call::<_, Empty>("engine.system.shutdown", &Empty {}, RPC_TIMEOUT)
                .await;
        });
    }

    fn kill(&mut self) {
        self.shutdown_requested = true;
        let _ = self.child.kill();
    }

    fn privilege(&self) -> LivePrivilege {
        privilege_of(Some(self.child.id()))
    }

    fn peer(&self) -> Option<Peer> {
        Some(self.peer.clone())
    }
}

impl Drop for EngineSession {
    fn drop(&mut self) {
        self.peer.close();
        self.stop.close();
        self.hooks.disconnected();
        self.callbacks.abort();
        let _ = self.child.kill();
    }
}

pub(super) struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn id(&self) -> Result<u32, Error> {
        self.0.as_ref().map(Child::id).ok_or(Error::Exited)
    }

    fn into_child(mut self) -> Result<Child, Error> {
        self.0.take().ok_or(Error::Exited)
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
        }
    }
}

pub(super) async fn finish<S, T>(
    mut host: S,
    stop_host: T,
    guard: ChildGuard,
    request: &LaunchRequest,
    callbacks: Arc<dyn HostCallbacks>,
) -> Result<EngineSession, Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let pid = guard.id()?;
    let expected = ProcessInstance::inspect(pid).map_err(Error::Handshake)?;
    let secret = Secret::random();
    let mut config = AuthConfig::current(Role::Host, Channel::Rpc).map_err(Error::Handshake)?;
    config.version = request.version;
    config.stopped = request.stopped;
    write_frame(&mut host, &Bootstrap::new(config.instance.clone(), &secret))
        .await
        .map_err(Error::Handshake)?;
    complete(host, stop_host, guard, config, expected, secret, callbacks).await
}

async fn complete<S, T>(
    host: S,
    stop_host: T,
    guard: ChildGuard,
    mut config: AuthConfig,
    expected: ProcessInstance,
    secret: Secret,
    callbacks: Arc<dyn HostCallbacks>,
) -> Result<EngineSession, Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let server = HostServer::accept(host, &config, &expected, &secret)
        .await
        .map_err(Error::Handshake)?;
    let peer = server.peer();
    callbacks.attached(expected.clone(), peer.clone());
    let task = tokio::spawn(server.run(Arc::clone(&callbacks)));
    config.channel = Channel::Stop;
    let stop = match connect_stop(stop_host, &config, &expected, &secret).await {
        Ok(stop) => stop,
        Err(error) => {
            task.abort();
            callbacks.disconnected();
            return Err(error);
        }
    };
    let child = match guard.into_child() {
        Ok(child) => child,
        Err(error) => {
            task.abort();
            callbacks.disconnected();
            return Err(error);
        }
    };
    Ok(EngineSession {
        peer,
        stop,
        child,
        callbacks: task,
        hooks: callbacks,
        shutdown_requested: false,
    })
}

async fn connect_stop<T>(
    stop_host: T,
    config: &AuthConfig,
    expected: &ProcessInstance,
    secret: &Secret,
) -> Result<Peer, Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let stop_secret = secret.stop_secret().map_err(Error::Handshake)?;
    let (stop, _, _) = Peer::connect(stop_host, config, expected, &stop_secret)
        .await
        .map_err(Error::Handshake)?;
    Ok(stop)
}

#[cfg(unix)]
async fn spawn_inherited(
    request: &LaunchRequest,
    callbacks: Arc<dyn HostCallbacks>,
) -> Result<EngineSession, Error> {
    use std::os::fd::AsRawFd;
    let (host_end, engine_end) = std::os::unix::net::UnixStream::pair()?;
    let (stop_end, stop_engine) = std::os::unix::net::UnixStream::pair()?;
    let guard = spawn_child(
        request,
        &[
            ("--host-fd", engine_end.as_raw_fd()),
            ("--stop-fd", stop_engine.as_raw_fd()),
        ],
        &[engine_end.as_raw_fd(), stop_engine.as_raw_fd()],
    )?;
    drop(engine_end);
    drop(stop_engine);
    host_end.set_nonblocking(true)?;
    stop_end.set_nonblocking(true)?;
    let host = tokio::net::UnixStream::from_std(host_end)?;
    let stop = tokio::net::UnixStream::from_std(stop_end)?;
    finish(host, stop, guard, request, callbacks).await
}

#[cfg(unix)]
async fn spawn_fallback(
    request: &LaunchRequest,
    callbacks: Arc<dyn HostCallbacks>,
) -> Result<EngineSession, Error> {
    use std::os::{fd::AsRawFd, unix::fs::PermissionsExt};
    let directory =
        std::env::temp_dir().join(format!("gcu-host-{}", &Secret::random().hex()[..16]));
    std::fs::create_dir(&directory)?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    let directory_owned = directory.clone();
    let _cleanup = scopeguard_dir(directory_owned);
    let path = directory.join("host.sock");
    let listener = crate::transport::PrivateListener::bind(&path).map_err(Error::Handshake)?;
    let (token_host, token_child) = std::os::unix::net::UnixStream::pair()?;
    let (stop_end, stop_child) = std::os::unix::net::UnixStream::pair()?;
    let path_arg = path.to_str().ok_or(Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "host socket path is not utf-8",
    )))?;
    let mut command = std::process::Command::new(&request.path);
    command.arg("engine");
    command.args(&request.extra_args);
    command.args([
        "--host-socket",
        path_arg,
        "--host-token-fd",
        &token_child.as_raw_fd().to_string(),
        "--stop-fd",
        &stop_child.as_raw_fd().to_string(),
    ]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::unsafe_platform::inherit(
        &mut command,
        vec![token_child.as_raw_fd(), stop_child.as_raw_fd()],
    );
    let guard = ChildGuard(Some(command.spawn()?));
    drop(token_child);
    drop(stop_child);
    let pid = guard.id()?;
    let expected = ProcessInstance::inspect(pid).map_err(Error::Handshake)?;
    token_host.set_nonblocking(true)?;
    stop_end.set_nonblocking(true)?;
    let mut token_host = tokio::net::UnixStream::from_std(token_host)?;
    let stop = tokio::net::UnixStream::from_std(stop_end)?;
    let secret = Secret::random();
    let mut config = AuthConfig::current(Role::Host, Channel::Rpc).map_err(Error::Handshake)?;
    config.version = request.version;
    config.stopped = request.stopped;
    write_frame(
        &mut token_host,
        &Bootstrap::new(config.instance.clone(), &secret).with_endpoint(listener.stamp()),
    )
    .await
    .map_err(Error::Handshake)?;
    drop(token_host);
    let stream = tokio::time::timeout(crate::auth::HANDSHAKE_TIMEOUT, listener.accept(&expected))
        .await
        .map_err(|_| Error::Handshake(ProtocolError::Timeout))?
        .map_err(Error::Handshake)?;
    // The token fd already carried the bootstrap. Accepting the pinned socket
    // must not write a second one or the engine's handshake desynchronizes.
    complete(stream, stop, guard, config, expected, secret, callbacks).await
}

#[cfg(unix)]
fn scopeguard_dir(path: std::path::PathBuf) -> impl Drop {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    Cleanup(path)
}

#[cfg(unix)]
fn spawn_child(
    request: &LaunchRequest,
    fds: &[(&str, i32)],
    inherit: &[i32],
) -> Result<ChildGuard, Error> {
    let mut command = std::process::Command::new(&request.path);
    command.arg("engine");
    command.args(&request.extra_args);
    for (flag, fd) in fds {
        command.arg(*flag);
        command.arg(fd.to_string());
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::unsafe_platform::inherit(&mut command, inherit.to_vec());
    Ok(ChildGuard(Some(command.spawn()?)))
}

#[cfg(windows)]
#[path = "windows.rs"]
mod windows;

#[cfg(unix)]
fn privilege_of(_pid: Option<u32>) -> LivePrivilege {
    LivePrivilege::normal()
}

#[cfg(windows)]
fn privilege_of(pid: Option<u32>) -> LivePrivilege {
    windows::privilege(pid)
}

#[cfg(not(any(unix, windows)))]
fn privilege_of(_pid: Option<u32>) -> LivePrivilege {
    LivePrivilege::unknown()
}
