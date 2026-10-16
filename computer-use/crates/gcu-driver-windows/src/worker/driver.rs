//! Driver adapter; only transport orchestration occurs in this process.
use super::{
    action::Action,
    supervisor::{Supervisor, WorkerConfig},
    wire::*,
};
use async_trait::async_trait;
use gcu_core::{
    driver::*,
    model::{Geometry, NativeHandle, RawTree},
};
use gcu_protocol::{AppInfo, Capabilities, WindowInfo};
use std::time::Duration;

/// Native Driver backed exclusively by a persistent killable child. Reads are
/// bounded by the Driver contract (shorter than the ten-second transport ceiling).
/// Subscriptions are unavailable; core uses its existing polling fallback.
pub struct WorkerDriver {
    supervisor: Supervisor,
    indexing: std::sync::atomic::AtomicBool,
    safety: Option<std::sync::Arc<crate::input::hotkey::SafetyGuard>>,
    safety_caps: std::sync::OnceLock<[Capabilities; 8]>,
}
impl WorkerDriver {
    /// Start a trusted executable and obtain its actual native capabilities.
    pub async fn start(config: WorkerConfig) -> Result<Self> {
        Self::start_with_releaser(config, super::input::default_releaser()).await
    }
    /// Start isolated execution with an independent injectable parent up sender.
    pub async fn start_with_releaser(
        config: WorkerConfig,
        release: std::sync::Arc<dyn crate::input::api::InputReleaser>,
    ) -> Result<Self> {
        Ok(Self {
            supervisor: Supervisor::start_with_releaser(config, release).await?,
            indexing: std::sync::atomic::AtomicBool::new(true),
            safety: None,
            safety_caps: std::sync::OnceLock::new(),
        })
    }
    /// Independent Stop watchdog signal, suitable for trusted Host/hotkey wiring.
    pub fn stop_handle(&self) -> super::StopHandle {
        self.supervisor.stop_handle()
    }
    pub(crate) fn native_capabilities(&self) -> &Capabilities {
        &self.supervisor.capabilities
    }
    /// Bind parent health and Host Stop before any foreground request can queue.
    pub fn with_safety(mut self, guard: std::sync::Arc<crate::input::hotkey::SafetyGuard>) -> Self {
        self.supervisor.attach_safety(guard.clone());
        self.safety = Some(guard);
        self
    }
    /// Terminate/reap the native process without entering its call queue.
    pub async fn shutdown(&self) -> Result<()> {
        self.supervisor.shutdown().await
    }
    async fn rpc(&self, request: Request, timeout: Duration, cancel: &Cancel) -> Result<Reply> {
        self.supervisor
            .execute_bounded(request, cancel, timeout.min(DRIVER_DEADLINE))
            .await
    }
}
fn invalid() -> DriverError {
    DriverError::Native {
        message: "unexpected worker reply".into(),
        maybe_executed: false,
    }
}
#[async_trait]
impl Driver for WorkerDriver {
    fn platform(&self) -> Platform {
        Platform::Windows
    }
    fn capabilities(&self) -> &Capabilities {
        let guard = self
            .safety
            .as_ref()
            .is_some_and(|s| s.activity_clock().available());
        let stop = self
            .safety
            .as_ref()
            .is_some_and(|s| s.available() && !s.is_stopped());
        &self
            .safety_caps
            .get_or_init(|| crate::input::watchdog::capabilities(&self.supervisor.capabilities))
            [usize::from(guard) | (usize::from(stop) << 1)]
    }
    async fn doctor(&self) -> DoctorReport {
        let mut report = match self
            .rpc(Request::Doctor, DRIVER_DEADLINE, &Cancel::default())
            .await
        {
            Ok(Reply::Doctor {
                capabilities: _,
                ready,
                issues,
            }) => DoctorReport {
                capabilities: self.capabilities().clone(),
                ready,
                issues,
            },
            _ => DoctorReport {
                capabilities: self.capabilities().clone(),
                ready: false,
                issues: vec!["worker unavailable".into()],
            },
        };
        if let Some(guard) = &self.safety {
            guard.doctor_issues(&mut report.issues);
        }
        report
    }
    fn discovery_status(&self) -> DiscoveryStatus {
        DiscoveryStatus {
            indexing: self.indexing.load(std::sync::atomic::Ordering::Acquire),
        }
    }
    async fn list_apps(&self) -> Result<Vec<AppInfo>> {
        match self
            .rpc(Request::ListApps, DRIVER_DEADLINE, &Cancel::default())
            .await?
        {
            Reply::Apps { apps, indexing } => {
                self.indexing
                    .store(indexing, std::sync::atomic::Ordering::Release);
                Ok(apps)
            }
            _ => Err(invalid()),
        }
    }
    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef> {
        match self
            .rpc(
                Request::ResolveApp(query.0.clone()),
                DRIVER_DEADLINE,
                &Cancel::default(),
            )
            .await?
        {
            Reply::App(info) => Ok(AppRef { info }),
            _ => Err(invalid()),
        }
    }
    async fn launch_app(&self, app: &AppRef) -> Result<AppRef> {
        match self
            .rpc(
                Request::LaunchApp(app.info.clone()),
                DRIVER_DEADLINE,
                &Cancel::default(),
            )
            .await?
        {
            Reply::App(info) => Ok(AppRef { info }),
            _ => Err(invalid()),
        }
    }
    async fn list_windows(&self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>> {
        match self
            .rpc(
                Request::ListWindows(app.map(|a| a.info.clone())),
                DRIVER_DEADLINE,
                &Cancel::default(),
            )
            .await?
        {
            Reply::Windows(value) => Ok(value),
            _ => Err(invalid()),
        }
    }
    async fn key_window(&self, app: &AppRef) -> Result<WindowInfo> {
        match self
            .rpc(
                Request::KeyWindow(app.info.clone()),
                DRIVER_DEADLINE,
                &Cancel::default(),
            )
            .await?
        {
            Reply::Window(value) => Ok(value),
            _ => Err(invalid()),
        }
    }
    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> Result<RawTree> {
        match self
            .rpc(
                Request::Snapshot(Window::from(win), opts.into()),
                opts.timeout,
                cancel,
            )
            .await?
        {
            Reply::Tree(value) => value.decode(),
            _ => Err(invalid()),
        }
    }
    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture> {
        let options = CaptureOptions {
            timeout_ms: millis(opts.timeout),
            include_cursor: opts.include_cursor,
        };
        match self
            .rpc(
                Request::Capture(Window::from(win), options),
                opts.timeout,
                cancel,
            )
            .await?
        {
            Reply::Capture {
                bytes,
                width,
                height,
                mime,
                geometry,
            } => Ok(Capture {
                bytes,
                width,
                height,
                mime,
                geometry,
            }),
            _ => Err(invalid()),
        }
    }
    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        match self
            .rpc(
                Request::Geometry(Window::from(win)),
                DRIVER_DEADLINE,
                &Cancel::default(),
            )
            .await?
        {
            Reply::Geometry(value) => Ok(value),
            _ => Err(invalid()),
        }
    }
    async fn focused_element(&self, app: &AppRef) -> Result<Option<NativeHandle>> {
        match self
            .rpc(
                Request::Focused(app.info.clone()),
                DRIVER_DEADLINE,
                &Cancel::default(),
            )
            .await?
        {
            Reply::Focused(value) => Ok(value.map(NativeHandle::new)),
            _ => Err(invalid()),
        }
    }
    async fn perform(&self, request: &DriverAction, cancel: &Cancel) -> Result<Delivery> {
        let foreground = crate::input::watchdog::foreground(request);
        match &self.safety {
            Some(guard) => guard.check(foreground)?,
            None if foreground => return Err(DriverError::CapabilityUnavailable),
            None => (),
        }
        match self
            .rpc(
                Request::Perform(Action::encode(request)?),
                DRIVER_DEADLINE,
                cancel,
            )
            .await?
        {
            Reply::Delivery(value) => Ok(value.into()),
            _ => Err(invalid()),
        }
    }
    fn subscribe(&self, _: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        None
    }
    async fn release_all_input(&self) {
        self.supervisor.release_all_input().await;
    }
}
