//! In-process access to the same session, router and results used by stdio.
use crate::{
    files::SessionFiles,
    server::{Cleanup, Server},
    Host, ServeError, ServerConfig,
};
use gcu_core::{driver::Driver, session::Session};
use rmcp::model::CallToolResult;
use serde_json::Value;
use std::sync::Arc;

/// One isolated MCP session. Close explicitly to await input/file cleanup.
/// Dropping also schedules cleanup. No additional tools or authorization paths.
pub struct Connection {
    pub(crate) server: Server,
    cleanup: Cleanup,
}
impl Connection {
    /// Allocate private session files and initialize the existing ActionRouter.
    /// File work has a five-second ceiling; dropping cancels the wait.
    pub async fn open(
        driver: Arc<dyn Driver>,
        host: Host,
        config: ServerConfig,
    ) -> Result<Self, ServeError> {
        let files = crate::files::blocking(SessionFiles::new).await?;
        let server = Server::new(driver, host, config, files)?;
        let cleanup = Cleanup::new(server.clone());
        Ok(Self { server, cleanup })
    }
    /// Open with the injected Host as the sole approval authority, without
    /// asking an MCP client to authorize. Used by the fake-only CLI until P4.
    /// Private-file startup caps at five seconds; dropping cancels the wait.
    pub async fn open_host_only(
        driver: Arc<dyn Driver>,
        host: Host,
        config: ServerConfig,
    ) -> Result<Self, ServeError> {
        let files = crate::files::blocking(SessionFiles::new).await?;
        let server = Server::new_with_approvals(driver, host, config, files, false)?;
        let cleanup = Cleanup::new(server.clone());
        Ok(Self { server, cleanup })
    }
    /// Trusted Host control handle; never publish this as a model tool.
    /// Memory only, nonblocking, with no timeout or cancellation.
    pub fn session(&self) -> Arc<Session> {
        self.server.session()
    }
    /// Session-local hook/idle lifecycle. Memory only, nonblocking; no timeout
    /// or cancellation. This handle can retire grants but cannot resume Stop.
    pub fn lifecycle(&self) -> crate::lifecycle::Lifecycle {
        self.server.lifecycle()
    }
    /// Export only files recorded by this session's safe result encoder, for a
    /// CLI inspection lease. Never parses paths from model text. At most 32 MB;
    /// blocking reads cap at five seconds, and dropping cancels the wait.
    pub async fn export_private_files(
        &self,
    ) -> Result<Vec<(std::path::PathBuf, Vec<u8>)>, ServeError> {
        let files = self.server.files();
        Ok(crate::files::blocking(move || files.export()).await?)
    }
    /// Execute once through the stdio dispatcher, without a peer/elicitation.
    /// Native calls cap at five seconds; approvals follow Host policy. No retries.
    pub async fn call(&self, name: &str, arguments: Value) -> CallToolResult {
        self.server.call_result(name, arguments).await
    }
    /// Await cancellation, input release and deletion of session-private files.
    /// Native/file cleanup caps at five seconds per operation; idempotent.
    pub async fn close(&mut self) -> Result<(), ServeError> {
        self.cleanup.close().await
    }
    /// Serve stdio using this session, allowing a separate trusted control task.
    /// Same lifecycle, timeouts and cancellation as [`crate::serve_stdio`].
    pub async fn serve_stdio(mut self) -> Result<(), ServeError> {
        crate::serve_connection(&mut self, tokio::io::stdin(), tokio::io::stdout()).await
    }
}
