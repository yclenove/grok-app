//! Linux driver implementation connecting AT-SPI tree to the core `Driver` trait.

use crate::atspi::{
    bus::{AtspiBus, AtspiNodeRef},
    connect_atspi,
    events::{AtspiEventSubscriber, LiveEventSubscriber},
    source::{FakeNode, FakeSource},
    tree::build_raw_tree,
};
use crate::identity::scope::{ProcessClock, ScopeBinding, ScopeGate};
use crate::wayland::{detect_environment, LinuxEnvironment, PortalInfo, SystemEnv};
use crate::x11::{
    activity::{ActivityClock, SharedActivityClock},
    align_windows, capture_window,
    hotkey::{attach_x11_listener, HotkeyConfig, HotkeyManager, SharedStopLatch, StopLatch},
    list_x11_windows,
    scope::{capture_in_scope, observe_x11, X11Scope},
    AtspiWindowCandidate, InputLedger, RealX, XServer,
};
use async_trait::async_trait;
use gcu_core::{
    driver::{
        AppQuery, AppRef, BoxStream, Cancel, Capture, CaptureOpts, Delivery, DiscoveryStatus,
        DoctorReport, Driver, DriverAction, DriverError, Result, TreeOpts, UiEvent, WindowRef,
    },
    model::{Geometry, NativeHandle, RawTree, Rect},
    scope::{ElementBinding, InstanceEvidence, ProcessInstance, WindowBinding},
};
use gcu_protocol::{capabilities::Platform, AppInfo, Capabilities, WindowInfo};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

#[path = "driver_bind.rs"]
mod bind;
#[path = "driver_portal.rs"]
mod portal;
#[path = "driver_wayland.rs"]
mod wayland_bind;

/// Concrete Linux computer-use driver backed by AT-SPI and X11.
pub struct LinuxDriver {
    portal_input: Option<Arc<crate::wayland::input::PortalInput>>,
    caps_all: Capabilities,
    caps_no_guard: Capabilities,
    caps_no_hotkey: Capabilities,
    caps_none: Capabilities,
    connection: Option<zbus::Connection>,
    subscriber: Option<Arc<dyn AtspiEventSubscriber>>,
    x_server: Option<Arc<dyn XServer>>,
    atspi_to_x: Arc<RwLock<HashMap<String, u32>>>,
    x_to_atspi: Arc<RwLock<HashMap<u32, String>>>,
    fake_source: Option<Arc<FakeSource>>,
    fake_root: Option<FakeNode>,
    fake_apps: Vec<AppInfo>,
    fake_windows: Vec<(String, WindowInfo)>,
    ledger: Arc<InputLedger>,
    activity_clock: Arc<dyn ActivityClock>,
    hotkey_manager: Arc<HotkeyManager>,
    stop_latch: Arc<dyn StopLatch>,
    _listener_handle: Option<tokio::task::JoinHandle<()>>,
    env: LinuxEnvironment,
    portal_info: PortalInfo,
    bound_capture: Option<Arc<crate::wayland::assembly::ProvenCapture>>,
    wayland_stop: Option<Arc<crate::wayland::input_control::StopControl>>,
    scope: Option<Arc<ScopeBinding>>,
}

impl LinuxDriver {
    /// Construct default capabilities for the Linux AT-SPI driver.
    pub fn default_capabilities() -> Capabilities {
        crate::wayland::capabilities::default_capabilities()
    }

    /// Connect to the live system AT-SPI accessibility bus and X11 display.
    pub async fn connect() -> std::result::Result<Self, DriverError> {
        let conn = connect_atspi().await?;
        let sub: Arc<dyn AtspiEventSubscriber> = Arc::new(LiveEventSubscriber::new(conn.clone()));
        let x: Option<Arc<dyn XServer>> = RealX::connect()
            .ok()
            .map(|x| Arc::new(x) as Arc<dyn XServer>);
        Ok(Self::new_internal(Some(conn), Some(sub), x, true))
    }

    /// Create a driver from an existing D-Bus connection.
    pub fn from_connection(connection: zbus::Connection) -> Self {
        let sub: Arc<dyn AtspiEventSubscriber> =
            Arc::new(LiveEventSubscriber::new(connection.clone()));
        let x: Option<Arc<dyn XServer>> = RealX::connect()
            .ok()
            .map(|x| Arc::new(x) as Arc<dyn XServer>);
        Self::new_internal(Some(connection), Some(sub), x, true)
    }

    fn new_internal(
        conn: Option<zbus::Connection>,
        sub: Option<Arc<dyn AtspiEventSubscriber>>,
        x: Option<Arc<dyn XServer>>,
        bind_listener: bool,
    ) -> Self {
        let env = detect_environment(&SystemEnv);
        let portal_info = PortalInfo::default();
        let stop_latch: Arc<dyn StopLatch> = Arc::new(SharedStopLatch::default());
        let hotkey_manager = Arc::new(HotkeyManager::new(
            HotkeyConfig::default(),
            stop_latch.clone(),
        ));
        let ledger = Arc::new(InputLedger::new());
        let clock: Arc<dyn ActivityClock> = Arc::new(SharedActivityClock::new(false));
        let _listener_handle = if bind_listener {
            x.as_ref().and_then(|server| {
                attach_x11_listener(
                    server.clone(),
                    &hotkey_manager,
                    &stop_latch,
                    &ledger,
                    &clock,
                )
            })
        } else {
            None
        };

        let mut driver = Self {
            portal_input: None,
            caps_all: Self::default_capabilities(),
            caps_no_guard: Self::default_capabilities(),
            caps_no_hotkey: Self::default_capabilities(),
            caps_none: Self::default_capabilities(),
            connection: conn,
            subscriber: sub,
            x_server: x,
            atspi_to_x: Arc::new(RwLock::new(HashMap::new())),
            x_to_atspi: Arc::new(RwLock::new(HashMap::new())),
            fake_source: None,
            fake_root: None,
            fake_apps: Vec::new(),
            fake_windows: Vec::new(),
            ledger,
            activity_clock: clock,
            hotkey_manager,
            stop_latch,
            _listener_handle,
            env,
            portal_info,
            bound_capture: None,
            wayland_stop: None,
            scope: None,
        };
        driver.recompute_capabilities();
        driver
    }

    /// Recompute the capability quad based on current environment, server, and portal status.
    pub fn recompute_capabilities(&mut self) {
        let has_x = self.x_server.is_some();
        let composite = if self.fake_source.is_some() && !has_x {
            true
        } else {
            self.x_server
                .as_ref()
                .and_then(|x| x.is_composite_available().ok())
                .unwrap_or(false)
        };
        let xtest = if self.fake_source.is_some() && !has_x {
            true
        } else {
            self.x_server
                .as_ref()
                .and_then(|x| x.is_xtest_available().ok())
                .unwrap_or(false)
        };
        let (c1, c2, c3, c4) = crate::wayland::capabilities::compute_driver_caps_quad(
            &self.env,
            &self.portal_info,
            has_x,
            composite,
            xtest,
        );

        self.caps_all = c1;
        self.caps_no_guard = c2;
        self.caps_no_hotkey = c3;
        self.caps_none = c4;
        bind::degrade_without_resident_tree(self);
        wayland_bind::apply_capture_capability(self);
    }

    /// Create a test driver backed by an in-memory `FakeSource`.
    pub fn new_fake(
        fake_source: FakeSource,
        root: FakeNode,
        apps: Vec<AppInfo>,
        windows: Vec<(String, WindowInfo)>,
    ) -> Self {
        let mut d = Self::new_internal(None, None, None, false);
        d.env = LinuxEnvironment::pure_x11_test();
        d.hotkey_manager.set_available(true);
        d.activity_clock = Arc::new(SharedActivityClock::new(true));
        d.fake_source = Some(Arc::new(fake_source));
        d.fake_root = Some(root);
        d.fake_apps = apps;
        d.fake_windows = windows;
        d.recompute_capabilities();
        d
    }

    /// Attach an XServer implementation to this driver (useful for injecting FakeX).
    pub fn with_x_server(mut self, x: Arc<dyn XServer>) -> Self {
        if let Some(h) = self._listener_handle.take() {
            h.abort();
        }
        self._listener_handle = attach_x11_listener(
            x.clone(),
            &self.hotkey_manager,
            &self.stop_latch,
            &self.ledger,
            &self.activity_clock,
        );

        self.x_server = Some(x);
        self.recompute_capabilities();
        self
    }

    /// Check whether a window is a native Wayland surface (not backed by an X11 XID).
    pub fn is_native_wayland_window(&self, win: &WindowRef) -> bool {
        if self.resolve_xid(win).is_ok() {
            return false;
        }
        if self.fake_source.is_some() {
            return self.fake_windows.iter().any(|(_, w)| w.id == win.id);
        }
        self.env.session_type.is_wayland()
            && (win.id.starts_with('/') || win.id.starts_with("wayland"))
    }

    /// Access the activity clock.
    pub fn activity_clock(&self) -> &Arc<dyn ActivityClock> {
        &self.activity_clock
    }

    /// Access the hotkey manager.
    pub fn hotkey_manager(&self) -> &Arc<HotkeyManager> {
        &self.hotkey_manager
    }

    /// Access the stop latch.
    pub fn stop_latch(&self) -> &Arc<dyn StopLatch> {
        &self.stop_latch
    }

    /// Whether Stop is currently latched.
    pub fn is_stopped(&self) -> bool {
        self.stop_latch.is_stopped()
    }

    /// Whether Stop hotkey is available.
    pub fn is_hotkey_available(&self) -> bool {
        self.hotkey_manager.is_available()
    }

    /// Whether physical user activity guard is active and healthy.
    pub fn is_activity_guard_available(&self) -> bool {
        self.activity_clock.is_available()
    }

    /// Access the underlying D-Bus connection if connected.
    pub fn connection(&self) -> Option<&zbus::Connection> {
        self.connection.as_ref()
    }

    /// Access the underlying XServer if available.
    pub fn x_server(&self) -> Option<&Arc<dyn XServer>> {
        self.x_server.as_ref()
    }

    /// Access the underlying input ledger.
    pub fn ledger(&self) -> &Arc<InputLedger> {
        &self.ledger
    }

    /// Access the fake root node if configured.
    pub fn fake_root(&self) -> Option<&FakeNode> {
        self.fake_root.as_ref()
    }

    /// Access the fake source if configured.
    pub fn fake_source(&self) -> Option<&Arc<FakeSource>> {
        self.fake_source.as_ref()
    }

    /// Resolve an X11 window ID from a WindowRef.
    pub fn resolve_xid(&self, win: &WindowRef) -> Result<u32, DriverError> {
        let xid = if let Ok(id) = win.id.parse::<u32>() {
            id
        } else {
            let map = self.atspi_to_x.read().unwrap_or_else(|p| p.into_inner());
            *map.get(&win.id).ok_or(DriverError::WindowGone)?
        };
        if let Some(ref x) = self.x_server {
            if x.window_geometry(xid).is_err() {
                return Err(DriverError::WindowGone);
            }
        }
        Ok(xid)
    }

    /// Register the fixture windows and the clock that re-reads their owner.
    ///
    /// Later reads and input prove against this binding. An unbound driver
    /// keeps the existing desktop paths.
    pub fn bind_scope(
        &mut self,
        gate: ScopeGate,
        clock: Arc<dyn ProcessClock>,
    ) -> Arc<ScopeBinding> {
        let binding = Arc::new(ScopeBinding::new(gate, clock));
        self.scope = Some(Arc::clone(&binding));
        binding
    }

    /// Install an already built fixture scope.
    pub fn bind_scope_binding(&mut self, binding: Arc<ScopeBinding>) {
        self.scope = Some(binding);
    }

    pub(crate) fn scope(&self) -> Option<&ScopeBinding> {
        self.scope.as_deref()
    }

    pub(crate) fn x11_scope(&self) -> Option<X11Scope<'_>> {
        let binding = self.scope.as_ref()?;
        Some(X11Scope::new(binding.gate(), binding.oracle().clock()))
    }

    /// Prove `win`, or do nothing when no scope is bound.
    ///
    /// An X11 id is proved from the server only. A failed X11 proof does not
    /// fall through to AT-SPI. Wayland without a peer credential is closed.
    pub(crate) fn prove_target(&self, win: &WindowRef) -> Result<()> {
        let Some(binding) = &self.scope else {
            return Ok(());
        };
        if self.is_native_wayland_window(win) {
            return self.admit_wayland_window(&win.id);
        }
        if self.x11_window_id(win) {
            let server = self.x_server.as_ref().ok_or(DriverError::ScopeDenied)?;
            let xid = self
                .resolve_xid(win)
                .map_err(|_| DriverError::ScopeDenied)?;
            return X11Scope::new(binding.gate(), binding.oracle().clock())
                .allow(server.as_ref(), xid);
        }
        crate::atspi::scope::admit_tree(binding, &win.app.info.id, &win.id)
    }

    pub(crate) fn admit_wayland_window(&self, window_id: &str) -> Result<()> {
        let Some(binding) = &self.scope else {
            return Ok(());
        };
        crate::wayland::scope::admit(binding.gate(), window_id, self.portal_source_kind(), None)
    }

    fn x11_window_id(&self, win: &WindowRef) -> bool {
        if win.id.parse::<u32>().is_ok() {
            return true;
        }
        self.atspi_to_x
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .contains_key(&win.id)
    }

    fn portal_source_kind(&self) -> crate::wayland::PortalSourceKind {
        self.bound_capture
            .as_ref()
            .map(|bound| bound.stream.source_type)
            .unwrap_or(crate::wayland::PortalSourceKind::Monitor)
    }

    async fn refresh_scope_owner(&self, app_id: &str) {
        let Some(binding) = &self.scope else {
            return;
        };
        let Some(connection) = &self.connection else {
            return;
        };
        if app_id.is_empty() {
            return;
        }
        let bus = AtspiBus::new(connection.clone());
        let node = AtspiNodeRef {
            destination: app_id.to_string(),
            path: "/org/a11y/atspi/accessible/0".to_string(),
        };
        let pid = bus.application_pid(&node).await;
        binding.oracle().set_atspi_pid(app_id, pid);
    }

    async fn admit_read(&self, win: &WindowRef) -> Result<()> {
        if self.scope.is_none() {
            return Ok(());
        }
        if self.is_native_wayland_window(win) {
            return self.admit_wayland_window(&win.id);
        }
        if self.x11_window_id(win) {
            self.prove_target(win)?;
        }
        self.refresh_scope_owner(&win.app.info.id).await;
        let Some(binding) = &self.scope else {
            return Ok(());
        };
        crate::atspi::scope::admit_tree(binding, &win.app.info.id, &win.id)
    }

    async fn read_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> Result<RawTree> {
        if let Some(ref fake_source) = self.fake_source {
            let root = self.fake_root.as_ref().ok_or(DriverError::WindowGone)?;
            return build_raw_tree(fake_source.as_ref(), root, opts, cancel).await;
        }

        let conn = self
            .connection
            .as_ref()
            .ok_or(DriverError::PermissionMissing {
                permission: "accessibility".into(),
            })?;

        let bus = AtspiBus::new(conn.clone());

        let target_path = if let Ok(xid) = win.id.parse::<u32>() {
            let map = match self.x_to_atspi.read() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            map.get(&xid).cloned().unwrap_or_else(|| win.id.clone())
        } else {
            win.id.clone()
        };

        let root = AtspiNodeRef {
            destination: win.app.info.id.clone(),
            path: target_path,
        };

        build_raw_tree(&bus, &root, opts, cancel).await
    }

    async fn geometry_of(&self, win: &WindowRef) -> Result<Geometry> {
        if let Some(geometry) = wayland_bind::bound_geometry(self, win) {
            return Ok(geometry);
        }
        if let Some(ref server) = self.x_server {
            if let Ok(id) = self.resolve_xid(win) {
                if let Ok(geom) = server.window_geometry(id) {
                    let scale = server.root_geometry().map(|root| root.scale).unwrap_or(1.0);
                    let frame = Rect {
                        x: geom.x as f64,
                        y: geom.y as f64,
                        w: geom.width as f64,
                        h: geom.height as f64,
                    };
                    return Ok(Geometry { frame, scale });
                }
            }
        }

        let windows = self.list_windows(Some(&win.app)).await?;
        let matched = windows
            .into_iter()
            .find(|candidate| candidate.id == win.id)
            .ok_or(DriverError::WindowGone)?;
        Ok(Geometry {
            frame: matched.frame,
            scale: matched.scale,
        })
    }

    fn align_and_record(&self, x: &dyn XServer, cands: &[AtspiWindowCandidate]) -> Vec<WindowInfo> {
        let x_cands = list_x11_windows(x).unwrap_or_default();
        let scale = x.root_geometry().map(|r| r.scale).unwrap_or(1.0);
        let aligned = align_windows(cands, &x_cands, scale);
        if let Ok(mut map) = self.atspi_to_x.write() {
            map.extend(aligned.atspi_to_x);
        }
        if let Ok(mut map) = self.x_to_atspi.write() {
            map.extend(aligned.x_to_atspi);
        }
        aligned.windows
    }
}

#[async_trait]
impl Driver for LinuxDriver {
    fn platform(&self) -> Platform {
        Platform::Linux
    }

    fn capabilities(&self) -> &Capabilities {
        if let Some(input) = self.portal_input.as_ref().filter(|i| i.available()) {
            return input.capabilities();
        }
        let hotkey_ok = self.hotkey_manager.is_available() && !self.stop_latch.is_stopped();
        let guard_ok = self.activity_clock.is_available();
        match (hotkey_ok, guard_ok) {
            (true, true) => &self.caps_all,
            (true, false) => &self.caps_no_guard,
            (false, true) => &self.caps_no_hotkey,
            (false, false) => &self.caps_none,
        }
    }

    async fn doctor(&self) -> DoctorReport {
        wayland_bind::doctor(self).await
    }

    fn discovery_status(&self) -> DiscoveryStatus {
        DiscoveryStatus { indexing: false }
    }

    async fn list_apps(&self) -> Result<Vec<AppInfo>> {
        if self.fake_source.is_some() {
            return Ok(self.fake_apps.clone());
        }

        let conn = self
            .connection
            .as_ref()
            .ok_or(DriverError::PermissionMissing {
                permission: "accessibility".into(),
            })?;

        crate::atspi::list_atspi_apps(conn).await
    }

    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef> {
        let apps = self.list_apps().await?;
        crate::atspi::resolve_atspi_app(&apps, query)
    }

    async fn launch_app(&self, _app: &AppRef) -> Result<AppRef> {
        Err(DriverError::CapabilityUnavailable)
    }

    async fn list_windows(&self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>> {
        if self.fake_source.is_some() {
            let base: Vec<WindowInfo> = self
                .fake_windows
                .iter()
                .filter(|(app_id, _)| app.is_none_or(|target| app_id == &target.info.id))
                .map(|(_, w)| w.clone())
                .collect();

            if let Some(ref x) = self.x_server {
                let cands: Vec<_> = base
                    .iter()
                    .map(|w| AtspiWindowCandidate {
                        path: w.id.clone(),
                        title: w.title.clone(),
                        frame: w.frame,
                        pid: app.and_then(|a| a.info.pid),
                    })
                    .collect();
                return Ok(wayland_bind::paint_frames(
                    self,
                    self.align_and_record(x.as_ref(), &cands),
                ));
            }
            return Ok(wayland_bind::paint_frames(self, base));
        }

        let conn = self
            .connection
            .as_ref()
            .ok_or(DriverError::PermissionMissing {
                permission: "accessibility".into(),
            })?;

        let candidates = crate::atspi::list_atspi_candidate_windows(conn, app).await?;
        let windows = if let Some(ref x) = self.x_server {
            self.align_and_record(x.as_ref(), &candidates)
        } else {
            candidates
                .into_iter()
                .map(|c| WindowInfo {
                    id: c.path,
                    title: c.title,
                    frame: c.frame,
                    scale: 1.0,
                    on_current_desktop: Some(true),
                })
                .collect()
        };
        Ok(wayland_bind::paint_frames(self, windows))
    }

    async fn key_window(&self, app: &AppRef) -> Result<WindowInfo> {
        let windows = self.list_windows(Some(app)).await?;
        if windows.is_empty() {
            return Err(DriverError::WindowGone);
        }

        if let Some(ref x) = self.x_server {
            if let Ok(Some(active_xid)) = x.active_window() {
                let active_str = active_xid.to_string();
                if let Some(matched) = windows.iter().find(|w| w.id == active_str) {
                    return Ok(matched.clone());
                }
            }
        }

        windows.into_iter().next().ok_or(DriverError::WindowGone)
    }

    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> Result<RawTree> {
        if self.scope.is_some() {
            self.admit_read(win).await?;
        }
        let tree = self.read_tree(win, opts, cancel).await?;
        if self.scope.is_some() {
            self.admit_read(win).await?;
            if let Some(binding) = &self.scope {
                crate::atspi::scope::seal_tree(binding, &tree, &win.id)?;
            }
        }
        Ok(tree)
    }

    async fn capture(
        &self,
        win: &WindowRef,
        _opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture> {
        if self.scope.is_some() {
            if self.is_native_wayland_window(win) {
                return wayland_bind::capture_native(self, win, cancel).await;
            }
            let server = self.x_server.as_ref().ok_or(DriverError::ScopeDenied)?;
            let xid = self
                .resolve_xid(win)
                .map_err(|_| DriverError::ScopeDenied)?;
            let binding = self.scope.as_ref().ok_or(DriverError::ScopeDenied)?;
            return capture_in_scope(
                server.as_ref(),
                xid,
                binding.gate(),
                binding.oracle().clock(),
            );
        }
        if self.is_native_wayland_window(win) {
            return wayland_bind::capture_native(self, win, cancel).await;
        }
        let server = self
            .x_server
            .as_ref()
            .ok_or(DriverError::CapabilityUnavailable)?;
        let xid = self.resolve_xid(win)?;
        capture_window(server.as_ref(), xid)
    }

    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        if self.scope.is_some() {
            self.prove_target(win)?;
        }
        let geometry = self.geometry_of(win).await?;
        if self.scope.is_some() {
            self.prove_target(win)?;
        }
        Ok(geometry)
    }

    async fn focused_element(&self, _app: &AppRef) -> Result<Option<NativeHandle>> {
        Ok(None)
    }

    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> Result<Delivery> {
        crate::perform::perform_action(self, req, cancel).await
    }

    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        self.subscriber.as_ref().and_then(|s| s.subscribe(&win.id))
    }

    async fn release_all_input(&self) {
        if let Some(input) = &self.portal_input {
            if input.release_all_input().is_err() {
                input.stop();
            }
        }
        if let Some(ref x) = self.x_server {
            self.ledger.release_all(x.as_ref());
        }
    }
}

impl InstanceEvidence for LinuxDriver {
    fn app_instance(&self, app: &AppRef) -> Option<ProcessInstance> {
        let binding = self.scope.as_ref()?;
        if app.info.id.is_empty() {
            return None;
        }
        let (owner_pid, start, boot) = {
            let registered = binding
                .gate()
                .windows()
                .iter()
                .find(|window| window.app_id() == app.info.id)?;
            (
                registered.owner().pid(),
                registered.owner().start_time(),
                registered.owner().boot_id().to_string(),
            )
        };
        let live = binding.oracle().process(owner_pid)?;
        if live.start_time != start || live.boot_id != boot {
            return None;
        }
        for window in binding.gate().windows() {
            if window.app_id() != app.info.id {
                continue;
            }
            crate::identity::scope::check_process(window.owner(), Some(&live)).ok()?;
        }
        binding
            .gate()
            .windows()
            .iter()
            .find(|window| window.app_id() == app.info.id)?
            .binding(&live)
            .map(|window| window.owner().clone())
    }

    fn window_binding(&self, window: &WindowRef) -> Option<WindowBinding> {
        let binding = self.scope.as_ref()?;
        if self.is_native_wayland_window(window) {
            let source = self.portal_source_kind();
            let view = crate::wayland::scope::view_for_scope(&window.id, source, None);
            let proved = binding.gate().prove_wayland(&view).ok()?;
            return proved.binding(view.owner.as_ref()?);
        }
        if self.x11_window_id(window) {
            let server = self.x_server.as_ref()?;
            let xid = self.resolve_xid(window).ok()?;
            let view = observe_x11(server.as_ref(), xid, binding.oracle().clock());
            let proved = binding.gate().prove_x11(&view).ok()?;
            return proved.binding(view.owner.as_ref()?);
        }
        let (native_id, incarnation) = {
            let expected = binding
                .gate()
                .windows()
                .iter()
                .find(|candidate| candidate.native_id() == window.id)?;
            (expected.native_id().to_string(), expected.incarnation())
        };
        let view = binding
            .oracle()
            .atspi(&window.app.info.id, &native_id, incarnation);
        let proved = binding.gate().prove_atspi(&view).ok()?;
        proved.binding(view.owner.as_ref()?)
    }

    fn element_binding(&self, handle: &NativeHandle) -> Option<ElementBinding> {
        self.scope.as_ref()?.element_binding(handle)
    }
}

impl Drop for LinuxDriver {
    fn drop(&mut self) {
        if let Some(input) = &self.portal_input {
            input.shutdown();
        }
        if let Some(bound) = self.bound_capture.take() {
            wayland_bind::retire_on_drop(&bound);
        }
        if let Some(h) = self._listener_handle.as_ref() {
            h.abort();
        }
    }
}
