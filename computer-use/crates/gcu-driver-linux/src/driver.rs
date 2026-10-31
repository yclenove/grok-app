//! Linux driver implementation connecting AT-SPI tree to the core `Driver` trait.

use crate::atspi::{
    bus::{AtspiBus, AtspiNodeRef},
    connect_atspi,
    events::{AtspiEventSubscriber, LiveEventSubscriber},
    source::{FakeNode, FakeSource},
    tree::build_raw_tree,
};
use crate::identity::scope::{ProcessClock, ScopeBinding, ScopeFault, ScopeGate};
use crate::wayland::{detect_environment, LinuxEnvironment, PortalInfo, SystemEnv};
use crate::x11::{
    activity::{ActivityClock, SharedActivityClock},
    align_windows, capture_window,
    hotkey::{attach_x11_listener, HotkeyConfig, HotkeyManager, SharedStopLatch, StopLatch},
    list_x11_windows,
    scope::{capture_off_runtime, load_x11_snapshot, X11Scope},
    AtspiWindowCandidate, InputLedger, RealX, XServer,
};
use async_trait::async_trait;
use gcu_core::{
    driver::{
        AppQuery, AppRef, BoxStream, Cancel, Capture, CaptureOpts, Delivery, DiscoveryStatus,
        DoctorReport, Driver, DriverAction, DriverError, Result, TreeOpts, UiEvent, WindowRef,
    },
    model::{Geometry, NativeHandle, RawTree, Rect},
    scope::{AttemptAuthority, ElementBinding, InstanceEvidence, ProcessInstance, WindowBinding},
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

    /// Driver whose app list comes from `connection`, with no X server.
    ///
    /// Does not open a display. Tests use this so scoped discovery hits the
    /// bus instead of the fake app list.
    #[cfg(test)]
    pub(crate) fn from_bus(connection: zbus::Connection) -> Self {
        let mut driver = Self::new_internal(Some(connection), None, None, false);
        driver.env = LinuxEnvironment::pure_x11_test();
        driver.recompute_capabilities();
        driver
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

    /// X id remembered for `native_id`, without asking the server.
    fn memory_xid_str(&self, native_id: &str) -> Option<u32> {
        if let Ok(id) = native_id.parse::<u32>() {
            return Some(id);
        }
        self.atspi_to_x
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .get(native_id)
            .copied()
    }

    /// X id remembered for `win`, without asking the server.
    pub(crate) fn memory_xid(&self, win: &WindowRef) -> Option<u32> {
        self.memory_xid_str(&win.id)
    }

    /// Wayland classification from memory. This does not resolve an X id.
    pub(crate) fn memory_wayland(&self, win: &WindowRef) -> bool {
        if self.memory_xid(win).is_some() {
            return false;
        }
        if self.fake_source.is_some() {
            return self
                .fake_windows
                .iter()
                .any(|(_, window)| window.id == win.id);
        }
        self.env.session_type.is_wayland()
            && (win.id.starts_with('/') || win.id.starts_with("wayland"))
    }

    /// Wayland route for one action. Scoped mode does not query X to decide.
    pub(crate) fn route_is_wayland(&self, win: &WindowRef) -> bool {
        if self.scope.is_some() {
            self.memory_wayland(win)
        } else {
            self.is_native_wayland_window(win)
        }
    }

    pub(crate) fn x11_scope(&self) -> Option<X11Scope<'_>> {
        let binding = self.scope.as_ref()?;
        Some(X11Scope::with_runtime(
            binding.gate(),
            binding.oracle().clock(),
            binding.oracle(),
            Arc::clone(&self.stop_latch),
        ))
    }

    fn continue_scope_budget(&self) {
        let Some(binding) = &self.scope else {
            return;
        };
        let continuation = binding.oracle().take_prepared();
        if !continuation && !binding.oracle().budget_is_live() {
            binding.oracle().begin_evidence_action();
        }
    }

    /// Prove `win` from the captured observation, or do nothing when unbound.
    ///
    /// X11 uses the snapshot captured off the runtime. A missing snapshot
    /// refuses. AT-SPI-only windows are closed. Wayland without a peer
    /// credential is closed. This function does not call the X server.
    pub(crate) fn prove_target(&self, win: &WindowRef) -> Result<()> {
        let Some(binding) = &self.scope else {
            return Ok(());
        };
        if self.memory_wayland(win) {
            return self.admit_wayland_window(&win.id);
        }
        let Some(xid) = self.memory_xid(win) else {
            return Err(DriverError::ScopeDenied);
        };
        let Some(snapshot) = binding.oracle().x11_snapshot(xid) else {
            return Err(DriverError::ScopeDenied);
        };
        binding
            .gate()
            .prove_x11(&snapshot.view)
            .map(|_| ())
            .map_err(ScopeFault::deny)
    }

    pub(crate) fn admit_wayland_window(&self, window_id: &str) -> Result<()> {
        let Some(binding) = &self.scope else {
            return Ok(());
        };
        crate::wayland::scope::admit(binding.gate(), window_id, self.portal_source_kind(), None)
    }

    fn portal_source_kind(&self) -> crate::wayland::PortalSourceKind {
        self.bound_capture
            .as_ref()
            .map(|bound| bound.stream.source_type)
            .unwrap_or(crate::wayland::PortalSourceKind::Monitor)
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

    /// Registered windows the current attempt admits, with a proved X11 snapshot.
    ///
    /// The catalog is only an index. Permission is the attempt entered on this
    /// task. A missing or stale attempt refuses. Scoped AT-SPI is closed: this
    /// reads no bus name, title, child, or extents. A window without a proved
    /// X11 snapshot is omitted.
    fn scoped_registered_windows(&self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>> {
        let Some(binding) = &self.scope else {
            return Ok(Vec::new());
        };
        let Some(authority) = AttemptAuthority::on_task() else {
            return Err(DriverError::ScopeDenied);
        };
        if !authority.usable() {
            return Err(DriverError::ScopeDenied);
        }
        let Some(gate) = binding.gate_for_scope(authority.scope()) else {
            return Err(DriverError::ScopeDenied);
        };
        let _ = binding.oracle().take_prepared();
        let registered: Vec<(String, u32, String)> = gate
            .windows()
            .iter()
            .map(|window| {
                (
                    window.native_id().to_string(),
                    window.owner().pid(),
                    window.app_id().to_string(),
                )
            })
            .collect();
        let mut listed = Vec::new();
        for (native_id, pid, app_id) in registered {
            if let Some(target) = app {
                if app_id != target.info.id {
                    continue;
                }
                if target.info.pid.is_some_and(|seen| seen != pid) {
                    continue;
                }
            }
            if binding.discovered_from(&gate, &app_id, pid).is_none() {
                continue;
            }
            // The registration id is the X id. The AT-SPI path map is not evidence.
            let Ok(xid) = native_id.parse::<u32>() else {
                continue;
            };
            let Some(snapshot) = binding.oracle().x11_snapshot(xid) else {
                continue;
            };
            if gate.prove_x11(&snapshot.view).is_err() {
                continue;
            }
            let frame = snapshot
                .geometry
                .map(|geometry| geometry.frame)
                .unwrap_or(Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                });
            let scale = snapshot
                .geometry
                .map(|geometry| geometry.scale)
                .unwrap_or(1.0);
            listed.push(WindowInfo {
                id: native_id,
                title: String::new(),
                frame,
                scale,
                on_current_desktop: Some(true),
            });
        }
        if !authority.usable() {
            return Err(DriverError::ScopeDenied);
        }
        Ok(listed)
    }

    /// Names for the attempt entered on this task.
    ///
    /// The catalog is filtered by that attempt's scope before any bus call.
    /// A missing or stale attempt refuses. An epoch change drops the payload.
    async fn scoped_app_list(&self, conn: &zbus::Connection) -> Result<Vec<AppInfo>> {
        let Some(binding) = &self.scope else {
            return Err(DriverError::ScopeDenied);
        };
        let Some(authority) = AttemptAuthority::on_task() else {
            return Err(DriverError::ScopeDenied);
        };
        if !authority.usable() {
            return Err(DriverError::ScopeDenied);
        }
        let Some(gate) = binding.gate_for_scope(authority.scope()) else {
            return Err(DriverError::ScopeDenied);
        };
        let listed =
            crate::atspi::discovery::list_scoped_atspi_apps(conn, binding, &gate, &authority).await;
        if !authority.usable() {
            return Err(DriverError::ScopeDenied);
        }
        listed
    }

    /// Catalog when no attempt is entered. The admitted subset when one is.
    ///
    /// A stale attempt is `None`. Callers that require an attempt refuse
    /// before this. No D-Bus and no X11.
    fn evidence_gate(&self, binding: &ScopeBinding) -> Option<ScopeGate> {
        match AttemptAuthority::on_task() {
            Some(authority) if authority.usable() => binding.gate_for_scope(authority.scope()),
            Some(_) => None,
            None => Some(binding.gate().clone()),
        }
    }

    fn proved_window(
        &self,
        binding: &ScopeBinding,
        gate: &ScopeGate,
        window: &WindowRef,
    ) -> Option<WindowBinding> {
        if self.memory_wayland(window) {
            let source = self.portal_source_kind();
            let view = crate::wayland::scope::view_for_scope(&window.id, source, None);
            let proved = gate.prove_wayland(&view).ok()?;
            return proved.binding(view.owner.as_ref()?);
        }
        let xid = self.memory_xid(window)?;
        let snapshot = binding.oracle().x11_snapshot(xid)?;
        let proved = gate.prove_x11(&snapshot.view).ok()?;
        proved.binding(snapshot.view.owner.as_ref()?)
    }

    async fn prepare_registered_x11(&self) {
        let Some(binding) = &self.scope else {
            return;
        };
        let Some(server) = &self.x_server else {
            return;
        };
        let xids: Vec<u32> = binding
            .gate()
            .windows()
            .iter()
            .filter_map(|window| self.memory_xid_str(window.native_id()))
            .collect();
        for xid in xids {
            if !binding.oracle().budget_is_live() {
                binding.oracle().clear_x11(xid);
                continue;
            }
            let _ = load_x11_snapshot(
                binding.gate(),
                binding.oracle(),
                &self.stop_latch,
                server,
                xid,
            )
            .await;
        }
    }

    async fn prepare_action_evidence(&self, req: &DriverAction) {
        let Some(binding) = &self.scope else {
            return;
        };
        self.continue_scope_budget();
        if crate::perform::action_element(req).is_some() {
            return;
        }
        let Some(window) = crate::perform::action_window(req) else {
            return;
        };
        let Some(xid) = self.memory_xid(window) else {
            return;
        };
        if binding.oracle().is_fresh(xid) || !binding.oracle().budget_is_live() {
            return;
        }
        let Some(server) = &self.x_server else {
            return;
        };
        let _ = load_x11_snapshot(
            binding.gate(),
            binding.oracle(),
            &self.stop_latch,
            server,
            xid,
        )
        .await;
    }

    async fn scoped_window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        if self.memory_wayland(win) {
            self.admit_wayland_window(&win.id)?;
            return wayland_bind::bound_geometry(self, win).ok_or(DriverError::ScopeDenied);
        }
        let xid = self.memory_xid(win).ok_or(DriverError::ScopeDenied)?;
        let binding = self.scope.as_ref().ok_or(DriverError::ScopeDenied)?;
        let server = self.x_server.as_ref().ok_or(DriverError::ScopeDenied)?;
        if !binding.oracle().is_fresh(xid) {
            if !binding.oracle().budget_is_live() {
                return Err(DriverError::ScopeDenied);
            }
            load_x11_snapshot(
                binding.gate(),
                binding.oracle(),
                &self.stop_latch,
                server,
                xid,
            )
            .await?;
        }
        let geometry = binding
            .oracle()
            .x11_snapshot(xid)
            .and_then(|snapshot| snapshot.geometry)
            .ok_or(DriverError::ScopeDenied)?;
        if !binding.oracle().budget_is_live() {
            binding.oracle().clear_x11(xid);
            return Err(DriverError::ScopeDenied);
        }
        let checked = load_x11_snapshot(
            binding.gate(),
            binding.oracle(),
            &self.stop_latch,
            server,
            xid,
        )
        .await?;
        let again = checked.geometry.ok_or(DriverError::ScopeDenied)?;
        if again != geometry {
            binding.oracle().clear_x11(xid);
            return Err(DriverError::ScopeDenied);
        }
        Ok(geometry)
    }

    async fn capture_scoped(&self, win: &WindowRef, cancel: &Cancel) -> Result<Capture> {
        if self.memory_wayland(win) {
            return wayland_bind::capture_native(self, win, cancel).await;
        }
        let xid = self.memory_xid(win).ok_or(DriverError::ScopeDenied)?;
        let binding = self.scope.as_ref().ok_or(DriverError::ScopeDenied)?;
        let server = self
            .x_server
            .as_ref()
            .ok_or(DriverError::ScopeDenied)?
            .clone();
        if !binding.oracle().budget_is_live() {
            return Err(DriverError::ScopeDenied);
        }
        let budget = binding.oracle().evidence_remaining();
        let shot = capture_off_runtime(
            server,
            xid,
            binding.gate().clone(),
            binding.oracle().clock_arc(),
            &self.stop_latch,
            budget,
        )
        .await?;
        let server = self.x_server.as_ref().ok_or(DriverError::ScopeDenied)?;
        load_x11_snapshot(
            binding.gate(),
            binding.oracle(),
            &self.stop_latch,
            server,
            xid,
        )
        .await?;
        Ok(shot)
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

        if self.scope.is_some() {
            return self.scoped_app_list(conn).await;
        }
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
        if self.scope.is_some() {
            let windows = self.scoped_registered_windows(app)?;
            return Ok(wayland_bind::paint_frames(self, windows));
        }
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
        if let Some(binding) = &self.scope {
            let _ = binding.oracle().take_prepared();
            let windows = self.list_windows(Some(app)).await?;
            return windows.into_iter().next().ok_or(DriverError::WindowGone);
        }
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
        if let Some(binding) = &self.scope {
            let _ = binding.oracle().take_prepared();
            crate::atspi::scope::admit_tree(binding, &win.app.info.id, &win.id)?;
            return Err(DriverError::ScopeDenied);
        }
        self.read_tree(win, opts, cancel).await
    }

    async fn capture(
        &self,
        win: &WindowRef,
        _opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture> {
        if self.scope.is_some() {
            self.continue_scope_budget();
            return self.capture_scoped(win, cancel).await;
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
            self.continue_scope_budget();
            return self.scoped_window_geometry(win).await;
        }
        self.geometry_of(win).await
    }

    async fn focused_element(&self, _app: &AppRef) -> Result<Option<NativeHandle>> {
        if let Some(binding) = &self.scope {
            let _ = binding.oracle().take_prepared();
        }
        Ok(None)
    }

    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> Result<Delivery> {
        self.prepare_action_evidence(req).await;
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

const SCOPED_ROUTE_NOTE: &str = "scoped mode: AT-SPI tree reads and semantic accessibility actions are unavailable; an X11 window without a live incarnation stays closed.";

impl InstanceEvidence for LinuxDriver {
    fn app_instance(&self, app: &AppRef) -> Option<ProcessInstance> {
        let binding = self.scope.as_ref()?;
        let discovered = app.info.pid.filter(|pid| *pid > 0)?;
        let gate = self.evidence_gate(binding)?;
        binding.discovered_from(&gate, &app.info.id, discovered)
    }

    fn window_binding(&self, window: &WindowRef) -> Option<WindowBinding> {
        let binding = self.scope.as_ref()?;
        let gate = self.evidence_gate(binding)?;
        self.proved_window(binding, &gate, window)
    }

    fn element_binding(&self, handle: &NativeHandle) -> Option<ElementBinding> {
        self.scope.as_ref()?.element_binding(handle)
    }

    fn scope_capabilities(
        &self,
        current: &gcu_protocol::Capabilities,
    ) -> gcu_protocol::Capabilities {
        let mut capabilities = current.clone();
        capabilities.observe.tree = false;
        capabilities.delivery.accessibility = false;
        capabilities
    }

    fn scoped_doctor_note(&self) -> Option<&'static str> {
        Some(SCOPED_ROUTE_NOTE)
    }

    fn prepare_evidence<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let Some(binding) = &self.scope else {
                return;
            };
            binding.oracle().begin_evidence_action();
            self.prepare_registered_x11().await;
        })
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
