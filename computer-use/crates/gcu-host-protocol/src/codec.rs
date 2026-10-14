//! Four-byte network-order length prefix followed by exactly one JSON value.
use crate::{messages::Envelope, Error};
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
/// Maximum encoded JSON frame; checked before allocation.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;
/// Read one frame. Clean EOF before the prefix is `None`; partial EOF is an
/// error. Waits on IO; caller owns the deadline. Cancel by dropping the stream,
/// never retry a cancelled read on the same stream.
pub async fn read_frame<R: AsyncRead + Unpin, T: DeserializeOwned>(
    reader: &mut R,
) -> Result<Option<T>, Error> {
    let mut prefix = [0; 4];
    if reader.read(&mut prefix[..1]).await? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut prefix[1..]).await?;
    let n = u32::from_be_bytes(prefix) as usize;
    if n == 0 || n > MAX_FRAME {
        return Err(Error::Invalid);
    }
    let mut bytes = vec![0; n];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| Error::Invalid)
}
/// Write a bounded frame, awaiting stream backpressure. Caller owns deadline;
/// cancelling a partial write requires closing the connection.
pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Invalid)?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(Error::Invalid);
    }
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}
/// Validate a complete framed envelope without allocation from the length.
/// Used by the fuzz target. Pure, nonblocking, never executes a method.
pub fn validate(bytes: &[u8]) -> Result<Envelope, Error> {
    let prefix: [u8; 4] = bytes
        .get(..4)
        .ok_or(Error::Invalid)?
        .try_into()
        .map_err(|_| Error::Invalid)?;
    let n = u32::from_be_bytes(prefix) as usize;
    if n == 0 || n > MAX_FRAME || bytes.len() != n + 4 {
        return Err(Error::Invalid);
    }
    let message: Envelope = serde_json::from_slice(&bytes[4..]).map_err(|_| Error::Invalid)?;
    message.validate()?;
    Ok(message)
}
