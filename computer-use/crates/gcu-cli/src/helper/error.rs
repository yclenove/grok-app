//! Helper failures. Display text never includes paths, frames, or secrets.

use super::auth::RejectReason;
#[cfg(unix)]
use std::io;

/// Recoverable helper failure. Callers map this to a static CLI message.
#[derive(Debug, thiserror::Error)]
pub enum HelperError {
    /// The peer was rejected. No session was opened.
    #[error("helper refused the peer")]
    Refused(RejectReason),
    /// The socket path does not exist.
    #[error("helper socket is absent")]
    Absent,
    /// The endpoint is not a private socket owned by this user.
    #[error("helper endpoint is not private")]
    Foreign,
    /// The bound socket was replaced.
    #[error("helper endpoint was replaced")]
    Replaced,
    /// Another live helper already owns the socket.
    #[error("helper is already running")]
    Busy,
    /// The peer's version does not match.
    #[error("helper version does not match")]
    Version,
    /// The audit token or signature call failed closed.
    #[error("helper audit failed")]
    Audit,
    /// A frame was malformed or larger than its limit.
    #[error("helper frame was rejected")]
    Frame,
    /// A bounded wait elapsed. The caller must not retry on the same stream.
    #[error("helper timed out")]
    Timeout,
    /// IO failed. The label is an error category, never a path.
    #[error("helper IO failed ({0})")]
    Io(&'static str),
    /// This operating system does not run the helper.
    #[error("helper is unsupported on this system")]
    Unsupported,
    /// This process has no usable code signature, so it must not listen.
    #[error("helper has no code signature")]
    UnsignedSelf,
}

#[cfg(unix)]
impl HelperError {
    pub(crate) fn from_io(error: &io::Error) -> Self {
        if error.kind() == io::ErrorKind::NotFound {
            return Self::Absent;
        }
        if error.kind() == io::ErrorKind::TimedOut {
            return Self::Timeout;
        }
        Self::Io(io_category(error))
    }
}

#[cfg(unix)]
pub(crate) fn io_category(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::NotFound => "not found",
        io::ErrorKind::PermissionDenied => "permission",
        io::ErrorKind::ConnectionRefused => "refused",
        io::ErrorKind::ConnectionReset => "reset",
        io::ErrorKind::ConnectionAborted => "aborted",
        io::ErrorKind::NotConnected => "closed",
        io::ErrorKind::AddrInUse => "busy",
        io::ErrorKind::TimedOut => "timeout",
        io::ErrorKind::WouldBlock => "again",
        io::ErrorKind::Interrupted => "interrupted",
        io::ErrorKind::UnexpectedEof => "eof",
        _ => "io",
    }
}
