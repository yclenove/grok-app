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
    supervisor: std::sync::Arc<Supervisor>,
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
            supervisor: std::sync::Arc::new(
                Supervisor::start_with_releaser(config, release).await?,
            ),
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
    /// Preserve the outer WindowsDriver's original deadline through preparation.
    pub(crate) async fn perform_until(
        &self,
        request: &DriverAction,
        cancel: &Cancel,
        deadline: tokio::time::Instant,
    ) -> Result<Delivery> {
        let foreground = crate::input::watchdog::foreground(request);
        if let Some(guard) = &self.safety {
            guard.check(false)?;
        }
        let action = Action::encode(request)?;
        if foreground {
            // The parent proof itself injects input. Use existing read-only RPCs
            // to validate the exact live target before permitting that proof;
            // the child still repeats its gate immediately before action input.
            self.preflight_foreground(action.window(), cancel, deadline)
                .await?;
        }
        let mut lease = if foreground {
            Some(
                self.safety
                    .as_ref()
                    .ok_or(DriverError::CapabilityUnavailable)?
                    .begin_foreground(cancel, deadline)
                    .await?,
            )
        } else {
            None
        };
        if let Some(lease) = &mut lease {
            let supervisor = self.supervisor.clone();
            lease.recover_with(move || {
                Box::pin(async move { supervisor.release_all_input().await })
            });
        }
        let result = async {
            // Mutation RPC cannot begin until the target gate and proof succeed.
            match self
                .rpc(
                    Request::Perform(action),
                    (if foreground {
                        deadline - Duration::from_millis(400)
                    } else {
                        deadline
                    })
                    .saturating_duration_since(tokio::time::Instant::now()),
                    cancel,
                )
                .await?
            {
                Reply::Delivery(value) => Ok(value.into()),
                _ => Err(invalid()),
            }
        }
        .await;
        if let Some(lease) = lease {
            if result.is_err() {
                self.supervisor.release_all_input().await;
            }
            // A successful child reply follows its InputBackend's up-only cleanup.
            lease.finish();
            if result.is_ok() && self.safety.as_ref().is_some_and(|s| s.is_stopped()) {
                return Err(DriverError::Cancelled {
                    maybe_executed: true,
                });
            }
        }
        result
    }
    async fn preflight_foreground(
        &self,
        window: &Window,
        cancel: &Cancel,
        deadline: tokio::time::Instant,
    ) -> Result<()> {
        // Geometry performs the common engine/token, HWND/process identity,
        // UIAccess, image provenance and secure-desktop gate. It cannot activate
        // or inject. Preserve these errors before checking foreground capability.
        match self
            .rpc(
                Request::Geometry(window.clone()),
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                cancel,
            )
            .await?
        {
            Reply::Geometry(_) => (),
            _ => return Err(invalid()),
        }
        // Foreground additionally requires a positively identified current
        // virtual desktop. Match only the original stamped ID, never a substitute.
        match self
            .rpc(
                Request::ListWindows(Some(window.app.clone())),
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                cancel,
            )
            .await?
        {
            Reply::Windows(windows) => {
                let current = windows
                    .iter()
                    .find(|w| w.id == window.id)
                    .ok_or(DriverError::WindowGone)?;
                if current.on_current_desktop != Some(true) {
                    return Err(DriverError::CapabilityUnavailable);
                }
                Ok(())
            }
            _ => Err(invalid()),
        }
    }
    async fn rpc(&self, request: Request, timeout: Duration, cancel: &Cancel) -> Result<Reply> {
        // Include the whole bounded transport call in parent diagnostics. A
        // multi-call Router case can exhaust its total budget even when every
        // native call returned before DRIVER_DEADLINE.
        let stage = match &request {
            Request::Doctor => "rpc_doctor",
            Request::ListApps => "rpc_list_apps",
            Request::ResolveApp(_) => "rpc_resolve_app",
            Request::LaunchApp(_) => "rpc_launch_app",
            Request::ListWindows(_) => "rpc_list_windows",
            Request::KeyWindow(_) => "rpc_key_window",
            Request::Snapshot(..) => "rpc_snapshot",
            Request::Capture(..) => "rpc_capture",
            Request::Geometry(_) => "rpc_geometry",
            Request::Focused(_) => "rpc_focused",
            Request::Perform(_) => "rpc_perform",
        };
        let _timing = crate::timing::Timing::start(stage);
        let result = self
            .supervisor
            .execute_bounded(request, cancel, timeout.min(DRIVER_DEADLINE))
            .await;
        if let Err(error) = &result {
            tracing::warn!(target: "gcu_windows_timing", stage, phase = "failed", error = %error);
        }
        result
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
        let guard = self.safety.as_ref().is_some_and(|s| s.activity_available());
        let stop = self.safety.as_ref().is_some_and(|s| s.transport_healthy());
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
        self.perform_until(
            request,
            cancel,
            tokio::time::Instant::now() + DRIVER_DEADLINE,
        )
        .await
    }
    fn subscribe(&self, _: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        None
    }
    async fn release_all_input(&self) {
        self.supervisor.release_all_input().await;
    }
}
