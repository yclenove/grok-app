//! Recovering NDJSON transport. Reading never waits for a tool to finish.
use crate::codec::{Decoded, NdjsonCodec};
use gcu_core::session::Session;
use rmcp::{
    model::ClientJsonRpcMessage, service::TxJsonRpcMessage, transport::Transport, RoleServer,
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    future::Future,
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::Mutex,
};

pub(crate) struct Stdio<R, W> {
    input: R,
    output: Arc<Mutex<W>>,
    codec: NdjsonCodec,
    pending: VecDeque<Decoded>,
    eof: bool,
    failed: Arc<AtomicBool>,
    failed_cancel: gcu_host_api::Cancel,
    session: Arc<Session>,
    turns: crate::lifecycle::Lifecycle,
    lifecycle: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
    error_send: Option<Pin<Box<dyn Future<Output = io::Result<()>> + Send>>>,
}
impl<R, W> Stdio<R, W> {
    pub(crate) fn new(
        input: R,
        output: W,
        session: Arc<Session>,
        turns: crate::lifecycle::Lifecycle,
    ) -> Self {
        Self {
            input,
            output: Arc::new(Mutex::new(output)),
            codec: NdjsonCodec::default(),
            pending: VecDeque::new(),
            eof: false,
            failed: Arc::new(AtomicBool::new(false)),
            failed_cancel: gcu_host_api::Cancel::new(),
            session,
            turns,
            lifecycle: None,
            error_send: None,
        }
    }
    pub(crate) fn failure_flag(&self) -> Arc<AtomicBool> {
        self.failed.clone()
    }
}
async fn write<W: AsyncWrite + Unpin>(
    output: &Mutex<W>,
    value: &impl serde::Serialize,
) -> io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(io::Error::other)?;
    line.push(b'\n');
    let mut output = output.lock().await;
    output.write_all(&line).await?;
    output.flush().await
}
impl<R, W> Transport<RoleServer> for Stdio<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    type Error = io::Error;
    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        let output = self.output.clone();
        let failed = self.failed.clone();
        let failed_cancel = self.failed_cancel.clone();
        async move {
            let result = write(&output, &item).await;
            if result.is_err() {
                failed.store(true, Ordering::Release);
                failed_cancel.cancel();
            }
            result
        }
    }
    async fn receive(&mut self) -> Option<ClientJsonRpcMessage> {
        loop {
            if self.failed_cancel.is_cancelled() && !self.eof {
                self.eof = true;
                self.pending.clear();
                let session = self.session.clone();
                self.lifecycle = Some(Box::pin(async move {
                    session.session_closed().await;
                }));
            }
            if let Some(lifecycle) = &mut self.lifecycle {
                lifecycle.await;
                self.lifecycle = None;
            }
            if let Some(send) = &mut self.error_send {
                if send.await.is_err() {
                    self.failed.store(true, Ordering::Release);
                    self.eof = true;
                    self.pending.clear();
                    let session = self.session.clone();
                    self.lifecycle = Some(Box::pin(async move {
                        session.session_closed().await;
                    }));
                }
                self.error_send = None;
                continue;
            }
            if let Some(item) = self.pending.pop_front() {
                let (code, id) = match item {
                    Decoded::Message(value) => {
                        let id = value
                            .get("id")
                            .filter(|id| id.is_string() || id.is_i64())
                            .cloned()
                            .unwrap_or(Value::Null);
                        let valid_id = value
                            .get("id")
                            .is_none_or(|id| id.is_string() || id.is_i64());
                        if valid_id && value.get("jsonrpc").and_then(Value::as_str) == Some("2.0") {
                            let turn_ended = value.get("method").and_then(Value::as_str)
                                == Some("notifications/turn-ended")
                                && value.get("id").is_none();
                            if let Ok(message) = serde_json::from_value(value) {
                                if turn_ended {
                                    let turns = self.turns.clone();
                                    self.lifecycle = Some(Box::pin(async move {
                                        turns.turn_ended(None).await;
                                    }));
                                    continue;
                                }
                                return Some(message);
                            }
                        }
                        (-32600, id)
                    }
                    Decoded::Error(code) => (code, Value::Null),
                };
                let message = if code == -32700 {
                    "Parse error"
                } else {
                    "Invalid request"
                };
                let output = self.output.clone();
                self.error_send = Some(Box::pin(async move {
                    write(
                        &output,
                        &json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}),
                    )
                    .await
                }));
            } else if self.eof {
                return None;
            } else {
                let mut bytes = [0; 8192];
                let read = tokio::select! {
                    biased;
                    _ = self.failed_cancel.cancelled() => Err(io::Error::other("protocol output failed")),
                    result = self.input.read(&mut bytes) => result,
                };
                if read.is_err() {
                    self.failed.store(true, Ordering::Release);
                }
                match read {
                    Ok(0) | Err(_) => {
                        self.eof = true;
                        self.pending.extend(self.codec.finish());
                        // Cancel approvals BEFORE rmcp's five-second graceful
                        // response drain. Store the future for select cancellation.
                        let session = self.session.clone();
                        self.lifecycle = Some(Box::pin(async move {
                            session.session_closed().await;
                        }));
                    }
                    Ok(n) => self.pending.extend(self.codec.push(&bytes[..n])),
                }
            }
        }
    }
    async fn close(&mut self) -> io::Result<()> {
        let result = self.output.lock().await.shutdown().await;
        if result.is_err() {
            self.failed.store(true, Ordering::Release);
        }
        result
    }
}
