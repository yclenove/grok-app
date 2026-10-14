//! Serial discovery worker and the portable Driver implementation.
use crate::{
    api::WinApi, apps::AppIndex, desktop::HwndStamp, discovery::State, gate::check_engine,
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

type Task = Box<dyn FnOnce(&mut State) + Send>;
/// Windows discovery, optional UIA observation/actions and optional capture. Portable sources inject OS data.
/// COM/OS calls use dedicated workers with bounded caller deadlines; indexing uses a separate
/// thread. Drop disconnects workers and stops watching.
pub struct WindowsDriver {
    isolated: Option<crate::worker::WorkerDriver>,
    worker: mpsc::SyncSender<Task>,
    indexing: Arc<std::sync::atomic::AtomicBool>,
    refreshing: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) caps: Capabilities,
    input: Option<Arc<crate::input::InputBackend>>,
    listed: std::sync::atomic::AtomicBool,
    observer: Option<crate::uia::observe::Observer>,
    pub(crate) capture_backend: Option<Arc<crate::capture::CaptureBackend>>,
}
impl WindowsDriver {
    /// Start a dedicated worker and refuse elevated/noninteractive engines before
    /// creating the index. Startup is bounded by NATIVE_TIMEOUT; no auto-elevation.
    pub async fn with_api(api: Arc<dyn WinApi>) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Task>(32);
        let (ready_tx, ready_rx) = oneshot::channel();
        let indexing = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let flag = indexing.clone();
        std::thread::Builder::new()
            .name("gcu-win-discovery".into())
            .spawn(move || {
                let result = (|| {
                    check_engine(&api.engine_security()?)?;
                    Ok(State {
                        _thread_context: api.enter_thread()?,
                        index: AppIndex::start(api.clone())?,
                        api,
                        entries: BTreeMap::new(),
                        issues: Vec::new(),
                        windows: BTreeMap::new(),
                    })
                })();
                match result {
                    Ok(mut state) => {
                        if ready_tx.send(Ok(())).is_err() {
                            return;
                        }
                        while let Ok(task) =
                            rx.recv_timeout(Duration::from_millis(50))
                                .or_else(|e| match e {
                                    mpsc::RecvTimeoutError::Timeout => {
                                        Ok(Box::new(|_: &mut State| {}) as Task)
                                    }
                                    mpsc::RecvTimeoutError::Disconnected => Err(e),
                                })
                        {
                            task(&mut state);
                            flag.store(
                                state.index.indexing(),
                                std::sync::atomic::Ordering::Release,
                            );
                        }
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                    }
                }
            })
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
        tokio::time::timeout(DRIVER_DEADLINE, ready_rx)
            .await
            .map_err(|_| DriverError::Timeout {
                maybe_executed: false,
            })?
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })??;
        Ok(Self {
            isolated: None,
            worker: tx,
            indexing,
            refreshing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            caps: discovery_capabilities(),
            listed: std::sync::atomic::AtomicBool::new(false),
            input: None,
            observer: None,
            capture_backend: None,
        })
    }
    /// Start the production Windows adapter in the default isolated mode.
    /// The executable must be `gcu`, which implements the private worker command.
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
    /// Only Worker can terminate wedged FFI. No native calls run in its parent.
    #[cfg(windows)]
    pub async fn with_exec_mode(
        mode: crate::worker::ExecMode,
        config: crate::worker::WorkerConfig,
    ) -> Result<Self> {
        if mode == crate::worker::ExecMode::InProcess {
            return Self::new_in_process().await;
        }
        Self::isolated(config).await
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
        let caps = isolated.capabilities().clone();
        let (worker, _) = mpsc::sync_channel(1);
        Self {
            isolated: Some(isolated),
            input: None,
            worker,
            caps,
            indexing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            refreshing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            listed: std::sync::atomic::AtomicBool::new(false),
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
        Self::native(Arc::new(crate::RealWinApi::for_worker(namespace)?)).await
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
        Ok(driver
            .with_input(Arc::new(crate::input::InputBackend::new(input)))
            .attach_uia(uia.clone(), uia)
            .with_capture(Arc::new(crate::capture::CaptureBackend::new(wgc, print))))
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
        self
    }
    /// Enable complete targeted/foreground input. Native or fake APIs preserve
    /// route ceilings; foreground requires core's explicit permission.
    pub fn with_input(mut self, input: Arc<crate::input::InputBackend>) -> Self {
        self.input = Some(input);
        self.caps.delivery.targeted_pointer = true;
        self.caps.delivery.targeted_keys = true;
        self.caps.delivery.foreground = ForegroundCapability::Policy;
        self.caps.background_operation = BackgroundOperation::Partial;
        self.caps.limits.type_text_max = crate::input::messages::TEXT_MAX_UNITS as u32;
        self
    }
    pub(crate) async fn call<T: Send + 'static>(
        &self,
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
        self.worker
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
