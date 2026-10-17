//! App-owned turn/session boundaries. No core/driver dependency and no resume API.
//!
//! Endpoint mutations and boundary RPCs serialize here; Stop uses its separate
//! control channel. Engine replacement invalidates credentials synchronously.
use crate::{
    approval::Office,
    auth::ProcessInstance,
    events::{Fault, Gallery, Role},
    inject::{Issued, TokenBook},
    messages::{
        Empty, Image, Request, RpcError, SessionId, SessionKind, SessionOpen, SessionOpened,
    },
    server::{CallbackFuture, HostCallbacks, Peer, RPC_TIMEOUT},
    Error,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

/// Opaque prompt boundary. A completion from a closed/replaced session is inert.
#[derive(Clone)]
pub struct Turn {
    session: String,
    nonce: [u8; 32],
}

struct State {
    epoch: u64,
    peer: Option<Peer>,
    book: TokenBook,
    turns: HashMap<String, [u8; 32]>,
    reconnect: HashSet<String>,
    closed: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}
struct Inner {
    state: Mutex<State>,
    gate: tokio::sync::Mutex<()>,
    office: Office,
    gallery: Gallery,
}
/// Shared lifecycle owner for all App sessions. Clones share the same state.
#[derive(Clone)]
pub struct Lifecycle(Arc<Inner>);

impl Lifecycle {
    /// Create a disconnected owner. No IO, timers, or automatic recovery of Stop.
    pub fn new(office: Office, gallery: Gallery) -> Self {
        Self(Arc::new(Inner {
            state: Mutex::new(State {
                epoch: 0,
                peer: None,
                book: TokenBook::new(),
                turns: HashMap::new(),
                reconnect: HashSet::new(),
                closed: None,
            }),
            gate: tokio::sync::Mutex::new(()),
            office,
            gallery,
        }))
    }

    /// Add generation checks around callbacks, preserving independent Stop hooks.
    pub fn observe(&self, inner: Arc<dyn HostCallbacks>) -> Arc<dyn HostCallbacks> {
        Arc::new(Callbacks {
            owner: self.clone(),
            epoch: Mutex::new(0),
            inner,
        })
    }

    /// Install the App's single close-notice consumer. No IO or waits; notices
    /// also cover engine replacement so UI image caches cannot outlive a session.
    pub fn subscribe_closed(&self) -> tokio::sync::mpsc::UnboundedReceiver<String> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        lock(&self.0.state).closed = Some(tx);
        rx
    }

    /// Store a full image only for the exact namespace that fetched it. Blocking
    /// filesystem IO; call on a worker. Close/reopen rejects a late read response.
    pub fn admit_full(
        &self,
        session: &str,
        credential: &Issued,
        id: &str,
        image: &Image,
    ) -> Result<std::path::PathBuf, Fault> {
        let state = lock(&self.0.state);
        if !state
            .book
            .current(session)
            .is_some_and(|issued| issued.token == credential.token)
            || !state.peer.as_ref().is_some_and(Peer::connected)
        {
            return Err(Fault::Missing);
        }
        self.0.gallery.admit(session, id, image, Role::Full)
    }

    /// Current credential, only while its authenticated engine is connected.
    pub fn current(&self, session: &str) -> Option<Issued> {
        let state = lock(&self.0.state);
        state.peer.as_ref().filter(|peer| peer.connected())?;
        state.book.current(session).cloned()
    }

    /// Live namespaces for the supervisor idle count. No IO or waiting.
    pub fn live_count(&self) -> u32 {
        lock(&self.0.state).book.live_count()
    }

    /// The ACP still holds a revoked endpoint and must reopen with fresh MCP
    /// headers before its next prompt. No IO; busy turns are left to finish.
    pub fn needs_reconnect(&self, session: &str) -> bool {
        let state = lock(&self.0.state);
        state.reconnect.contains(session)
            || (state.book.current(session).is_some()
                && !state.peer.as_ref().is_some_and(Peer::connected))
    }

    /// A newly constructed ACP catalog intentionally omits CU (disabled or
    /// nonlocal). Acknowledge that the old header is gone without opening one.
    pub fn catalog_cleared(&self, session: &str) {
        let mut state = lock(&self.0.state);
        if state.book.current(session).is_none() {
            state.reconnect.remove(session);
        }
    }

    /// Open or reuse a local namespace. Five-second RPC and serialized boundary
    /// waits; cancellation never installs a partially received credential.
    pub async fn open(&self, session: &str) -> Result<Issued, Error> {
        let _gate = self.0.gate.lock().await;
        if let Some(issued) = self.current(session) {
            return Ok(issued);
        }
        let (epoch, peer) = self.connection()?;
        let mut guard = BoundaryGuard(Some(peer.clone()));
        let opened: SessionOpened = peer
            .call(
                "engine.session.open",
                &SessionOpen {
                    session_id: session.to_owned(),
                    kind: SessionKind::Local,
                },
                RPC_TIMEOUT,
            )
            .await?;
        let gallery = self.0.gallery.clone();
        let id = session.to_owned();
        tokio::task::spawn_blocking(move || gallery.open_session(&id))
            .await
            .map_err(|_| Error::Disconnected)?;
        let mut state = lock(&self.0.state);
        if state.epoch != epoch || !peer.connected() {
            return Err(Error::Disconnected);
        }
        let issued = Issued::from(&opened);
        state.book.issue(session, issued.clone());
        state.reconnect.remove(session);
        guard.0 = None;
        Ok(issued)
    }

    /// Capture before dispatching one ACP prompt. No IO; absent CU is a no-op.
    pub fn begin_turn(&self, session: &str) -> Option<Turn> {
        let mut state = lock(&self.0.state);
        state.book.current(session)?;
        let nonce = rand::random();
        state.turns.insert(session.to_owned(), nonce);
        Some(Turn {
            session: session.to_owned(),
            nonce,
        })
    }

    /// Forward the terminal ACP prompt result (success, cancel, or error) once.
    /// Clears pending approvals; the engine clears turn grants/input/overlay.
    /// Five-second RPC budget. Failure closes the peer so authority fails closed.
    pub async fn end_turn(&self, turn: Turn) -> Result<(), Error> {
        let _gate = self.0.gate.lock().await;
        let peer = {
            let mut state = lock(&self.0.state);
            if state.turns.get(&turn.session) != Some(&turn.nonce) {
                return Ok(());
            }
            state.turns.remove(&turn.session);
            self.0.office.close_session(&turn.session);
            state.peer.clone().ok_or(Error::Disconnected)?
        };
        self.boundary(&peer, "engine.session.turnEnded", &turn.session)
            .await
    }

    /// Revoke before awaiting engine teardown. Unknown/duplicate closes are inert.
    /// Denies approvals, blocks late image writes, then asks the engine to close
    /// grants, queues, subscriptions, workers, portals and restore tokens.
    /// Five-second RPC budget; failure disconnects the engine. Stop is untouched.
    pub async fn close(&self, session: &str) -> Result<(), Error> {
        let _gate = self.0.gate.lock().await;
        let peer = {
            let mut state = lock(&self.0.state);
            let live = state.book.revoke(session).is_some();
            if live {
                state.reconnect.insert(session.to_owned());
            }
            state.turns.remove(session);
            self.0.office.close_session(session);
            self.0.gallery.block_session(session);
            if live {
                if let Some(tx) = &state.closed {
                    let _ = tx.send(session.to_owned());
                }
                state.peer.clone()
            } else {
                None
            }
        };
        // Schedule before awaiting RPC so cancellation cannot strand files.
        let cleanup = self.purge(vec![session.to_owned()]);
        let result = match peer {
            Some(peer) => self.boundary(&peer, "engine.session.close", session).await,
            None => Ok(()),
        };
        let _ = cleanup.await;
        result
    }

    /// Feature-off cleanup. Serialized per-session closes keep other users of
    /// the shared engine isolated. Never calls the trusted resume channel.
    pub async fn close_all(&self) -> Result<(), Error> {
        let ids = lock(&self.0.state).book.live_ids();
        let mut result = Ok(());
        for id in ids {
            if let Err(error) = self.close(&id).await {
                result = Err(error);
            }
        }
        result
    }

    fn connection(&self) -> Result<(u64, Peer), Error> {
        let state = lock(&self.0.state);
        let peer = state
            .peer
            .clone()
            .filter(Peer::connected)
            .ok_or(Error::Disconnected)?;
        Ok((state.epoch, peer))
    }

    async fn boundary(&self, peer: &Peer, method: &str, session: &str) -> Result<(), Error> {
        let mut guard = BoundaryGuard(Some(peer.clone()));
        let result = peer
            .call::<_, Empty>(
                method,
                &SessionId {
                    session_id: session.to_owned(),
                },
                RPC_TIMEOUT,
            )
            .await;
        if result.is_ok() {
            guard.0 = None;
        }
        result.map(|_| ())
    }

    fn invalidate(&self, state: &mut State) -> Vec<String> {
        let ids = state.book.live_ids();
        state.book.revoke_all();
        state.turns.clear();
        for id in &ids {
            state.reconnect.insert(id.clone());
            self.0.office.close_session(id);
            self.0.gallery.block_session(id);
            if let Some(tx) = &state.closed {
                let _ = tx.send(id.clone());
            }
        }
        ids
    }

    fn purge(&self, ids: Vec<String>) -> tokio::task::JoinHandle<()> {
        let gallery = self.0.gallery.clone();
        tokio::task::spawn_blocking(move || {
            for id in ids {
                gallery.purge_closed(&id);
            }
        })
    }
}

// Cancellation is ambiguous once an RPC is sent. Losing the trusted connection
// makes the engine revoke all endpoints instead of keeping an orphan namespace.
struct BoundaryGuard(Option<Peer>);
impl Drop for BoundaryGuard {
    fn drop(&mut self) {
        if let Some(peer) = &self.0 {
            peer.close();
        }
    }
}

struct Callbacks {
    owner: Lifecycle,
    epoch: Mutex<u64>,
    inner: Arc<dyn HostCallbacks>,
}
impl Callbacks {
    fn live(&self, state: &State, request: &Request) -> bool {
        state.epoch == *lock(&self.epoch)
            && state.peer.is_some()
            && request
                .params
                .get("session_id")
                .and_then(|v| v.as_str())
                .is_none_or(|id| state.book.current(id).is_some())
    }
}
impl HostCallbacks for Callbacks {
    fn handle(&self, request: Request) -> CallbackFuture<'_> {
        let checked = request.clone();
        let (credential, turn) = {
            let state = lock(&self.owner.0.state);
            let session = checked.params.get("session_id").and_then(|v| v.as_str());
            (
                session
                    .and_then(|id| state.book.current(id))
                    .map(|issued| issued.token.clone()),
                session.and_then(|id| state.turns.get(id)).copied(),
            )
        };
        let mut future = self.inner.handle(request);
        Box::pin(std::future::poll_fn(move |cx| {
            // Serialize the actual poll/bind with close; no mutex is held while
            // a pending approval waits for its user response.
            let state = lock(&self.owner.0.state);
            let current = checked
                .params
                .get("session_id")
                .and_then(|v| v.as_str())
                .and_then(|id| state.book.current(id))
                .map(|issued| &issued.token);
            let current_turn = checked
                .params
                .get("session_id")
                .and_then(|v| v.as_str())
                .and_then(|id| state.turns.get(id))
                .copied();
            if !self.live(&state, &checked)
                || current != credential.as_ref()
                || current_turn != turn
            {
                return std::task::Poll::Ready(Err(RpcError::new(-32004, "closed session")));
            }
            std::future::Future::poll(future.as_mut(), cx)
        }))
    }
    fn notification(&self, request: &Request) -> Option<Result<serde_json::Value, RpcError>> {
        let state = lock(&self.owner.0.state);
        if !self.live(&state, request) {
            return Some(Err(RpcError::new(-32004, "closed session")));
        }
        self.inner.notification(request)
    }
    fn attached(&self, engine: ProcessInstance, peer: Peer) {
        let mut state = lock(&self.owner.0.state);
        let ids = self.owner.invalidate(&mut state);
        if let Some(previous) = state.peer.take() {
            previous.close();
        }
        state.epoch = state.epoch.wrapping_add(1);
        *lock(&self.epoch) = state.epoch;
        state.peer = Some(peer.clone());
        self.inner.attached(engine, peer);
        drop(state);
        let _cleanup = self.owner.purge(ids);
    }
    fn stop_state(&self) -> Option<bool> {
        self.inner.stop_state()
    }
    fn control_attached(&self, peer: Peer) {
        let state = lock(&self.owner.0.state);
        if state.epoch == *lock(&self.epoch) && state.peer.is_some() {
            self.inner.control_attached(peer);
        } else {
            peer.close();
        }
    }
    fn disconnected(&self) {
        let mut state = lock(&self.owner.0.state);
        if state.epoch != *lock(&self.epoch) {
            return;
        }
        let ids = self.owner.invalidate(&mut state);
        state.peer = None;
        self.inner.disconnected();
        drop(state);
        let _cleanup = self.owner.purge(ids);
    }
}
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
