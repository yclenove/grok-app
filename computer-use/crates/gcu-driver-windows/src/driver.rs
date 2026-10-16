//! Independent discovery queues and the portable Driver implementation.
use crate::{
    api::WinApi,
    desktop::HwndStamp,
    discovery::State,
    discovery_workers::{Discovery, Task},
    identity::identity_for_path,
};
use gcu_core::{
    driver::*,
    model::{Geometry, NativeHandle, RawTree},
};
use gcu_protocol::{capabilities::*, AppInfo, WindowInfo};
use std::{
    collections::BTreeMap,
    sync::{mpsc, Arc},
    time::Duration,
};
use tokio::sync::oneshot;

/// Windows discovery, optional UIA observation/actions and optional capture. Portable sources inject OS data.
/// COM/OS calls use dedicated workers with bounded caller deadlines. Catalog
/// enrichment, Start Menu scanning, running resolution and installed lookup have
/// independent queues. Drop disconnects workers and stops watching.
pub struct WindowsDriver {
    isolated: Option<crate::worker::WorkerDriver>,
    worker: mpsc::SyncSender<Task>,
    discovery: Option<Discovery>,
    pub(crate) caps: Capabilities,
    pub(crate) safety_caps: std::sync::OnceLock<[Capabilities; 8]>,
    pub(crate) safety: Option<Arc<crate::input::hotkey::SafetyGuard>>,
    pub(crate) overlay: Option<Arc<crate::overlay::OverlayExclusion>>,
    child: bool,
    diagnostic: bool,
    input: Option<Arc<crate::input::InputBackend>>,
    observer: Option<crate::uia::observe::Observer>,
    pub(crate) capture_backend: Option<Arc<crate::capture::CaptureBackend>>,
}
impl WindowsDriver {
    /// Start independent native queues after refusing elevated/noninteractive
    /// engines. Each startup probe has a DRIVER_DEADLINE timeout; no auto-elevation.
    /// Lists read in-memory hints after an independent live engine-token gate;
    /// they never wait for indexing. Resolution still verifies fresh provenance.
    pub async fn with_api(api: Arc<dyn WinApi>) -> Result<Self> {
        let (discovery, worker) = Discovery::start(api).await?;
        Ok(Self {
            isolated: None,
            worker,
            discovery: Some(discovery),
            caps: discovery_capabilities(),
            safety_caps: std::sync::OnceLock::new(),
            safety: None,
            overlay: None,
            child: false,
            diagnostic: false,
            input: None,
            observer: None,
            capture_backend: None,
        })
    }
    /// Start the production Windows adapter in the default isolated mode.
    /// The executable must be `gcu`, which implements the private worker command.
    /// Starts stopped until a trusted Host supplies its pre-restored latch.
    #[cfg(windows)]
    pub async fn new() -> Result<Self> {
        let executable = std::env::current_exe().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        Self::with_exec_mode(
            crate::worker::ExecMode::default(),
            crate::worker::WorkerConfig::new(executable),
        )
        .await
    }
    /// Select isolated production execution or explicit in-process diagnostics.
    /// Only Worker can terminate wedged action FFI. Parent FFI is limited to
    /// safety monitoring, overlay exclusion, up-only emergency cleanup and
    /// conditional clipboard recovery.
    #[cfg(windows)]
    pub async fn with_exec_mode(
        mode: crate::worker::ExecMode,
        config: crate::worker::WorkerConfig,
    ) -> Result<Self> {
        if mode == crate::worker::ExecMode::InProcess {
            return Self::new_in_process().await;
        }
        Self::monitored(
            config,
            Default::default(),
            Arc::new(crate::input::hotkey::UnboundStop),
        )
        .await
    }
    /// Attach a portable supervisor. Startup probes the child before enabling
    /// capabilities; there is no discovery/UIA/capture thread in this process.
    pub async fn isolated(config: crate::worker::WorkerConfig) -> Result<Self> {
        let isolated = crate::worker::WorkerDriver::start(config).await?;
        Ok(Self::from_isolated(isolated))
    }
    /// Inject the parent's independent up-only sender for portable worker tests.
    pub async fn isolated_with_releaser(
        config: crate::worker::WorkerConfig,
        release: Arc<dyn crate::input::api::InputReleaser>,
    ) -> Result<Self> {
        let isolated = crate::worker::WorkerDriver::start_with_releaser(config, release).await?;
        Ok(Self::from_isolated(isolated))
    }
    fn from_isolated(isolated: crate::worker::WorkerDriver) -> Self {
        let caps = isolated.native_capabilities().clone();
        let (worker, _) = mpsc::sync_channel(1);
        Self {
            isolated: Some(isolated),
            input: None,
            worker,
            caps,
            safety_caps: std::sync::OnceLock::new(),
            safety: None,
            overlay: None,
            child: false,
            diagnostic: false,
            discovery: None,
            observer: None,
            capture_backend: None,
        }
    }
    /// Start native threads inside this process. For the worker executable or
    /// explicit diagnostics only; caller timeouts cannot terminate blocked FFI.
    #[cfg(windows)]
    pub async fn new_in_process() -> Result<Self> {
        Self::native(Arc::new(crate::RealWinApi::new()?)).await
    }
    /// Initialize the child adapter with the launcher's process-instance namespace.
    /// Called only after the private startup frame; window stamps survive restart.
    #[cfg(windows)]
    pub async fn new_worker(namespace: &str) -> Result<Self> {
        let mut driver = Self::native(Arc::new(crate::RealWinApi::for_worker(namespace)?)).await?;
        driver.child = true;
        Ok(driver)
    }
    /// Worker startup with the parent's press-before-send acknowledgement path.
    #[cfg(windows)]
    pub async fn new_worker_with_input(
        namespace: &str,
        tracker: Arc<dyn crate::input::api::InputTracker>,
    ) -> Result<Self> {
        let mut driver = Self::new_worker(namespace).await?;
        let input = driver
            .input
            .take()
            .ok_or(DriverError::CapabilityUnavailable)?;
        driver.input = Some(Arc::new(
            Arc::try_unwrap(input)
                .map_err(|_| DriverError::CapabilityUnavailable)?
                .with_tracker(tracker),
        ));
        Ok(driver)
    }
    #[cfg(windows)]
    async fn native(api: Arc<dyn WinApi>) -> Result<Self> {
        let driver = Self::with_api(api.clone()).await?;
        let uia = Arc::new(crate::uia::RealUia::start().await?);
        let wgc = Arc::new(crate::capture::WgcCapture::new().await?);
        let print = Arc::new(crate::capture::PrintWindowCapture::new().await?);
        let input = Arc::new(crate::input::RealInput::start(api, uia.clone()).await?);
        let mut driver = driver
            .with_input(Arc::new(
                crate::input::InputBackend::new(input)
                    .with_clipboard(Arc::new(crate::input::clipboard::NativeClipboard::default())),
            ))
            .attach_uia(uia.clone(), uia)
            .with_capture(Arc::new(crate::capture::CaptureBackend::new(wgc, print)));
        driver.diagnostic = true;
        Ok(driver)
    }
    /// Start discovery with portable/native UIA sources. Startup validates the
    /// engine before enabling tree/events/actions; capture and input injection remain unavailable.
    pub async fn with_uia(
        api: Arc<dyn WinApi>,
        source: Arc<dyn crate::uia::source::UiaSource>,
        events: Arc<dyn crate::uia::events::UiaEventSource>,
    ) -> Result<Self> {
        Ok(Self::with_api(api).await?.attach_uia(source, events))
    }
    fn attach_uia(
        mut self,
        source: Arc<dyn crate::uia::source::UiaSource>,
        events: Arc<dyn crate::uia::events::UiaEventSource>,
    ) -> Self {
        self.observer = Some(crate::uia::observe::Observer::new(source, events));
        self.caps.observe.tree = true;
        self.caps.observe.events = true;
        self.caps.delivery.accessibility = true;
        self.caps.background_operation = BackgroundOperation::Partial;
        self.caps.limits.max_tree_nodes = crate::uia::tree::NODE_CAP as u32;
        self.caps.version = "Windows 10 1903+; discovery and UIA semantic actions".into();
        self.safety_caps = std::sync::OnceLock::new();
        self
    }
    /// Enable complete targeted/foreground input. Native or fake APIs preserve
    /// route ceilings; foreground requires core's explicit permission.
    pub fn with_input(mut self, input: Arc<crate::input::InputBackend>) -> Self {
        self.child |= input.parent_controlled();
        self.input = Some(input);
        self.caps.delivery.targeted_pointer = true;
        self.caps.delivery.targeted_keys = true;
        self.caps.delivery.foreground = ForegroundCapability::Policy;
        self.caps.background_operation = BackgroundOperation::Partial;
        self.caps.limits.type_text_max = crate::input::text::TEXT_MAX_UNITS as u32;
        self.safety_caps = std::sync::OnceLock::new();
        if let Some(guard) = self.safety.clone() {
            return self.with_safety(guard);
        }
        self
    }
    /// Attach parent monitoring using a Host-owned, pre-restored Stop latch.
    /// Isolated delivery rechecks this guard at the parent input-ack barrier.
    /// In-process native diagnostics keep foreground disabled (not killable).
    pub fn with_safety(mut self, guard: Arc<crate::input::hotkey::SafetyGuard>) -> Self {
        if let Some(worker) = self.isolated.take() {
            self.isolated = Some(worker.with_safety(guard.clone()));
        } else if let Some(input) = &self.input {
            input.set_safety(guard.clone());
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                let input = input.clone();
                guard.bind_stop(Arc::new(move || {
                    let input = input.clone();
                    runtime.spawn(async move {
                        input.release_all().await;
                    });
                }));
            } else {
                guard.disable("independent input release runtime unavailable");
            }
        }
        self.safety = Some(guard);
        self.safety_caps = std::sync::OnceLock::new();
        self
    }
    /// Attach an exact display-affinity probe; failure stays visible in capabilities.
    pub fn with_overlay_exclusion(
        mut self,
        exclusion: Arc<crate::overlay::OverlayExclusion>,
    ) -> Self {
        self.overlay = Some(exclusion);
        self.safety_caps = std::sync::OnceLock::new();
        self
    }
    /// Production monitored worker. Host restores/persists Stop and supplies its
    /// latch; no model-facing resume API exists. Guard config is trusted policy.
    #[cfg(windows)]
    pub async fn monitored(
        config: crate::worker::WorkerConfig,
        hotkey: crate::input::hotkey::HotkeyConfig,
        latch: Arc<dyn crate::input::hotkey::StopLatch>,
    ) -> Result<Self> {
        let guard = crate::input::hotkey::SafetyGuard::start(
            Arc::new(crate::input::RealSafetyApi),
            hotkey,
            latch,
        )
        .await;
        let overlay =
            crate::overlay::OverlayExclusion::probe(Arc::new(crate::overlay::RealOverlayApi)).await;
        Ok(Self::isolated(config)
            .await?
            .with_safety(guard)
            .with_overlay_exclusion(overlay))
    }
    pub(crate) async fn call<T: Send + 'static>(
        &self,
        timeout: Duration,
        cancel: Option<&Cancel>,
        mutation: bool,
        task: impl FnOnce(&mut State) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        Self::call_on(&self.worker, timeout, cancel, mutation, task).await
    }
    pub(super) async fn call_on<T: Send + 'static>(
        worker: &mpsc::SyncSender<Task>,
        timeout: Duration,
        cancel: Option<&Cancel>,
        mutation: bool,
        task: impl FnOnce(&mut State) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        if cancel.is_some_and(Cancel::is_cancelled) {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        let (tx, rx) = oneshot::channel();
        worker
            .try_send(Box::new(move |state| {
                // Timed-out/cancelled queued calls must never begin work later.
                if !tx.is_closed() {
                    let result = task(state);
                    let _ = tx.send(result);
                }
            }))
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
        let wait = tokio::time::timeout(timeout.min(DRIVER_DEADLINE), rx);
        let result = if let Some(cancel) = cancel {
            tokio::select! { biased;
                _ = cancel.cancelled() => return Err(DriverError::Cancelled { maybe_executed: mutation }),
                result = wait => result,
            }
        } else {
            wait.await
        };
        result
            .map_err(|_| DriverError::Timeout {
                maybe_executed: mutation,
            })?
            .map_err(|_| DriverError::Crashed {
                maybe_executed: mutation,
            })?
    }
}
fn discovery_capabilities() -> Capabilities {
    Capabilities {
        platform: Platform::Windows,
        version: "Windows 10 1903+; discovery only (P2-01)".into(),
        observe: ObserveCapabilities {
            tree: false,
            screenshot: false,
            events: false,
        },
        delivery: DeliveryCapabilities {
            accessibility: false,
            targeted_pointer: false,
            targeted_keys: false,
            foreground: ForegroundCapability::Unavailable,
        },
        background_operation: BackgroundOperation::None,
        user_activity_guard: false,
        overlay_excluded_from_capture: false,
        private_apis: BTreeMap::new(),
        limits: CapabilityLimits {
            max_tree_nodes: 0,
            max_image_edge: 0,
            type_text_max: 0,
        },
    }
}
/// Private element binding for the common gate; P2-02 will attach UIA handles.
#[derive(Clone, Debug)]
pub struct WindowsElement {
    /// Exact stamped window owning the element.
    pub window: WindowRef,
}

#[path = "driver_impl.rs"]
mod implementation;
