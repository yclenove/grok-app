//! Linux driver implementation connecting AT-SPI tree to the core `Driver` trait.

use crate::atspi::{
    bus::{AtspiBus, AtspiNodeRef},
    connect_atspi,
    events::{AtspiEventSubscriber, LiveEventSubscriber},
    source::{FakeNode, FakeSource},
    tree::build_raw_tree,
};
use crate::wayland::{detect_environment, LinuxEnvironment, PortalInfo, SystemEnv};
use crate::x11::{
    activity::{ActivityClock, SharedActivityClock},
    align_windows, capture_window,
    hotkey::{attach_x11_listener, HotkeyConfig, HotkeyManager, SharedStopLatch, StopLatch},
    list_x11_windows, AtspiWindowCandidate, InputLedger, RealX, XServer,
};
use async_trait::async_trait;
use gcu_core::{
    driver::{
        AppQuery, AppRef, BoxStream, Cancel, Capture, CaptureOpts, Delivery, DiscoveryStatus,
        DoctorReport, Driver, DriverAction, DriverError, Result, TreeOpts, UiEvent, WindowRef,
    },
    model::{Geometry, NativeHandle, RawTree, Rect},
};
use gcu_protocol::{capabilities::Platform, AppInfo, Capabilities, WindowInfo};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

/// Concrete Linux computer-use driver backed by AT-SPI and X11.
pub struct LinuxDriver {
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
        Ok(Self::new_internal(Some(conn), Some(sub), x))
    }

    /// Create a driver from an existing D-Bus connection.
    pub fn from_connection(connection: zbus::Connection) -> Self {
        let sub: Arc<dyn AtspiEventSubscriber> =
            Arc::new(LiveEventSubscriber::new(connection.clone()));
        let x: Option<Arc<dyn XServer>> = RealX::connect()
            .ok()
            .map(|x| Arc::new(x) as Arc<dyn XServer>);
        Self::new_internal(Some(connection), Some(sub), x)
    }

    fn new_internal(
        conn: Option<zbus::Connection>,
        sub: Option<Arc<dyn AtspiEventSubscriber>>,
        x: Option<Arc<dyn XServer>>,
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
        let _listener_handle = x.as_ref().and_then(|s| {
            attach_x11_listener(s.clone(), &hotkey_manager, &stop_latch, &ledger, &clock)
        });

        let mut driver = Self {
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
    }

    /// Create a test driver backed by an in-memory `FakeSource`.
    pub fn new_fake(
        fake_source: FakeSource,
        root: FakeNode,
        apps: Vec<AppInfo>,
        windows: Vec<(String, WindowInfo)>,
    ) -> Self {
        let mut d = Self::new_internal(None, None, None);
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

    /// Override the detected Linux environment.
    pub fn with_environment(mut self, env: LinuxEnvironment) -> Self {
        self.env = env;
        self.recompute_capabilities();
        self
    }

    /// Override the detected portal capabilities.
    pub fn with_portal_info(mut self, portal: PortalInfo) -> Self {
        self.portal_info = portal;
        self.recompute_capabilities();
        self
    }

    /// Access the detected Linux environment.
    pub fn environment(&self) -> &LinuxEnvironment {
        &self.env
    }

    /// Access the detected portal info.
    pub fn portal_info(&self) -> &PortalInfo {
        &self.portal_info
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

    /// Override the activity clock.
    pub fn with_activity_clock(mut self, clock: Arc<dyn ActivityClock>) -> Self {
        self.activity_clock = clock;
        self.recompute_capabilities();
        self
    }

    /// Override the stop latch.
    pub fn with_stop_latch(mut self, latch: Arc<dyn StopLatch>) -> Self {
        self.stop_latch = latch;
        self.recompute_capabilities();
        self
    }

    /// Override the hotkey manager.
    pub fn with_hotkey_manager(mut self, mgr: Arc<HotkeyManager>) -> Self {
        self.hotkey_manager = mgr;
        self.recompute_capabilities();
        self
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
        let atspi_ready = if self.fake_source.is_some() {
            true
        } else if let Some(ref conn) = self.connection {
            !conn.is_closed()
        } else {
            connect_atspi().await.is_ok()
        };
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
        crate::wayland::doctor::run_driver_doctor(
            &self.env,
            &self.portal_info,
            self.capabilities(),
            composite,
            xtest,
            self.hotkey_manager.is_available() && !self.stop_latch.is_stopped(),
            self.activity_clock.is_available(),
            self.stop_latch.is_stopped(),
            atspi_ready,
            self.hotkey_manager.last_error().as_deref(),
            has_x,
        )
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
                return Ok(self.align_and_record(x.as_ref(), &cands));
            }
            return Ok(base);
        }

        let conn = self
            .connection
            .as_ref()
            .ok_or(DriverError::PermissionMissing {
                permission: "accessibility".into(),
            })?;

        let candidates = crate::atspi::list_atspi_candidate_windows(conn, app).await?;
        if let Some(ref x) = self.x_server {
            Ok(self.align_and_record(x.as_ref(), &candidates))
        } else {
            Ok(candidates
                .into_iter()
                .map(|c| WindowInfo {
                    id: c.path,
                    title: c.title,
                    frame: c.frame,
                    scale: 1.0,
                    on_current_desktop: Some(true),
                })
                .collect())
        }
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
                Ok(g) => g,
                Err(p) => p.into_inner(),
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

    async fn capture(
        &self,
        win: &WindowRef,
        _opts: &CaptureOpts,
        _cancel: &Cancel,
    ) -> Result<Capture> {
        if self.is_native_wayland_window(win) {
            return Err(DriverError::CapabilityUnavailable);
        }
        let x = self
            .x_server
            .as_ref()
            .ok_or(DriverError::CapabilityUnavailable)?;
        let xid = self.resolve_xid(win)?;
        capture_window(x.as_ref(), xid)
    }

    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        if let Some(ref x) = self.x_server {
            if let Ok(id) = self.resolve_xid(win) {
                if let Ok(geom) = x.window_geometry(id) {
                    let scale = x.root_geometry().map(|r| r.scale).unwrap_or(1.0);
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
            .find(|w| w.id == win.id)
            .ok_or(DriverError::WindowGone)?;
        Ok(Geometry {
            frame: matched.frame,
            scale: matched.scale,
        })
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
        if let Some(ref x) = self.x_server {
            self.ledger.release_all(x.as_ref());
        }
    }
}

impl Drop for LinuxDriver {
    fn drop(&mut self) {
        if let Some(h) = self._listener_handle.as_ref() {
            h.abort();
        }
    }
}
