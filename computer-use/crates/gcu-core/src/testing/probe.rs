//! Counting driver for scoped script tests.
//!
//! It is not a platform driver. `list_apps` returns an external canary and one
//! allowed target. Content, capture, delivery, and launch counters stay at zero
//! unless a caller forwards those calls. `key_window` counts a restore and, for
//! the allowed app, returns the unregistered window. Memory only. No blocking,
//! timeout, or cancellation.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use gcu_protocol::capabilities::{
    BackgroundOperation, CapabilityLimits, DeliveryCapabilities, ForegroundCapability,
    ObserveCapabilities, PrivateApiStatus,
};
use gcu_protocol::types::AppIdentity;
use gcu_protocol::{AppInfo, Capabilities, WindowInfo};

use crate::driver::{
    AppQuery, AppRef, BoxStream, Cancel, Capture, CaptureOpts, Delivery, DiscoveryStatus,
    DoctorReport, Driver, DriverAction, DriverError, Platform, Result, TreeOpts, UiEvent,
    WindowRef,
};
use crate::model::{Fingerprint, Geometry, NativeHandle, RawNode, RawTree, Rect, Role, States};
use crate::scope::{
    BootIdentity, ElementBinding, ExecutableProof, FixtureScope, FixtureTarget, InstanceEvidence,
    OsStartTime, ProcessInstance, RegisteredWindow, WindowBinding,
};

const CANARY_NAME: &str = "CANARY-EXTERNAL-C";
const CANARY_PATH: &str = "/canary/external/c";
const ALLOWED_NAME: &str = "Fixture Allowed";

struct Inner {
    content: AtomicUsize,
    capture: AtomicUsize,
    perform: AtomicUsize,
    launch: AtomicUsize,
    restore: AtomicUsize,
    caps: Capabilities,
    allowed: NativeHandle,
    canary: NativeHandle,
}

/// Shared counters around one external canary and one allowed target.
#[derive(Clone)]
pub struct ScopeProbe {
    inner: Arc<Inner>,
}

impl ScopeProbe {
    /// Canary app `CANARY-EXTERNAL-C` plus allowed app `Fixture Allowed`.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn external_canary() -> Self {
        let mut caps = empty_caps();
        caps.private_apis
            .insert("CANARY-PRIVATE".into(), PrivateApiStatus::Available);
        Self {
            inner: Arc::new(Inner {
                content: AtomicUsize::new(0),
                capture: AtomicUsize::new(0),
                perform: AtomicUsize::new(0),
                launch: AtomicUsize::new(0),
                restore: AtomicUsize::new(0),
                caps,
                allowed: NativeHandle::new("allowed"),
                canary: NativeHandle::new("canary"),
            }),
        }
    }

    /// Process evidence returned for the canary. Not admitted by [`Self::allowed_scope`].
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn external_instance() -> ProcessInstance {
        proved(1103, 12)
    }

    /// A different instance. The canary and the hidden window are outside this scope.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn allowed_scope() -> FixtureScope {
        let instance = proved(1101, 10);
        let window = WindowBinding::new("w-allowed", 1, instance.clone())
            .unwrap_or_else(|_| unreachable!("non-empty window id and non-zero incarnation"));
        let target = FixtureTarget::new(
            instance,
            "allowed",
            ExecutableProof::new("/fixtures/allowed")
                .unwrap_or_else(|_| unreachable!("non-empty proof path")),
            vec![RegisteredWindow::new(window)],
        )
        .unwrap_or_else(|_| unreachable!("window owner matches the instance"));
        FixtureScope::try_new("run-probe", 1, "engine", "desktop", "lease", vec![target])
            .unwrap_or_else(|_| unreachable!("complete scope record"))
    }

    /// Accessibility snapshots. Memory only; no timeout or cancellation.
    pub fn content(&self) -> usize {
        self.inner.content.load(Ordering::SeqCst)
    }

    /// Captures. Memory only; no timeout or cancellation.
    pub fn capture(&self) -> usize {
        self.inner.capture.load(Ordering::SeqCst)
    }

    /// Deliveries. Memory only; no timeout or cancellation.
    pub fn perform(&self) -> usize {
        self.inner.perform.load(Ordering::SeqCst)
    }

    /// Launches. Memory only; no timeout or cancellation.
    pub fn launch(&self) -> usize {
        self.inner.launch.load(Ordering::SeqCst)
    }

    /// `key_window` calls. Each one is the inner restore/raise.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn restores(&self) -> usize {
        self.inner.restore.load(Ordering::SeqCst)
    }
}

fn proved(pid: u32, ticks: u64) -> ProcessInstance {
    let start = match OsStartTime::new(ticks) {
        Ok(start) => start,
        Err(_) => unreachable!("non-zero start"),
    };
    let boot = match BootIdentity::new("boot-1") {
        Ok(boot) => boot,
        Err(_) => unreachable!("non-empty boot"),
    };
    match ProcessInstance::new(pid, start, boot) {
        Ok(instance) => instance,
        Err(_) => unreachable!("non-zero pid"),
    }
}

fn empty_caps() -> Capabilities {
    Capabilities {
        platform: Platform::Linux,
        version: "scope-probe".into(),
        observe: ObserveCapabilities {
            tree: true,
            screenshot: true,
            events: false,
        },
        delivery: DeliveryCapabilities {
            accessibility: true,
            targeted_pointer: true,
            targeted_keys: true,
            foreground: ForegroundCapability::Policy,
        },
        background_operation: BackgroundOperation::Full,
        user_activity_guard: false,
        overlay_excluded_from_capture: false,
        private_apis: Default::default(),
        limits: CapabilityLimits {
            max_tree_nodes: 5000,
            max_image_edge: 2560,
            type_text_max: 8192,
        },
    }
}

fn app(id: &str, name: &str, path: &str, pid: u32) -> AppInfo {
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

fn canary_app() -> AppInfo {
    app("fixture.c", CANARY_NAME, CANARY_PATH, 1103)
}

fn allowed_app() -> AppInfo {
    app("fixture.allowed", ALLOWED_NAME, "/fixtures/allowed", 1101)
}

fn window(id: &str, title: &str) -> WindowInfo {
    WindowInfo {
        id: id.into(),
        title: title.into(),
        frame: Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 120.0,
        },
        scale: 1.0,
        on_current_desktop: Some(true),
    }
}

fn node(name: &str, handle: NativeHandle) -> RawNode {
    RawNode {
        role: Role::Window,
        native_role: "window".into(),
        subrole: None,
        name: Some(name.to_owned()),
        value: None,
        description: None,
        placeholder: None,
        help: None,
        identifier: None,
        states: States::ENABLED,
        actions: Default::default(),
        frame: Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 120.0,
        }),
        text_extras: None,
        url: None,
        children: Vec::new(),
        handle,
        fingerprint: Fingerprint::default(),
    }
}

fn binding(id: &str, pid: u32, ticks: u64) -> WindowBinding {
    match WindowBinding::new(id, 1, proved(pid, ticks)) {
        Ok(binding) => binding,
        Err(_) => unreachable!("complete window binding"),
    }
}

impl InstanceEvidence for ScopeProbe {
    fn app_instance(&self, app: &AppRef) -> Option<ProcessInstance> {
        match app.info.id.as_str() {
            "fixture.c" => Some(Self::external_instance()),
            "fixture.allowed" => Some(proved(1101, 10)),
            _ => None,
        }
    }

    fn window_binding(&self, window: &WindowRef) -> Option<WindowBinding> {
        match window.id.as_str() {
            "w-c" => Some(binding("w-c", 1103, 12)),
            "w-allowed" | "w-hidden" => Some(binding(window.id.as_str(), 1101, 10)),
            _ => None,
        }
    }

    fn element_binding(&self, handle: &NativeHandle) -> Option<ElementBinding> {
        if handle.ptr_eq(&self.inner.allowed) {
            Some(ElementBinding::new(binding("w-allowed", 1101, 10)))
        } else if handle.ptr_eq(&self.inner.canary) {
            Some(ElementBinding::new(binding("w-c", 1103, 12)))
        } else {
            None
        }
    }
}

#[async_trait]
impl Driver for ScopeProbe {
    fn platform(&self) -> Platform {
        Platform::Linux
    }

    fn capabilities(&self) -> &Capabilities {
        &self.inner.caps
    }

    async fn doctor(&self) -> DoctorReport {
        DoctorReport {
            capabilities: self.inner.caps.clone(),
            ready: false,
            issues: vec![CANARY_NAME.into()],
        }
    }

    async fn list_apps(&self) -> Result<Vec<AppInfo>> {
        Ok(vec![canary_app(), allowed_app()])
    }

    fn discovery_status(&self) -> DiscoveryStatus {
        DiscoveryStatus::default()
    }

    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef> {
        let apps = self.list_apps().await?;
        let mut found = apps.into_iter().filter(|app| {
            app.id == query.0
                || app.name == query.0
                || app
                    .identity
                    .as_ref()
                    .and_then(|id| id.executable.as_deref())
                    == Some(&query.0)
        });
        match (found.next(), found.next()) {
            (Some(info), None) => Ok(AppRef { info }),
            _ => Err(DriverError::ScopeDenied),
        }
    }

    async fn launch_app(&self, _app: &AppRef) -> Result<AppRef> {
        self.inner.launch.fetch_add(1, Ordering::SeqCst);
        Err(DriverError::ScopeDenied)
    }

    async fn list_windows(&self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>> {
        let id = app.map(|app| app.info.id.as_str());
        let windows = match id {
            Some("fixture.c") => vec![window("w-c", CANARY_NAME)],
            Some("fixture.allowed") => vec![
                window("w-allowed", "Allowed Target"),
                window("w-hidden", "Hidden Target"),
            ],
            None => vec![
                window("w-c", CANARY_NAME),
                window("w-allowed", "Allowed Target"),
                window("w-hidden", "Hidden Target"),
            ],
            _ => Vec::new(),
        };
        Ok(windows)
    }

    async fn key_window(&self, app: &AppRef) -> Result<WindowInfo> {
        self.inner.restore.fetch_add(1, Ordering::SeqCst);
        if app.info.id == "fixture.allowed" {
            Ok(window("w-hidden", "Hidden Target"))
        } else if app.info.id == "fixture.c" {
            Ok(window("w-c", CANARY_NAME))
        } else {
            Err(DriverError::ScopeDenied)
        }
    }

    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        _opts: &TreeOpts,
        _cancel: &Cancel,
    ) -> Result<RawTree> {
        self.inner.content.fetch_add(1, Ordering::SeqCst);
        let (name, handle) = if win.id == "w-c" {
            (CANARY_NAME, self.inner.canary.clone())
        } else if win.id == "w-allowed" {
            ("Allowed Target", self.inner.allowed.clone())
        } else {
            return Err(DriverError::ScopeDenied);
        };
        Ok(RawTree {
            root: Some(node(name, handle)),
            incomplete: None,
        })
    }

    async fn capture(
        &self,
        _win: &WindowRef,
        _opts: &CaptureOpts,
        _cancel: &Cancel,
    ) -> Result<Capture> {
        self.inner.capture.fetch_add(1, Ordering::SeqCst);
        Err(DriverError::ScopeDenied)
    }

    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        if !matches!(win.id.as_str(), "w-c" | "w-allowed" | "w-hidden") {
            return Err(DriverError::ScopeDenied);
        }
        Ok(Geometry {
            frame: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 120.0,
            },
            scale: 1.0,
        })
    }

    async fn focused_element(&self, _app: &AppRef) -> Result<Option<NativeHandle>> {
        Ok(None)
    }

    async fn perform(&self, _req: &DriverAction, _cancel: &Cancel) -> Result<Delivery> {
        self.inner.perform.fetch_add(1, Ordering::SeqCst);
        Err(DriverError::ScopeDenied)
    }

    fn subscribe(&self, _win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        None
    }

    async fn release_all_input(&self) {}
}
