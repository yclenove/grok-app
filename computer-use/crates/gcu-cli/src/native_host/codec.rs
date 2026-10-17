//! Chrome native messaging framing. These functions never log JSON payloads.
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::Error;

/// Conservative limit in both directions (Chrome's host-to-browser limit is 1 MiB).
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Read one four-byte little-endian length and JSON object, or clean boundary EOF.
///
/// Waiting for the first byte is cancellable by dropping the future. Once a frame
/// starts it has a five-second deadline; partial EOF is an error, never a retry.
pub async fn read<R: AsyncRead + Unpin>(input: &mut R) -> Result<Option<Value>, Error> {
    let mut prefix = [0; 4];
    if input.read(&mut prefix[..1]).await? == 0 {
        return Ok(None);
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        input.read_exact(&mut prefix[1..]).await?;
        let length = u32::from_le_bytes(prefix) as usize;
        if length == 0 || length > MAX_MESSAGE_BYTES {
            return Err(Error::Frame);
        }
        let mut bytes = vec![0; length];
        input.read_exact(&mut bytes).await?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Frame)?;
        if !value.is_object() {
            return Err(Error::Frame);
        }
        Ok(Some(value))
    })
    .await
    .map_err(|_| Error::Deadline)?
}

/// Write and flush a bounded JSON object within five seconds.
///
/// Serialization/size errors write nothing. IO errors may have written a partial
/// frame; the caller must close the connection and never replay the message.
pub async fn write<W: AsyncWrite + Unpin>(output: &mut W, value: &Value) -> Result<(), Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Frame)?;
    if !value.is_object() || bytes.is_empty() || bytes.len() > MAX_MESSAGE_BYTES {
        return Err(Error::Frame);
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        output
            .write_all(&(bytes.len() as u32).to_le_bytes())
            .await?;
        output.write_all(&bytes).await?;
        output.flush().await?;
        Ok(())
    })
    .await
    .map_err(|_| Error::Deadline)?
}
