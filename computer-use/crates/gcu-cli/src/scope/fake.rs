//! Synthetic S3 evidence. Never assembled with a native driver. The allow-set
//! comes only from control; this table merely describes the fake desktop.
use async_trait::async_trait;
use gcu_core::{
    driver::*,
    model::{Geometry, NativeHandle, RawNode, RawTree},
    scope::*,
};
use gcu_driver_fake::FakeDriver;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(super) struct FakeEvidence {
    pub(super) driver: FakeDriver,
    apps: Arc<Vec<(AppRef, ProcessInstance)>>,
    windows: Arc<Vec<(WindowRef, WindowBinding)>>,
    handles: Arc<Mutex<Vec<(NativeHandle, WindowBinding)>>>,
}
impl FakeEvidence {
    pub(super) async fn new(driver: FakeDriver) -> Result<Self> {
        let mut apps = Vec::new();
        let mut windows = Vec::new();
        for info in driver.list_apps().await? {
            let instance = ProcessInstance::new(
                info.pid.ok_or(DriverError::ScopeDenied)?,
                OsStartTime::new(1).map_err(|_| DriverError::ScopeDenied)?,
                BootIdentity::new("s3-fake-boot").map_err(|_| DriverError::ScopeDenied)?,
            )
            .map_err(|_| DriverError::ScopeDenied)?;
            let app = AppRef { info };
            for win in driver.list_windows(Some(&app)).await? {
                let binding = WindowBinding::new(&win.id, 1, instance.clone())
                    .map_err(|_| DriverError::ScopeDenied)?;
                windows.push((
                    WindowRef {
                        app: app.clone(),
                        id: win.id,
                    },
                    binding,
                ));
            }
            apps.push((app, instance));
        }
        Ok(Self {
            driver,
            apps: Arc::new(apps),
            windows: Arc::new(windows),
            handles: Arc::new(Mutex::new(Vec::new())),
        })
    }
    fn register(&self, node: &RawNode, binding: &WindowBinding) {
        let mut handles = self.handles.lock().unwrap_or_else(|e| e.into_inner());
        fn walk(
            node: &RawNode,
            binding: &WindowBinding,
            handles: &mut Vec<(NativeHandle, WindowBinding)>,
        ) {
            if !handles.iter().any(|(h, _)| h.ptr_eq(&node.handle)) {
                handles.push((node.handle.clone(), binding.clone()));
            }
            for child in &node.children {
                walk(child, binding, handles);
            }
        }
        walk(node, binding, &mut handles);
    }
}
impl InstanceEvidence for FakeEvidence {
    fn app_instance(&self, app: &AppRef) -> Option<ProcessInstance> {
        self.apps
            .iter()
            .find(|(a, _)| a == app)
            .map(|(_, p)| p.clone())
    }
    fn window_binding(&self, win: &WindowRef) -> Option<WindowBinding> {
        self.windows
            .iter()
            .find(|(w, _)| w == win)
            .map(|(_, b)| b.clone())
    }
    fn element_binding(&self, handle: &NativeHandle) -> Option<ElementBinding> {
        self.handles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(h, _)| h.ptr_eq(handle))
            .map(|(_, b)| ElementBinding::new(b.clone()))
    }
}
#[async_trait]
impl Driver for FakeEvidence {
    fn platform(&self) -> Platform {
        self.driver.platform()
    }
    fn capabilities(&self) -> &Capabilities {
        self.driver.capabilities()
    }
    fn discovery_status(&self) -> DiscoveryStatus {
        self.driver.discovery_status()
    }
    async fn doctor(&self) -> DoctorReport {
        self.driver.doctor().await
    }
    async fn list_apps(&self) -> Result<Vec<AppInfo>> {
        self.driver.list_apps().await
    }
    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef> {
        self.driver.resolve_app(query).await
    }
    async fn launch_app(&self, app: &AppRef) -> Result<AppRef> {
        self.driver.launch_app(app).await
    }
    async fn list_windows(&self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>> {
        self.driver.list_windows(app).await
    }
    async fn key_window(&self, app: &AppRef) -> Result<WindowInfo> {
        self.driver.key_window(app).await
    }
    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> Result<RawTree> {
        let tree = self.driver.snapshot_tree(win, opts, cancel).await?;
        if let (Some(root), Some(binding)) = (&tree.root, self.window_binding(win)) {
            self.register(root, &binding);
        }
        Ok(tree)
    }
    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture> {
        self.driver.capture(win, opts, cancel).await
    }
    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        self.driver.window_geometry(win).await
    }
    async fn focused_element(&self, app: &AppRef) -> Result<Option<NativeHandle>> {
        // Ownership is recorded from actual fake tree reads. Unknown focus
        // handles have no evidence and are refused rather than assigned to an
        // arbitrary window in this app.
        self.driver.focused_element(app).await
    }

    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> Result<Delivery> {
        self.driver.perform(req, cancel).await
    }
    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        self.driver.subscribe(win)
    }
    async fn release_all_input(&self) {
        self.driver.release_all_input().await;
    }
}
