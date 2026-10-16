//! One admitted connection: handshake, then NDJSON bridged into MCP.
//!
//! The session is registered before the server hello is written. A rejected
//! peer never reaches [`gcu_mcp::Connection::open_host_only`].

use super::auth::{
    admit, valid_idle_secs, valid_version, versions_match, Admit, AuthInput, CodeSignature,
    EndpointStamp, RejectReason,
};
use super::error::HelperError;
use super::frame::{read_frame, write_frame, Frame, Role, ShutdownReason, CONTROL_MAX, MCP_MAX};
use super::proxy::{endpoint_stamp, AuditSource, LoopCmd, SessionRegistry};
use gcu_core::driver::Driver;
use gcu_mcp::codec::{Decoded, NdjsonCodec, MAX_LINE_BYTES};
use gcu_mcp::{Connection, ServerConfig};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::unix::OwnedReadHalf;
use tokio::net::UnixStream;
use tokio::sync::mpsc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HandleResult {
    Closed,
    Notified,
}

pub(crate) struct ServeCtx {
    pub audit: Arc<dyn AuditSource>,
    pub local: CodeSignature,
    pub version: String,
    pub uid: u32,
    pub path: PathBuf,
    pub bound: EndpointStamp,
    pub host: gcu_mcp::Host,
    pub driver: Arc<dyn Driver>,
    pub registry: Arc<SessionRegistry>,
    pub handshake_timeout: Duration,
    pub idle_limit: Duration,
}

pub(crate) async fn handle_connection(
    stream: UnixStream,
    ctx: &ServeCtx,
    cmd_tx: &mpsc::UnboundedSender<LoopCmd>,
) -> HandleResult {
    let peer = match ctx.audit.peer(&stream).await {
        Ok(peer) => peer,
        Err(_) => {
            tracing::info!("helper audit failed closed");
            return HandleResult::Closed;
        }
    };
    let observed = match endpoint_stamp(&ctx.path) {
        Ok(stamp) => stamp,
        Err(_) => return notify(cmd_tx, false).await,
    };
    let decision = admit(AuthInput {
        local_uid: ctx.uid,
        local: &ctx.local,
        peer_uid: peer.uid,
        peer: peer.signature.as_ref(),
        bound: ctx.bound,
        observed,
        directory: None,
    });
    match decision {
        Admit::Reject(reason) => {
            tracing::info!(reason = reason_label(reason), "helper rejected peer");
            if matches!(
                reason,
                RejectReason::EndpointReplaced | RejectReason::EndpointForeign
            ) {
                notify(cmd_tx, false).await
            } else {
                HandleResult::Closed
            }
        }
        Admit::Replace => replace_mode(stream, ctx, cmd_tx).await,
        Admit::Serve => serve_mode(stream, ctx, cmd_tx).await,
    }
}

async fn replace_mode(
    mut stream: UnixStream,
    ctx: &ServeCtx,
    cmd_tx: &mpsc::UnboundedSender<LoopCmd>,
) -> HandleResult {
    let frame = match read_control(&mut stream, ctx.handshake_timeout).await {
        Ok(frame) => frame,
        Err(_) => return HandleResult::Closed,
    };
    if matches!(frame, Frame::Shutdown { .. }) {
        notify(cmd_tx, true).await
    } else {
        HandleResult::Closed
    }
}

async fn serve_mode(
    stream: UnixStream,
    ctx: &ServeCtx,
    cmd_tx: &mpsc::UnboundedSender<LoopCmd>,
) -> HandleResult {
    let (mut read, mut write) = stream.into_split();
    let frame = match read_control(&mut read, ctx.handshake_timeout).await {
        Ok(frame) => frame,
        Err(_) => return HandleResult::Closed,
    };
    let Frame::Hello {
        version,
        role,
        profile,
        turn_idle_secs,
    } = frame
    else {
        return HandleResult::Closed;
    };
    if role != Role::Cli || !valid_idle_secs(turn_idle_secs) || !valid_version(&version) {
        return HandleResult::Closed;
    }
    if !versions_match(&ctx.version, &version) {
        let _ = write_frame(
            &mut write,
            &Frame::Shutdown {
                reason: ShutdownReason::Version,
            },
            CONTROL_MAX,
        )
        .await;
        return notify(cmd_tx, true).await;
    }
    let Ok(connection) = Connection::open_host_only(
        Arc::clone(&ctx.driver),
        ctx.host.clone(),
        ServerConfig {
            profile: profile.into(),
        },
    )
    .await
    else {
        tracing::info!("helper could not open a session");
        return HandleResult::Closed;
    };
    connection
        .lifecycle()
        .set_idle_timeout(Duration::from_secs(turn_idle_secs));
    ctx.registry.insert(connection.session());
    let hello = Frame::Hello {
        version: ctx.version.clone(),
        role: Role::Agent,
        profile,
        turn_idle_secs,
    };
    if write_frame(&mut write, &hello, CONTROL_MAX).await.is_err() {
        return HandleResult::Closed;
    }
    let _ = bridge_connection(connection, read, write).await;
    HandleResult::Closed
}

async fn read_control<R>(reader: &mut R, timeout: Duration) -> Result<Frame, HelperError>
where
    R: AsyncRead + Unpin,
{
    match tokio::time::timeout(timeout, read_frame(reader, CONTROL_MAX)).await {
        Ok(Ok(Some(frame))) => Ok(frame),
        Ok(Ok(None)) | Ok(Err(_)) | Err(_) => Err(HelperError::Frame),
    }
}

async fn notify(cmd_tx: &mpsc::UnboundedSender<LoopCmd>, shutdown: bool) -> HandleResult {
    if shutdown {
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        if cmd_tx.send(LoopCmd::Shutdown(ack_tx)).is_ok() {
            let _ = ack_rx.await;
        }
    } else {
        let _ = cmd_tx.send(LoopCmd::Replaced);
    }
    HandleResult::Notified
}

async fn bridge_connection(
    connection: Connection,
    read: OwnedReadHalf,
    write: tokio::net::unix::OwnedWriteHalf,
) -> Result<(), HelperError> {
    let (bridge_io, server_io) = tokio::io::duplex(MAX_LINE_BYTES + 4096);
    let (server_read, server_write) = tokio::io::split(server_io);
    let (bridge_read, bridge_write) = tokio::io::split(bridge_io);
    let serve = tokio::spawn(async move {
        let _ = connection.serve_stream(server_read, server_write).await;
    });
    let bridged = bridge_ndjson(read, write, bridge_read, bridge_write).await;
    serve.abort();
    bridged
}

/// Bridge length-prefixed helper frames with NDJSON.
///
/// Waits until either side reaches EOF or a frame is rejected. Dropping the
/// future cancels both directions. There is no per-frame retry. MCP bodies are
/// not logged.
pub(crate) async fn bridge_ndjson<SR, SW, NR, NW>(
    mut socket_read: SR,
    mut socket_write: SW,
    mut ndjson_read: NR,
    mut ndjson_write: NW,
) -> Result<(), HelperError>
where
    SR: AsyncRead + Unpin,
    SW: AsyncWrite + Unpin,
    NR: AsyncRead + Unpin,
    NW: AsyncWrite + Unpin,
{
    let inbound = async {
        loop {
            match read_frame(&mut socket_read, MCP_MAX).await? {
                None => return Ok(()),
                Some(Frame::Mcp { body }) => write_ndjson(&mut ndjson_write, &body).await?,
                Some(Frame::Hello { .. } | Frame::Shutdown { .. }) => {
                    return Err(HelperError::Frame)
                }
            }
        }
    };
    let outbound = async {
        let mut codec = NdjsonCodec::default();
        let mut buffer = [0u8; 8192];
        loop {
            let read = ndjson_read
                .read(&mut buffer)
                .await
                .map_err(|error| HelperError::from_io(&error))?;
            if read == 0 {
                return match codec.finish() {
                    None => Ok(()),
                    Some(_) => Err(HelperError::Frame),
                };
            }
            for event in codec.push(&buffer[..read]) {
                match event {
                    Decoded::Message(body) => {
                        write_frame(&mut socket_write, &Frame::Mcp { body }, MCP_MAX).await?;
                    }
                    Decoded::Error(_) => return Err(HelperError::Frame),
                }
            }
        }
    };
    tokio::select! {
        result = inbound => result,
        result = outbound => result,
    }
}

async fn write_ndjson<W>(writer: &mut W, body: &serde_json::Value) -> Result<(), HelperError>
where
    W: AsyncWrite + Unpin,
{
    let mut line = serde_json::to_vec(body).map_err(|_| HelperError::Frame)?;
    if line.len() > MAX_LINE_BYTES {
        return Err(HelperError::Frame);
    }
    line.push(b'\n');
    writer
        .write_all(&line)
        .await
        .map_err(|error| HelperError::from_io(&error))?;
    writer
        .flush()
        .await
        .map_err(|error| HelperError::from_io(&error))
}

fn reason_label(reason: RejectReason) -> &'static str {
    match reason {
        RejectReason::EndpointForeign => "foreign",
        RejectReason::EndpointReplaced => "replaced",
        RejectReason::UidMismatch => "uid",
        RejectReason::UnsignedPeer => "unsigned",
        RejectReason::SigningIdentityMismatch => "identity",
    }
}
