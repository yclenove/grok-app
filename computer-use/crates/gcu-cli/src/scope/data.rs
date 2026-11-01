//! MCP data plane. This module can read a lease and arm attempts, never install,
//! renew, rebind or revoke it. A generation change closes existing connections.
use super::fake::FakeEvidence;
use super::DataPlane;
use crate::exit::Failure;
use gcu_core::{
    driver::Driver,
    scope::{AttemptAuthority, ScopeLease},
};
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::{TcpListener, TcpStream},
    task::JoinSet,
};

pub(super) async fn serve(
    driver: gcu_driver_fake::FakeDriver,
    lease: ScopeLease,
    children: crate::script_children::Children,
    profile: gcu_mcp::Profile,
    mode: Option<String>,
    plane: DataPlane,
) -> Result<(), Failure> {
    let mut initial = lease.arm_attempt().ok_or_else(closed)?;
    if !initial.confirm(&lease) {
        return Err(closed());
    }
    let host = Arc::new(crate::runtime::open_local_host(mode).await?);
    let evidence = initial
        .enter(FakeEvidence::new(driver.clone()))
        .await
        .map_err(|_| Failure::internal("Fake instance evidence unavailable"))?;
    if !initial.confirm(&lease) {
        return Err(closed());
    }
    let signal_sources = StopSignals::new()?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| closed())?;
    let address = listener.local_addr().map_err(|_| closed())?;
    // This stdout channel is supervisor-only; only the data address is later
    // copied to client configuration. Install ACK has already been flushed.
    let mut stdout = tokio::io::stdout();
    let ready = format!("{}\n", serde_json::json!({"data_address":address}));
    stdout
        .write_all(ready.as_bytes())
        .await
        .map_err(|_| closed())?;
    stdout.flush().await.map_err(|_| closed())?;
    let signals = stop_signals(host.clone(), driver, signal_sources);
    tokio::pin!(signals);
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            result = &mut signals => return result,
            // Always reap completed clients. JoinSet Drop aborts all remaining
            // workers if the private control future wins the outer select.
            _ = clients.join_next(), if !clients.is_empty() => {},
            accepted = listener.accept(), if clients.len() < 32 => {
                let (socket, _) = accepted.map_err(|_| closed())?;
                match admit_data(&plane, &lease) {
                    Ok(authority) => {
                        let children = children.clone();
                        let driver = evidence.clone();
                        let lease = lease.clone();
                        let host = host.clone();
                        clients.spawn(async move {
                            let _ = serve_one(
                                socket, driver, lease, authority, host, children, profile,
                            )
                            .await;
                        });
                    }
                    Err(_) => {
                        drop(socket);
                        // A rebind gap still has a current lease. Keep serving.
                        // A revoked lease has nothing left to admit.
                        if lease.current().is_none() {
                            return Err(closed());
                        }
                    }
                }
            }
        }
    }
}
fn closed() -> Failure {
    Failure::internal("Scope data plane unavailable")
}

/// Arm one fresh connection only when `plane` already admits the lease's
/// current generation.
///
/// A refusal does not arm an attempt and does not count an effect. Memory
/// only. No timeout or cancellation. The published generation may be newer
/// than the admitted one while its ACK is still being written.
pub(crate) fn admit_data(
    plane: &DataPlane,
    lease: &ScopeLease,
) -> Result<AttemptAuthority, Failure> {
    let current = lease.current().ok_or_else(closed)?;
    if !plane.allows(current.generation()) {
        return Err(closed());
    }
    let mut authority = lease.arm_attempt().ok_or_else(closed)?;
    if !authority.confirm(&lease) || !plane.allows(authority.scope().generation()) {
        return Err(closed());
    }
    plane.request().map_err(|_| closed())?;
    Ok(authority)
}

async fn serve_one(
    mut socket: TcpStream,
    driver: FakeEvidence,
    lease: ScopeLease,
    mut authority: AttemptAuthority,
    host: Arc<crate::host::LocalHost>,
    children: crate::script_children::Children,
    profile: gcu_mcp::Profile,
) -> Result<(), Failure> {
    if !authority.confirm(&lease) {
        return Err(closed());
    }
    let connection_authority = authority.clone();
    let connection = tokio::select! {
        biased;
        _=authority.changed()=>return Err(closed()),
        result=gcu_mcp::Connection::open_scoped_with_authority(driver,lease.clone(),connection_authority,host.host.clone(),gcu_mcp::ServerConfig {
            profile,browser_session:None,route_note:String::new(),script:crate::script_worker::runner_owned(children),
        })=>result?,
    };
    if !authority.confirm(&lease) {
        return Err(closed());
    }
    let session = connection.session();
    let lifecycle = connection.lifecycle();
    let control = crate::resume::Control::start_with_lifecycle(
        session.clone(),
        host.activity.clone(),
        lifecycle,
    )
    .await?;
    // rmcp may spawn service writers. Give it only an in-memory stream. The
    // actual socket stays in this cancellable attempt, so no detached service
    // task can keep the MCP data connection alive after authority is lost.
    let (mut bridge, server) = tokio::io::duplex(64 * 1024);
    let (input, output) = tokio::io::split(server);
    let bridge_authority = authority.clone();
    let result = tokio::select! {
        biased;
        _=authority.changed()=>Err(closed()),
        result=connection.serve_stream(input,output)=>result.map_err(Failure::from),
        result=copy_scoped(&mut socket,&mut bridge,bridge_authority)=>result.map_err(|_|closed()),
    };
    drop(socket);
    drop(bridge);
    drop(control);
    let _ = tokio::time::timeout(Duration::from_secs(5), session.session_closed()).await;
    result
}

// Check on every IO poll, including polls resumed after a pending read/write.
// Cancellation alone cannot protect a copy future polled before its watcher.
struct ScopedIo<T> {
    inner: T,
    authority: AttemptAuthority,
}
impl<T> ScopedIo<T> {
    fn check(&self) -> io::Result<()> {
        self.authority
            .usable()
            .then_some(())
            .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "scope closed"))
    }
}
impl<T: AsyncRead + Unpin> AsyncRead for ScopedIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if let Err(error) = self.check() {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for ScopedIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if let Err(error) = self.check() {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Err(error) = self.check() {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

async fn copy_scoped<A, B>(
    socket: A,
    mut bridge: B,
    mut authority: AttemptAuthority,
) -> io::Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let copy_authority = authority.clone();
    tokio::select! {
        biased;
        _ = authority.changed() => Err(io::Error::new(io::ErrorKind::PermissionDenied, "scope closed")),
        copied = copy_authorized(socket, &mut bridge, copy_authority) => copied,
    }
}

async fn copy_authorized<A, B>(
    socket: A,
    mut bridge: B,
    authority: AttemptAuthority,
) -> io::Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let mut socket = ScopedIo {
        inner: socket,
        authority,
    };
    tokio::io::copy_bidirectional(&mut socket, &mut bridge)
        .await
        .map(|_| ())
}

#[cfg(test)]
#[path = "data_tests.rs"]
mod tests;

struct StopSignals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
}
impl StopSignals {
    fn new() -> Result<Self, Failure> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            Ok(Self {
                interrupt: signal(SignalKind::interrupt()).map_err(|_| closed())?,
                terminate: signal(SignalKind::terminate()).map_err(|_| closed())?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {})
        }
    }
}
async fn stop_signals(
    host: Arc<crate::host::LocalHost>,
    driver: gcu_driver_fake::FakeDriver,
    #[allow(unused_mut)] mut _signals: StopSignals,
) -> Result<(), Failure> {
    loop {
        #[cfg(unix)]
        tokio::select! {biased;_=_signals.interrupt.recv()=>{},_=_signals.terminate.recv()=>{}}
        #[cfg(windows)]
        tokio::signal::ctrl_c().await.map_err(|_| closed())?;
        host.activity.latch();
        let activity = host.activity.clone();
        let persist = tokio::task::spawn_blocking(move || activity.persist_stop());
        let _ = tokio::time::timeout(
            gcu_core::session::NATIVE_CALL_TIMEOUT,
            driver.release_all_input(),
        )
        .await;
        if !matches!(
            tokio::time::timeout(Duration::from_secs(5), persist).await,
            Ok(Ok(Ok(())))
        ) {
            return Err(closed());
        }
    }
}
