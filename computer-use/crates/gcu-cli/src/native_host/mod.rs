//! Browser native messaging, isolated from MCP and from page-controlled identity.
//!
//! Chrome/Edge enforce the installed manifest's single allowed origin and supply
//! that origin on launch. A fresh challenge pins all messages to this exact pipe
//! connection. This is a browser boundary, not authentication against arbitrary
//! programs running as the same user or a compromised browser. There is no HTTP
//! endpoint, persistent capability, session selection or approval RPC here.

pub mod codec;
mod identity;
pub mod install;
#[cfg(windows)]
mod unsafe_registry;
mod wire;

pub use identity::ExtensionIdentity;
use serde_json::{json, Value};
use std::collections::BTreeMap;
pub use wire::Scope;

/// Redacted boundary error; never contains page text, action values or raw JSON.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Missing fixed public key, invalid key, or unexpected browser launch origin.
    #[error("Native extension identity unavailable or origin rejected; maintainer must set manifest key")]
    Identity,
    /// Invalid, oversized, truncated or unexpected frame/message.
    #[error("Invalid native messaging frame")]
    Frame,
    /// A started frame or output exceeded its five-second deadline.
    #[error("Native messaging deadline exceeded")]
    Deadline,
    /// The live connection, share, trusted session or one-shot dispatch did not match.
    #[error("Native messaging scope rejected")]
    Scope,
    /// The complete ActionRouter did not authorize this exact dispatch.
    #[error("Native messaging Router preflight rejected")]
    Denied,
    /// IO failed. The source is intentionally not printed (it may contain paths).
    #[error("Native messaging IO failed")]
    Io(#[from] std::io::Error),
    /// Existing files/registry entries are foreign or changed since planning.
    #[error("Native host registration is foreign or changed during planning")]
    Registration,
}

/// Trusted, in-process authorization boundary for P8-03's full ActionRouter.
///
/// Implementations must verify App/action approval, persistent Stop/Stop health,
/// real browser window alignment and session lifecycle immediately before return.
/// Synchronous and nonblocking; sharing and wire messages cannot implement this.
pub trait RouterGate {
    /// Authorize this exact operation in the server-owned session and browser scope.
    fn authorize(&self, session: &str, scope: &Scope, operation: &Value) -> bool;
}

struct Share {
    scope: Scope,
    session: Option<String>,
    pending: Option<String>,
}
struct Pending {
    session: String,
    share: String,
    mutation: bool,
}

/// Result whose ownership comes from the pending server request, never from JSON.
#[derive(Debug)]
pub struct RoutedResult {
    /// Trusted session supplied by `bind`/`request`.
    pub session_id: String,
    /// One-shot server request ID.
    pub request_id: String,
    /// DOM response. P8-03 must validate/merge/redact snapshots before use.
    pub result: Value,
}

/// Validated input event; sharing provides browser consent, never App approval.
#[derive(Debug)]
pub enum Event {
    /// Challenge completed on this pipe connection.
    Ready,
    /// A browser-owned top document was explicitly shared.
    Shared(Scope),
    /// Share removed; any pending work completes without retry.
    Revoked(Vec<RoutedResult>),
    /// Matched response carrying server-owned session identity.
    Response(RoutedResult),
}

/// Per-native-pipe state; nothing survives EOF, disconnect or process restart.
///
/// All methods are pure synchronous memory operations, nonblocking and bounded.
/// No session/window identity from an extension response is accepted.
pub struct Connection {
    id: String,
    ready: bool,
    live: bool,
    shares: BTreeMap<String, Share>,
    pending: BTreeMap<String, Pending>,
    sequence: u64,
}

impl Connection {
    /// Validate the browser-supplied launch origin before accepting any frame.
    pub fn new(identity: &ExtensionIdentity, origin: &str) -> Result<Self, Error> {
        if identity.origin() != origin {
            return Err(Error::Identity);
        }
        Ok(Self {
            id: gcu_host_protocol::auth::Secret::random().hex(),
            ready: false,
            live: true,
            shares: BTreeMap::new(),
            pending: BTreeMap::new(),
            sequence: 0,
        })
    }

    /// Fresh, unguessable connection challenge; it grants no action permission.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Initial frame. The extension must echo it before announcing a share.
    pub fn hello(&self) -> Value {
        json!({"kind":"hello", "protocol":1, "connectionId":self.id})
    }

    /// Validate strict wire DTOs, scope and request ownership before routing.
    /// A caller must disconnect on any error; it must not retry a rejected frame.
    pub fn receive(&mut self, value: Value) -> Result<Event, Error> {
        let message: wire::Incoming = serde_json::from_value(value).map_err(|_| Error::Frame)?;
        if !self.live || message.connection() != self.id {
            return Err(Error::Scope);
        }
        if let wire::Incoming::Ready { protocol, .. } = message {
            if self.ready || protocol != 1 {
                return Err(Error::Scope);
            }
            self.ready = true;
            return Ok(Event::Ready);
        }
        if !self.ready {
            return Err(Error::Scope);
        }
        match message {
            wire::Incoming::Shared { scope, .. } => {
                if !scope.valid()
                    || self.shares.len() >= 64
                    || self.shares.contains_key(&scope.share_id)
                    || self.shares.values().any(|s| s.scope.tab_id == scope.tab_id)
                {
                    return Err(Error::Scope);
                }
                self.shares.insert(
                    scope.share_id.clone(),
                    Share {
                        scope: scope.clone(),
                        session: None,
                        pending: None,
                    },
                );
                Ok(Event::Shared(scope))
            }
            wire::Incoming::Revoked { share_id, .. } => {
                if !wire::identifier(&share_id) || !self.shares.contains_key(&share_id) {
                    return Err(Error::Scope);
                }
                Ok(Event::Revoked(self.revoke(&share_id)))
            }
            wire::Incoming::Response {
                request_id, result, ..
            } => {
                if !wire::identifier(&request_id) || !wire::valid_result(&result) {
                    return Err(Error::Frame);
                }
                let pending = self.pending.get(&request_id).ok_or(Error::Scope)?;
                if (pending.mutation && result["kind"] == "snapshot")
                    || (!pending.mutation && result["kind"] == "action")
                {
                    return Err(Error::Frame);
                }
                let pending = self.pending.remove(&request_id).ok_or(Error::Scope)?;
                let share = self.shares.get_mut(&pending.share).ok_or(Error::Scope)?;
                if share.pending.as_deref() != Some(&request_id)
                    || share.session.as_deref() != Some(&pending.session)
                {
                    return Err(Error::Scope);
                }
                share.pending = None;
                Ok(Event::Response(RoutedResult {
                    session_id: pending.session,
                    request_id,
                    result,
                }))
            }
            wire::Incoming::Ready { .. } => Err(Error::Scope),
        }
    }

    /// Bind a live shared document to a trusted core session. Wire cannot call this.
    /// P8-03 must first verify the real browser window/document alignment.
    pub fn bind(&mut self, share: &str, session: &str) -> Result<(), Error> {
        if !self.live || !self.ready || !wire::identifier(session) {
            return Err(Error::Scope);
        }
        let share = self.shares.get_mut(share).ok_or(Error::Scope)?;
        if share.session.as_deref().is_some_and(|old| old != session) {
            return Err(Error::Scope);
        }
        share.session = Some(session.to_owned());
        Ok(())
    }

    /// Create one authorized request, at most one outstanding per shared document.
    /// Failure/timeout after writing requires revocation, never automatic replay.
    pub fn request(
        &mut self,
        session: &str,
        share_id: &str,
        operation: Value,
        gate: &dyn RouterGate,
    ) -> Result<Value, Error> {
        if !self.live || !self.ready || !wire::valid_operation(&operation) || self.sequence >= 256 {
            return Err(Error::Scope);
        }
        let share = self.shares.get_mut(share_id).ok_or(Error::Scope)?;
        if share.session.as_deref() != Some(session) || share.pending.is_some() {
            return Err(Error::Scope);
        }
        if !gate.authorize(session, &share.scope, &operation) {
            return Err(Error::Denied);
        }
        self.sequence += 1;
        let request_id = format!("native-{}", self.sequence);
        share.pending = Some(request_id.clone());
        self.pending.insert(
            request_id.clone(),
            Pending {
                session: session.to_owned(),
                share: share_id.to_owned(),
                mutation: operation["type"] != "observe",
            },
        );
        Ok(json!({"kind":"request", "connectionId":self.id, "request":{
            "requestId":request_id, "sessionId":session, "shareId":share_id, "operation":operation}}))
    }

    /// Revoke browser consent, cancelling pending input as unknown without replay.
    pub fn revoke(&mut self, share: &str) -> Vec<RoutedResult> {
        self.shares.remove(share);
        let requests: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, p)| p.share == share)
            .map(|(id, _)| id.clone())
            .collect();
        requests
            .into_iter()
            .filter_map(|id| {
                self.pending.remove(&id).map(|p| RoutedResult {
                    session_id: p.session,
                    request_id: id,
                    result: if p.mutation {
                        json!({"kind":"action","outcome":"unknown"})
                    } else {
                        json!({"kind":"error","code":"app_denied"})
                    },
                })
            })
            .collect()
    }

    /// Retire one session's consent and pending replies. Rebinding requires a new share.
    pub fn close_session(&mut self, session: &str) -> Vec<RoutedResult> {
        let shares: Vec<_> = self
            .shares
            .iter()
            .filter(|(_, s)| s.session.as_deref() == Some(session))
            .map(|(id, _)| id.clone())
            .collect();
        shares.into_iter().flat_map(|id| self.revoke(&id)).collect()
    }

    /// Invalidate synchronously on EOF, Stop, unhealthy Stop channel or parse/IO error.
    /// A new Connection cannot recover shares, approvals or the persistent Stop latch.
    pub fn disconnect(&mut self) -> Vec<RoutedResult> {
        self.live = false;
        let shares: Vec<_> = self.shares.keys().cloned().collect();
        shares.into_iter().flat_map(|id| self.revoke(&id)).collect()
    }
}

/// Serve browser consent announcements over a real framed transport until EOF.
///
/// P8-03 owns Router integration; this entry cannot bind sessions or dispatch DOM.
/// Handshake has a five-second deadline; each subsequent idle wait is at most 60s.
/// Errors invalidate all scopes before return. Dropping this future drops its state.
pub async fn serve<R: tokio::io::AsyncRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(
    input: &mut R,
    output: &mut W,
    identity: &ExtensionIdentity,
    origin: &str,
) -> Result<(), Error> {
    let mut connection = Connection::new(identity, origin)?;
    let result = async {
        codec::write(output, &connection.hello()).await?;
        let first = tokio::time::timeout(std::time::Duration::from_secs(5), codec::read(input))
            .await
            .map_err(|_| Error::Deadline)??
            .ok_or(Error::Frame)?;
        if !matches!(connection.receive(first)?, Event::Ready) {
            return Err(Error::Scope);
        }
        loop {
            let frame = tokio::time::timeout(std::time::Duration::from_secs(60), codec::read(input))
                .await
                .map_err(|_| Error::Deadline)??;
            let Some(frame) = frame else {
                return Ok(());
            };
            match connection.receive(frame)? {
                Event::Shared(scope) => {
                    let ack = json!({"kind":"sharedAck", "connectionId":connection.id(), "shareId":scope.share_id});
                    codec::write(output, &ack).await?;
                }
                Event::Revoked(_) => (),
                _ => return Err(Error::Scope),
            }
        }
    }
    .await;
    connection.disconnect();
    result
}

pub(crate) async fn run(origin: &str) -> Result<u8, crate::exit::Failure> {
    let identity = ExtensionIdentity::embedded()
        .map_err(|e| crate::exit::Failure::arguments(e.to_string()))?;
    serve(
        &mut tokio::io::stdin(),
        &mut tokio::io::stdout(),
        &identity,
        origin,
    )
    .await
    .map_err(|e| crate::exit::Failure::arguments(e.to_string()))?;
    Ok(0)
}
