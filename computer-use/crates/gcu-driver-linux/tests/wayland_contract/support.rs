//! Oracle, discovery, harness, and outcome reporting for Wayland contract tests.

use async_trait::async_trait;
use gcu_core::{
    driver::{AppQuery, AppRef, Driver, DriverError, WindowRef},
    model::{NativeHandle, RawNode, Role, States},
};
use gcu_driver_linux::{
    wayland::{
        ei::{EiSnapshot, EiTransport, InputEvent, InputRegion},
        input_control::{
            AnchoredClock, InputAuthority, RemoteDesktopConnection, RemoteDesktopPortal,
            StopControl,
        },
        portal::{PortalSourceKind, PortalStream},
        restore::{OriginBinding, RestoreTokenStore},
        windows::{StreamIdentity, WindowBinding, WindowRecord},
        Compositor, FakeFrameReceiver, FakePortalSession, RawCapturedFrame, VideoTransform,
    },
    x11::hotkey::SharedStopLatch,
    AppIdentity, LinuxDriver,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tempfile::TempDir;
use tokio::time::Instant;

/// Outcome of an individual contract test case.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractOutcome {
    /// Case ran and met all assertions.
    Passed,
    /// Case was skipped due to platform/environment capability constraints.
    Skipped(&'static str),
    /// Case could not be run due to missing environment/runner.
    NotRun(&'static str),
}

/// Accumulator for test outcomes and report generation.
#[derive(Debug, Default)]
pub struct SummaryReport {
    /// Total passed cases.
    pub passed: usize,
    /// Total skipped cases.
    pub skipped: usize,
    /// Total not_run cases.
    pub not_run: usize,
    /// Total failed cases.
    pub failed: usize,
}

impl SummaryReport {
    /// Record the result of a single step with standard formatted output.
    pub fn record(&mut self, id: &str, desc: &str, result: Result<ContractOutcome, String>) {
        print!("  [{:^7}] {:<65}", id, desc);
        match result {
            Ok(ContractOutcome::Passed) => {
                println!("-> PASS");
                self.passed += 1;
            }
            Ok(ContractOutcome::Skipped(reason)) => {
                println!("-> SKIP ({reason})");
                self.skipped += 1;
            }
            Ok(ContractOutcome::NotRun(reason)) => {
                println!("-> NOT_RUN ({reason})");
                self.not_run += 1;
            }
            Err(err) => {
                println!("-> FAIL: {err}");
                self.failed += 1;
            }
        }
    }

    /// Print suite summary line.
    pub fn print_summary(&self, title: &str) {
        println!("======================================================================");
        println!(
            "SUMMARY ({}): {} passed, {} skipped, {} not_run, {} failed",
            title, self.passed, self.skipped, self.not_run, self.failed
        );
        println!("======================================================================\n");
    }
}

/// Read fixture state JSON from `/tmp/fixture_state.json` or `$GCU_FIXTURE_STATE`.
#[allow(dead_code)]
pub fn read_fixture_state() -> Option<serde_json::Value> {
    let path =
        std::env::var("GCU_FIXTURE_STATE").unwrap_or_else(|_| "/tmp/fixture_state.json".into());
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Poll external fixture state until condition matches or timeout elapses.
/// On timeout, outputs the last read state.
#[allow(dead_code)]
pub async fn poll_oracle<T, F>(timeout: Duration, f: F) -> Result<T, String>
where
    F: Fn(&serde_json::Value) -> Option<T>,
{
    let start = tokio::time::Instant::now();
    let mut last_state = None;
    while start.elapsed() < timeout {
        if let Some(state) = read_fixture_state() {
            if let Some(res) = f(&state) {
                return Ok(res);
            }
            last_state = Some(state);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let state_dump = last_state
        .as_ref()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "<no state read>".into());
    let text_val = last_state
        .as_ref()
        .and_then(|s| s.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("<no text field>");
    Err(format!(
        "oracle poll timed out after {:?}; last text: {:?}; last fixture state: {}",
        timeout, text_val, state_dump
    ))
}

/// Recursively find a node in a `RawNode` hierarchy matching a predicate.
#[allow(dead_code)]
pub fn find_raw_node<'a, F>(node: &'a RawNode, pred: &F) -> Option<&'a RawNode>
where
    F: Fn(&RawNode) -> bool,
{
    if pred(node) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_raw_node(child, pred) {
            return Some(found);
        }
    }
    None
}

/// Wait for fixture discovery in live driver.
#[allow(dead_code)]
pub async fn wait_for_fixture(driver: &LinuxDriver) -> Result<(AppRef, WindowRef), DriverError> {
    for _ in 0..40 {
        if let Ok(app) = driver.resolve_app(&AppQuery("fixture".into())).await {
            if let Ok(win_info) = driver.key_window(&app).await {
                let win_ref = WindowRef {
                    app: app.clone(),
                    id: win_info.id.clone(),
                };
                return Ok((app, win_ref));
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(DriverError::WindowGone)
}

/// Generate a sample origin binding for restore token tests.
pub fn sample_origin() -> OriginBinding {
    OriginBinding {
        compositor: "gnome".into(),
        source_kind: "window".into(),
        window_identity: "org.gnome.TextEditor-win-1".into(),
        process_instance: "pid:1234:start:5678".into(),
        auth_version: "v1.0".into(),
    }
}

/// Create a test raw node with given role, native role, name, and id.
pub fn make_test_node(
    role: Role,
    native_role: &str,
    name: Option<&str>,
    id: Option<&str>,
) -> RawNode {
    RawNode {
        role,
        native_role: native_role.into(),
        subrole: None,
        name: name.map(Into::into),
        value: None,
        description: None,
        placeholder: None,
        help: None,
        identifier: id.map(Into::into),
        states: States::empty(),
        frame: None,
        actions: Default::default(),
        text_extras: None,
        url: None,
        children: Vec::new(),
        handle: NativeHandle::new(()),
        fingerprint: Default::default(),
    }
}

/// Construct raw unpadded 32-bit RGBA test frame buffer.
pub fn make_sample_frame(stream_node_id: u32, width: u32, height: u32) -> RawCapturedFrame {
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let offset = ((y * width + x) * 4) as usize;
            rgba[offset] = 0x22;
            rgba[offset + 1] = 0x88;
            rgba[offset + 2] = 0xcc;
            rgba[offset + 3] = 0xff;
        }
    }
    RawCapturedFrame {
        stream_node_id,
        sequence: 1,
        width,
        height,
        crop_region: None,
        transform: VideoTransform::Normal,
        captured_at_secs: 1000,
        rgba: Arc::from(rgba.into_boxed_slice()),
    }
}

/// In-memory fake EI transport recording dispatched events.
#[derive(Default)]
pub struct FakeEiTransport {
    pub log: Mutex<Vec<(Instant, InputEvent)>>,
    pub revision: AtomicUsize,
    pub disconnected: AtomicBool,
    pub fail_at: AtomicUsize,
    pub calls: AtomicUsize,
}

impl FakeEiTransport {
    /// Retrieve list of dispatched input events.
    pub fn events(&self) -> Vec<InputEvent> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|(_, e)| e.clone())
            .collect()
    }
}

impl EiTransport for FakeEiTransport {
    fn snapshot(&self) -> Option<EiSnapshot> {
        if self.disconnected.load(Ordering::SeqCst) {
            return None;
        }
        let keys = BTreeMap::from([
            (0xffe1, (42, false)),
            (0xffe3, (29, false)),
            (0xffe9, (56, false)),
            (0xffeb, (125, false)),
            (u32::from('a'), (30, false)),
            (u32::from('A'), (30, true)),
        ]);
        Some(EiSnapshot {
            generation: self.revision.load(Ordering::SeqCst) as u64 + 1,
            region: InputRegion {
                mapping_id: "m1".into(),
                x: 100,
                y: 200,
                width: 800,
                height: 600,
            },
            keys,
            text: true,
        })
    }

    fn send(&self, event: &InputEvent, generation: Option<u64>) -> Result<(), DriverError> {
        if self.disconnected.load(Ordering::SeqCst)
            || generation.is_some_and(|g| g != self.revision.load(Ordering::SeqCst) as u64 + 1)
        {
            return Err(DriverError::CapabilityUnavailable);
        }
        let count = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let fail = self.fail_at.load(Ordering::SeqCst);
        if fail != 0 && count >= fail {
            return Err(DriverError::Timeout {
                maybe_executed: true,
            });
        }
        self.log
            .lock()
            .unwrap()
            .push((Instant::now(), event.clone()));
        Ok(())
    }

    fn disconnect(&self) {
        self.disconnected.store(true, Ordering::SeqCst);
        self.revision.fetch_add(1, Ordering::SeqCst);
    }
}

/// Fake RemoteDesktopPortal implementation for portable testing.
#[derive(Clone)]
pub struct FakeRemoteDesktopPortal {
    pub deny: Arc<AtomicBool>,
    pub stream_kind: Arc<Mutex<PortalSourceKind>>,
    pub session_id: String,
    pub stream_node_id: u32,
    pub ei: Arc<FakeEiTransport>,
}

impl Default for FakeRemoteDesktopPortal {
    fn default() -> Self {
        Self {
            deny: Arc::new(AtomicBool::new(false)),
            stream_kind: Arc::new(Mutex::new(PortalSourceKind::Window)),
            session_id: "fake-portal-session-1".into(),
            stream_node_id: 42,
            ei: Arc::new(FakeEiTransport::default()),
        }
    }
}

#[async_trait]
impl RemoteDesktopPortal for FakeRemoteDesktopPortal {
    async fn open(&self, restore: Option<&str>) -> Result<RemoteDesktopConnection, DriverError> {
        if self.deny.load(Ordering::SeqCst) {
            return Err(DriverError::PermissionMissing {
                permission: "remote_desktop".into(),
            });
        }
        let kind = *self.stream_kind.lock().unwrap();
        let (fake_session, _active) = FakePortalSession::new(&self.session_id);
        let session = Arc::new(fake_session);
        let stream = PortalStream {
            node_id: self.stream_node_id,
            source_type: kind,
            position: Some((0, 0)),
            size: Some((1920, 1080)),
            mapping_id: Some("m1".into()),
            pipewire_serial: Some(1),
        };
        let token = restore
            .map(|t| format!("{t}-rotated"))
            .or_else(|| Some("initial-token-1".into()));
        Ok(RemoteDesktopConnection {
            session,
            stream,
            restore_token: token,
            ei: self.ei.clone(),
        })
    }
}

/// Fake InputAuthority implementation for portable testing.
pub struct FakeInputAuthority {
    pub confined: AtomicBool,
    pub mismatch: AtomicBool,
}

impl Default for FakeInputAuthority {
    fn default() -> Self {
        Self {
            confined: AtomicBool::new(true),
            mismatch: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl InputAuthority for FakeInputAuthority {
    async fn prove(
        &self,
        session: &str,
        stream: &PortalStream,
        window: &WindowBinding,
    ) -> Result<StreamIdentity, DriverError> {
        if self.mismatch.load(Ordering::SeqCst) {
            return Err(DriverError::Native {
                message: "stream mismatch".into(),
                maybe_executed: false,
            });
        }
        Ok(StreamIdentity {
            session: session.into(),
            node_id: stream.node_id,
            serial: 1,
            source: "m1".into(),
            window: window.clone(),
        })
    }

    fn confined(&self) -> bool {
        self.confined.load(Ordering::SeqCst)
    }
}

/// Portable test harness assembling token store, capture pipeline, and fake components.
#[allow(dead_code)]
pub struct PortableWaylandHarness {
    pub dir: TempDir,
    pub store: Arc<RestoreTokenStore>,
    pub portal: FakeRemoteDesktopPortal,
    pub authority: Arc<FakeInputAuthority>,
    pub clock: Arc<AnchoredClock>,
    pub stop_latch: Arc<SharedStopLatch>,
    pub stop_control: Arc<StopControl>,
    pub frame_receiver: FakeFrameReceiver,
    pub stream: PortalStream,
}

impl PortableWaylandHarness {
    /// Create a new isolated portable harness.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir creation");
        let store = Arc::new(RestoreTokenStore::new(dir.path().join("tokens")));
        let portal = FakeRemoteDesktopPortal::default();
        let authority = Arc::new(FakeInputAuthority::default());
        let clock = Arc::new(AnchoredClock::new(1000));
        let stop_latch = Arc::new(SharedStopLatch::default());
        let stop_control = StopControl::registered(stop_latch.clone());
        let frame_receiver = FakeFrameReceiver::new(42, 1000);
        let stream = PortalStream {
            node_id: 42,
            source_type: PortalSourceKind::Window,
            position: Some((0, 0)),
            size: Some((800, 600)),
            mapping_id: Some("stream-0".into()),
            pipewire_serial: Some(1),
        };

        Self {
            dir,
            store,
            portal,
            authority,
            clock,
            stop_latch,
            stop_control,
            frame_receiver,
            stream,
        }
    }

    /// Helper to create a sample WindowBinding.
    pub fn sample_binding(&self) -> WindowBinding {
        WindowBinding {
            compositor: Compositor::Gnome,
            epoch: "1".into(),
            record: WindowRecord {
                id: "win-1".into(),
                generation: 1,
                title: "Fixture Window".into(),
                app_id: "gcu-fixture.desktop".into(),
                pid: Some(1234),
                peer_pid: true,
            },
            image: AppIdentity {
                exe_path: PathBuf::from("/usr/bin/gcu-fixture"),
                dev: 1,
                inode: 42,
                sha256: "ab".repeat(32),
                pid: 1234,
                start_time: 5678,
                desktop_id: Some("gcu-fixture.desktop".into()),
            },
        }
    }
}
