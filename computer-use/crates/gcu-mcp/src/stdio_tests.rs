//! IO failure propagation and immediate lifecycle cancellation.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crate::stdio::Stdio;
use gcu_core::session::{InputRelease, Session};
use gcu_host_api::testing::FakeUserActivity;
use rmcp::{model::ServerJsonRpcMessage, transport::Transport};
use std::{
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
#[derive(Default)]
struct Release(AtomicUsize);
#[async_trait::async_trait]
impl InputRelease for Release {
    async fn release_all_input(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
fn session() -> (Arc<Session>, Arc<Release>) {
    let release = Arc::new(Release::default());
    (
        Arc::new(Session::open(
            "io-errors",
            Arc::new(FakeUserActivity::new()),
            release.clone(),
        )),
        release,
    )
}
struct Broken;
impl AsyncRead for Broken {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::other("fixture read error")))
    }
}
impl AsyncWrite for Broken {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::other("fixture write error")))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::other("fixture shutdown error")))
    }
}
#[tokio::test]
async fn read_error_records_transport_failure_and_closes_session() {
    let (session, release) = session();
    let mut transport = Stdio::new(Broken, tokio::io::sink(), session.clone());
    assert!(transport.receive().await.is_none());
    assert!(transport.failure_flag().load(Ordering::Acquire));
    assert!(session.is_closed());
    assert!(release.0.load(Ordering::Relaxed) > 0);
}
#[tokio::test]
async fn parse_error_write_failure_closes_session_without_spinning() {
    let (session, release) = session();
    let mut transport = Stdio::new(&b"invalid\n"[..], Broken, session.clone());
    assert!(transport.receive().await.is_none());
    assert!(transport.failure_flag().load(Ordering::Acquire));
    assert!(session.is_closed());
    assert!(release.0.load(Ordering::Relaxed) > 0);
}
#[tokio::test]
async fn protocol_write_and_shutdown_errors_are_reported() {
    let (session, _) = session();
    let (_writer, input) = tokio::io::duplex(64);
    let mut transport = Stdio::new(input, Broken, session.clone());
    let message: ServerJsonRpcMessage =
        serde_json::from_value(serde_json::json!({"jsonrpc":"2.0","id":1,"result":{}})).unwrap();
    assert!(transport.send(message).await.is_err());
    assert!(transport.failure_flag().load(Ordering::Acquire));
    assert!(transport.receive().await.is_none());
    assert!(session.is_closed());
    assert!(transport.close().await.is_err());
}
