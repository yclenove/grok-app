//! Independent Stop and persisted latch. Never waits on the supervisor/RPC lock.
use crate::{
    messages::{Empty, Resume, Stop},
    server::Peer,
    Error,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Storage is called on a blocking worker, except the initial startup read.
pub trait LatchStore: Send + Sync + 'static {
    /// Missing storage means no prior Stop. Unreadable storage must fail closed.
    fn read(&self) -> std::io::Result<bool>;
    /// Persist the latest latch. A failed write must never clear Stop.
    fn write(&self, stopped: bool) -> std::io::Result<()>;
}
/// App-private Stop file. Data contains only a boolean; no credentials or sessions.
pub struct FileLatch(pub PathBuf);
impl LatchStore for FileLatch {
    fn read(&self) -> std::io::Result<bool> {
        use std::io::Read;
        let read = || {
            let file = std::fs::File::open(&self.0)?;
            let mut bytes = Vec::with_capacity(9);
            file.take(9).read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        };
        match read() {
            Ok(bytes) if bytes == b"stopped\n" => Ok(true),
            Ok(bytes) if bytes == b"running\n" => Ok(false),
            Ok(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid Stop latch",
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
    fn write(&self, stopped: bool) -> std::io::Result<()> {
        use std::io::Write;
        if let Some(parent) = self.0.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.0)?;
        file.write_all(if stopped { b"stopped\n" } else { b"running\n" })?;
        file.sync_all()
    }
}
struct State {
    stopped: bool,
    revision: u64,
    sequence: u64,
    peer: Option<Peer>,
    runtime: Option<tokio::runtime::Handle>,
}
struct Inner {
    state: Mutex<State>,
    writes: Mutex<()>,
    store: Arc<dyn LatchStore>,
}
/// Cloneable control owner. State locks contain no IO or await points.
#[derive(Clone)]
pub struct Safety(Arc<Inner>);
struct ResumeGuard {
    safety: Safety,
    peer: Peer,
    armed: bool,
}
impl Drop for ResumeGuard {
    fn drop(&mut self) {
        if self.armed {
            self.safety.latch();
            self.peer.close();
            let safety = self.safety.clone();
            let runtime = lock(&safety.0.state).runtime.clone();
            if let Some(runtime) = runtime {
                runtime.spawn(async move {
                    // A failed persist keeps the in-memory latch set. A corrupt
                    // or unreadable file is also stopped on the next startup.
                    if safety.persist().await.is_err() {
                        safety.latch();
                    }
                });
            }
        }
    }
}
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
impl Safety {
    /// Startup read only. An unreadable latch is stopped; it is never reset.
    pub fn new(store: Arc<dyn LatchStore>) -> Self {
        let stopped = store.read().unwrap_or(true);
        Self(Arc::new(Inner {
            state: Mutex::new(State {
                stopped,
                revision: 0,
                sequence: 0,
                peer: None,
                runtime: None,
            }),
            writes: Mutex::new(()),
            store,
        }))
    }
    /// Nonblocking memory read used for every new engine handshake.
    pub fn stopped(&self) -> bool {
        lock(&self.0.state).stopped
    }
    /// Whether the dedicated authenticated control peer is connected.
    pub fn connected(&self) -> bool {
        lock(&self.0.state)
            .peer
            .as_ref()
            .is_some_and(Peer::connected)
    }
    /// Install from the peer's Tokio runtime. Native UI callers subsequently
    /// need no runtime context. Old-peer disconnect cannot clear Stop.
    pub fn attach(&self, peer: Peer) {
        let mut state = lock(&self.0.state);
        state.revision = state.revision.wrapping_add(1);
        state.sequence = 0;
        // Native UI callbacks may run outside Tokio. Retain the authenticated
        // connection's runtime so their synchronous Stop never panics.
        state.runtime = tokio::runtime::Handle::try_current().ok();
        if state.stopped
            && peer
                .notify("engine.session.stop", &Stop { session_id: None })
                .is_err()
        {
            peer.close();
        }
        state.peer = Some(peer);
    }
    /// Latch immediately (before any await), then enqueue Stop independently of actions.
    /// A failed/full channel is closed; the engine treats its loss as Stop.
    pub fn latch(&self) {
        let mut state = lock(&self.0.state);
        state.stopped = true;
        state.revision = state.revision.wrapping_add(1);
        if let Some(peer) = &state.peer {
            match peer.start_call("engine.session.stop", &Stop { session_id: None }) {
                Ok(reply) => {
                    if let Some(runtime) = &state.runtime {
                        let peer = peer.clone();
                        runtime.spawn(async move {
                            let result: Result<Empty, Error> =
                                reply.wait(Duration::from_millis(500)).await;
                            if result.is_err() {
                                peer.close();
                            }
                        });
                    } else {
                        peer.close();
                    }
                }
                Err(_) => peer.close(),
            }
        }
    }
    /// Persist off the async/UI thread with a five-second caller budget.
    /// Cancellation never clears the latch; an OS write already in progress
    /// finishes on its worker, serialized before any later persistence.
    pub async fn persist(&self) -> Result<(), Error> {
        let inner = self.0.clone();
        let write = tokio::task::spawn_blocking(move || {
            let _writes = lock(&inner.writes);
            let stopped = lock(&inner.state).stopped;
            inner.store.write(stopped)
        });
        tokio::time::timeout(Duration::from_secs(5), write)
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Disconnected)?
            .map_err(Error::Io)
    }
    /// Only a trusted local user control invokes this. Fresh sequence/nonce are
    /// Host-generated. A concurrent Stop, failed persistence, timeout or channel
    /// replacement keeps Stop latched. Five hundred millisecond RPC deadline.
    pub async fn resume_by_user(&self) -> Result<(), Error> {
        let (peer, revision, sequence) = {
            let mut state = lock(&self.0.state);
            let peer = state
                .peer
                .clone()
                .filter(Peer::connected)
                .ok_or(Error::Disconnected)?;
            state.sequence = state.sequence.checked_add(1).ok_or(Error::Invalid)?;
            (peer, state.revision, state.sequence)
        };
        let mut guard = ResumeGuard {
            safety: self.clone(),
            peer: peer.clone(),
            armed: true,
        };
        let result: Result<Empty, Error> = peer
            .call(
                "engine.session.resume",
                &Resume {
                    session_id: None,
                    sequence,
                    user_event: rand::random(),
                },
                Duration::from_millis(500),
            )
            .await;
        if let Err(error) = result {
            self.latch();
            peer.close();
            return Err(error);
        }
        {
            let mut state = lock(&self.0.state);
            if state.revision != revision {
                drop(state);
                self.latch();
                return Err(Error::Invalid);
            }
            state.stopped = false;
        }
        if let Err(error) = self.persist().await {
            self.latch();
            return Err(error);
        }
        guard.armed = false;
        Ok(())
    }
}
