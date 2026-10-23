//! Spy desktop and scope fixtures for ADR 0017 S1.
#![allow(dead_code)]

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Waker;
use std::time::Duration;

use async_trait::async_trait;
use gcu_core::action::ActionRouter;
use gcu_core::driver::{
    AppQuery, AppRef, BoxStream, Button, Cancel, Capture, CaptureOpts, Delivery, Dir,
    DiscoveryStatus, DoctorReport, Driver, DriverAction, DriverError, KeyChord, KeyRoute, Platform,
    PointerRoute, Result as DriverResult, TreeOpts, UiEvent, WindowRef,
};
use gcu_core::model::{NativeHandle, NodeAction, RawNode, RawTree, TextRange, WindowPoint};
use gcu_core::scope::{
    BootIdentity, ElementBinding, ExecutableProof, FixtureScope, FixtureTarget, InstanceEvidence,
    OsStartTime, ProcessInstance, RegisteredWindow, ScopeLease, WindowBinding,
    SCOPE_DENIED_MESSAGE,
};
use gcu_core::session::{Session, WindowLocks};
use gcu_core::settle::{Clock, SettleConfig, SettleEngine};
use gcu_driver_fake::FakeDriver;
use gcu_host_api::testing::{
    FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals,
};
use gcu_host_api::{
    ApprovalProvider, ConfirmationLevel, EventSink, ForegroundPolicy, PolicyConfig, PolicyStore,
};
use gcu_protocol::capabilities::PrivateApiStatus;
use gcu_protocol::types::AppIdentity;
use gcu_protocol::{AppInfo, Capabilities, WindowInfo};

pub const CANARIES: &[&str] = &[
    "CANARY-EXTERNAL-C",
    "CANARY-WINDOW-C",
    "/canary/external/c",
    "CANARY-IMPOSTOR-D",
    "CANARY-WINDOW-A3",
    "CANARY-GHOST",
    "fixture.d",
    "CANARY-PRIVATE",
];

pub const SCOPE_MESSAGE: &str = SCOPE_DENIED_MESSAGE;

const SCENE: &str = r#"(
apps: [
(id: "fixture.a", name: "Fixture A", executable: "/fixtures/a", pid: 1101, windows: [
(id: "w-a", title: "Allowed A", frame: (x: 0.0, y: 0.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("Allowed A"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [
(id: "go", role: button, states: "ENABLED", name: Some("Go"),
frame: Some((x: 20.0, y: 40.0, w: 80.0, h: 24.0)), actions: [press])
]))),
(id: "w-a2", title: "Allowed A2", frame: (x: 220.0, y: 0.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("Allowed A2"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [
(id: "go", role: button, states: "ENABLED", name: Some("Go"),
frame: Some((x: 20.0, y: 40.0, w: 80.0, h: 24.0)), actions: [press])
]))),
(id: "w-a3", title: "CANARY-WINDOW-A3", frame: (x: 440.0, y: 0.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("CANARY-WINDOW-A3"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [])))
]),
(id: "fixture.b", name: "Fixture B", executable: "/fixtures/b", pid: 1102, windows: [
(id: "w-b", title: "Allowed B", frame: (x: 0.0, y: 200.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("Allowed B"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [
(id: "go", role: button, states: "ENABLED", name: Some("Go"),
frame: Some((x: 20.0, y: 40.0, w: 80.0, h: 24.0)), actions: [press])
])))
]),
(id: "fixture.c", name: "CANARY-EXTERNAL-C", executable: "/canary/external/c", pid: 1103, windows: [
(id: "w-c", title: "CANARY-WINDOW-C", frame: (x: 0.0, y: 400.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("CANARY-WINDOW-C"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [
(id: "go", role: button, states: "ENABLED", name: Some("CANARY-EXTERNAL-C"),
frame: Some((x: 20.0, y: 40.0, w: 80.0, h: 24.0)), actions: [press])
])))
]),
(id: "fixture.d", name: "Fixture A", executable: "/fixtures/a", pid: 1104, windows: [
(id: "w-d", title: "CANARY-IMPOSTOR-D", frame: (x: 220.0, y: 400.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("CANARY-IMPOSTOR-D"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [
(id: "go", role: button, states: "ENABLED", name: Some("CANARY-IMPOSTOR-D"),
frame: Some((x: 20.0, y: 40.0, w: 80.0, h: 24.0)), actions: [press])
])))
]),
(id: "fixture.g", name: "CANARY-GHOST", executable: "/canary/ghost", pid: 1105, windows: [
(id: "w-g", title: "CANARY-GHOST", frame: (x: 440.0, y: 400.0, w: 200.0, h: 120.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("CANARY-GHOST"),
frame: Some((x: 0.0, y: 0.0, w: 200.0, h: 120.0)), children: [])))
])
])"#;

pub fn scene_driver() -> FakeDriver {
    FakeDriver::from_ron(SCENE).unwrap()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tally {
    pub content: usize,
    pub capture: usize,
    pub perform: usize,
    pub launch: usize,
    pub geometry: usize,
    pub focus: usize,
    pub subscribe: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldOp {
    ListApps,
    ListWindows,
    Snapshot,
    Capture,
    Geometry,
    Perform,
    Focus,
    BeforeOpen,
    Admit,
    MarkSeen,
}

struct Counts {
    metadata: AtomicUsize,
    content: AtomicUsize,
    capture: AtomicUsize,
    perform: AtomicUsize,
    launch: AtomicUsize,
    geometry: AtomicUsize,
    focus: AtomicUsize,
    subscribe: AtomicUsize,
    windows_none: AtomicUsize,
    windows_some: AtomicUsize,
    resolve: AtomicUsize,
    release: AtomicUsize,
    lists_after_flip: AtomicUsize,
    restore: AtomicUsize,
    held: AtomicBool,
    started: AtomicUsize,
    held_input: AtomicUsize,
    unsubscribed: AtomicUsize,
    events_parked: AtomicBool,
}

impl Counts {
    fn new() -> Self {
        Self {
            metadata: AtomicUsize::new(0),
            content: AtomicUsize::new(0),
            capture: AtomicUsize::new(0),
            perform: AtomicUsize::new(0),
            launch: AtomicUsize::new(0),
            geometry: AtomicUsize::new(0),
            focus: AtomicUsize::new(0),
            subscribe: AtomicUsize::new(0),
            windows_none: AtomicUsize::new(0),
            windows_some: AtomicUsize::new(0),
            resolve: AtomicUsize::new(0),
            release: AtomicUsize::new(0),
            lists_after_flip: AtomicUsize::new(0),
            restore: AtomicUsize::new(0),
            held: AtomicBool::new(false),
            started: AtomicUsize::new(0),
            held_input: AtomicUsize::new(0),
            unsubscribed: AtomicUsize::new(0),
            events_parked: AtomicBool::new(false),
        }
    }
}

struct Evidence {
    apps: BTreeMap<String, ProcessInstance>,
    windows: BTreeMap<String, WindowBinding>,
    handles: Vec<(NativeHandle, String)>,
    flip: bool,
    flipped: bool,
    change_during_perform: bool,
    stop_during_perform: bool,
    hold: Option<HoldOp>,
    hold_started: bool,
    hold_skip: usize,
    hold_go: bool,
    deny_perform: bool,
    snapshot_fault: Option<usize>,
    fault_lease: Option<ScopeLease>,
    permission: Option<String>,
    events: Option<Arc<Mutex<VecDeque<UiEvent>>>>,
    waker: Arc<Mutex<Option<Waker>>>,
}

#[derive(Clone)]
pub struct Spy {
    inner: FakeDriver,
    caps: Capabilities,
    evidence: Arc<Mutex<Evidence>>,
    counts: Arc<Counts>,
    activity: Arc<FakeUserActivity>,
}

impl Spy {
    pub fn new(inner: FakeDriver, activity: Arc<FakeUserActivity>) -> Self {
        let mut caps = inner.capabilities().clone();
        caps.private_apis
            .insert("CANARY-PRIVATE".into(), PrivateApiStatus::Available);
        Self {
            inner,
            caps,
            evidence: Arc::new(Mutex::new(initial_evidence())),
            counts: Arc::new(Counts::new()),
            activity,
        }
    }

    pub fn set_targeted(&mut self, pointer: bool, keys: bool) {
        self.inner = self.inner.clone().with_targeted_input(pointer, keys);
        self.caps.delivery.targeted_pointer = pointer;
        self.caps.delivery.targeted_keys = keys;
    }

    /// Queue a fake-driver fault. The next snapshot or capture applies it.
    ///
    /// May wait on the fake desktop mutex, bounded by the driver timeout.
    /// Dropping the future cancels that wait. Does not change the scope lease.
    pub async fn inject(
        &self,
        fault: gcu_driver_fake::faults::Fault,
    ) -> gcu_core::driver::Result<()> {
        self.inner.inject(fault).await
    }

    pub fn external(&self) -> Tally {
        Tally {
            content: self.counts.content.load(Ordering::SeqCst),
            capture: self.counts.capture.load(Ordering::SeqCst),
            perform: self.counts.perform.load(Ordering::SeqCst),
            launch: self.counts.launch.load(Ordering::SeqCst),
            geometry: self.counts.geometry.load(Ordering::SeqCst),
            focus: self.counts.focus.load(Ordering::SeqCst),
            subscribe: self.counts.subscribe.load(Ordering::SeqCst),
        }
    }

    pub fn windows_none(&self) -> usize {
        self.counts.windows_none.load(Ordering::SeqCst)
    }

    pub fn resolve_calls(&self) -> usize {
        self.counts.resolve.load(Ordering::SeqCst)
    }

    pub fn releases(&self) -> usize {
        self.counts.release.load(Ordering::SeqCst)
    }

    pub fn lists_after_flip(&self) -> usize {
        self.counts.lists_after_flip.load(Ordering::SeqCst)
    }

    pub fn metadata(&self) -> usize {
        self.counts.metadata.load(Ordering::SeqCst)
    }

    pub fn restores(&self) -> usize {
        self.counts.restore.load(Ordering::SeqCst)
    }

    pub fn set_version(&mut self, version: &str) {
        self.caps.version = version.into();
    }

    pub fn arm_deny_perform(&self) {
        lock(&self.evidence).deny_perform = true;
    }

    pub fn arm_permission(&self, token: &str) {
        lock(&self.evidence).permission = Some(token.to_owned());
    }

    pub fn arm_events(&self) {
        let mut ev = lock(&self.evidence);
        ev.events = Some(Arc::new(Mutex::new(VecDeque::new())));
    }

    pub fn push_event(&self, event: UiEvent) {
        let slot = {
            let ev = lock(&self.evidence);
            let Some(queue) = ev.events.clone() else {
                return;
            };
            queue
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push_back(event);
            ev.waker.clone()
        };
        let taken = {
            let mut guard = slot.lock().unwrap_or_else(|err| err.into_inner());
            guard.take()
        };
        if let Some(waker) = taken {
            waker.wake();
        }
    }

    pub fn arm_flip(&self) {
        lock(&self.evidence).flip = true;
    }

    pub fn arm_change_during_perform(&self) {
        lock(&self.evidence).change_during_perform = true;
    }

    pub fn arm_stop_during_perform(&self) {
        lock(&self.evidence).stop_during_perform = true;
    }

    pub fn shift_start(&self, app_id: &str, ticks: u64) {
        let mut ev = lock(&self.evidence);
        shift_start(&mut ev, app_id, ticks);
    }

    pub fn set_incarnation(&self, window: &str, incarnation: u64) {
        let mut ev = lock(&self.evidence);
        let binding = ev.windows.get(window).unwrap().clone();
        ev.windows.insert(
            window.to_owned(),
            WindowBinding::new(binding.native_id(), incarnation, binding.owner().clone()).unwrap(),
        );
    }

    pub fn retarget_handles(&self, from: &str, to: &str) {
        let mut ev = lock(&self.evidence);
        for (_, window) in &mut ev.handles {
            if window == from {
                *window = to.to_owned();
            }
        }
    }

    pub async fn real_window(&self, query: &str, id: &str) -> WindowRef {
        let app = self
            .inner
            .resolve_app(&AppQuery::from(query))
            .await
            .unwrap();
        WindowRef {
            app,
            id: id.to_owned(),
        }
    }

    pub async fn raw_handle(&self, query: &str, id: &str, node: &str) -> (WindowRef, NativeHandle) {
        let win = self.real_window(query, id).await;
        let handle = self.inner.element(&win, node).await.unwrap();
        lock(&self.evidence)
            .handles
            .push((handle.clone(), id.to_owned()));
        (win, handle)
    }

    fn remember(&self, tree: &RawTree, window: &str) {
        let mut ev = lock(&self.evidence);
        if let Some(root) = &tree.root {
            remember_node(root, window, &mut ev);
        }
    }
}

fn remember_node(node: &RawNode, window: &str, ev: &mut Evidence) {
    ev.handles.push((node.handle.clone(), window.to_owned()));
    for child in &node.children {
        remember_node(child, window, ev);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

fn shift_start(ev: &mut Evidence, app_id: &str, ticks: u64) {
    let Some(current) = ev.apps.get(app_id).cloned() else {
        return;
    };
    let next = ProcessInstance::new(
        current.pid(),
        OsStartTime::new(ticks).unwrap(),
        current.boot().clone(),
    )
    .unwrap();
    let pid = next.pid();
    ev.apps.insert(app_id.to_owned(), next.clone());
    let ids: Vec<String> = ev.windows.keys().cloned().collect();
    for id in ids {
        let binding = ev.windows.get(&id).unwrap().clone();
        if binding.owner().pid() == pid {
            ev.windows.insert(
                id,
                WindowBinding::new(binding.native_id(), binding.incarnation(), next.clone())
                    .unwrap(),
            );
        }
    }
}

fn initial_evidence() -> Evidence {
    let mut apps = BTreeMap::new();
    let mut windows = BTreeMap::new();
    for (id, pid, ticks, wins) in [
        ("fixture.a", 1101, 10, &["w-a", "w-a2", "w-a3"][..]),
        ("fixture.b", 1102, 11, &["w-b"][..]),
        ("fixture.c", 1103, 12, &["w-c"][..]),
        ("fixture.d", 1104, 13, &["w-d"][..]),
    ] {
        let instance = process(pid, ticks);
        apps.insert(id.to_owned(), instance.clone());
        for win in wins {
            windows.insert((*win).to_owned(), binding(win, 1, instance.clone()));
        }
    }
    Evidence {
        apps,
        windows,
        handles: Vec::new(),
        flip: false,
        flipped: false,
        change_during_perform: false,
        stop_during_perform: false,
        hold: None,
        hold_started: false,
        hold_skip: 0,
        hold_go: false,
        deny_perform: false,
        snapshot_fault: None,
        fault_lease: None,
        permission: None,
        events: None,
        waker: Arc::new(Mutex::new(None)),
    }
}

impl InstanceEvidence for Spy {
    fn app_instance(&self, app: &AppRef) -> Option<ProcessInstance> {
        lock(&self.evidence).apps.get(&app.info.id).cloned()
    }

    fn window_binding(&self, window: &WindowRef) -> Option<WindowBinding> {
        lock(&self.evidence).windows.get(&window.id).cloned()
    }

    fn element_binding(&self, handle: &NativeHandle) -> Option<ElementBinding> {
        let ev = lock(&self.evidence);
        let window = ev
            .handles
            .iter()
            .find(|(known, _)| known.ptr_eq(handle))
            .map(|(_, id)| id.clone())?;
        ev.windows.get(&window).cloned().map(ElementBinding::new)
    }

    fn attempt_fence<'a>(
        &'a self,
        phase: gcu_core::scope::AttemptPhase,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        let op = match phase {
            gcu_core::scope::AttemptPhase::BeforeOpen => HoldOp::BeforeOpen,
            gcu_core::scope::AttemptPhase::Admit => HoldOp::Admit,
            gcu_core::scope::AttemptPhase::MarkSeen => HoldOp::MarkSeen,
        };
        Box::pin(async move {
            self.pause(op).await;
        })
    }
}

#[async_trait]
impl Driver for Spy {
    fn platform(&self) -> Platform {
        Platform::Linux
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn doctor(&self) -> DoctorReport {
        self.counts.metadata.fetch_add(1, Ordering::SeqCst);
        let mut report = self.inner.doctor().await;
        report.issues = vec![
            "CANARY-EXTERNAL-C".into(),
            "permission missing: accessibility".into(),
            "permission missing: screen_recording".into(),
            "leak /canary/external/c".into(),
            "accessibility".into(),
            "zz".into(),
        ];
        report
    }

    async fn list_apps(&self) -> DriverResult<Vec<AppInfo>> {
        self.pause(HoldOp::ListApps).await;
        {
            let mut ev = lock(&self.evidence);
            let armed = ev.flip && !ev.flipped && self.counts.perform.load(Ordering::SeqCst) > 0;
            if armed {
                ev.flipped = true;
                shift_start(&mut ev, "fixture.a", 99);
            }
            if ev.flipped {
                self.counts.lists_after_flip.fetch_add(1, Ordering::SeqCst);
            }
        }
        self.counts.metadata.fetch_add(1, Ordering::SeqCst);
        self.inner.list_apps().await
    }

    fn discovery_status(&self) -> DiscoveryStatus {
        DiscoveryStatus::default()
    }

    async fn resolve_app(&self, query: &AppQuery) -> DriverResult<AppRef> {
        self.counts.resolve.fetch_add(1, Ordering::SeqCst);
        self.inner.resolve_app(query).await
    }

    async fn launch_app(&self, app: &AppRef) -> DriverResult<AppRef> {
        self.counts.launch.fetch_add(1, Ordering::SeqCst);
        self.inner.launch_app(app).await
    }

    async fn list_windows(&self, app: Option<&AppRef>) -> DriverResult<Vec<WindowInfo>> {
        self.pause(HoldOp::ListWindows).await;
        self.counts.metadata.fetch_add(1, Ordering::SeqCst);
        if app.is_none() {
            self.counts.windows_none.fetch_add(1, Ordering::SeqCst);
        } else {
            self.counts.windows_some.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.list_windows(app).await
    }

    async fn key_window(&self, app: &AppRef) -> DriverResult<WindowInfo> {
        self.counts.restore.fetch_add(1, Ordering::SeqCst);
        if app.info.id == "fixture.a" {
            return Ok(WindowInfo {
                id: "w-a3".into(),
                title: "CANARY-WINDOW-A3".into(),
                frame: gcu_protocol::Rect {
                    x: 440.0,
                    y: 0.0,
                    w: 200.0,
                    h: 120.0,
                },
                scale: 1.0,
                on_current_desktop: Some(false),
            });
        }
        self.inner.key_window(app).await
    }

    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> DriverResult<RawTree> {
        self.pause(HoldOp::Snapshot).await;
        if self.fire_snapshot_fault() {
            return Err(DriverError::Native {
                message: "injected snapshot fault".into(),
                maybe_executed: false,
            });
        }
        if let Some(permission) = lock(&self.evidence).permission.take() {
            return Err(DriverError::PermissionMissing { permission });
        }
        self.counts.content.fetch_add(1, Ordering::SeqCst);
        let raw = self.inner.snapshot_tree(win, opts, cancel).await?;
        self.remember(&raw, &win.id);
        Ok(raw)
    }

    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> DriverResult<Capture> {
        self.pause(HoldOp::Capture).await;
        self.counts.capture.fetch_add(1, Ordering::SeqCst);
        self.inner.capture(win, opts, cancel).await
    }

    async fn window_geometry(&self, win: &WindowRef) -> DriverResult<gcu_core::model::Geometry> {
        self.pause(HoldOp::Geometry).await;
        self.counts.geometry.fetch_add(1, Ordering::SeqCst);
        self.inner.window_geometry(win).await
    }

    async fn focused_element(&self, app: &AppRef) -> DriverResult<Option<NativeHandle>> {
        self.pause(HoldOp::Focus).await;
        self.counts.focus.fetch_add(1, Ordering::SeqCst);
        self.inner.focused_element(app).await
    }

    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> DriverResult<Delivery> {
        let started = {
            let ev = lock(&self.evidence);
            ev.hold == Some(HoldOp::Perform) && ev.hold_skip == 0 && ev.hold_started
        };
        self.pause(HoldOp::Perform).await;
        if lock(&self.evidence).deny_perform {
            return Err(DriverError::ScopeDenied);
        }
        if started {
            self.counts.perform.fetch_add(1, Ordering::SeqCst);
            return Ok(Delivery::Accessibility);
        }
        if cancel.is_cancelled() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        self.counts.perform.fetch_add(1, Ordering::SeqCst);
        let result = self.inner.perform(req, cancel).await;
        let (change, stop) = {
            let ev = lock(&self.evidence);
            (ev.change_during_perform, ev.stop_during_perform)
        };
        if change {
            self.set_incarnation("w-a", 99);
        }
        if stop {
            self.activity.request_stop();
        }
        result
    }

    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        self.counts.subscribe.fetch_add(1, Ordering::SeqCst);
        let ev = lock(&self.evidence);
        if let Some(events) = ev.events.clone() {
            return Some(Box::pin(live::LiveEvents {
                events,
                waker: ev.waker.clone(),
                counts: Arc::clone(&self.counts),
            }));
        }
        drop(ev);
        self.inner.subscribe(win)
    }

    async fn release_all_input(&self) {
        self.counts.held_input.store(0, Ordering::SeqCst);
        self.counts.release.fetch_add(1, Ordering::SeqCst);
        self.inner.release_all_input().await;
    }
}

pub fn process(pid: u32, ticks: u64) -> ProcessInstance {
    ProcessInstance::new(pid, OsStartTime::new(ticks).unwrap(), boot()).unwrap()
}

fn boot() -> BootIdentity {
    BootIdentity::new("boot-1").unwrap()
}

pub fn binding(id: &str, incarnation: u64, owner: ProcessInstance) -> WindowBinding {
    WindowBinding::new(id, incarnation, owner).unwrap()
}

pub fn allowed_scope(generation: u64) -> FixtureScope {
    let a = process(1101, 10);
    let b = process(1102, 11);
    scope(
        generation,
        "run-s1",
        vec![
            target(a, "a", "/fixtures/a", &[("w-a", 1), ("w-a2", 1)]),
            target(b, "b", "/fixtures/b", &[("w-b", 1)]),
        ],
    )
}

pub fn scope_only_b(generation: u64) -> FixtureScope {
    scope(
        generation,
        "run-s1",
        vec![target(process(1102, 11), "b", "/fixtures/b", &[("w-b", 1)])],
    )
}

pub fn scope(generation: u64, run: &str, targets: Vec<FixtureTarget>) -> FixtureScope {
    FixtureScope::try_new(
        run,
        generation,
        "engine-s1",
        "desktop-s1",
        "lease-s1",
        targets,
    )
    .unwrap()
}

fn target(
    instance: ProcessInstance,
    role: &str,
    path: &str,
    windows: &[(&str, u64)],
) -> FixtureTarget {
    let registered = windows
        .iter()
        .map(|(id, incarnation)| RegisteredWindow::new(binding(id, *incarnation, instance.clone())))
        .collect();
    FixtureTarget::new(
        instance,
        role,
        ExecutableProof::new(path).unwrap(),
        registered,
    )
    .unwrap()
}

pub fn permissive() -> PolicyConfig {
    PolicyConfig {
        sensitive_action_confirmation: ConfirmationLevel::Off,
        allow_foreground: ForegroundPolicy::Always,
        timeout_secs: 2,
        ..PolicyConfig::default()
    }
}

pub fn catalog_names() -> Vec<String> {
    let source = include_str!("../../../gcu-mcp/src/catalog/profiles.rs");
    let mut names = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("ToolName::") else {
            continue;
        };
        let variant = rest.trim_end_matches(',').trim();
        if variant.chars().all(|ch| ch.is_ascii_alphanumeric()) && !variant.is_empty() {
            names.push(wire_name(variant));
        }
    }
    names.sort();
    names.dedup();
    names
}

fn wire_name(variant: &str) -> String {
    let mut out = String::new();
    for (index, ch) in variant.chars().enumerate() {
        if ch.is_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn assert_clean(label: &str, text: &str) {
    for canary in CANARIES {
        assert!(!text.contains(canary), "{label} leaked {canary}: {text}");
    }
}

struct InstantClock(Mutex<Duration>);

#[async_trait]
impl Clock for InstantClock {
    fn now(&self) -> Duration {
        *lock(&self.0)
    }

    async fn wait_until(&self, deadline: Duration) {
        {
            let mut now = lock(&self.0);
            if *now < deadline {
                *now = deadline;
            }
        }
        tokio::task::yield_now().await;
    }
}

pub struct World {
    pub spy: Spy,
    pub lease: ScopeLease,
    pub activity: Arc<FakeUserActivity>,
    pub approvals: Arc<ScriptedApprovals>,
    pub router: ActionRouter,
    pub session: Session,
}

impl World {
    pub fn open(config: PolicyConfig) -> Self {
        Self::open_targeted(config, true)
    }

    pub fn open_targeted(config: PolicyConfig, targeted: bool) -> Self {
        let activity = Arc::new(FakeUserActivity::new());
        let mut spy = Spy::new(FakeDriver::from_ron(SCENE).unwrap(), activity.clone());
        spy.set_targeted(targeted, targeted);
        let lease = ScopeLease::install(allowed_scope(1));
        let approvals = Arc::new(ScriptedApprovals::new());
        for _ in 0..64 {
            approvals.push_decision(gcu_host_api::ApprovalDecision::Session);
        }
        Self::assemble(spy, lease, activity, approvals, config)
    }

    pub fn assemble(
        spy: Spy,
        lease: ScopeLease,
        activity: Arc<FakeUserActivity>,
        approvals: Arc<ScriptedApprovals>,
        config: PolicyConfig,
    ) -> Self {
        let router = routed(
            spy.clone(),
            lease.clone(),
            activity.clone(),
            approvals.clone(),
            Arc::new(MemoryPolicyStore::new(config)),
            Arc::new(RecordingEventSink::new()),
        );
        let session = Session::open("scope-s1", activity.clone(), router.input_release());
        session.begin_turn().unwrap();
        Self {
            spy,
            lease,
            activity,
            approvals,
            router,
            session,
        }
    }

    pub fn direct(&self) -> gcu_core::scope::ScopedDriver<Spy> {
        gcu_core::scope::ScopedDriver::new(
            self.spy.clone(),
            self.lease.clone(),
            self.activity.clone(),
        )
    }
}

pub fn routed(
    spy: Spy,
    lease: ScopeLease,
    activity: Arc<FakeUserActivity>,
    approvals: Arc<dyn ApprovalProvider>,
    policy: Arc<dyn PolicyStore>,
    events: Arc<dyn EventSink>,
) -> ActionRouter {
    ActionRouter::with_fixture_scope(
        spy,
        lease,
        activity,
        approvals,
        policy,
        events,
        WindowLocks::new(),
    )
    .with_settle_engine(SettleEngine::new(
        SettleConfig::default(),
        Arc::new(InstantClock(Mutex::new(Duration::ZERO))),
    ))
}

pub fn app_info(id: &str, name: &str, pid: u32, path: &str) -> AppInfo {
    AppInfo {
        id: id.into(),
        name: name.into(),
        pid: Some(pid),
        identity: Some(AppIdentity {
            id: id.into(),
            executable: Some(path.into()),
            team_id: None,
            publisher: None,
            code_hash: None,
        }),
    }
}

pub fn forbidden_actions(win: &WindowRef, handle: NativeHandle) -> Vec<DriverAction> {
    let point = WindowPoint { x: 10.0, y: 10.0 };
    let path = vec![point, WindowPoint { x: 12.0, y: 14.0 }];
    let chord = KeyChord::parse("a").unwrap();
    let mut actions = vec![
        DriverAction::Press { el: handle.clone() },
        DriverAction::SecondaryAction {
            el: handle.clone(),
            action: NodeAction::Press,
        },
        DriverAction::SetValue {
            el: handle.clone(),
            value: "x".into(),
        },
        DriverAction::SelectText {
            el: handle.clone(),
            range: TextRange {
                start: 0,
                length: 1,
            },
        },
        DriverAction::ScrollElement {
            el: handle.clone(),
            dir: Dir::Down,
            pages: 1.0,
        },
        DriverAction::Activate { win: win.clone() },
    ];
    for route in [
        PointerRoute::Auto,
        PointerRoute::TargetedOnly,
        PointerRoute::AllowForeground,
    ] {
        actions.push(DriverAction::PointerClick {
            win: win.clone(),
            at: point,
            button: Button::Left,
            count: 1,
            route,
        });
        actions.push(DriverAction::PointerScroll {
            win: win.clone(),
            at: point,
            dx: 0,
            dy: -1,
            route,
        });
        actions.push(DriverAction::PointerDrag {
            win: win.clone(),
            path: path.clone(),
            route,
        });
    }
    for route in [
        KeyRoute::Auto,
        KeyRoute::TargetedOnly,
        KeyRoute::AllowForeground,
    ] {
        actions.push(DriverAction::TypeText {
            win: win.clone(),
            focus: None,
            text: "paste".into(),
            route,
        });
        actions.push(DriverAction::TypeText {
            win: win.clone(),
            focus: Some(handle.clone()),
            text: "paste".into(),
            route,
        });
        actions.push(DriverAction::PressKeys {
            win: win.clone(),
            chord: chord.clone(),
            route,
        });
    }
    actions
}

mod hold;
mod live;

pub fn element_index(text: &str, label: &str) -> u32 {
    text.lines()
        .find(|line| line.contains(label))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|token| token.parse().ok())
        .unwrap_or_else(|| panic!("missing {label} in observation"))
}

pub fn script_source_mentions_scope() -> bool {
    let source = include_str!("../../../gcu-script/tests/fixture_scope.rs");
    source.contains(SCOPE_MESSAGE)
        && source.contains("CANARY-EXTERNAL-C")
        && source.contains("run_router")
}
