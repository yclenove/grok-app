//! Linux driver implementation connecting AT-SPI tree to the core `Driver` trait.

use crate::atspi::{
    bus::{AtspiBus, AtspiNodeRef},
    connect_atspi,
    events::{AtspiEventSubscriber, LiveEventSubscriber},
    source::{FakeNode, FakeSource},
    tree::{build_raw_tree, MAX_NODES_CEILING},
};
use crate::x11::{
    activity::{ActivityClock, SharedActivityClock},
    align_windows, build_caps_quad, capture_window,
    hotkey::{attach_x11_listener, HotkeyConfig, HotkeyManager, SharedStopLatch, StopLatch},
    list_x11_windows, update_x11_caps, AtspiWindowCandidate, InputLedger, RealX, XServer,
};
use async_trait::async_trait;
use gcu_core::{
    driver::{
        AppQuery, AppRef, BoxStream, Cancel, Capture, CaptureOpts, Delivery, DiscoveryStatus,
        DoctorReport, Driver, DriverAction, DriverError, Result, TreeOpts, UiEvent, WindowRef,
    },
    model::{Geometry, NativeHandle, RawTree, Rect},
};
use gcu_protocol::{
    capabilities::{
        BackgroundOperation, CapabilityLimits, DeliveryCapabilities, ForegroundCapability,
        ObserveCapabilities, Platform,
    },
    AppInfo, Capabilities, WindowInfo,
};
use std::{
    collections::{BTreeMap, HashMap},
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
}

impl LinuxDriver {
    /// Construct default capabilities for the Linux AT-SPI driver.
    pub fn default_capabilities() -> Capabilities {
        Capabilities {
            platform: Platform::Linux,
            version: "x11-atspi".into(),
            observe: ObserveCapabilities {
                tree: true,
                screenshot: true,
                events: true,
            },
            delivery: DeliveryCapabilities {
                accessibility: true,
                targeted_pointer: false,
                targeted_keys: false,
                foreground: ForegroundCapability::Policy,
            },
            background_operation: BackgroundOperation::Partial,
            user_activity_guard: false,
            overlay_excluded_from_capture: false,
            private_apis: BTreeMap::new(),
            limits: CapabilityLimits {
                max_tree_nodes: MAX_NODES_CEILING as u32,
                max_image_edge: 2560,
                type_text_max: 8192,
            },
        }
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
        let mut base = Self::default_capabilities();
        let stop_latch: Arc<dyn StopLatch> = Arc::new(SharedStopLatch::default());
        let hotkey_manager = Arc::new(HotkeyManager::new(
            HotkeyConfig::default(),
            stop_latch.clone(),
        ));
        let ledger = Arc::new(InputLedger::new());
        let clock: Arc<dyn ActivityClock> = Arc::new(SharedActivityClock::new(false));
        let _listener_handle = x.as_ref().and_then(|s| {
            update_x11_caps(&mut base, s.as_ref());
            attach_x11_listener(s.clone(), &hotkey_manager, &stop_latch, &ledger, &clock)
        });

        let (c1, c2, c3, c4) = build_caps_quad(&base);

        Self {
            caps_all: c1,
            caps_no_guard: c2,
            caps_no_hotkey: c3,
            caps_none: c4,
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
        }
    }

    /// Create a test driver backed by an in-memory `FakeSource`.
    pub fn new_fake(
        fake_source: FakeSource,
        root: FakeNode,
        apps: Vec<AppInfo>,
        windows: Vec<(String, WindowInfo)>,
    ) -> Self {
        let mut d = Self::new_internal(None, None, None);
        d.hotkey_manager.set_available(true);
        d.activity_clock = Arc::new(SharedActivityClock::new(true));
        d.fake_source = Some(Arc::new(fake_source));
        d.fake_root = Some(root);
        d.fake_apps = apps;
        d.fake_windows = windows;
        d
    }

    /// Attach an XServer implementation to this driver (useful for injecting FakeX).
    pub fn with_x_server(mut self, x: Arc<dyn XServer>) -> Self {
        let mut base = Self::default_capabilities();
        update_x11_caps(&mut base, x.as_ref());
        let (c1, c2, c3, c4) = build_caps_quad(&base);
        self.caps_all = c1;
        self.caps_no_guard = c2;
        self.caps_no_hotkey = c3;
        self.caps_none = c4;

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
        self
    }

    /// Override the activity clock.
    pub fn with_activity_clock(mut self, clock: Arc<dyn ActivityClock>) -> Self {
        self.activity_clock = clock;
        self
    }

    /// Override the stop latch.
    pub fn with_stop_latch(mut self, latch: Arc<dyn StopLatch>) -> Self {
        self.stop_latch = latch;
        self
    }

    /// Override the hotkey manager.
    pub fn with_hotkey_manager(mut self, mgr: Arc<HotkeyManager>) -> Self {
        self.hotkey_manager = mgr;
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
            let map = match self.atspi_to_x.read() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
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
        let mut issues = Vec::new();
        let ready = if self.fake_source.is_some() {
            true
        } else if let Some(ref conn) = self.connection {
            !conn.is_closed()
        } else {
            match connect_atspi().await {
                Ok(_) => true,
                Err(e) => {
                    issues.push(format!("AT-SPI bus not connected: {e}"));
                    issues.push(
                        "Note: Ensure at-spi2-core is running. Electron/Chromium apps require \
                         '--force-renderer-accessibility' flag for full accessibility tree exposure."
                            .into(),
                    );
                    false
                }
            }
        };

        if let Some(ref x) = self.x_server {
            if !x.is_composite_available().unwrap_or(false) {
                issues.push(
                    "XComposite extension missing: window screenshots degrade to root-window \
                     crop and will fail with CapabilityUnavailable if window is occluded or off-screen \
                     (observe.screenshot=occluded_unsafe)"
                        .into(),
                );
            }
        } else if self.fake_source.is_none() {
            issues.push("X11 server not connected: window capture unavailable".into());
        }

        let guard_status = if self.activity_clock.is_available() {
            "available"
        } else {
            "unavailable"
        };
        issues.push(format!("user_activity_guard: {guard_status}"));

        if !self.hotkey_manager.is_available() {
            let reason = self
                .hotkey_manager
                .last_error()
                .unwrap_or_else(|| "not registered or listener inactive".into());
            issues.push(format!(
                "stop_hotkey: unavailable ({reason}); foreground input disabled"
            ));
        } else {
            issues.push("stop_hotkey: available".into());
        }

        DoctorReport {
            capabilities: self.capabilities().clone(),
            ready,
            issues,
        }
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
            let base_windows: Vec<WindowInfo> = if let Some(target) = app {
                self.fake_windows
                    .iter()
                    .filter(|(app_id, _)| app_id == &target.info.id)
                    .map(|(_, w)| w.clone())
                    .collect()
            } else {
                self.fake_windows.iter().map(|(_, w)| w.clone()).collect()
            };

            if let Some(ref x) = self.x_server {
                let atspi_cands: Vec<AtspiWindowCandidate> = base_windows
                    .iter()
                    .map(|w| AtspiWindowCandidate {
                        path: w.id.clone(),
                        title: w.title.clone(),
                        frame: w.frame,
                        pid: app.and_then(|a| a.info.pid),
                    })
                    .collect();
                return Ok(self.align_and_record(x.as_ref(), &atspi_cands));
            }

            return Ok(base_windows);
        }

        let conn = self
            .connection
            .as_ref()
            .ok_or(DriverError::PermissionMissing {
                permission: "accessibility".into(),
            })?;

        let atspi_candidates = crate::atspi::list_atspi_candidate_windows(conn, app).await?;

        if let Some(ref x) = self.x_server {
            Ok(self.align_and_record(x.as_ref(), &atspi_candidates))
        } else {
            Ok(atspi_candidates
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
                    return Ok(Geometry {
                        frame: Rect {
                            x: geom.x as f64,
                            y: geom.y as f64,
                            w: geom.width as f64,
                            h: geom.height as f64,
                        },
                        scale,
                    });
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
