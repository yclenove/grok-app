//! Remote session assembly, using the exact existing MCP connection dispatcher.
use super::RemoteHost;
use crate::{http::HttpEndpoint, Connection, ServeError, ServerConfig};
use gcu_core::driver::Driver;
use gcu_host_api::{OverlayProvider, SessionCtx};
use std::sync::Arc;
/// One App namespace, one token, one core lifecycle. App close revokes all three.
pub struct RemoteSession {
    host: Arc<RemoteHost>,
    endpoint: HttpEndpoint,
    id: String,
}
impl RemoteSession {
    /// Initialize the Host-only connection and bind a random loopback endpoint.
    /// Startup fails closed on Host disconnect; no elicitation fallback authority.
    pub async fn open(
        driver: Arc<dyn Driver>,
        host: Arc<RemoteHost>,
        id: &str,
        config: ServerConfig,
    ) -> Result<Self, ServeError> {
        if !host.peer.connected() {
            return Err(ServeError::Protocol);
        }
        let host = host.for_session(id);
        let connection = Connection::open_host_only(driver, host.host(), config).await?;
        let endpoint = HttpEndpoint::start_remote(connection, host.peer.clone()).await?;
        Ok(Self {
            host,
            endpoint,
            id: id.into(),
        })
    }
    /// Credential response for the authenticated App only. No logging.
    pub fn opened(&self) -> gcu_host_protocol::messages::SessionOpened {
        gcu_host_protocol::messages::SessionOpened {
            mcp_url: self.endpoint.url().into(),
            token: self.endpoint.token().into(),
        }
    }
    /// Trusted core lifecycle control, independent of action queues.
    pub fn session(&self) -> Arc<gcu_core::session::Session> {
        self.endpoint.session()
    }
    /// Return only this session's already redacted delivered images. No waits.
    pub fn screenshot(&self, id: &str) -> Option<gcu_host_protocol::messages::Image> {
        self.endpoint.screenshot(id)
    }
    /// Drop turn grants, force a fresh next observation, hide overlay and release
    /// input. Existing Session ceilings/cancellation semantics apply.
    pub async fn turn_ended(&self) {
        self.session().turn_ended().await;
        self.host.hide(&SessionCtx {
            session_id: self.id.clone(),
        });
    }
    /// Revoke token/endpoint, cancel approvals, hide overlay and release input.
    pub async fn close(&mut self) -> Result<(), ServeError> {
        self.host.hide(&SessionCtx {
            session_id: self.id.clone(),
        });
        self.endpoint.close().await
    }
}
