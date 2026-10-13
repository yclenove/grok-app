//! Linux driver implementation connecting AT-SPI tree to the core `Driver` trait.

use crate::atspi::{
    bus::{AtspiBus, AtspiNodeRef},
    connect_atspi,
    events::{AtspiEventSubscriber, LiveEventSubscriber},
    source::{AtspiSource, FakeNode, FakeSource},
    tree::{build_raw_tree, MAX_NODES_CEILING},
};
use crate::x11::{
    align_windows, capture_window, list_x11_windows, AtspiWindowCandidate, RealX, XServer,
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
        ObserveCapabilities, Platform, PrivateApiStatus,
    },
    AppInfo, Capabilities, WindowInfo,
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, RwLock},
};

/// Concrete Linux computer-use driver backed by AT-SPI and X11.
pub struct LinuxDriver {
    capabilities: Capabilities,
    connection: Option<zbus::Connection>,
    subscriber: Option<Arc<dyn AtspiEventSubscriber>>,
    x_server: Option<Arc<dyn XServer>>,
    atspi_to_x: Arc<RwLock<HashMap<String, u32>>>,
    x_to_atspi: Arc<RwLock<HashMap<u32, String>>>,
    fake_source: Option<Arc<FakeSource>>,
    fake_root: Option<FakeNode>,
    fake_apps: Vec<AppInfo>,
    fake_windows: Vec<(String, WindowInfo)>,
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
        let subscriber = Arc::new(LiveEventSubscriber::new(conn.clone()));
        let x_server: Option<Arc<dyn XServer>> = match RealX::connect() {
            Ok(x) => Some(Arc::new(x)),
            Err(e) => {
                tracing::warn!("X11 connection failed during LinuxDriver::connect: {e}");
                None
            }
        };

        let mut capabilities = Self::default_capabilities();
        if let Some(ref x) = x_server {
            if x.is_composite_available().unwrap_or(false) {
                capabilities
                    .private_apis
                    .insert("x11_composite".into(), PrivateApiStatus::Available);
            } else {
                capabilities
                    .private_apis
                    .insert("x11_composite".into(), PrivateApiStatus::Missing);
            }
        }

        Ok(Self {
            capabilities,
            connection: Some(conn),
            subscriber: Some(subscriber),
            x_server,
            atspi_to_x: Arc::new(RwLock::new(HashMap::new())),
            x_to_atspi: Arc::new(RwLock::new(HashMap::new())),
            fake_source: None,
            fake_root: None,
            fake_apps: Vec::new(),
            fake_windows: Vec::new(),
        })
    }

    /// Create a driver from an existing D-Bus connection.
    pub fn from_connection(connection: zbus::Connection) -> Self {
        let subscriber = Arc::new(LiveEventSubscriber::new(connection.clone()));
        let x_server: Option<Arc<dyn XServer>> = match RealX::connect() {
            Ok(x) => Some(Arc::new(x)),
            Err(e) => {
                tracing::warn!("X11 connection failed during LinuxDriver::from_connection: {e}");
                None
            }
        };

        let mut capabilities = Self::default_capabilities();
        if let Some(ref x) = x_server {
            if x.is_composite_available().unwrap_or(false) {
                capabilities
                    .private_apis
                    .insert("x11_composite".into(), PrivateApiStatus::Available);
            } else {
                capabilities
                    .private_apis
                    .insert("x11_composite".into(), PrivateApiStatus::Missing);
            }
        }

        Self {
            capabilities,
            connection: Some(connection),
            subscriber: Some(subscriber),
            x_server,
            atspi_to_x: Arc::new(RwLock::new(HashMap::new())),
            x_to_atspi: Arc::new(RwLock::new(HashMap::new())),
            fake_source: None,
            fake_root: None,
            fake_apps: Vec::new(),
            fake_windows: Vec::new(),
        }
    }

    /// Create a test driver backed by an in-memory `FakeSource`.
    pub fn new_fake(
        fake_source: FakeSource,
        root: FakeNode,
        apps: Vec<AppInfo>,
        windows: Vec<(String, WindowInfo)>,
    ) -> Self {
        Self {
            capabilities: Self::default_capabilities(),
            connection: None,
            subscriber: None,
            x_server: None,
            atspi_to_x: Arc::new(RwLock::new(HashMap::new())),
            x_to_atspi: Arc::new(RwLock::new(HashMap::new())),
            fake_source: Some(Arc::new(fake_source)),
            fake_root: Some(root),
            fake_apps: apps,
            fake_windows: windows,
        }
    }

    /// Attach an XServer implementation to this driver (useful for injecting FakeX).
    pub fn with_x_server(mut self, x: Arc<dyn XServer>) -> Self {
        if x.is_composite_available().unwrap_or(false) {
            self.capabilities
                .private_apis
                .insert("x11_composite".into(), PrivateApiStatus::Available);
        } else {
            self.capabilities
                .private_apis
                .insert("x11_composite".into(), PrivateApiStatus::Missing);
        }
        self.x_server = Some(x);
        self
    }

    /// Access the underlying D-Bus connection if connected.
    pub fn connection(&self) -> Option<&zbus::Connection> {
        self.connection.as_ref()
    }

    /// Access the underlying XServer if available.
    pub fn x_server(&self) -> Option<&Arc<dyn XServer>> {
        self.x_server.as_ref()
    }
}

#[async_trait]
impl Driver for LinuxDriver {
    fn platform(&self) -> Platform {
        Platform::Linux
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
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

        DoctorReport {
            capabilities: self.capabilities.clone(),
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

        let bus = AtspiBus::new(conn.clone());
        let root = AtspiNodeRef {
            destination: "org.a11y.atspi.Registry".into(),
            path: "/org/a11y/atspi/accessible/root".into(),
        };

        let app_nodes = bus.children(&root).await.unwrap_or_default();
        let mut apps = Vec::new();
        for node in app_nodes {
            let name = bus
                .name(&node)
                .await
                .unwrap_or_default()
                .unwrap_or_default();
            if !name.is_empty() {
                apps.push(AppInfo {
                    id: node.destination.clone(),
                    name,
                    pid: None,
                    identity: None,
                });
            }
        }
        Ok(apps)
    }

    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef> {
        let query_str = query.0.to_ascii_lowercase();
        let apps = self.list_apps().await?;
        let matches: Vec<AppInfo> = apps
            .into_iter()
            .filter(|app| {
                app.name.to_ascii_lowercase().contains(&query_str)
                    || app.id.to_ascii_lowercase().contains(&query_str)
            })
            .collect();

        if matches.is_empty() {
            return Err(DriverError::AppNotFound);
        }
        if matches.len() > 1 {
            return Err(DriverError::AmbiguousApp);
        }

        let matched = matches.into_iter().next().ok_or(DriverError::AppNotFound)?;
        Ok(AppRef { info: matched })
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
                let x_cands = list_x11_windows(x.as_ref()).unwrap_or_default();
                let atspi_cands: Vec<AtspiWindowCandidate> = base_windows
                    .iter()
                    .map(|w| AtspiWindowCandidate {
                        path: w.id.clone(),
                        title: w.title.clone(),
                        frame: w.frame,
                        pid: app.and_then(|a| a.info.pid),
                    })
                    .collect();
                let scale = x.root_geometry().map(|r| r.scale).unwrap_or(1.0);
                let aligned = align_windows(&atspi_cands, &x_cands, scale);
                if let Ok(mut map) = self.atspi_to_x.write() {
                    map.extend(aligned.atspi_to_x);
                }
                if let Ok(mut map) = self.x_to_atspi.write() {
                    map.extend(aligned.x_to_atspi);
                }
                return Ok(aligned.windows);
            }

            return Ok(base_windows);
        }

        let conn = self
            .connection
            .as_ref()
            .ok_or(DriverError::PermissionMissing {
                permission: "accessibility".into(),
            })?;

        let bus = AtspiBus::new(conn.clone());
        let root = AtspiNodeRef {
            destination: "org.a11y.atspi.Registry".into(),
            path: "/org/a11y/atspi/accessible/root".into(),
        };

        let app_nodes = bus.children(&root).await.unwrap_or_default();
        let mut atspi_candidates = Vec::new();

        for a_node in app_nodes {
            let app_name = bus
                .name(&a_node)
                .await
                .unwrap_or_default()
                .unwrap_or_default();
            if let Some(target) = app {
                if a_node.destination != target.info.id
                    && !app_name.eq_ignore_ascii_case(&target.info.name)
                {
                    continue;
                }
            }

            let win_nodes = bus.children(&a_node).await.unwrap_or_default();
            for w_node in win_nodes {
                let title = bus
                    .name(&w_node)
                    .await
                    .unwrap_or_default()
                    .unwrap_or_default();
                let extents = bus
                    .extents(&w_node)
                    .await
                    .unwrap_or_default()
                    .unwrap_or(Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 1280.0,
                        h: 860.0,
                    });

                atspi_candidates.push(AtspiWindowCandidate {
                    path: w_node.path.clone(),
                    title,
                    frame: extents,
                    pid: app.and_then(|a| a.info.pid),
                });
            }
        }

        if let Some(ref x) = self.x_server {
            let x_cands = list_x11_windows(x.as_ref()).unwrap_or_default();
            let scale = x.root_geometry().map(|r| r.scale).unwrap_or(1.0);
            let aligned = align_windows(&atspi_candidates, &x_cands, scale);
            if let Ok(mut map) = self.atspi_to_x.write() {
                map.extend(aligned.atspi_to_x);
            }
            if let Ok(mut map) = self.x_to_atspi.write() {
                map.extend(aligned.x_to_atspi);
            }
            Ok(aligned.windows)
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

        let xid = if let Ok(id) = win.id.parse::<u32>() {
            id
        } else {
            let map = match self.atspi_to_x.read() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            map.get(&win.id).copied().ok_or(DriverError::WindowGone)?
        };

        capture_window(x.as_ref(), xid)
    }

    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        if let Some(ref x) = self.x_server {
            let xid = if let Ok(id) = win.id.parse::<u32>() {
                Some(id)
            } else {
                let map = match self.atspi_to_x.read() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                map.get(&win.id).copied()
            };

            if let Some(id) = xid {
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

    async fn perform(&self, _req: &DriverAction, _cancel: &Cancel) -> Result<Delivery> {
        Err(DriverError::CapabilityUnavailable)
    }

    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        self.subscriber.as_ref().and_then(|s| s.subscribe(&win.id))
    }

    async fn release_all_input(&self) {}
}
