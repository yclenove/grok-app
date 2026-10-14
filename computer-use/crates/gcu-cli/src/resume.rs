//! Best-effort human resume gate. Never reachable from a model tool.
use crate::{exit::Failure, host::Activity, private};
use gcu_core::session::Session;
use std::{
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const TIMEOUT: Duration = Duration::from_secs(5);
const PREFIX: &str = "gcu-control-";

/// Injectable TTY detection for human-controlled CLI entry. Nonblocking; no
/// timeout or cancellation. It is a usability gate, not proof of human identity.
pub trait IsTty {
    /// Whether standard input is an interactive terminal.
    fn stdin_tty(&self) -> bool;
    /// Whether standard output is an interactive terminal.
    fn stdout_tty(&self) -> bool;
    /// True only if both input and output are interactive terminals.
    fn interactive(&self) -> bool {
        self.stdin_tty() && self.stdout_tty()
    }
}
struct Terminal;
impl IsTty for Terminal {
    fn stdin_tty(&self) -> bool {
        io::stdin().is_terminal()
    }
    fn stdout_tty(&self) -> bool {
        io::stdout().is_terminal()
    }
}
fn tty_gate(tty: &dyn IsTty) -> Result<(), Failure> {
    if !tty.interactive() {
        return Err(Failure::arguments(
            "Resume requires both stdin and stdout to be interactive TTYs",
        ));
    }
    Ok(())
}
fn random() -> String {
    let bytes: [u8; 32] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
struct Credentials {
    token: String,
    challenge: String,
}
struct Auth {
    credentials: Mutex<Credentials>,
    directory: PathBuf,
    activity: Arc<Activity>,
    session: Arc<Session>,
    lifecycle: gcu_mcp::lifecycle::Lifecycle,
    active: std::sync::atomic::AtomicBool,
}
impl Auth {
    fn rotate(&self, c: &mut Credentials) -> io::Result<()> {
        // Remove the old bearer before publishing the replacement. A crash
        // leaves the latch set and the old token unusable.
        c.token.clear();
        c.challenge.clear();
        let _ = std::fs::remove_file(self.directory.join("token"));
        let token = random();
        let challenge = random()[..8].to_owned();
        private::write_secret(&self.directory.join("token"), token.as_bytes())?;
        private::write_secret(&self.directory.join("challenge"), challenge.as_bytes())?;
        *c = Credentials { token, challenge };
        Ok(())
    }
    async fn handle<S: AsyncRead + AsyncWrite + Unpin>(
        self: &Arc<Self>,
        stream: &mut S,
    ) -> io::Result<()> {
        // Exact length framing bounds untrusted input; never read until EOF.
        let size = stream.read_u16().await? as usize;
        if size > 256 {
            return stream.write_all(b"DENIED\n").await;
        }
        let mut data = vec![0; size];
        stream.read_exact(&mut data).await?;
        let request = String::from_utf8_lossy(&data);
        if request == "stop" {
            // Stop needs no bearer: untrusted callers can deny operation but
            // can never resume. Latch before input release, outside all queues.
            self.activity.latch();
            let activity = self.activity.clone();
            let persist = tokio::task::spawn_blocking(move || activity.persist_stop());
            self.session.handle_stop().await;
            let persist = persist.await.map_err(io::Error::other)?;
            stream
                .write_all(if persist.is_ok() {
                    b"OK\n"
                } else {
                    b"FAILED\n"
                })
                .await?;
            return Ok(());
        }
        if self.session.is_closed() {
            return stream.write_all(b"DENIED\n").await;
        }
        if let Some(notification) = request.strip_prefix("turn-ended ") {
            let Some((token, event)) = notification.split_once(' ') else {
                return stream.write_all(b"DENIED\n").await;
            };
            let this = self.clone();
            let token = token.to_owned();
            let valid_event = event.len() <= 128;
            // Resume rotates credentials under this mutex while doing file IO.
            // Authentication must not block the independent Stop event loop.
            let accepted = tokio::task::spawn_blocking(move || {
                let c = this.credentials.lock().unwrap_or_else(|e| e.into_inner());
                this.active.load(std::sync::atomic::Ordering::Acquire)
                    && !c.token.is_empty()
                    && token == c.token
                    && valid_event
            })
            .await
            .map_err(io::Error::other)?;
            if !accepted {
                return stream.write_all(b"DENIED\n").await;
            }
            self.lifecycle
                .turn_ended((!event.is_empty()).then_some(event))
                .await;
            return stream.write_all(b"OK\n").await;
        }
        let this = self.clone();
        let request = request.into_owned();
        let accepted = tokio::task::spawn_blocking(move || {
            if !this.active.load(std::sync::atomic::Ordering::Acquire) || this.session.is_closed() {
                return Ok(false);
            }
            let mut c = this.credentials.lock().unwrap_or_else(|e| e.into_inner());
            let expected = format!("resume {} {}", c.token, c.challenge);
            if c.token.is_empty() || request != expected {
                Ok(false)
            } else {
                // Consume before resuming, so replays fail even if persistence fails.
                this.rotate(&mut c)?;
                if !this.active.load(std::sync::atomic::Ordering::Acquire)
                    || this.session.is_closed()
                {
                    return Ok(false);
                }
                this.activity
                    .resume()
                    .map_err(|_| io::Error::other("resume failed"))?;
                Ok::<_, io::Error>(true)
            }
        })
        .await
        .map_err(io::Error::other)??;
        if !accepted {
            return stream.write_all(b"DENIED\n").await;
        }
        stream.write_all(b"OK\n").await
    }
}

pub(crate) struct Control {
    directory: tempfile::TempDir,
    task: tokio::task::JoinHandle<()>,
    auth: Arc<Auth>,
}
impl Control {
    pub(crate) async fn start_with_lifecycle(
        session: Arc<Session>,
        activity: Arc<Activity>,
        lifecycle: gcu_mcp::lifecycle::Lifecycle,
    ) -> Result<Self, Failure> {
        let directory = private::work(|| {
            private::session_directory(PREFIX)
                .map_err(|_| Failure::internal("Cannot create private resume directory"))
        })
        .await?;
        let auth = Arc::new(Auth {
            credentials: Mutex::new(Credentials {
                token: String::new(),
                challenge: String::new(),
            }),
            directory: directory.path().into(),
            activity,
            session,
            lifecycle,
            active: std::sync::atomic::AtomicBool::new(true),
        });
        let publisher = auth.clone();
        private::work(move || {
            publisher
                .rotate(
                    &mut publisher
                        .credentials
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()),
                )
                .map_err(|_| Failure::internal("Cannot create resume credentials"))
        })
        .await?;
        #[cfg(unix)]
        let listener = {
            let path = directory.path().join("control.sock");
            let listener = tokio::net::UnixListener::bind(&path).map_err(|_| {
                Failure::arguments("Cannot bind private control socket (check runtime path length)")
            })?;
            use std::os::unix::fs::PermissionsExt;
            let directory = directory.path().to_owned();
            private::work(move || {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                    .map_err(|_| Failure::internal("Cannot secure control socket"))?;
                private::endpoint::publish(&directory, None)
            })
            .await?;
            listener
        };
        #[cfg(windows)]
        let listener = {
            let pipe_name = format!(r"\\.\pipe\gcu-{}", random());
            let listener = private::unsafe_platform::pipe(&pipe_name, true)
                .map_err(|_| Failure::internal("Cannot create private control pipe"))?;
            let path = directory.path().to_owned();
            let name = pipe_name.clone();
            private::work(move || {
                private::write_secret(&path.join("endpoint"), name.as_bytes())
                    .map_err(|_| Failure::internal("Cannot publish control pipe"))?;
                private::endpoint::publish(&path, Some(name))
            })
            .await?;
            (listener, pipe_name)
        };
        let task_auth = auth.clone();
        let task = tokio::spawn(async move {
            let auth = task_auth;
            #[cfg(unix)]
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let auth = auth.clone();
                // Each connection has a deadline and its own task; a silent
                // unauthenticated peer cannot block Stop or another resume.
                tokio::spawn(async move {
                    let _ = tokio::time::timeout(TIMEOUT, auth.handle(&mut stream)).await;
                });
            }
            #[cfg(windows)]
            {
                let (mut listener, pipe_name) = listener;
                loop {
                    if listener.connect().await.is_err() {
                        return;
                    }
                    let Ok(next) = private::unsafe_platform::pipe(&pipe_name, false) else {
                        return;
                    };
                    let mut stream = std::mem::replace(&mut listener, next);
                    let auth = auth.clone();
                    tokio::spawn(async move {
                        let _ = tokio::time::timeout(TIMEOUT, auth.handle(&mut stream)).await;
                    });
                }
            }
        });
        tracing::info!(directory = %directory.path().display(), "Human resume control directory (no credentials logged)");
        Ok(Self {
            directory,
            task,
            auth,
        })
    }
    pub(crate) fn directory(&self) -> &Path {
        self.directory.path()
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        self.auth
            .active
            .store(false, std::sync::atomic::Ordering::Release);
        self.task.abort();
    }
}

pub(crate) fn discover(selected: Option<PathBuf>) -> Result<PathBuf, Failure> {
    if let Some(path) = selected {
        private::check_dir(&path)?;
        return Ok(path);
    }
    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(private::runtime_root())
        .map_err(|_| Failure::arguments("Cannot discover MCP sessions"))?
    {
        let entry = entry.map_err(|_| Failure::arguments("Cannot read MCP session entry"))?;
        if entry.file_name().to_string_lossy().starts_with(PREFIX)
            && private::check_dir(&entry.path()).is_ok()
        {
            candidates.push(entry.path());
        }
    }
    if candidates.len() != 1 {
        return Err(Failure::arguments(
            "Expected one running MCP session; specify --session <private directory>",
        ));
    }
    Ok(candidates.remove(0))
}
pub(crate) async fn run(selected: Option<PathBuf>) -> Result<u8, Failure> {
    tty_gate(&Terminal)?;
    let (directory, token, challenge) = private::work(move || {
        let directory = discover(selected)?;
        for name in ["token", "challenge"] {
            private::check_file(&directory.join(name))?;
        }
        let token = std::fs::read_to_string(directory.join("token"))
            .map_err(|_| Failure::arguments("Cannot read resume token"))?;
        let challenge = std::fs::read_to_string(directory.join("challenge"))
            .map_err(|_| Failure::arguments("Cannot read resume challenge"))?;
        if token.len() != 64
            || challenge.len() != 8
            || !token
                .bytes()
                .chain(challenge.bytes())
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(Failure::arguments("Invalid resume credentials"));
        }
        Ok((directory, token, challenge))
    })
    .await?;
    println!("You are resuming stopped model operations. Type yes {challenge} to confirm:");
    io::stdout()
        .flush()
        .map_err(|_| Failure::internal("Cannot display resume confirmation"))?;
    let confirmation = tokio::time::timeout(
        Duration::from_secs(120),
        tokio::task::spawn_blocking(move || {
            let mut input = String::new();
            io::stdin().read_line(&mut input).map(|_| input)
        }),
    )
    .await
    .map_err(|_| Failure::arguments("Resume confirmation timed out"))?
    .map_err(|_| Failure::internal("Terminal input failed"))?
    .map_err(|_| Failure::internal("Cannot read confirmation"))?;
    if confirmation.trim() != format!("yes {challenge}") {
        return Err(Failure::arguments(
            "Resume was not confirmed; remains stopped",
        ));
    }
    tty_gate(&Terminal)?;
    send(&directory, &format!("resume {token} {challenge}")).await?;
    println!("Resumed model operations.");
    Ok(0)
}
pub(crate) async fn read_token(directory: &Path) -> Result<String, Failure> {
    let directory = directory.to_owned();
    private::work(move || {
        private::check_dir(&directory)?;
        private::check_file(&directory.join("token"))?;
        let token = std::fs::read_to_string(directory.join("token"))
            .map_err(|_| Failure::arguments("Cannot read control token"))?;
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Failure::arguments("Invalid control token"));
        }
        Ok(token)
    })
    .await
}
pub(crate) async fn send(directory: &Path, request: &str) -> Result<(), Failure> {
    let path = directory.to_owned();
    let endpoint = private::work(move || private::endpoint::inspect(&path)).await?;
    let result = tokio::time::timeout(TIMEOUT, async {
        #[cfg(unix)]
        let mut stream = tokio::net::UnixStream::connect(directory.join("control.sock")).await?;
        #[cfg(windows)]
        let mut stream =
            { tokio::net::windows::named_pipe::ClientOptions::new().open(&endpoint.name)? };
        private::endpoint::check_peer(&stream, &endpoint)?;
        let path = directory.to_owned();
        private::work(move || private::endpoint::recheck(&path, &endpoint))
            .await
            .map_err(|failure| io::Error::other(failure.message))?;
        stream.write_u16(request.len() as u16).await?;
        stream.write_all(request.as_bytes()).await?;
        let mut response = [0; 3];
        stream.read_exact(&mut response).await?;
        if &response != b"OK\n" {
            return Err(io::Error::other("control request refused"));
        }
        Ok::<_, io::Error>(())
    })
    .await;
    match result {
        Ok(Ok(())) => Ok(()),
        // Only the OS error is added; request text, tokens and challenges never appear.
        Ok(Err(error)) => Err(Failure::arguments(format!(
            "Control authentication/connection failed ({error}); remains stopped"
        ))),
        Err(_elapsed) => Err(Failure::arguments(
            "Control authentication/connection timed out; remains stopped",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcu_host_api::UserActivity;
    struct Tty(bool, bool);
    impl IsTty for Tty {
        fn stdin_tty(&self) -> bool {
            self.0
        }
        fn stdout_tty(&self) -> bool {
            self.1
        }
    }
    #[test]
    fn tty_injected_noninteractive_refused_interactive_allowed() {
        for (input, output) in [(false, false), (true, false), (false, true)] {
            assert!(tty_gate(&Tty(input, output)).is_err());
        }
        assert!(tty_gate(&Tty(true, true)).is_ok());
    }
    #[tokio::test]
    async fn turn_ended_authentication_wait_does_not_block_independent_stop() {
        let fixture = tempfile::TempDir::new().unwrap();
        let host = crate::host::LocalHost::fixture(fixture.path());
        let driver = Arc::new(gcu_driver_fake::FakeDriver::from_scene("form").unwrap());
        let mut connection =
            gcu_mcp::Connection::open(driver.clone(), host.host.clone(), Default::default())
                .await
                .unwrap();
        let control = Control::start_with_lifecycle(
            connection.session(),
            host.activity.clone(),
            connection.lifecycle(),
        )
        .await
        .unwrap_or_else(|error| panic!("{}", error.message));
        let token = std::fs::read_to_string(control.directory().join("token")).unwrap();
        let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let auth = control.auth.clone();
        let holder = tokio::task::spawn_blocking(move || {
            let _credentials = auth.credentials.lock().unwrap();
            locked_tx.send(()).unwrap();
            // Simulate slow credential-file rotation, with a bounded fallback
            // so a regression blocks the runtime for two seconds, not forever.
            let _ = release_rx.recv_timeout(Duration::from_secs(2));
        });
        locked_rx.await.unwrap();
        let (mut server, mut client) = tokio::io::duplex(256);
        let request = format!("turn-ended {token} event");
        client.write_u16(request.len() as u16).await.unwrap();
        client.write_all(request.as_bytes()).await.unwrap();
        let auth = control.auth.clone();
        let started = std::time::Instant::now();
        let notification = tokio::spawn(async move {
            auth.handle(&mut server).await.unwrap();
        });
        tokio::task::yield_now().await;
        send(control.directory(), "stop")
            .await
            .unwrap_or_else(|error| panic!("{}", error.message));
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(host.activity.latched_stop());
        assert!(driver.release_calls() > 0);
        release_tx.send(()).unwrap();
        holder.await.unwrap();
        notification.await.unwrap();
        let mut ack = [0; 3];
        client.read_exact(&mut ack).await.unwrap();
        assert_eq!(&ack, b"OK\n");
        connection.close().await.unwrap();
    }
    #[tokio::test]
    async fn control_unauthenticated_replay_refused_authenticated_clears_latch() {
        let fixture = tempfile::TempDir::new().unwrap();
        let host = crate::host::LocalHost::fixture(fixture.path());
        let driver = Arc::new(gcu_driver_fake::FakeDriver::from_scene("form").unwrap());
        let mut connection =
            gcu_mcp::Connection::open(driver.clone(), host.host.clone(), Default::default())
                .await
                .unwrap();
        let control = Control::start_with_lifecycle(
            connection.session(),
            host.activity.clone(),
            connection.lifecycle(),
        )
        .await
        .unwrap_or_else(|e| panic!("control start failed: {}", e.message));
        send(control.directory(), "stop")
            .await
            .unwrap_or_else(|e| panic!("send failed: {}", e.message));
        assert!(host.activity.latched_stop());
        assert!(crate::host::LocalHost::fixture(fixture.path())
            .activity
            .latched_stop());
        assert!(driver.release_calls() > 0);
        let queued = tokio::spawn({
            let session = connection.session();
            async move { session.enqueue(std::future::pending::<()>()).await }
        });
        // The queue is intentionally occupied; Stop must still issue release.
        tokio::task::yield_now().await;
        let start = tokio::time::Instant::now();
        send(control.directory(), "stop")
            .await
            .unwrap_or_else(|e| panic!("send failed: {}", e.message));
        assert!(start.elapsed() < Duration::from_millis(500));
        queued.abort();
        assert!(send(control.directory(), "resume invalid invalid")
            .await
            .is_err());
        assert!(host.activity.latched_stop());
        let token = std::fs::read_to_string(control.directory().join("token")).unwrap();
        let challenge = std::fs::read_to_string(control.directory().join("challenge")).unwrap();
        let request = format!("resume {token} {challenge}");
        assert!(tty_gate(&Tty(true, true)).is_ok());
        send(control.directory(), &request)
            .await
            .unwrap_or_else(|e| panic!("send failed: {}", e.message));
        assert!(!host.activity.latched_stop());
        assert!(!crate::host::LocalHost::fixture(fixture.path())
            .activity
            .latched_stop());
        send(control.directory(), "stop")
            .await
            .unwrap_or_else(|e| panic!("send failed: {}", e.message));
        assert!(send(control.directory(), &request).await.is_err());
        assert!(host.activity.latched_stop());
        let token = std::fs::read_to_string(control.directory().join("token")).unwrap();
        let challenge = std::fs::read_to_string(control.directory().join("challenge")).unwrap();
        send(control.directory(), &format!("resume {token} {challenge}"))
            .await
            .unwrap_or_else(|e| panic!("send failed: {}", e.message));
        connection.close().await.unwrap();
    }
}
