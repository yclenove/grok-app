//! Bounded bidirectional RPC and external Host callback dispatch.
use crate::{
    auth::{authenticate, AuthConfig, Authenticated, ProcessInstance, Secret},
    codec::{read_frame, write_frame},
    messages::*,
    Error,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, oneshot, watch},
    task::JoinSet,
};
/// Maximum queued frames, pending requests, and concurrent callbacks.
pub const QUEUE_LIMIT: usize = 32;
/// Default ordinary request timeout. Approvals may use their policy deadline.
pub const RPC_TIMEOUT: Duration = Duration::from_secs(5);
type Answer = Result<Value, i32>;
struct State {
    next: u64,
    pending: HashMap<u64, oneshot::Sender<Answer>>,
}
struct Inner {
    outgoing: mpsc::Sender<Outbound>,
    closed: watch::Sender<bool>,
    state: Mutex<State>,
    minor: u16,
}
struct Outbound {
    value: Envelope,
    flushed: Option<oneshot::Sender<()>>,
}
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
/// Cloneable authenticated connection. Call close when the owner shuts down;
/// outstanding calls fail immediately, including those waiting for approvals.
#[derive(Clone)]
pub struct Peer(Arc<Inner>);
/// Inbound requests for the authenticated direction only.
pub struct Incoming(mpsc::Receiver<Request>);
impl Incoming {
    /// Wait for the next request, or None at disconnect. Cancel by dropping.
    pub async fn recv(&mut self) -> Option<Request> {
        self.0.recv().await
    }
}
impl Peer {
    /// Mutually authenticate a private stream, then start bounded reader/writer
    /// tasks. Five-second handshake deadline. Failure consumes the connection;
    /// never retry the same one-shot secret with another peer.
    pub async fn connect<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        mut stream: S,
        config: &AuthConfig,
        expected: &ProcessInstance,
        secret: &Secret,
    ) -> Result<(Self, Incoming, Authenticated), Error> {
        let authenticated = authenticate(&mut stream, config, expected, secret).await?;
        let (outgoing, mut output) = mpsc::channel(QUEUE_LIMIT);
        let (input, incoming) = mpsc::channel(QUEUE_LIMIT);
        let (closed, _) = watch::channel(false);
        let minor = authenticated.version.minor;
        let this = Self(Arc::new(Inner {
            outgoing,
            closed,
            state: Mutex::new(State {
                next: 1,
                pending: HashMap::new(),
            }),
            minor,
        }));
        let (mut reader, mut writer) = tokio::io::split(stream);
        let write_peer = this.clone();
        tokio::spawn(async move {
            let mut end = write_peer.0.closed.subscribe();
            loop {
                tokio::select! { biased;
                    _ = async { let _=end.wait_for(|v|*v).await; } => break,
                    value = output.recv() => {
                        let Some(value) = value else { break };
                        let Outbound { value, flushed } = value;
                        if !matches!(tokio::time::timeout(RPC_TIMEOUT, write_frame(&mut writer,&value)).await, Ok(Ok(()))) { break; }
                        if let Some(flushed) = flushed { let _ = flushed.send(()); }
                    }
                }
            }
            write_peer.close();
        });
        let read_peer = this.clone();
        let is_host = config.role == crate::auth::Role::Host;
        let channel = config.channel;
        tokio::spawn(async move {
            let mut end = read_peer.0.closed.subscribe();
            let mut last_id = 0;
            loop {
                let value = tokio::select! { biased;
                    _ = async { let _=end.wait_for(|v|*v).await; } => break,
                    value = read_frame::<_,Envelope>(&mut reader) => value,
                };
                let Ok(Some(value)) = value else { break };
                if value.validate().is_err() {
                    break;
                }
                match value {
                    Envelope::Request(request) => {
                        if let Some(id) = request.id {
                            if id <= last_id {
                                break;
                            }
                            last_id = id;
                        }
                        let permitted = inbound_permitted(
                            is_host,
                            channel == crate::auth::Channel::Stop,
                            read_peer.0.minor,
                            &request,
                        );
                        if !permitted {
                            read_peer.respond(
                                &request,
                                Err(RpcError::new(-32601, "method unavailable on this channel")),
                            );
                            continue;
                        }
                        if validate_params(&request).is_err() {
                            read_peer.respond(
                                &request,
                                Err(RpcError::new(-32602, "invalid parameters")),
                            );
                            continue;
                        }
                        if let Err(e) = input.try_send(request) {
                            read_peer.respond(&e.into_inner(), Err(RpcError::new(-32002, "busy")));
                        }
                    }
                    Envelope::Success(r) => {
                        if let Some(tx) = lock(&read_peer.0.state).pending.remove(&r.id) {
                            let _ = tx.send(Ok(r.result));
                        }
                    }
                    Envelope::Failure(r) => {
                        if let Some(tx) = lock(&read_peer.0.state).pending.remove(&r.id) {
                            let _ = tx.send(Err(r.error.code));
                        }
                    }
                }
            }
            read_peer.close();
        });
        Ok((this, Incoming(incoming), authenticated))
    }
    /// Negotiated minor version. Browser methods require at least 1.
    pub fn protocol_minor(&self) -> u16 {
        self.0.minor
    }
    /// Whether the authenticated stream remains connected. Nonblocking.
    pub fn connected(&self) -> bool {
        !*self.0.closed.borrow()
    }
    /// Wait for disconnect. Never clears Stop or reconnects automatically.
    pub async fn disconnected(&self) {
        let mut closed = self.0.closed.subscribe();
        let _ = closed.wait_for(|v| *v).await;
    }
    /// Close immediately and deny every pending request. Nonblocking/idempotent.
    pub fn close(&self) {
        self.0.closed.send_replace(true);
        lock(&self.0.state).pending.clear();
    }
    /// Issue one bounded RPC; queue saturation rejects immediately. Timeout or
    /// dropping removes the pending waiter; late/duplicate replies are ignored.
    pub async fn call<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
        deadline: Duration,
    ) -> Result<R, Error> {
        self.start_call(method, params)?.wait(deadline).await
    }
    /// Enqueue synchronously, before returning to a trusted control callback.
    /// The returned reply owns cancellation cleanup. No IO or waits here.
    pub fn start_call<P: Serialize>(
        &self,
        method: &str,
        params: &P,
    ) -> Result<PendingReply, Error> {
        if !self.connected() {
            return Err(Error::Disconnected);
        }
        let params = serde_json::to_value(params).map_err(|_| Error::Invalid)?;
        let (tx, rx) = oneshot::channel();
        let id = {
            let mut state = lock(&self.0.state);
            if state.pending.len() >= QUEUE_LIMIT {
                return Err(Error::Busy);
            }
            let id = state.next;
            state.next = state.next.checked_add(1).ok_or(Error::Invalid)?;
            self.0
                .outgoing
                .try_send(Outbound {
                    value: Envelope::Request(Request {
                        jsonrpc: "2.0".into(),
                        id: Some(id),
                        method: method.into(),
                        params,
                    }),
                    flushed: None,
                })
                .map_err(|_| Error::Busy)?;
            state.pending.insert(id, tx);
            id
        };
        Ok(PendingReply {
            pending: Pending(self.clone(), id),
            reply: rx,
        })
    }
    /// Enqueue a notification without waiting. Full queues drop the update;
    /// they never block actions or the independent Stop connection.
    pub fn notify<P: Serialize>(&self, method: &str, params: &P) -> Result<(), Error> {
        if !self.connected() {
            return Err(Error::Disconnected);
        }
        let params = serde_json::to_value(params).map_err(|_| Error::Invalid)?;
        self.0
            .outgoing
            .try_send(Outbound {
                value: Envelope::Request(Request {
                    jsonrpc: "2.0".into(),
                    id: None,
                    method: method.into(),
                    params,
                }),
                flushed: None,
            })
            .map_err(|_| Error::Busy)
    }
    /// Answer once. A full response queue closes the connection, denying pending
    /// work instead of leaving approvals hanging. No waiting or retries.
    pub fn respond(&self, request: &Request, answer: Result<Value, RpcError>) {
        self.respond_inner(request, answer, None);
    }
    /// Answer and await the actual frame flush before orderly shutdown. Uses
    /// the same bounded queue and five-second deadline; disconnect fails closed.
    /// Cancelling the wait does not retry or duplicate the response.
    pub async fn respond_flushed(
        &self,
        request: &Request,
        answer: Result<Value, RpcError>,
    ) -> Result<(), Error> {
        if request.id.is_none() {
            return Ok(());
        }
        let (tx, rx) = oneshot::channel();
        self.respond_inner(request, answer, Some(tx));
        tokio::time::timeout(RPC_TIMEOUT, rx)
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Disconnected)
    }
    fn respond_inner(
        &self,
        request: &Request,
        answer: Result<Value, RpcError>,
        flushed: Option<oneshot::Sender<()>>,
    ) {
        let Some(id) = request.id else { return };
        let value = match answer {
            Ok(result) => Envelope::Success(Success {
                jsonrpc: "2.0".into(),
                id,
                result,
            }),
            Err(error) => Envelope::Failure(Failure {
                jsonrpc: "2.0".into(),
                id,
                error,
            }),
        };
        if self
            .0
            .outgoing
            .try_send(Outbound { value, flushed })
            .is_err()
        {
            self.close();
        }
    }
}
/// One already-enqueued request. Dropping it removes its waiter without retry.
pub struct PendingReply {
    pending: Pending,
    reply: oneshot::Receiver<Answer>,
}
impl PendingReply {
    /// Wait at most the supplied monotonic deadline. Late replies are ignored.
    pub async fn wait<R: DeserializeOwned>(self, deadline: Duration) -> Result<R, Error> {
        let _pending = self.pending;
        let value = tokio::time::timeout(deadline, self.reply)
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Disconnected)?
            .map_err(Error::Remote)?;
        serde_json::from_value(value).map_err(|_| Error::Invalid)
    }
}
struct Pending(Peer, u64);
impl Drop for Pending {
    fn drop(&mut self) {
        lock(&self.0 .0.state).pending.remove(&self.1);
    }
}
/// App callback future. Dropped at disconnect; approval UI must dismiss then.
pub type CallbackFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, RpcError>> + Send + 'a>>;
/// App's independent Host implementation; no core/host-api dependency required.
pub trait HostCallbacks: Send + Sync + 'static {
    /// Consume a bounded notification synchronously in wire order. Optional;
    /// must do memory work only, with no IO/waits. Returning None uses the
    /// existing concurrent callback path (including ordinary approvals).
    fn notification(&self, _request: &Request) -> Option<Result<Value, RpcError>> {
        None
    }
    /// Handle a strict typed method. Approval callbacks may wait up to 120s;
    /// all other work caps at five seconds. Dropping dismisses pending UI.
    fn handle(&self, request: Request) -> CallbackFuture<'_>;
    /// The child finished mutual authentication. Default keeps the previous
    /// callbacks unchanged. Must not block.
    fn attached(&self, _engine: ProcessInstance, _peer: Peer) {}
    /// Current Host Stop latch for a new handshake; no IO or waits.
    fn stop_state(&self) -> Option<bool> {
        None
    }
    /// Authenticated independent control connection. Never expose it to a model.
    fn control_attached(&self, _peer: Peer) {}
    /// Dismiss all pending approvals and preserve the Host's persistent Stop
    /// latch. Must return immediately and must not perform blocking IO.
    fn disconnected(&self);
}
/// App-owned lifetime for one authenticated engine connection.
pub struct HostServer {
    peer: Peer,
    incoming: Incoming,
}
impl HostServer {
    /// Accept only the launcher's expected child instance. Same authentication
    /// and five-second deadline as Peer::connect; only Host role is accepted.
    pub async fn accept<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        stream: S,
        config: &AuthConfig,
        expected: &ProcessInstance,
        secret: &Secret,
    ) -> Result<Self, Error> {
        if config.role != crate::auth::Role::Host {
            return Err(Error::Authentication);
        }
        let (peer, incoming, _) = Peer::connect(stream, config, expected, secret).await?;
        Ok(Self { peer, incoming })
    }
    /// Clone the engine-control RPC handle. Never expose it to MCP/model tools.
    pub fn peer(&self) -> Peer {
        self.peer.clone()
    }
    /// Dispatch callbacks concurrently with a hard bound of 32. Disconnect
    /// cancels every callback and calls disconnected. No automatic reconnect.
    pub async fn run(mut self, callbacks: Arc<dyn HostCallbacks>) {
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! { biased;
                _=self.peer.disconnected()=>break,
                Some(_)=tasks.join_next(),if !tasks.is_empty()=>(),
                request=self.incoming.recv()=>{
                    let Some(request)=request else {break};
                    if request.id.is_none() {
                        if let Some(answer) = callbacks.notification(&request) {
                            self.peer.respond(&request, answer);
                            continue;
                        }
                    }
                    if tasks.len()>=QUEUE_LIMIT {self.peer.respond(&request,Err(RpcError::new(-32002,"busy")));continue;}
                    let peer=self.peer.clone(); let callbacks=callbacks.clone();
                    tasks.spawn(async move {
                        let duration=if request.method.starts_with("host.approval."){Duration::from_secs(120)}else{RPC_TIMEOUT};
                        let result=tokio::time::timeout(duration,callbacks.handle(request.clone())).await.unwrap_or_else(|_|Err(RpcError::new(-32001,"approval_timeout")));
                        peer.respond(&request,result);
                    });
                }
            }
        }
        self.peer.close();
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        callbacks.disconnected();
    }
}
impl Drop for HostServer {
    fn drop(&mut self) {
        self.peer.close();
    }
}
