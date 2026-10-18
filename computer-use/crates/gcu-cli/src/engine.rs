//! App-managed process: Host-only approvals, isolated HTTP sessions, Stop task.
use crate::exit::Failure;
use gcu_core::{driver::Driver, session::Session};
use gcu_host_protocol::{
    auth::*,
    messages as w,
    server::{Incoming, Peer, RPC_TIMEOUT},
    transport, Error,
};
use gcu_mcp::host::{PhysicalActivity, RemoteHost, RemoteSession, StopMirror, UnavailableActivity};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};
struct Entry {
    lifecycle: Arc<Session>,
    remote: Mutex<RemoteSession>,
}
type Sessions = Arc<RwLock<HashMap<String, Arc<Entry>>>>;
fn parse<T: DeserializeOwned>(v: Value) -> Result<T, w::RpcError> {
    serde_json::from_value(v).map_err(|_| w::RpcError::new(-32602, "invalid parameters"))
}
fn encode<T: Serialize>(v: T) -> Result<Value, w::RpcError> {
    serde_json::to_value(v).map_err(|_| w::RpcError::new(-32603, "internal"))
}
fn internal(_: impl std::fmt::Debug) -> w::RpcError {
    w::RpcError::new(-32000, "handoff_required")
}
async fn lookup(sessions: &Sessions, id: &str) -> Result<Arc<Entry>, w::RpcError> {
    sessions
        .read()
        .await
        .get(id)
        .cloned()
        .ok_or_else(|| w::RpcError::new(-32004, "unknown session"))
}

enum Prepared {
    /// Driver and activity already chosen. Fake stays on this path.
    Given {
        driver: Arc<dyn Driver>,
        activity: Arc<dyn PhysicalActivity>,
    },
    /// macOS App subprocess. The driver is built only after authentication.
    ///
    /// Linux builds keep the variant so the refusal path type-checks, and
    /// never construct it: `run_macos` returns before `run_prepared`.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Macos,
}

/// Handshake input. The macOS arm has no driver until [`connect_engine`] returns.
pub(crate) enum ConnectInput<'a> {
    /// Snapshot this driver. Fake, and Linux X11 before the Host latch is installed.
    Given(&'a dyn Driver, &'a dyn PhysicalActivity),
    /// Conservative capabilities. The macOS driver is attached after connect.
    Macos,
    /// Conservative capabilities. The Windows worker starts after connect.
    ///
    /// Other builds keep the variant so the handshake match type-checks.
    /// Only the Windows `native-windows` route constructs it.
    #[cfg_attr(not(all(windows, feature = "native-windows")), allow(dead_code))]
    Windows,
}

pub(crate) async fn run(
    driver: Arc<dyn Driver>,
    route_note: String,
    host_fd: Option<usize>,
    socket: Option<PathBuf>,
    token_fd: Option<usize>,
    stop_fd: usize,
) -> Result<u8, Failure> {
    // Fake stays on UnavailableActivity. Linux X11 and Windows native bind the
    // Host latch after connect, then pass their clock to [`serve_engine`].
    run_prepared(
        Prepared::Given {
            driver,
            activity: Arc::new(UnavailableActivity),
        },
        route_note,
        host_fd,
        socket,
        token_fd,
        stop_fd,
    )
    .await
}

/// Authenticated macOS engine. Refuses before any native driver when the
/// App TCC chain is not the measured macOS 27 arm64 case.
pub(crate) async fn run_macos(
    route_note: String,
    host_fd: Option<usize>,
    socket: Option<PathBuf>,
    token_fd: Option<usize>,
    stop_fd: usize,
) -> Result<u8, Failure> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (route_note, host_fd, socket, token_fd, stop_fd);
        Err(Failure::arguments(
            crate::host::macos_wire::TERMINAL_NATIVE_REFUSAL,
        ))
    }
    #[cfg(target_os = "macos")]
    {
        run_prepared(
            Prepared::Macos,
            route_note,
            host_fd,
            socket,
            token_fd,
            stop_fd,
        )
        .await
    }
}

async fn run_prepared(
    prepared: Prepared,
    route_note: String,
    host_fd: Option<usize>,
    socket: Option<PathBuf>,
    token_fd: Option<usize>,
    stop_fd: usize,
) -> Result<u8, Failure> {
    let input = match &prepared {
        Prepared::Given { driver, activity } => {
            ConnectInput::Given(driver.as_ref(), activity.as_ref())
        }
        Prepared::Macos => ConnectInput::Macos,
    };
    let session = connect_engine(input, host_fd, socket, token_fd, stop_fd).await?;
    // macOS builds its driver here, after both channels have authenticated.
    let (driver, activity) = match prepared {
        Prepared::Given { driver, activity } => (driver, activity),
        Prepared::Macos => {
            match crate::host::macos_wire::engine_after_auth(
                session.stop.clone(),
                session.local_stop.clone(),
            )
            .await
            {
                Ok(wired) => wired,
                Err(error) => {
                    session.peer.close();
                    session.stop_peer.close();
                    return Err(error);
                }
            }
        }
    };
    serve_engine(driver, activity, route_note, session).await
}

/// Handshake state. Capabilities are snapshotted before a platform bind replaces the driver.
pub(crate) struct EngineSession {
    pub(crate) peer: Peer,
    incoming: Incoming,
    pub(crate) stop_peer: Peer,
    stop_incoming: Incoming,
    /// Host Stop mirror from the authenticated handshake.
    pub(crate) stop: StopMirror,
    /// Buffers one local Stop until the control task is running.
    pub(crate) local_stop: tokio::sync::mpsc::Sender<gcu_host_protocol::stop_reason::StopReason>,
    local_stop_rx: tokio::sync::mpsc::Receiver<gcu_host_protocol::stop_reason::StopReason>,
    sessions: Sessions,
}

/// Authenticate both channels and snapshot capabilities before any Host latch is installed.
pub(crate) async fn connect_engine(
    input: ConnectInput<'_>,
    host_fd: Option<usize>,
    socket: Option<PathBuf>,
    token_fd: Option<usize>,
    stop_fd: usize,
) -> Result<EngineSession, Failure> {
    let (stream, bootstrap) = if let Some(fd) = host_fd {
        let mut stream = transport::inherited(fd)?;
        let bootstrap = Bootstrap::read(&mut stream).await?;
        (stream, bootstrap)
    } else {
        let bootstrap = transport::bootstrap_fd(
            token_fd.ok_or_else(|| Failure::arguments("Missing host token fd"))?,
        )
        .await?;
        let path = socket.ok_or_else(|| Failure::arguments("Missing host socket"))?;
        let pin = bootstrap
            .endpoint
            .as_ref()
            .ok_or_else(|| Failure::arguments("Missing private endpoint pin"))?;
        (
            transport::connect(&path, pin, &bootstrap.host).await?,
            bootstrap,
        )
    };
    let secret = bootstrap.secret();
    let launcher = bootstrap.host.clone();
    let mut config = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        tokio::task::spawn_blocking(move || {
            transport::check_launcher(&launcher)?;
            AuthConfig::current(Role::Engine, Channel::Rpc)
        }),
    )
    .await
    .map_err(|_| Error::Timeout)?
    .map_err(|_| Error::Authentication)??;
    if matches!(input, ConnectInput::Macos)
        && crate::host::macos_wire::engine_assembly_allowed(
            true,
            crate::host::macos_wire::TccEvidence::current(),
        )
        .is_err()
    {
        return Err(Failure::arguments(
            crate::host::macos_wire::TERMINAL_NATIVE_REFUSAL,
        ));
    }
    let capabilities = match input {
        ConnectInput::Given(driver, activity) => activity_capabilities(driver, activity),
        ConnectInput::Macos => crate::host::macos_wire::conservative_handshake(),
        ConnectInput::Windows => crate::native_windows::conservative_handshake(),
    };
    config.capabilities = Some(capabilities);
    // Registration is deliberately before any driver work or approval request.
    let (peer, incoming, authenticated) =
        Peer::connect(stream, &config, &bootstrap.host, &secret).await?;
    let mut stop_config = config.clone();
    stop_config.channel = Channel::Stop;
    let (stop_peer, stop_incoming, stop_auth) = Peer::connect(
        transport::inherited(stop_fd)?,
        &stop_config,
        &bootstrap.host,
        &secret.stop_secret()?,
    )
    .await?;
    if authenticated.stopped != stop_auth.stopped {
        return Err(Failure::internal("Inconsistent Host Stop state"));
    }
    let stop = StopMirror::new(authenticated.stopped);
    let sessions: Sessions = Arc::new(RwLock::new(HashMap::new()));
    // The sender stays alive so a dropped channel is not a local Stop.
    let (local_stop, local_stop_rx) = tokio::sync::mpsc::channel(1);
    Ok(EngineSession {
        peer,
        incoming,
        stop_peer,
        stop_incoming,
        stop,
        local_stop,
        local_stop_rx,
        sessions,
    })
}

/// Serve RPC with the driver that already holds the Host latch.
pub(crate) async fn serve_engine(
    driver: Arc<dyn Driver>,
    activity: Arc<dyn PhysicalActivity>,
    route_note: String,
    session: EngineSession,
) -> Result<u8, Failure> {
    let EngineSession {
        peer,
        incoming,
        stop_peer,
        stop_incoming,
        stop,
        local_stop,
        local_stop_rx,
        sessions,
    } = session;
    let marker_dir = gcu_host_protocol::stop_reason::state_dir();
    let stop_task = tokio::spawn(control(
        stop_peer.clone(),
        stop_incoming,
        stop.clone(),
        sessions.clone(),
        driver.clone(),
        local_stop_rx,
        marker_dir,
    ));
    let remote = match RemoteHost::new(peer.clone(), stop.clone(), activity).await {
        Ok(remote) => remote,
        Err(e) => {
            stop_task.abort();
            stop_peer.close();
            peer.close();
            return Err(e.into());
        }
    };
    let result = rpc(
        peer.clone(),
        incoming,
        stop_peer.clone(),
        driver,
        remote,
        sessions.clone(),
        &route_note,
    )
    .await;
    // Ordinary teardown. Not an engine-local stop, so no reason file is written.
    stop.latch(None);
    peer.close();
    stop_peer.close();
    stop_task.abort();
    drop(local_stop);
    let entries: Vec<_> = sessions.write().await.drain().map(|(_, s)| s).collect();
    for entry in entries {
        entry.remote.lock().await.close().await?;
    }
    result.map(|_| 0).map_err(Failure::from)
}
fn activity_capabilities(
    driver: &dyn Driver,
    activity: &dyn PhysicalActivity,
) -> gcu_protocol::Capabilities {
    let mut capabilities = driver.capabilities().clone();
    capabilities.user_activity_guard &= activity.ms_since_physical_input().is_some();
    capabilities
}

#[cfg(test)]
mod activity_tests {
    use super::*;
    #[test]
    fn engine_without_clock_keeps_activity_unavailable() {
        let driver = gcu_driver_fake::FakeDriver::contract_fixture().unwrap();
        assert_eq!(UnavailableActivity.ms_since_physical_input(), None);
        assert!(!activity_capabilities(&driver, &UnavailableActivity).user_activity_guard);
    }
}

async fn control(
    peer: Peer,
    mut incoming: Incoming,
    stop: StopMirror,
    sessions: Sessions,
    driver: Arc<dyn Driver>,
    mut local_stop: tokio::sync::mpsc::Receiver<gcu_host_protocol::stop_reason::StopReason>,
    marker_dir: Option<PathBuf>,
) {
    let mut local_open = true;
    loop {
        let request = tokio::select! {
            biased;
            _ = peer.disconnected() => break,
            reason = local_stop.recv(), if local_open => {
                let Some(reason) = reason else {
                    local_open = false;
                    continue;
                };
                local_stop_commit(&stop, &driver, marker_dir.as_deref(), reason).await;
                peer.close();
                break;
            }
            request = incoming.recv() => { let Some(request) = request else { break }; request }
        };
        let answer = match request.method.as_str() {
            "engine.session.stop" => match parse::<w::Stop>(request.params.clone()) {
                Ok(p) => {
                    // Synchronous latch first. Release tasks never await the action
                    // mutex or block processing another Stop or trusted resume.
                    stop.latch(p.session_id.as_deref());
                    // Session creation/close may own the map write lock. Do not
                    // let that delay input release or acknowledgment of Stop.
                    let sessions = sessions.clone();
                    tokio::spawn(async move {
                        let entries: Vec<_> = sessions
                            .read()
                            .await
                            .iter()
                            .filter(|(id, _)| {
                                p.session_id.as_ref().is_none_or(|wanted| wanted == *id)
                            })
                            .map(|(_, e)| e.lifecycle.clone())
                            .collect();
                        for session in entries {
                            tokio::spawn(async move {
                                session.handle_stop().await;
                            });
                        }
                    });
                    // Also covers a Stop received during initial policy/bootstrap.
                    let driver = driver.clone();
                    tokio::spawn(async move {
                        let _ = tokio::time::timeout(
                            gcu_core::session::NATIVE_CALL_TIMEOUT,
                            driver.release_all_input(),
                        )
                        .await;
                    });
                    encode(w::Empty {})
                }
                Err(e) => Err(e),
            },
            "engine.session.resume" => match parse::<w::Resume>(request.params.clone()) {
                Ok(p) if stop.resume_by_user(p.session_id.as_deref(), p.sequence, p.user_event) => {
                    encode(w::Empty {})
                }
                Ok(_) => Err(w::RpcError::new(-32003, "untrusted or replayed user event")),
                Err(e) => Err(e),
            },
            _ => Err(w::RpcError::new(-32601, "control method unavailable")),
        };
        peer.respond(&request, answer);
    }
    stop.latch(None);
    // Losing the trusted control channel is itself a Stop. Release immediately
    // without waiting for ordinary RPC teardown or an in-flight native call.
    tokio::spawn(async move {
        let _ = tokio::time::timeout(
            gcu_core::session::NATIVE_CALL_TIMEOUT,
            driver.release_all_input(),
        )
        .await;
    });
    peer.close();
}
/// Latch and release first, then write the display-only marker, then return.
/// The caller closes the control connection. A failed write still returns.
async fn local_stop_commit(
    stop: &StopMirror,
    driver: &Arc<dyn Driver>,
    marker_dir: Option<&std::path::Path>,
    reason: gcu_host_protocol::stop_reason::StopReason,
) {
    stop.latch(None);
    let releasing = driver.clone();
    tokio::spawn(async move {
        let _ = tokio::time::timeout(
            gcu_core::session::NATIVE_CALL_TIMEOUT,
            releasing.release_all_input(),
        )
        .await;
    });
    let Some(dir) = marker_dir.map(std::path::Path::to_path_buf) else {
        return;
    };
    let write = tokio::task::spawn_blocking(move || {
        let _ = gcu_host_protocol::stop_reason::write_marker(&dir, reason);
    });
    let _ = tokio::time::timeout(Duration::from_secs(5), write).await;
}
async fn rpc(
    peer: Peer,
    mut incoming: Incoming,
    stop_peer: Peer,
    driver: Arc<dyn Driver>,
    host: Arc<RemoteHost>,
    sessions: Sessions,
    route_note: &str,
) -> Result<(), Error> {
    let start = Instant::now();
    let mut health = tokio::time::interval(Duration::from_secs(10));
    loop {
        let request = tokio::select! {biased;
            _=peer.disconnected()=>return Ok(()),
            _=stop_peer.disconnected()=>return Err(Error::Disconnected),
            request=incoming.recv()=>{let Some(r)=request else {return Ok(())};r},
            _=health.tick()=>{let _=peer.notify("host.engine.health",&w::Health {uptime_ms:start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,sessions:sessions.read().await.len() as u32});continue;}
        };
        let shutdown = request.method == "engine.system.shutdown";
        let answer = tokio::select! {biased;
            _=peer.disconnected()=>return Ok(()),
            _=stop_peer.disconnected()=>return Err(Error::Disconnected),
            answer=dispatch(&request, driver.clone(), host.clone(), &sessions, &peer, route_note)=>answer,
        };
        let success = answer.is_ok();
        if shutdown && success {
            peer.respond_flushed(&request, answer).await?;
            return Ok(());
        }
        peer.respond(&request, answer);
    }
}
async fn dispatch(
    request: &w::Request,
    driver: Arc<dyn Driver>,
    host: Arc<RemoteHost>,
    sessions: &Sessions,
    peer: &Peer,
    route_note: &str,
) -> Result<Value, w::RpcError> {
    match request.method.as_str() {
        "engine.session.open" => {
            let p: w::SessionOpen = parse(request.params.clone())?;
            if p.kind != w::SessionKind::Local {
                return Err(w::RpcError::new(-32005, "local sessions only"));
            }
            if p.session_id.is_empty()
                || p.session_id.len() > 256
                || sessions.read().await.contains_key(&p.session_id)
                || sessions.read().await.len() >= 32
            {
                return Err(w::RpcError::new(-32602, "invalid or duplicate session"));
            }
            let remote = RemoteSession::open(driver, host, &p.session_id, Default::default())
                .await
                .map_err(internal)?;
            let result = encode(remote.opened())?;
            sessions.write().await.insert(
                p.session_id,
                Arc::new(Entry {
                    lifecycle: remote.session(),
                    remote: Mutex::new(remote),
                }),
            );
            Ok(result)
        }
        "engine.session.turnEnded" => {
            let p: w::SessionId = parse(request.params.clone())?;
            lookup(sessions, &p.session_id)
                .await?
                .remote
                .lock()
                .await
                .turn_ended()
                .await;
            encode(w::Empty {})
        }
        "engine.session.close" => {
            let p: w::SessionId = parse(request.params.clone())?;
            let entry = sessions
                .write()
                .await
                .remove(&p.session_id)
                .ok_or_else(|| w::RpcError::new(-32004, "unknown session"))?;
            entry.remote.lock().await.close().await.map_err(internal)?;
            if let Err(gcu_protocol::builtin_browser::ApplyFault::Saturated) =
                gcu_mcp::browser_tool::clear_owner(&p.session_id).await
            {
                peer.close();
            }
            encode(w::Empty {})
        }
        "engine.browser.sync" => crate::engine_browser::sync(request, peer).await,
        "engine.policy.changed" => {
            let p: w::PolicyChanged = parse(request.params.clone())?;
            host.policy_changed(p.version).await.map_err(internal)?;
            encode(w::Empty {})
        }
        "engine.system.doctor" => {
            let _: w::Empty = parse(request.params.clone())?;
            let mut r = host.doctor(
                tokio::time::timeout(RPC_TIMEOUT, driver.doctor())
                    .await
                    .map_err(internal)?,
            );
            if !route_note.is_empty() && !r.issues.iter().any(|issue| issue == route_note) {
                r.issues.push(route_note.to_owned());
            }
            encode(w::DoctorReport {
                capabilities: r.capabilities,
                ready: r.ready,
                issues: r.issues,
            })
        }
        "engine.screenshot.get" => {
            let p: w::ScreenshotGet = parse(request.params.clone())?;
            if gcu_protocol::builtin_browser::with_shared(|book| {
                book.image_blocked(&p.session_id, &p.id)
            }) {
                return Err(w::RpcError::new(
                    -32004,
                    "image unavailable in this session",
                ));
            }
            let entry = lookup(sessions, &p.session_id).await?;
            let remote = entry.remote.lock().await;
            let image = remote
                .screenshot(&p.id)
                .ok_or_else(|| w::RpcError::new(-32004, "image unavailable in this session"))?;
            encode(image)
        }
        "engine.system.shutdown" => {
            let _: w::Empty = parse(request.params.clone())?;
            encode(w::Empty {})
        }
        _ => Err(w::RpcError::new(-32601, "method unavailable")),
    }
}

#[cfg(test)]
#[path = "engine_stop_tests.rs"]
mod tests;
