//! Rust test Host for process-level engine tests; never included by default.
use crate::{auth::*, messages::*, server::*, Error};
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
/// One observed Host callback, including its one-shot user response channel.
pub struct TestRequest {
    /// Exact authenticated engine request/notification.
    pub request: Request,
    answer: Option<oneshot::Sender<Result<Value, RpcError>>>,
}
impl TestRequest {
    /// Consume one real test user response. Duplicate responses are impossible.
    /// Serialization errors deny the request. No waiting or cancellation.
    pub fn reply<T: Serialize>(mut self, value: T) {
        if let Some(answer) = self.answer.take() {
            let result = serde_json::to_value(value)
                .map_err(|_| RpcError::new(-32603, "invalid test response"));
            let _ = answer.send(result);
        }
    }
}
struct Callbacks {
    sender: mpsc::Sender<TestRequest>,
    policy: Arc<Mutex<Policy>>,
}
impl HostCallbacks for Callbacks {
    fn handle(&self, request: Request) -> CallbackFuture<'_> {
        Box::pin(async move {
            if request.method == "host.policy.load" {
                return serde_json::to_value(
                    self.policy
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone(),
                )
                .map_err(|_| RpcError::new(-32603, "test policy"));
            }
            let (tx, rx) = oneshot::channel();
            let notification = request.id.is_none();
            self.sender
                .try_send(TestRequest {
                    request,
                    answer: if notification { None } else { Some(tx) },
                })
                .map_err(|_| RpcError::new(-32002, "test Host busy"))?;
            if notification {
                Ok(serde_json::json!({}))
            } else {
                rx.await
                    .unwrap_or_else(|_| Err(RpcError::new(-32000, "test Host disconnected")))
            }
        })
    }
    fn disconnected(&self) {}
}
/// Conservative test policy with no persistent grants and foreground asking.
pub fn policy() -> Policy {
    Policy {
        version: 1,
        always_allow_enabled: true,
        sensitive_action_confirmation: ConfirmationLevel::Standard,
        timeout_secs: 120,
        allow_foreground: ForegroundPolicy::Ask,
        macos_private_spi: true,
        user_idle_ms: 3000,
        stop_hotkey: "default".into(),
        max_tree_nodes: 1200,
        max_image_edge: 1568,
        image_format: "png".into(),
        extra_denied_apps: vec![],
        always: vec![],
    }
}
/// Process-level test Host. Owns/terminates the child and both private streams.
#[cfg(unix)]
pub struct TestHost {
    /// Ordinary control RPC (resume/stop are rejected here).
    pub peer: Peer,
    /// Independent authenticated user-control RPC.
    pub stop: Peer,
    /// Engine subprocess, killed on drop.
    pub child: tokio::process::Child,
    /// Mutable App-owned policy used by host.policy.load.
    pub policy: Arc<Mutex<Policy>>,
    requests: mpsc::Receiver<TestRequest>,
    callbacks: tokio::task::JoinHandle<()>,
}
#[cfg(unix)]
impl TestHost {
    /// Start gcu engine --driver fake with inherited socketpairs and the current
    /// protocol version. Five-second handshakes; killed on failure/drop.
    pub async fn spawn(binary: &std::path::Path, scene: &str) -> Result<Self, Error> {
        Self::spawn_version(binary, scene, crate::messages::VERSION, false).await
    }
    /// Start with custom version/persisted Stop for negative/restart tests.
    /// Same timeout and lifetime semantics as spawn.
    pub async fn spawn_version(
        binary: &std::path::Path,
        scene: &str,
        version: Version,
        stopped: bool,
    ) -> Result<Self, Error> {
        use std::{os::fd::AsRawFd, process::Stdio};
        let (host, engine) = std::os::unix::net::UnixStream::pair()?;
        let (stop_host, stop_engine) = std::os::unix::net::UnixStream::pair()?;
        let mut command = std::process::Command::new(binary);
        command
            .args([
                "engine",
                "--driver",
                "fake",
                "--scene",
                scene,
                "--host-fd",
                &engine.as_raw_fd().to_string(),
                "--stop-fd",
                &stop_engine.as_raw_fd().to_string(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        crate::unsafe_platform::inherit(
            &mut command,
            vec![engine.as_raw_fd(), stop_engine.as_raw_fd()],
        );
        let mut command = tokio::process::Command::from(command);
        command.kill_on_drop(true);
        let child = command.spawn()?;
        drop(engine);
        drop(stop_engine);
        let expected = ProcessInstance::inspect(child.id().ok_or(Error::Disconnected)?)?;
        host.set_nonblocking(true)?;
        stop_host.set_nonblocking(true)?;
        let mut host = tokio::net::UnixStream::from_std(host)?;
        let stop_host = tokio::net::UnixStream::from_std(stop_host)?;
        let secret = Secret::random();
        let mut config = AuthConfig::current(Role::Host, Channel::Rpc)?;
        config.version = version;
        config.stopped = stopped;
        crate::codec::write_frame(&mut host, &Bootstrap::new(config.instance.clone(), &secret))
            .await?;
        let server = HostServer::accept(host, &config, &expected, &secret).await?;
        let peer = server.peer();
        let policy = Arc::new(Mutex::new(policy()));
        let (sender, requests) = mpsc::channel(128);
        let callbacks = tokio::spawn(server.run(Arc::new(Callbacks {
            sender,
            policy: policy.clone(),
        })));
        config.channel = Channel::Stop;
        let (stop, _, _) =
            Peer::connect(stop_host, &config, &expected, &secret.stop_secret()?).await?;
        Ok(Self {
            peer,
            stop,
            child,
            policy,
            requests,
            callbacks,
        })
    }
    /// Observe the next callback (five-second ceiling). Does not auto-approve.
    pub async fn next(&mut self) -> Result<TestRequest, Error> {
        tokio::time::timeout(RPC_TIMEOUT, self.requests.recv())
            .await
            .map_err(|_| Error::Timeout)?
            .ok_or(Error::Disconnected)
    }
    /// Wait for a particular method, discarding only unrelated notifications.
    /// Unexpected requests fail rather than silently granting anything.
    pub async fn next_method(&mut self, method: &str) -> Result<TestRequest, Error> {
        loop {
            let event = self.next().await?;
            if event.request.method == method {
                return Ok(event);
            }
            if event.request.id.is_some() {
                return Err(Error::Invalid);
            }
        }
    }
    /// Disconnect both transports. Engine must revoke endpoints and exit.
    pub fn disconnect(&self) {
        self.peer.close();
        self.stop.close();
    }
}
#[cfg(unix)]
impl Drop for TestHost {
    fn drop(&mut self) {
        self.disconnect();
        self.callbacks.abort();
        let _ = self.child.start_kill();
    }
}
/// Mark exactly the supplied owned descriptors inheritable in a Unix child.
/// The caller keeps them alive until spawn and closes its copies afterwards.
/// No IO wait; post-fork work only uses async-signal-safe fcntl.
#[cfg(unix)]
pub fn inherit_fds(command: &mut std::process::Command, fds: Vec<i32>) {
    crate::unsafe_platform::inherit(command, fds);
}
#[cfg(unix)]
impl TestHost {
    /// Launch through the pinned UDS fallback, keeping token and Stop on separate
    /// inherited descriptors. Five-second handshake; fixture paths only.
    pub async fn spawn_fallback(binary: &std::path::Path, scene: &str) -> Result<Self, Error> {
        use std::{
            os::{fd::AsRawFd, unix::fs::PermissionsExt},
            process::Stdio,
        };
        let directory =
            std::env::temp_dir().join(format!("gcu-hf-{}", &Secret::random().hex()[..8]));
        std::fs::create_dir(&directory)?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(directory.clone());
        let path = directory.join("host.sock");
        let listener = crate::transport::PrivateListener::bind(&path)?;
        let (token_host, token_child) = std::os::unix::net::UnixStream::pair()?;
        let (stop_host, stop_child) = std::os::unix::net::UnixStream::pair()?;
        let mut command = std::process::Command::new(binary);
        command
            .args([
                "engine",
                "--driver",
                "fake",
                "--scene",
                scene,
                "--host-socket",
                path.to_str().ok_or(Error::Invalid)?,
                "--host-token-fd",
                &token_child.as_raw_fd().to_string(),
                "--stop-fd",
                &stop_child.as_raw_fd().to_string(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        crate::unsafe_platform::inherit(
            &mut command,
            vec![token_child.as_raw_fd(), stop_child.as_raw_fd()],
        );
        let mut command = tokio::process::Command::from(command);
        command.kill_on_drop(true);
        let child = command.spawn()?;
        drop(token_child);
        drop(stop_child);
        let expected = ProcessInstance::inspect(child.id().ok_or(Error::Disconnected)?)?;
        let mut config = AuthConfig::current(Role::Host, Channel::Rpc)?;
        let secret = Secret::random();
        token_host.set_nonblocking(true)?;
        stop_host.set_nonblocking(true)?;
        let mut token_host = tokio::net::UnixStream::from_std(token_host)?;
        let stop_host = tokio::net::UnixStream::from_std(stop_host)?;
        crate::codec::write_frame(
            &mut token_host,
            &Bootstrap::new(config.instance.clone(), &secret).with_endpoint(listener.stamp()),
        )
        .await?;
        drop(token_host);
        let stream = tokio::time::timeout(RPC_TIMEOUT, listener.accept(&expected))
            .await
            .map_err(|_| Error::Timeout)??;
        let server = HostServer::accept(stream, &config, &expected, &secret).await?;
        let peer = server.peer();
        let policy = Arc::new(Mutex::new(policy()));
        let (sender, requests) = mpsc::channel(128);
        let callbacks = tokio::spawn(server.run(Arc::new(Callbacks {
            sender,
            policy: policy.clone(),
        })));
        config.channel = Channel::Stop;
        let (stop, _, _) =
            Peer::connect(stop_host, &config, &expected, &secret.stop_secret()?).await?;
        Ok(Self {
            peer,
            stop,
            child,
            policy,
            requests,
            callbacks,
        })
    }
}
