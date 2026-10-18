//! Versioned, authenticated Host wire protocol; independent of core and drivers.
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
pub mod approval;
pub mod auth;
mod convert;
pub use convert::persistent_grant_matches;
mod browser;
pub use browser::clear_profile;
pub mod codec;
pub mod events;
pub mod inject;
pub mod lifecycle;
pub mod messages;
pub mod overlay;
pub mod panel;
pub mod server;
pub mod stop_reason;
pub mod supervisor;
#[cfg(all(feature = "testing", unix))]
pub mod testing;
pub mod transport;
mod types;
mod unsafe_platform;

/// Transport/protocol errors deliberately exclude payloads and secrets.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// IO failed or the connection closed.
    #[error("Host connection IO failed")]
    Io(#[from] std::io::Error),
    /// Invalid frame, JSON, envelope, or parameters.
    #[error("invalid Host protocol message")]
    Invalid,
    /// Protocol major versions differ.
    #[error("Host protocol major version mismatch")]
    Version,
    /// Peer proof or expected process instance did not match.
    #[error("Host authentication failed")]
    Authentication,
    /// A bounded queue is full.
    #[error("Host connection is busy")]
    Busy,
    /// A request or handshake exceeded its deadline.
    #[error("Host request timed out")]
    Timeout,
    /// Peer disconnected; no new operations are accepted.
    #[error("Host disconnected")]
    Disconnected,
    /// A remote JSON-RPC error, without its potentially sensitive message.
    #[error("Host rejected request ({0})")]
    Remote(i32),
}
