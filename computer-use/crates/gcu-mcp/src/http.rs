//! Loopback streamable HTTP reuses the stdio dispatcher, router, and results.
use crate::{Connection, ServeError};
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
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
    if gate.host.as_ref().is_some_and(|host| !host.connected()) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if gate.session.is_closed() {
        return StatusCode::GONE.into_response();
    }
    let headers = request.headers();
    let hosts: Vec<_> = headers.get_all(header::HOST).iter().collect();
    let origins: Vec<_> = headers.get_all(header::ORIGIN).iter().collect();
    let tokens: Vec<_> = headers.get_all(header::AUTHORIZATION).iter().collect();
    if hosts.len() != 1
        || hosts[0].to_str().ok() != Some(gate.authority.as_str())
        || request
            .uri()
            .authority()
            .is_some_and(|a| a.as_str() != gate.authority)
        || origins.len() > 1
        || origins
            .first()
            .is_some_and(|o| o.to_str().ok() != Some(gate.origin.as_str()))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if tokens.len() != 1
        || !tokens[0]
            .to_str()
            .ok()
            .and_then(|t| t.strip_prefix("Bearer "))
            .is_some_and(|t| gate.secret.verify_hex(t))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}
impl HttpEndpoint {
    /// Bind 127.0.0.1 only, with a fresh 32-byte bearer secret. Port zero selects
    /// a random port. Startup IO uses the Connection's existing bounded cleanup.
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
        (self.cancel)();
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
