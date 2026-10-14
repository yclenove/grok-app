//! Bounded JSON frames. Streams must be discarded after a cancelled/invalid frame.
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Maximum JSON payload, including raw PNG bytes represented as JSON integers.
pub const MAX_FRAME: usize = 32 * 1024 * 1024;
/// Payload-free framing error; no provider data enters diagnostics.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// EOF in a prefix/body, or failed pipe IO.
    #[error("worker pipe IO failed")]
    Io(#[from] std::io::Error),
    /// Empty, oversized, malformed or unexpected JSON value.
    #[error("invalid worker frame")]
    Invalid,
}
/// Read a four-byte big-endian length and one JSON value. EOF before a prefix
/// returns None. Size is checked before allocation. Caller supplies a deadline.
pub async fn read_frame<R: AsyncRead + Unpin, T: DeserializeOwned + Send + 'static>(
    input: &mut R,
) -> Result<Option<T>, FrameError> {
    let mut prefix = [0; 4];
    if input.read(&mut prefix[..1]).await? == 0 {
        return Ok(None);
    }
    input.read_exact(&mut prefix[1..]).await?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(FrameError::Invalid);
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes).await?;
    tokio::task::spawn_blocking(move || serde_json::from_slice(&bytes))
        .await
        .map_err(|_| FrameError::Invalid)?
        .map(Some)
        .map_err(|_| FrameError::Invalid)
}
/// Encode without writing, so serialization/size errors prove no execution.
/// The capped writer also bounds allocation for an oversized local request.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, FrameError> {
    struct Bounded(Vec<u8>);
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_FRAME.saturating_sub(self.0.len()) {
                return Err(std::io::Error::other("frame limit"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded(Vec::new());
    serde_json::to_writer(&mut output, value).map_err(|_| FrameError::Invalid)?;
    Ok(output.0)
}
/// Write a previously bounded payload and flush. A partial/cancelled write
/// poisons the stream; the supervisor kills its worker before any subsequent RPC.
pub async fn write_payload<W: AsyncWrite + Unpin>(
    output: &mut W,
    bytes: &[u8],
) -> Result<(), FrameError> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(FrameError::Invalid);
    }
    output
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await?;
    output.write_all(bytes).await?;
    output.flush().await?;
    Ok(())
}
/// Serialize and write one frame. Caller owns the deadline.
pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    output: &mut W,
    value: &T,
) -> Result<(), FrameError> {
    write_payload(output, &encode(value)?).await
}
