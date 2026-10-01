//! Real private D-Bus transport and SCM_RIGHTS fixtures, not a desktop emulator.
mod adapter;
mod authorization;
mod eis;
mod fd_inheritance;
mod logind;
mod logind_user;
mod native;
mod policy;
mod registry;
mod registry_gnome;
mod registry_parent;
use super::*;
use crate::{grant::Dict, session::*};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    os::{fd::OwnedFd, unix::net::UnixStream},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;
use zbus::{
    message::Header,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
    Connection,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    Denied,
    OldVersion,
    WrongRequest,
    WrongSession,
    TwoStreams,
    WrongSource,
    PartialDevices,
    BadGeometry,
    NoMapping,
    EisFails,
    SilentStart,
    ClosedInStart,
    SlowCleanup,
    InvalidPipewireFd,
}

pub(super) struct Bus {
    child: Child,
    pub(super) address: String,
}

fn owned_child(command: &mut Command) -> &mut Command {
    use std::os::unix::process::CommandExt;
    let parent = std::process::id() as libc::pid_t;
    // Async-signal-safe operations only, before exec; also close the race where
    // a test crash happens before the child installs its parent-death signal.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
            }
            Ok(())
        });
    }
    command
}

impl Bus {
    pub(super) fn start() -> Self {
        Self::start_on(None)
    }
    pub(super) fn start_on(address: Option<&str>) -> Self {
        let mut command = Command::new("dbus-daemon");
        command.args(["--session", "--nofork", "--print-address=1"]);
        if let Some(address) = address {
            command.arg(format!("--address={address}"));
        }
        let mut child = owned_child(
            command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit()),
        )
        .spawn()
        .expect("private dbus-daemon is required; do not skip native transport tests");
        let stdout = child.stdout.take().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut address = String::new();
            let result = BufReader::new(stdout)
                .read_line(&mut address)
                .map(|_| address.trim().to_owned());
            let _ = send.send(result);
        });
        let address = match receive.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(address)) if address.starts_with("unix:") => address,
            result => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                panic!("private D-Bus startup failed: {result:?}");
            }
        };
        reader.join().unwrap();
        Self { child, address }
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        // Retained owned handle only; never pkill a desktop daemon or portal.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Shared {
    mode: Mode,
    calls: Mutex<Vec<String>>,
    sessions: Mutex<Vec<String>>,
    parents: Mutex<Vec<String>>,
    source: Mutex<HashMap<String, u32>>,
    peers: Mutex<Vec<UnixStream>>,
    native: Mutex<Option<native::NativeGrant>>,
    eis_socket: Mutex<Option<std::path::PathBuf>>,
    start_release: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
}

impl Shared {
    fn record(&self, value: impl Into<String>) {
        self.calls.lock().unwrap().push(value.into());
    }

    fn called(&self, value: &str) -> bool {
        self.calls.lock().unwrap().iter().any(|call| call == value)
    }

    fn fd(&self, kind: &str) -> zbus::zvariant::OwnedFd {
        self.record(kind);
        if kind == "ConnectToEIS" {
            if let Some(socket) = self.eis_socket.lock().unwrap().as_ref() {
                return OwnedFd::from(UnixStream::connect(socket).unwrap()).into();
            }
        }
        if kind == "OpenPipeWireRemote" {
            if let Some(grant) = self.native.lock().unwrap().as_ref() {
                return OwnedFd::from(UnixStream::connect(&grant.socket).unwrap()).into();
            }
        }
        let (client, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        self.peers.lock().unwrap().push(peer);
        OwnedFd::from(client).into()
    }

    fn assert_peers_closed(&self, expected: usize) {
        let mut peers = self.peers.lock().unwrap();
        assert_eq!(peers.len(), expected);
        for peer in peers.iter_mut() {
            assert_eq!(peer.read(&mut [0u8]).expect("remote grant FD leaked"), 0);
        }
    }

    async fn response(
        self: &Arc<Self>,
        conn: &Connection,
        sender: &str,
        method: &str,
        options: Dict,
        status: u32,
        results: Dict,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        self.record(method);
        if method == "Start" {
            let gate = self.start_release.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.await
                    .map_err(|_| zbus::fdo::Error::Failed("owned picker gate dropped".into()))?;
            }
        }
        let token = <&str>::try_from(options.get("handle_token").unwrap()).unwrap();
        let id = sender.trim_start_matches(':').replace('.', "_");
        let path: OwnedObjectPath = format!("{ROOT}/request/{id}/{token}").try_into().unwrap();
        conn.object_server()
            .at(path.clone(), RequestObject(self.clone()))
            .await?;
        if self.mode != Mode::SilentStart || method != "Start" {
            // Deliberately emit BEFORE the method reply to prove pre-subscription.
            conn.emit_signal(
                Some(sender),
                path.clone(),
                REQUEST,
                "Response",
                &(status, results),
            )
            .await?;
        }
        if self.mode == Mode::WrongRequest && method == "Start" {
            Ok(format!("{ROOT}/request/not_ours/not_ours")
                .try_into()
                .unwrap())
        } else {
            Ok(path)
        }
    }
}

struct RemoteObject(Arc<Shared>);

#[zbus::interface(name = "org.freedesktop.portal.RemoteDesktop")]
impl RemoteObject {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        if self.0.mode == Mode::OldVersion {
            1
        } else {
            2
        }
    }

    #[zbus(property)]
    fn available_device_types(&self) -> u32 {
        7
    }

    async fn create_session(
        &self,
        options: Dict,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let sender = header.sender().unwrap().as_str();
        let id = sender.trim_start_matches(':').replace('.', "_");
        let token = <&str>::try_from(options.get("session_handle_token").unwrap()).unwrap();
        let path = format!("{ROOT}/session/{id}/{token}");
        conn.object_server()
            .at(path.clone(), SessionObject(self.0.clone(), path.clone()))
            .await?;
        self.0.sessions.lock().unwrap().push(path.clone());
        let returned = if self.0.mode == Mode::WrongSession {
            format!("{ROOT}/session/not_ours/not_ours")
        } else {
            path
        };
        self.0
            .response(
                conn,
                sender,
                "CreateSession",
                options,
                0,
                Dict::from([("session_handle".into(), string_value(returned))]),
            )
            .await
    }

    async fn select_devices(
        &self,
        session: OwnedObjectPath,
        options: Dict,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        assert!(self
            .0
            .sessions
            .lock()
            .unwrap()
            .contains(&session.to_string()));
        assert_eq!(u32::try_from(options.get("types").unwrap()).unwrap(), 3);
        assert_eq!(
            u32::try_from(options.get("persist_mode").unwrap()).unwrap(),
            0
        );
        assert!(!options.contains_key("restore_token"));
        self.0
            .response(
                conn,
                header.sender().unwrap().as_str(),
                "SelectDevices",
                options,
                0,
                Dict::new(),
            )
            .await
    }

    async fn start(
        &self,
        session: OwnedObjectPath,
        parent: String,
        options: Dict,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        assert!(parent.is_empty() || parent.starts_with("wayland:"));
        self.0.parents.lock().unwrap().push(parent);
        let sender = header.sender().unwrap().as_str();
        if self.0.mode == Mode::ClosedInStart {
            conn.emit_signal(
                Some(sender),
                session.clone(),
                SESSION,
                "Closed",
                &(Dict::new(),),
            )
            .await?;
        }
        let source = self.0.source.lock().unwrap()[session.as_str()];
        let mut props = Dict::from([
            (
                "source_type".into(),
                if self.0.mode == Mode::WrongSource {
                    4u32
                } else {
                    source
                }
                .into(),
            ),
            (
                "size".into(),
                OwnedValue::try_from(Value::from(if self.0.mode == Mode::BadGeometry {
                    (0i32, 900i32)
                } else {
                    (1600i32, 900i32)
                }))
                .unwrap(),
            ),
            (
                "position".into(),
                OwnedValue::try_from(Value::from((-1600i32, 0i32))).unwrap(),
            ),
            (
                "mapping_id".into(),
                string_value("owned-fixture-region".into()),
            ),
            ("pipewire-serial".into(), 12000u64.into()),
        ]);
        if self.0.mode == Mode::NoMapping {
            props.remove("mapping_id");
        }
        let mut node = 42u32;
        if let Some(grant) = self.0.native.lock().unwrap().as_ref() {
            node = grant.node;
            props.insert("pipewire-serial".into(), grant.serial.into());
        }
        let mut streams = vec![(node, props)];
        if self.0.mode == Mode::TwoStreams {
            streams.push((43u32, Dict::new()));
        }
        let results = Dict::from([
            (
                "devices".into(),
                if self.0.mode == Mode::PartialDevices {
                    2u32
                } else {
                    3u32
                }
                .into(),
            ),
            (
                "streams".into(),
                OwnedValue::try_from(Value::from(streams)).unwrap(),
            ),
        ]);
        self.0
            .response(
                conn,
                sender,
                "Start",
                options,
                if self.0.mode == Mode::Denied { 1 } else { 0 },
                results,
            )
            .await
    }

    #[zbus(name = "ConnectToEIS")]
    async fn connect_to_eis(
        &self,
        _session: OwnedObjectPath,
        options: Dict,
    ) -> zbus::fdo::Result<zbus::zvariant::OwnedFd> {
        assert!(options.is_empty());
        if self.0.mode == Mode::EisFails {
            return Err(zbus::fdo::Error::Failed("fixture EIS refusal".into()));
        }
        Ok(self.0.fd("ConnectToEIS"))
    }
}

struct ScreenObject(Arc<Shared>);

#[zbus::interface(name = "org.freedesktop.portal.ScreenCast")]
impl ScreenObject {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        6
    }

    #[zbus(property)]
    fn available_source_types(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn available_cursor_modes(&self) -> u32 {
        7
    }

    async fn select_sources(
        &self,
        session: OwnedObjectPath,
        options: Dict,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        assert!(!bool::try_from(options.get("multiple").unwrap()).unwrap());
        assert_eq!(
            u32::try_from(options.get("cursor_mode").unwrap()).unwrap(),
            1
        );
        assert!(!options.contains_key("persist_mode"));
        assert!(!options.contains_key("restore_token"));
        let source = u32::try_from(options.get("types").unwrap()).unwrap();
        self.0
            .source
            .lock()
            .unwrap()
            .insert(session.to_string(), source);
        self.0
            .response(
                conn,
                header.sender().unwrap().as_str(),
                "SelectSources",
                options,
                0,
                Dict::new(),
            )
            .await
    }

    async fn open_pipe_wire_remote(
        &self,
        _session: OwnedObjectPath,
        options: Dict,
    ) -> zbus::fdo::Result<zbus::zvariant::OwnedFd> {
        assert!(options.is_empty());
        if self.0.mode == Mode::InvalidPipewireFd {
            return Ok(OwnedFd::from(std::fs::File::open("/dev/null").unwrap()).into());
        }
        Ok(self.0.fd("OpenPipeWireRemote"))
    }
}

struct RequestObject(Arc<Shared>);

#[zbus::interface(name = "org.freedesktop.portal.Request")]
impl RequestObject {
    async fn close(&self) {
        self.0.record("Request.Close");
        if self.0.mode == Mode::SlowCleanup {
            // zbus serves on its own async-io executor, not a Tokio reactor.
            // A truly nonresponding peer, not a panicking handler, tests timeout.
            std::future::pending::<()>().await;
        }
    }
}

struct SessionObject(Arc<Shared>, String);

#[zbus::interface(name = "org.freedesktop.portal.Session")]
impl SessionObject {
    async fn close(&self) {
        self.0.record(format!("Session.Close:{}", self.1));
        if self.0.mode == Mode::SlowCleanup {
            std::future::pending::<()>().await;
        }
    }
}

struct Fixture {
    shared: Arc<Shared>,
    server: Connection,
    bus: Bus,
}

impl Fixture {
    async fn new(mode: Mode) -> Self {
        let bus = Bus::start();
        let shared = Arc::new(Shared {
            mode,
            calls: Mutex::new(Vec::new()),
            sessions: Mutex::new(Vec::new()),
            parents: Mutex::new(Vec::new()),
            source: Mutex::new(HashMap::new()),
            peers: Mutex::new(Vec::new()),
            native: Mutex::new(None),
            eis_socket: Mutex::new(None),
            start_release: Mutex::new(None),
        });
        let server = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .serve_at(ROOT, RemoteObject(shared.clone()))
            .unwrap()
            .serve_at(ROOT, ScreenObject(shared.clone()))
            .unwrap()
            .build()
            .await
            .unwrap();
        Self {
            shared,
            server,
            bus,
        }
    }

    fn start(&self) -> PortalSession {
        let mut options = PortalOptions::new("owned-native-fixture".into(), SourceKind::Monitor);
        options.timeout = Duration::from_secs(3);
        PortalSession::on_test_bus(options, self.bus.address.clone()).unwrap()
    }

    fn assert_sessions_closed(&self) {
        for session in self.shared.sessions.lock().unwrap().iter() {
            assert!(
                self.shared.called(&format!("Session.Close:{session}")),
                "missing close for {session}"
            );
        }
    }
}

async fn terminal(state: &mut watch::Receiver<SessionState>) -> CloseReason {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let SessionState::Closed(reason) = state.borrow_and_update().clone() {
                return reason;
            }
            state
                .changed()
                .await
                .expect("owner disappeared without a terminal state");
        }
    })
    .await
    .expect("portal owner did not terminate")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn early_response_and_real_fd_transfer_preserve_single_source_identity() {
    let fixture = Fixture::new(Mode::Normal).await;
    let mut session = fixture.start();
    let grant = session.ready().await.unwrap();
    assert_eq!(grant.stream.node_id, 42);
    assert_eq!(grant.stream.pipewire_serial, Some(12000));
    assert_eq!(
        grant.stream.mapping_id.as_deref(),
        Some("owned-fixture-region")
    );
    assert_eq!(grant.stream.compositor_position, Some((-1600, 0)));
    assert_eq!(grant.stream.compositor_size, Some((1600, 900)));
    assert!(grant.stream.source.scope_label().contains("session-wide"));
    assert_eq!(
        *fixture.shared.calls.lock().unwrap(),
        [
            "CreateSession",
            "SelectDevices",
            "SelectSources",
            "Start",
            "OpenPipeWireRemote",
            "ConnectToEIS"
        ]
    );
    session.stop().await.unwrap();
    assert_eq!(session.state(), SessionState::Closed(CloseReason::Stopped));
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn denial_malformed_scope_old_protocol_and_second_fd_failure_retire_owned_session() {
    for mode in [
        Mode::Denied,
        Mode::OldVersion,
        Mode::WrongRequest,
        Mode::WrongSession,
        Mode::TwoStreams,
        Mode::WrongSource,
        Mode::PartialDevices,
        Mode::BadGeometry,
        Mode::EisFails,
    ] {
        let fixture = Fixture::new(mode).await;
        let mut session = fixture.start();
        let failure = session.ready().await.expect_err("invalid grant accepted");
        assert!(
            matches!(failure, CloseReason::Failed(_)),
            "{mode:?}: {failure:?}"
        );
        let expected = match mode {
            Mode::Denied => "user cancelled portal consent",
            Mode::OldVersion => "RemoteDesktop >= 2",
            Mode::WrongRequest => "different request identity",
            Mode::WrongSession => "different session identity",
            Mode::TwoStreams => "exactly one selected source",
            Mode::WrongSource => "requested scope",
            Mode::PartialDevices => "exactly keyboard and pointer",
            Mode::BadGeometry => "nonpositive stream size",
            Mode::EisFails => "fixture EIS refusal",
            _ => unreachable!(),
        };
        assert!(
            matches!(&failure, CloseReason::Failed(message) if message.contains(expected)),
            "{mode:?}: wrong failure path: {failure:?}"
        );
        if mode == Mode::EisFails {
            assert!(
                matches!(&failure, CloseReason::Failed(message) if message.contains("fixture EIS refusal")),
                "wrong failure path: {failure:?}"
            );
        }
        terminal(&mut session.subscribe()).await;
        fixture.assert_sessions_closed();
        fixture
            .shared
            .assert_peers_closed(if mode == Mode::EisFails { 1 } else { 0 });
        assert!(!fixture.shared.called("ConnectToEIS"));
        if mode == Mode::OldVersion {
            assert!(!fixture.shared.called("CreateSession"));
        }
        eprintln!("native portal negative {mode:?}: closed with no input fallback");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_drop_closes_request_and_session_without_waiting_for_consent() {
    let fixture = Fixture::new(Mode::SilentStart).await;
    let session = fixture.start();
    let mut state = session.subscribe();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !fixture.shared.called("Start") {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    drop(session);
    assert_eq!(terminal(&mut state).await, CloseReason::Stopped);
    assert!(fixture.shared.called("Request.Close"));
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn negotiation_deadline_closes_outstanding_consent_request() {
    let fixture = Fixture::new(Mode::SilentStart).await;
    let mut options = PortalOptions::new("timeout".into(), SourceKind::Monitor);
    options.timeout = Duration::from_millis(500);
    let mut session = PortalSession::on_test_bus(options, fixture.bus.address.clone()).unwrap();
    assert_eq!(session.ready().await, Err(CloseReason::TimedOut));
    assert_eq!(
        terminal(&mut session.subscribe()).await,
        CloseReason::TimedOut
    );
    assert!(fixture.shared.called("Request.Close"));
    fixture.assert_sessions_closed();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn portal_revocation_invalidates_grant_and_releases_both_native_fds() {
    let fixture = Fixture::new(Mode::Normal).await;
    let mut session = fixture.start();
    let grant = session.ready().await.unwrap();
    fixture
        .server
        .emit_signal(
            None::<&str>,
            grant.session_path.as_str(),
            SESSION,
            "Closed",
            &(Dict::new(),),
        )
        .await
        .unwrap();
    assert_eq!(
        terminal(&mut session.subscribe()).await,
        CloseReason::PortalClosed
    );
    fixture.shared.assert_peers_closed(2);
    fixture.assert_sessions_closed();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spoofed_closed_is_ignored_and_owner_replacement_cannot_inherit_session() {
    let fixture = Fixture::new(Mode::Normal).await;
    let mut session = fixture.start();
    let grant = session.ready().await.unwrap();
    let replacement = zbus::connection::Builder::address(fixture.bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    replacement
        .emit_signal(
            None::<&str>,
            grant.session_path.as_str(),
            SESSION,
            "Closed",
            &(Dict::new(),),
        )
        .await
        .unwrap();
    // A round trip orders the forged emission; only the pinned owner may revoke.
    zbus::fdo::DBusProxy::new(&replacement)
        .await
        .unwrap()
        .get_id()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(matches!(session.state(), SessionState::Granted(_)));
    fixture.server.release_name(SERVICE).await.unwrap();
    replacement.request_name(SERVICE).await.unwrap();
    assert_eq!(
        terminal(&mut session.subscribe()).await,
        CloseReason::PortalOwnerLost
    );
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closed_before_start_reply_never_publishes_an_active_grant() {
    let fixture = Fixture::new(Mode::ClosedInStart).await;
    let mut session = fixture.start();
    assert_eq!(session.ready().await, Err(CloseReason::PortalClosed));
    terminal(&mut session.subscribe()).await;
    fixture.assert_sessions_closed();
    assert!(!fixture.shared.called("ConnectToEIS"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unresponsive_cleanup_cannot_hold_native_descriptors_forever() {
    let fixture = Fixture::new(Mode::SlowCleanup).await;
    let mut session = fixture.start();
    session.ready().await.unwrap();
    let before = std::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(3), session.stop())
        .await
        .unwrap()
        .unwrap();
    assert!(
        before.elapsed() >= Duration::from_millis(900),
        "must exercise the real one-second cleanup deadline, not a handler panic"
    );
    fixture.shared.assert_peers_closed(2);
    fixture.assert_sessions_closed();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn window_capture_does_not_claim_single_application_input_or_invent_mapping() {
    let fixture = Fixture::new(Mode::NoMapping).await;
    let options = PortalOptions::new("window-scope".into(), SourceKind::Window);
    let mut session = PortalSession::on_test_bus(options, fixture.bus.address.clone()).unwrap();
    let grant = session.ready().await.unwrap();
    assert_eq!(grant.stream.source, SourceKind::Window);
    assert!(grant.stream.mapping_id.is_none());
    assert!(grant
        .stream
        .source
        .scope_label()
        .contains("session-wide input"));
    session.stop().await.unwrap();
    fixture.shared.assert_peers_closed(2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_run_handles_do_not_close_each_other() {
    let fixture = Fixture::new(Mode::Normal).await;
    let mut first = fixture.start();
    let first_grant = first.ready().await.unwrap();
    let mut second = PortalSession::on_test_bus(
        PortalOptions::new("second-run".into(), SourceKind::Monitor),
        fixture.bus.address.clone(),
    )
    .unwrap();
    let second_grant = second.ready().await.unwrap();
    assert_ne!(first_grant.session_path, second_grant.session_path);
    first.stop().await.unwrap();
    assert!(matches!(second.state(), SessionState::Granted(_)));
    assert!(!fixture
        .shared
        .called(&format!("Session.Close:{}", second_grant.session_path)));
    second.stop().await.unwrap();
    fixture.shared.assert_peers_closed(4);
    fixture.assert_sessions_closed();
}

#[test]
fn invalid_options_do_not_start_any_task_or_request() {
    assert!(PortalSession::start(PortalOptions::new(String::new(), SourceKind::Monitor)).is_err());
    let mut options = PortalOptions::new("run".into(), SourceKind::Monitor);
    options.parent_window = "x11:123".into();
    assert!(PortalSession::start(options).is_err());
    let mut options = PortalOptions::new("run".into(), SourceKind::Monitor);
    options.timeout = Duration::ZERO;
    assert!(PortalSession::start(options).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_socket_loss_revokes_grant_without_a_portal_closed_signal() {
    for index in [0, 1] {
        let fixture = Fixture::new(Mode::Normal).await;
        let mut session = fixture.start();
        session.ready().await.unwrap();
        drop(fixture.shared.peers.lock().unwrap().remove(index));
        assert_eq!(
            terminal(&mut session.subscribe()).await,
            CloseReason::TransportClosed(if index == 0 { "PipeWire" } else { "EIS" })
        );
        fixture.shared.assert_peers_closed(1);
        fixture.assert_sessions_closed();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn silent_and_partial_eis_handshakes_remain_cancellable() {
    use std::io::Write;
    for partial in [false, true] {
        let fixture = Fixture::new(Mode::Normal).await;
        let mut session = PortalSession::on_input_test_bus(
            PortalOptions::new("silent-ei".into(), SourceKind::Monitor),
            fixture.bus.address.clone(),
        )
        .unwrap();
        session.ready().await.unwrap();
        if partial {
            // Incomplete native protocol header, not a valid handshake. Real
            // libei must not block waiting for the rest of this stream.
            fixture.shared.peers.lock().unwrap()[1]
                .write_all(&[0])
                .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(session.input_state(), InputState::Starting);
        tokio::time::timeout(Duration::from_secs(2), session.stop())
            .await
            .expect("stalled EIS prevented native-owner retirement")
            .unwrap();
        assert_eq!(session.state(), SessionState::Closed(CloseReason::Stopped));
        assert_eq!(session.input_state(), InputState::Revoked);
        // libei may send its own handshake; drain bounded bytes to EOF rather
        // than mistaking that legitimate outbound protocol for a leaked fd.
        for peer in fixture.shared.peers.lock().unwrap().iter_mut() {
            let mut wire = Vec::new();
            peer.take(65536).read_to_end(&mut wire).unwrap();
            assert!(wire.len() < 65536, "unbounded EIS handshake");
            assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        }
        fixture.assert_sessions_closed();
    }
    println!("PASS silent and partial native EIS handshakes permit exact Stop retirement");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn regular_file_fd_is_not_accepted_as_a_native_transport() {
    let fixture = Fixture::new(Mode::InvalidPipewireFd).await;
    let mut session = fixture.start();
    assert!(
        matches!(session.ready().await, Err(CloseReason::Failed(message)) if message.contains("not a stream socket"))
    );
    terminal(&mut session.subscribe()).await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_bus_disconnect_revokes_and_drops_all_local_transports() {
    let mut fixture = Fixture::new(Mode::Normal).await;
    let mut session = fixture.start();
    session.ready().await.unwrap();
    fixture.bus.child.kill().unwrap();
    fixture.bus.child.wait().unwrap();
    let reason = terminal(&mut session.subscribe()).await;
    assert!(
        matches!(
            reason,
            CloseReason::BusDisconnected | CloseReason::PortalOwnerLost
        ),
        "{reason:?}"
    );
    fixture.shared.assert_peers_closed(2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_a_stop_wait_retains_the_exact_retirement_handle() {
    let fixture = Fixture::new(Mode::SlowCleanup).await;
    let mut session = fixture.start();
    session.ready().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), session.stop())
            .await
            .is_err()
    );
    assert!(matches!(session.state(), SessionState::Closing(_)));
    session.stop().await.unwrap();
    assert_eq!(session.state(), SessionState::Closed(CloseReason::Stopped));
    fixture.shared.assert_peers_closed(2);
}
