//! Scoped frame admission below rmcp's bootstrap and default dispatch.
use crate::{
    codec::Decoded,
    server::Server,
    stdio::{Admission, Stdio},
};
use rmcp::{
    model::ClientJsonRpcMessage, service::TxJsonRpcMessage, transport::Transport, RoleServer,
};
use serde_json::{json, Value};
use std::{future::Future, io};
use tokio::io::{AsyncRead, AsyncWrite};

pub(crate) struct Scoped<R, W> {
    inner: Stdio<R, W>,
    server: Server,
}
impl<R, W> Scoped<R, W> {
    pub(crate) fn new(inner: Stdio<R, W>, server: Server) -> Self {
        Self { inner, server }
    }
}
impl<R, W> Transport<RoleServer> for Scoped<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    type Error = io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        // Already admitted tool replies keep Results' publication boundary,
        // including native unknown delivery and Stop priority.
        self.inner.send(item)
    }

    async fn receive(&mut self) -> Option<ClientJsonRpcMessage> {
        // Server retains the authority captured at connection creation. This
        // never arms a new attempt from the lease's current generation.
        let server = &self.server;
        self.inner
            .receive_checked(|frame| {
                if server.usable() {
                    return Admission::Allow;
                }
                // Never forward an unusable frame, even if rmcp would answer
                // it without calling the Service. No id means close silently.
                let Decoded::Message(value) = frame else {
                    return Admission::Close;
                };
                let Some(id) = value.get("id").filter(|id| id.is_string() || id.is_i64()) else {
                    return Admission::Close;
                };
                let Some(method) = value.get("method").and_then(Value::as_str) else {
                    return Admission::Close;
                };
                let response = if method == "tools/call" {
                    json!({"jsonrpc":"2.0","id":id,"result":server.scope_refusal()})
                } else {
                    json!({"jsonrpc":"2.0","id":id,"error":crate::server_authority::denied()})
                };
                Admission::Refuse(response)
            })
            .await
    }

    async fn close(&mut self) -> io::Result<()> {
        self.inner.close().await
    }
}
