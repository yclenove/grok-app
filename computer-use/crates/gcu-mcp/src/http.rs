//! Loopback streamable HTTP reuses the stdio dispatcher, router, and results.
use crate::{Connection, ServeError};
use axum::{
    body::Body,
    http::{header, HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use gcu_host_protocol::{auth::Secret, server::Peer};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};
/// How long `HttpEndpoint::close` lets the transport write responses that are already produced.
const RESPONSE_FLUSH_GRACE: std::time::Duration = std::time::Duration::from_millis(150);
/// Upper bound for waiting on tool calls that a close has cancelled.
const CALLS_FINISHED_LIMIT: std::time::Duration = std::time::Duration::from_secs(1);

/// One App session endpoint. Drop revokes its token and closes all HTTP streams;
/// call close to await input/resource cleanup too. Never Debug/log the token.
pub struct HttpEndpoint {
    connection: Connection,
    task: tokio::task::JoinHandle<()>,
    cancel: Box<dyn Fn() + Send + Sync>,
    url: String,
    token: String,
}
struct Gate {
    authority: String,
    origin: String,
    secret: Secret,
    session: Arc<gcu_core::session::Session>,
    host: Option<Peer>,
}
async fn authorize(
    axum::extract::State(gate): axum::extract::State<Arc<Gate>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if let Some(status) = reject(&gate, &request) {
        return status.into_response();
    }
    next.run(request).await
}

fn reject(gate: &Gate, request: &Request<Body>) -> Option<StatusCode> {
    if gate.host.as_ref().is_some_and(|host| !host.connected()) {
        return Some(StatusCode::SERVICE_UNAVAILABLE);
    }
    if gate.session.is_closed() {
        return Some(StatusCode::GONE);
    }
    if !host_ok(gate, request) || !origin_ok(gate, request.headers()) {
        return Some(StatusCode::FORBIDDEN);
    }
    if !token_ok(gate, request.headers()) {
        return Some(StatusCode::UNAUTHORIZED);
    }
    None
}

fn host_ok(gate: &Gate, request: &Request<Body>) -> bool {
    let hosts: Vec<_> = request.headers().get_all(header::HOST).iter().collect();
    hosts.len() == 1
        && hosts[0].to_str().ok() == Some(gate.authority.as_str())
        && request
            .uri()
            .authority()
            .is_none_or(|authority| authority.as_str() == gate.authority)
}

fn origin_ok(gate: &Gate, headers: &HeaderMap) -> bool {
    let origins: Vec<_> = headers.get_all(header::ORIGIN).iter().collect();
    match origins.as_slice() {
        [] => !browser_client(headers),
        [origin] => origin.to_str().ok() == Some(gate.origin.as_str()),
        _ => false,
    }
}

/// Browsers send a `Sec-Fetch-*` hint. A browser that omits Origin is refused
/// even when it copied a bearer.
fn browser_client(headers: &HeaderMap) -> bool {
    [
        "sec-fetch-site",
        "sec-fetch-mode",
        "sec-fetch-dest",
        "sec-fetch-user",
    ]
    .into_iter()
    .any(|name| headers.contains_key(name))
}

fn token_ok(gate: &Gate, headers: &HeaderMap) -> bool {
    let tokens: Vec<_> = headers.get_all(header::AUTHORIZATION).iter().collect();
    tokens.len() == 1
        && tokens[0]
            .to_str()
            .ok()
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|hex| gate.secret.verify_hex(hex))
}

impl HttpEndpoint {
    /// Bind 127.0.0.1 only, with a fresh 32-byte bearer secret. Port zero selects
    /// a random port. Startup IO uses the Connection's existing bounded cleanup.
    /// Scoped connections are rejected before binding.
    pub async fn start(connection: Connection, port: u16) -> Result<Self, ServeError> {
        Self::start_with_secret(connection, port, Secret::random()).await
    }
    /// Bind with a launcher-supplied secret read from an inherited token fd.
    /// Same lifecycle and timeout semantics as start.
    pub async fn start_with_secret(
        connection: Connection,
        port: u16,
        secret: Secret,
    ) -> Result<Self, ServeError> {
        Self::start_with_host(connection, port, secret, None).await
    }
    pub(crate) async fn start_remote(
        connection: Connection,
        host: Peer,
    ) -> Result<Self, ServeError> {
        Self::start_with_host(connection, 0, Secret::random(), Some(host)).await
    }
    async fn start_with_host(
        connection: Connection,
        port: u16,
        secret: Secret,
        host: Option<Peer>,
    ) -> Result<Self, ServeError> {
        if connection.is_scoped() {
            return Err(ServeError::ScopedHttp);
        }
        let listener =
            tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
                .await?;
        let authority = listener.local_addr()?.to_string();
        let url = format!("http://{authority}/mcp");
        let token = secret.hex();
        let origin = format!("http://{authority}");
        let gate = Arc::new(Gate {
            authority: authority.clone(),
            origin: origin.clone(),
            secret,
            session: connection.session(),
            host,
        });
        let config = StreamableHttpServerConfig::default()
            .with_allowed_hosts([authority])
            .with_allowed_origins([origin])
            .enforce_origin_validation()
            .with_json_response(true)
            .with_max_request_body_bytes(crate::codec::MAX_LINE_BYTES);
        let ct = config.cancellation_token.clone();
        let end = ct.clone();
        let server = connection.server.clone();
        let service = StreamableHttpService::new(
            move || Ok(server.clone()),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let app = Router::new()
            .nest_service("/mcp", service)
            .layer(middleware::from_fn_with_state(gate, authorize));
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move { end.cancelled().await })
                .await;
        });
        Ok(Self {
            connection,
            task,
            cancel: Box::new(move || ct.cancel()),
            url,
            token,
        })
    }
    /// Exact endpoint URL; contains no credentials. Pure and nonblocking.
    pub fn url(&self) -> &str {
        &self.url
    }
    /// Secret token for the authenticated Host open result / inherited proxy fd.
    /// Never pass this through environment/argv or log it.
    pub fn token(&self) -> &str {
        &self.token
    }
    /// Independent trusted lifecycle/Stop handle. Never register as a tool.
    pub fn session(&self) -> Arc<gcu_core::session::Session> {
        self.connection.session()
    }
    /// Retained redacted image only in this endpoint's App session. Nonblocking.
    pub fn screenshot(&self, id: &str) -> Option<gcu_host_protocol::messages::Image> {
        self.connection.server.screenshot(id)
    }
    /// Revoke the endpoint, cancel calls, and await cleanup. Native/file cleanup
    /// uses the existing five-second ceilings; HTTP tasks are aborted afterwards.
    pub async fn close(&mut self) -> Result<(), ServeError> {
        // Session close revokes the gate and cancels approval waits first. Keep
        // transport actors alive during bounded cleanup so Deny can be delivered
        // before their response channels are torn down.
        let result = self.connection.close().await;
        // A call cancelled by the close (a pending approval) returns its Deny only after
        // `close` has returned. Cancelling the transport first would drop that response and
        // leave the client waiting, so wait for running calls, then let the transport write.
        self.connection
            .server
            .calls_finished(CALLS_FINISHED_LIMIT)
            .await;
        tokio::time::sleep(RESPONSE_FLUSH_GRACE).await;
        (self.cancel)();
        // Graceful shutdown finishes responses already being written. Open event streams
        // never end on their own, so this wait is bounded.
        let _ = tokio::time::timeout(RESPONSE_FLUSH_GRACE, &mut self.task).await;
        self.task.abort();
        result
    }
}
impl Drop for HttpEndpoint {
    fn drop(&mut self) {
        (self.cancel)();
        self.task.abort();
    }
}
