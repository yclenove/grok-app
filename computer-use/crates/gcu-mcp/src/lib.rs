//! Computer-use MCP server. No native driver or automatic authorization is selected here.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
mod catalog;
pub mod codec;
mod connection;
mod elicitation;
mod files;
pub mod host;
pub mod http;
mod image_cache;
mod instructions;
pub mod lifecycle;
mod params;
mod result;
mod server;
mod stdio;

pub use catalog::{Profile, IMPLEMENTED_TOOLS};
pub use connection::Connection;
use gcu_core::driver::Driver;
pub use params::validate_arguments;
use rmcp::ServiceExt;
pub use server::{Host, ServerConfig};
use std::sync::{atomic::Ordering, Arc};
use tokio::io::{AsyncRead, AsyncWrite};

/// Startup/transport failure. Payloads and client parameters are never included.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// The session-private file area could not be created or cleaned up.
    #[error("session file IO failed")]
    Io(#[from] std::io::Error),
    /// MCP initialization failed or the service task failed.
    #[error("MCP connection failed")]
    Protocol,
}

/// Serve one NDJSON stdio connection with injected Driver and trusted Host.
/// Waits until stdin EOF or transport failure; native calls cap at 5 seconds,
/// approvals at Host policy timeout. Close cancels pending work, releases input,
/// and deletes session files. Dropping the future triggers the same cleanup.
pub async fn serve_stdio(
    driver: Arc<dyn Driver>,
    host: Host,
    config: ServerConfig,
) -> Result<(), ServeError> {
    serve_io(
        driver,
        host,
        config,
        tokio::io::stdin(),
        tokio::io::stdout(),
    )
    .await
}

/// Serve injectable byte streams with exactly the stdio protocol/lifecycle.
/// Same timeout/cancellation semantics as [`serve_stdio`]; useful for embedding
/// and duplex tests. Output contains protocol messages only.
pub async fn serve_io<R, W>(
    driver: Arc<dyn Driver>,
    host: Host,
    config: ServerConfig,
    input: R,
    output: W,
) -> Result<(), ServeError>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut connection = Connection::open(driver, host, config).await?;
    serve_connection(&mut connection, input, output).await
}

async fn serve_connection<R, W>(
    connection: &mut Connection,
    input: R,
    output: W,
) -> Result<(), ServeError>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let server = connection.server.clone();
    let session = connection.session();
    let transport = stdio::Stdio::new(input, output, session, server.lifecycle());
    let failed = transport.failure_flag();
    let result = match server.serve(transport).await {
        Ok(running) => running
            .waiting()
            .await
            .map_err(|_| ServeError::Protocol)
            .and_then(|reason| match reason {
                rmcp::service::QuitReason::Closed => Ok(()),
                _ => Err(ServeError::Protocol),
            }),
        Err(_) => Err(ServeError::Protocol),
    };
    connection.close().await?;
    if failed.load(Ordering::Acquire) {
        return Err(ServeError::Protocol);
    }
    result
}

#[cfg(test)]
mod stdio_tests;
