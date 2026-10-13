//! Linux driver implementation connecting AT-SPI tree to the core `Driver` trait.

use crate::atspi::{
    bus::{AtspiBus, AtspiNodeRef},
    connect_atspi,
    events::{AtspiEventSubscriber, LiveEventSubscriber},
    source::{AtspiSource, FakeNode, FakeSource},
    tree::{build_raw_tree, MAX_NODES_CEILING},
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
use std::{collections::BTreeMap, sync::Arc};

/// Concrete Linux computer-use driver backed by AT-SPI.
pub struct LinuxDriver {
    capabilities: Capabilities,
    connection: Option<zbus::Connection>,
    subscriber: Option<Arc<dyn AtspiEventSubscriber>>,
    // Fake source injected for tests
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
                screenshot: false, // Window capture deferred to P3-02
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

    /// Connect to the live system AT-SPI accessibility bus.
    pub async fn connect() -> std::result::Result<Self, DriverError> {
        let conn = connect_atspi().await?;
        let subscriber = Arc::new(LiveEventSubscriber::new(conn.clone()));
        Ok(Self {
            capabilities: Self::default_capabilities(),
            connection: Some(conn),
            subscriber: Some(subscriber),
            fake_source: None,
            fake_root: None,
            fake_apps: Vec::new(),
            fake_windows: Vec::new(),
        })
    }

    /// Create a driver from an existing D-Bus connection.
    pub fn from_connection(connection: zbus::Connection) -> Self {
        let subscriber = Arc::new(LiveEventSubscriber::new(connection.clone()));
        Self {
            capabilities: Self::default_capabilities(),
            connection: Some(connection),
            subscriber: Some(subscriber),
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
            fake_source: Some(Arc::new(fake_source)),
            fake_root: Some(root),
            fake_apps: apps,
            fake_windows: windows,
        }
    }

    /// Access the underlying D-Bus connection if connected.
    pub fn connection(&self) -> Option<&zbus::Connection> {
        self.connection.as_ref()
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
            if let Some(target) = app {
                return Ok(self
                    .fake_windows
                    .iter()
                    .filter(|(app_id, _)| app_id == &target.info.id)
                    .map(|(_, w)| w.clone())
                    .collect());
            }
            return Ok(self.fake_windows.iter().map(|(_, w)| w.clone()).collect());
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
        let mut windows = Vec::new();

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

                windows.push(WindowInfo {
                    id: w_node.path.clone(),
                    title,
                    frame: extents,
                    scale: 1.0,
                    on_current_desktop: Some(true),
                });
            }
        }

        Ok(windows)
    }

    async fn key_window(&self, app: &AppRef) -> Result<WindowInfo> {
        let windows = self.list_windows(Some(app)).await?;
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
        let root = AtspiNodeRef {
            destination: win.app.info.id.clone(),
            path: win.id.clone(),
        };

        build_raw_tree(&bus, &root, opts, cancel).await
    }

    async fn capture(
        &self,
        _win: &WindowRef,
        _opts: &CaptureOpts,
        _cancel: &Cancel,
    ) -> Result<Capture> {
        Err(DriverError::CapabilityUnavailable)
    }

    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
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
