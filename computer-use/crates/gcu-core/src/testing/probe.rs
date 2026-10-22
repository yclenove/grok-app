//! Counting driver for scoped script tests.
//!
//! It is not a platform driver. `list_apps` returns one external canary and
//! evidence for a process the supplied scope does not admit. Content, capture,
//! delivery, and launch counters stay at zero unless a wrapper forwards those
//! calls. Memory only. No blocking, timeout, or cancellation.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use gcu_protocol::capabilities::{
    BackgroundOperation, CapabilityLimits, DeliveryCapabilities, ForegroundCapability,
    ObserveCapabilities, PrivateApiStatus,
};
use gcu_protocol::types::AppIdentity;
use gcu_protocol::{AppInfo, Capabilities};

use crate::driver::{
    AppQuery, AppRef, BoxStream, Cancel, Capture, CaptureOpts, Delivery, DiscoveryStatus,
    DoctorReport, Driver, DriverAction, DriverError, Platform, Result, TreeOpts, UiEvent,
    WindowRef,
};
use crate::model::{NativeHandle, RawTree};
use crate::scope::{
    BootIdentity, ExecutableProof, FixtureScope, FixtureTarget, InstanceEvidence, OsStartTime,
    ProcessInstance, RegisteredWindow, WindowBinding,
};

const CANARY_NAME: &str = "CANARY-EXTERNAL-C";
const CANARY_PATH: &str = "/canary/external/c";

struct Inner {
    content: AtomicUsize,
    capture: AtomicUsize,
    perform: AtomicUsize,
    launch: AtomicUsize,
    caps: Capabilities,
}

/// Shared counters around one external canary app.
#[derive(Clone)]
pub struct ScopeProbe {
    inner: Arc<Inner>,
}

impl ScopeProbe {
    /// Canary app `CANARY-EXTERNAL-C` at `/canary/external/c`.
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
                caps,
            }),
        }
    }

    /// Process evidence returned for the canary. Not admitted by [`Self::allowed_scope`].
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn external_instance() -> ProcessInstance {
        proved(1103, 12)
    }

    /// A different instance. The canary is outside this scope.
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

fn canary_app() -> AppInfo {
    AppInfo {
        id: "fixture.c".into(),
        name: CANARY_NAME.into(),
        pid: Some(1103),
        identity: Some(AppIdentity {
            id: "fixture.c".into(),
            executable: Some(CANARY_PATH.into()),
            team_id: None,
            publisher: None,
            code_hash: None,
        }),
    }
}

impl InstanceEvidence for ScopeProbe {
    fn app_instance(&self, app: &AppRef) -> Option<ProcessInstance> {
        if app.info.id == "fixture.c" {
            Some(Self::external_instance())
        } else {
            None
        }
    }

    fn window_binding(&self, _window: &WindowRef) -> Option<WindowBinding> {
        None
    }

    fn element_binding(&self, _handle: &NativeHandle) -> Option<crate::scope::ElementBinding> {
        None
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
        Ok(vec![canary_app()])
    }

    fn discovery_status(&self) -> DiscoveryStatus {
        DiscoveryStatus::default()
    }

    async fn resolve_app(&self, _query: &AppQuery) -> Result<AppRef> {
        Err(DriverError::ScopeDenied)
    }

    async fn launch_app(&self, _app: &AppRef) -> Result<AppRef> {
        self.inner.launch.fetch_add(1, Ordering::SeqCst);
        Err(DriverError::ScopeDenied)
    }

    async fn list_windows(&self, _app: Option<&AppRef>) -> Result<Vec<gcu_protocol::WindowInfo>> {
        Ok(Vec::new())
    }

    async fn key_window(&self, _app: &AppRef) -> Result<gcu_protocol::WindowInfo> {
        Err(DriverError::ScopeDenied)
    }

    async fn snapshot_tree(
        &self,
        _win: &WindowRef,
        _opts: &TreeOpts,
        _cancel: &Cancel,
    ) -> Result<RawTree> {
        self.inner.content.fetch_add(1, Ordering::SeqCst);
        Ok(RawTree::default())
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

    async fn window_geometry(&self, _win: &WindowRef) -> Result<crate::model::Geometry> {
        Err(DriverError::ScopeDenied)
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
