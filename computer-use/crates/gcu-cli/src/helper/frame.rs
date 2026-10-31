//! Length-prefixed JSON between `gcu mcp` and `gcu agent`.
//!
//! The first frame is a hello or a shutdown, at most [`CONTROL_MAX`] bytes.
//! Later MCP frames wrap one JSON value and stay within one NDJSON line plus
//! the wrapper. Unknown fields are rejected. Callers own the deadline.

use super::error::HelperError;
use gcu_mcp::codec::MAX_LINE_BYTES;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Largest hello or shutdown frame.
pub const CONTROL_MAX: usize = 4096;
/// Largest MCP frame: one max NDJSON line plus the wrapper.
pub const MCP_MAX: usize = MAX_LINE_BYTES + 256;

/// One helper frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Frame {
    /// Version and role handshake. Sent before any MCP body.
    Hello {
        /// Exact build version.
        version: String,
        /// Which side is speaking.
        role: Role,
        /// Tool profile for this connection.
        profile: WireProfile,
        /// Per-connection turn idle, in seconds.
        turn_idle_secs: u64,
    },
    /// Ask the other side to exit. This is not an MCP session.
    Shutdown {
        /// Why the process should exit.
        reason: ShutdownReason,
    },
    /// One MCP JSON value, not yet newline-delimited.
    Mcp {
        /// JSON-RPC value.
        body: Value,
    },
}

/// Side of the helper link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The `gcu mcp` process.
    Cli,
    /// The helper process.
    Agent,
}

/// Profile names on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireProfile {
    /// Default tool list.
    Default,
    /// Minimal tool list.
    Minimal,
    /// Full tool list.
    Full,
}

impl From<WireProfile> for gcu_mcp::Profile {
    fn from(profile: WireProfile) -> Self {
        match profile {
            WireProfile::Default => Self::Default,
            WireProfile::Minimal => Self::Minimal,
            WireProfile::Full => Self::Full,
        }
    }
}

impl From<gcu_mcp::Profile> for WireProfile {
    fn from(profile: gcu_mcp::Profile) -> Self {
        match profile {
            gcu_mcp::Profile::Default => Self::Default,
            gcu_mcp::Profile::Minimal => Self::Minimal,
            gcu_mcp::Profile::Full => Self::Full,
        }
    }
}

/// Why a helper should exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownReason {
    /// A newer binary with the same signing identity connected.
    Replace,
    /// The version strings differ.
    Version,
}

/// Read one frame, or `None` on clean EOF before the length.
///
/// A partial header or body is [`HelperError::Frame`]. Waits on `reader`.
/// `max` is checked before the body is allocated. Dropping `reader` cancels
/// the wait. Do not retry a cancelled read on the same stream.
pub async fn read_frame<R>(reader: &mut R, max: usize) -> Result<Option<Frame>, HelperError>
where
    R: AsyncRead + Unpin,
{
    let mut prefix = [0u8; 4];
    match reader.read(&mut prefix[..1]).await {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(error) => return Err(HelperError::from_io(&error)),
    }
    if let Err(error) = reader.read_exact(&mut prefix[1..]).await {
        return Err(if error.kind() == std::io::ErrorKind::UnexpectedEof {
            HelperError::Frame
        } else {
            HelperError::from_io(&error)
        });
    }
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > max {
        return Err(HelperError::Frame);
    }
    let mut bytes = vec![0u8; length];
    reader
        .read_exact(&mut bytes)
        .await
        .map_err(|error| HelperError::from_io(&error))?;
    // A scoped bootstrap is a different endpoint. Refuse it before MCP.
    if super::scope_link::ordinary_must_refuse(&bytes) {
        return Err(HelperError::Frame);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| HelperError::Frame)
}

/// Write one frame and flush.
///
/// Fails when the encoding is empty or larger than `max`. Waits on `writer`.
/// Dropping `writer` cancels the wait. A partial write is not retried.
pub async fn write_frame<W>(writer: &mut W, frame: &Frame, max: usize) -> Result<(), HelperError>
where
    W: AsyncWrite + Unpin,
{
    let bytes = serde_json::to_vec(frame).map_err(|_| HelperError::Frame)?;
    if bytes.is_empty() || bytes.len() > max {
        return Err(HelperError::Frame);
    }
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(|error| HelperError::from_io(&error))?;
    writer
        .write_all(&bytes)
        .await
        .map_err(|error| HelperError::from_io(&error))?;
    writer
        .flush()
        .await
        .map_err(|error| HelperError::from_io(&error))?;
    Ok(())
}
