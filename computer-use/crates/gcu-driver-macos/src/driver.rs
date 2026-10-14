//! Serial discovery worker and the portable [`Driver`] implementation.
//!
//! Spotlight indexing uses a separate thread. The first `list_apps` does not
//! wait for it. Screenshots use [`crate::capture`] and stop at
//! [`gcu_core::driver::DRIVER_DEADLINE`]. Focus and input stay
//! [`DriverError::CapabilityUnavailable`]. Tree reads use [`crate::ax`] and, on
//! a live macOS driver, a separate `gcu-macos-ax` thread. Queued calls that
//! time out or cancel before the worker starts them never run.

use crate::{
    api::MacApi,
    ax::{self, EventHub, FakeAxNode, FakeAxSource},
    capture::{self, CaptureSource},
    discovery::State,
    gate::MacGate,
    identity::TRUST_NOTE,
    spotlight::SpotlightIndex,
};
use async_trait::async_trait;
use gcu_core::{
    driver::*,
    model::{Geometry, NativeHandle, RawTree},
};
use gcu_protocol::{capabilities::*, AppInfo, WindowInfo};
use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicBool, mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

#[cfg(target_os = "macos")]
use crate::discovery::TreeTarget;
#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;

#[derive(Clone)]
struct InjectedTree {
    source: Arc<FakeAxSource>,
    root: FakeAxNode,
}

type Task = Box<dyn FnOnce(&mut State) + Send>;

/// Discovery-only macOS driver. Portable tests pass a [`crate::testing::FakeMacApi`].
///
/// `new` exists only on macOS. Both constructors refuse a root engine with
/// [`DriverError::ElevationRequired`] before any Spotlight work.
pub struct MacosDriver {
    worker: mpsc::SyncSender<Task>,
    indexing: Arc<AtomicBool>,
    caps: Capabilities,
    listed: AtomicBool,
    injected: Mutex<Option<InjectedTree>>,
    capture: Mutex<Option<Arc<dyn CaptureSource>>>,
    /// Declared before the AX thread so Drop restores toggles while the thread
    /// still exists.
    toggles: Mutex<Vec<Box<dyn ax::ErasedToggle>>>,
    events: EventHub,
    snapshot: tokio::sync::Mutex<()>,
    /// True only after the macOS constructor. Portable drivers never set it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    live_ax: AtomicBool,
    #[cfg(target_os = "macos")]
    ax: Option<Arc<crate::real::AxThread>>,
    #[cfg(target_os = "macos")]
    observers: crate::ax::ObserverPool,
}

impl MacosDriver {
    /// Start the worker and the Spotlight thread. Startup is bounded by
    /// [`NATIVE_TIMEOUT`]. The caller is not blocked on the first metadata query.
    pub async fn with_api(api: Arc<dyn MacApi>) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Task>(32);
        let (ready_tx, ready_rx) = oneshot::channel();
        std::thread::Builder::new()
            .name("gcu-macos-discovery".into())
            .spawn(move || {
                let started = (|| {
                    MacGate::engine(api.engine_euid()?)?;
                    let spotlight = SpotlightIndex::start(api.clone())?;
                    let indexing = spotlight.flag();
                    Ok((State::new(api, spotlight), indexing))
                })();
                match started {
                    Ok((mut state, indexing)) => {
                        if ready_tx.send(Ok(indexing)).is_err() {
                            return;
                        }
                        while let Ok(task) = rx.recv() {
                            task(&mut state);
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
        let indexing = tokio::time::timeout(NATIVE_TIMEOUT, ready_rx)
            .await
            .map_err(|_| DriverError::Timeout {
                maybe_executed: false,
            })?
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })??;
        Ok(Self {
            worker: tx,
            indexing,
            caps: discovery_capabilities(),
            listed: AtomicBool::new(false),
            injected: Mutex::new(None),
            capture: Mutex::new(None),
            toggles: Mutex::new(Vec::new()),
            events: EventHub::new(),
            snapshot: tokio::sync::Mutex::new(()),
            live_ax: AtomicBool::new(false),
            #[cfg(target_os = "macos")]
            ax: None,
            #[cfg(target_os = "macos")]
            observers: crate::ax::ObserverPool::new(),
        })
    }

    /// Start the production adapter. Available only on macOS.
    ///
    /// This is the only constructor that starts the AX thread. [`Self::with_api`]
    /// stays portable and never calls Accessibility.
    #[cfg(target_os = "macos")]
    pub async fn new() -> Result<Self> {
        let mut driver = Self::with_api(Arc::new(crate::RealMacApi::new())).await?;
        driver.live_ax.store(true, Ordering::Release);
        driver.ax = Some(Arc::new(crate::real::AxThread::spawn()?));
        driver.inject_capture(Arc::new(capture::SckCapture::new()));
        Ok(driver)
    }

    /// Use an in-memory screenshot backend after the window gate.
    ///
    /// Tests on every platform call this. A live macOS driver installs
    /// [`capture::SckCapture`] instead. Without a source, [`Driver::capture`]
    /// stays [`DriverError::CapabilityUnavailable`].
    pub fn inject_capture(&self, source: Arc<dyn CaptureSource>) {
        if let Ok(mut slot) = self.capture.lock() {
            *slot = Some(source);
        }
    }

    fn capture_source(&self) -> Option<Arc<dyn CaptureSource>> {
        self.capture.lock().ok().and_then(|slot| slot.clone())
    }

    /// Use an in-memory tree after the accessibility gate and window check.
    ///
    /// Tests on every platform call this. A missing accessibility permission
    /// still returns `PermissionMissing` and does not read the fake tree.
    pub fn inject_tree(&self, source: Arc<FakeAxSource>, root: FakeAxNode) {
        if let Ok(mut slot) = self.injected.lock() {
            *slot = Some(InjectedTree { source, root });
        }
    }

    /// Remember toggles so [`Driver::release_all_input`] and Drop restore them.
    pub fn track_toggles(&self, toggles: impl ax::ErasedToggle + 'static) {
        if let Ok(mut slot) = self.toggles.lock() {
            slot.push(Box::new(toggles));
        }
    }

    /// Deliver one event to subscribers. Tests use this; the live observer does too.
    pub fn emit_ui_event(&self, event: UiEvent) {
        self.events.emit(event);
    }

    fn restore_toggles(&self) {
        let Ok(mut slot) = self.toggles.lock() else {
            return;
        };
        for toggles in slot.iter() {
            toggles.restore();
        }
        slot.clear();
    }

    async fn call<T: Send + 'static>(
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
                if !tx.is_closed() {
                    let _ = tx.send(task(state));
                }
            }))
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
        let wait = tokio::time::timeout(timeout.min(NATIVE_TIMEOUT), rx);
        let result = if let Some(cancel) = cancel {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    return Err(DriverError::Cancelled { maybe_executed: mutation });
                }
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
        platform: Platform::Macos,
        version: "macOS 14+; AX tree (P1-03); screenshot (P1-04)".into(),
        observe: ObserveCapabilities {
            tree: true,
            screenshot: true,
            events: true,
        },
        delivery: DeliveryCapabilities {
            accessibility: false,
            targeted_pointer: false,
            targeted_keys: false,
            foreground: ForegroundCapability::Unavailable,
        },
        background_operation: BackgroundOperation::Partial,
        user_activity_guard: false,
        overlay_excluded_from_capture: false,
        private_apis: BTreeMap::new(),
        limits: CapabilityLimits {
            max_tree_nodes: ax::MAX_NODES_CEILING as u32,
            max_image_edge: 2560,
            type_text_max: 0,
        },
    }
}

/// Private element binding. Action targets still use the window; tree handles
/// carry [`crate::ax::AxHandle`] separately.
#[derive(Clone, Debug)]
pub struct MacElement {
    /// Window that owned the element when it was observed.
    pub window: WindowRef,
}

#[async_trait]
impl Driver for MacosDriver {
    fn platform(&self) -> Platform {
        Platform::Macos
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn doctor(&self) -> DoctorReport {
        let result = self
            .call(NATIVE_TIMEOUT, None, false, |state| {
                State::doctor_issues(state)
            })
            .await;
        let (ready, mut issues) = match result {
            Ok(pair) => pair,
            Err(error) => (false, vec![error.to_string()]),
        };
        issues.push(
            "Focus and input are unavailable. Tree reads need Accessibility. Screenshots need Screen Recording.".into(),
        );
        issues.push(TRUST_NOTE.into());
        DoctorReport {
            capabilities: self.caps.clone(),
            ready,
            issues,
        }
    }

    fn discovery_status(&self) -> DiscoveryStatus {
        DiscoveryStatus {
            indexing: self.indexing.load(std::sync::atomic::Ordering::Acquire),
        }
    }

    async fn list_apps(&self) -> Result<Vec<AppInfo>> {
        let timeout = if !self.listed.swap(true, std::sync::atomic::Ordering::AcqRel) {
            Duration::from_millis(100)
        } else {
            NATIVE_TIMEOUT
        };
        self.call(timeout, None, false, State::list).await
    }

    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef> {
        let query = query.clone();
        self.call(NATIVE_TIMEOUT, None, false, move |state| {
            state.resolve(&query)
        })
        .await
    }

    async fn launch_app(&self, app: &AppRef) -> Result<AppRef> {
        let app = app.clone();
        self.call(NATIVE_TIMEOUT, None, true, move |state| state.launch(&app))
            .await
    }

    async fn list_windows(&self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>> {
        let app = app.cloned();
        self.call(NATIVE_TIMEOUT, None, false, move |state| {
            state.list_windows(app.as_ref())
        })
        .await
    }

    async fn key_window(&self, app: &AppRef) -> Result<WindowInfo> {
        let app = app.clone();
        self.call(NATIVE_TIMEOUT, None, true, move |state| {
            state.key_window(&app)
        })
        .await
    }

    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> Result<RawTree> {
        if cancel.is_cancelled() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        let started = Instant::now();
        let win = win.clone();
        let target = self
            .call(opts.timeout, Some(cancel), false, move |state| {
                state.prepare_tree(&win)
            })
            .await?;
        let remaining = opts.timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(DriverError::Timeout {
                maybe_executed: false,
            });
        }
        let _guard = self.snapshot.lock().await;
        let injected = self.injected.lock().ok().and_then(|slot| slot.clone());
        if let Some(injected) = injected {
            let mut walk = opts.clone();
            walk.timeout = remaining.min(ax::TREE_BUILD_TIMEOUT);
            return ax::build_raw_tree(
                &*injected.source,
                &injected.root,
                &walk,
                cancel,
                target.scale,
            )
            .await;
        }
        #[cfg(target_os = "macos")]
        if self.live_ax.load(Ordering::Acquire) {
            return self.snapshot_live(&target, opts, cancel, remaining).await;
        }
        let _ = (target, remaining);
        Err(DriverError::Native {
            message: "accessibility tree source is not connected".into(),
            maybe_executed: false,
        })
    }

    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture> {
        if cancel.is_cancelled() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        let win = win.clone();
        let budget = capture::capture_budget(opts.timeout);
        let target = self
            .call(budget, Some(cancel), false, move |state| {
                state.prepare_capture(&win)
            })
            .await?;
        let Some(source) = self.capture_source() else {
            return Err(DriverError::CapabilityUnavailable);
        };
        let pixels = capture::take_capture(source, &target, budget, cancel).await?;
        capture::capture_from_pixels(&target, pixels)
    }

    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        let win = win.clone();
        self.call(DRIVER_DEADLINE, None, false, move |state| {
            state.prepare_geometry(&win)
        })
        .await
    }

    async fn focused_element(&self, app: &AppRef) -> Result<Option<NativeHandle>> {
        let app = app.clone();
        self.call(NATIVE_TIMEOUT, None, false, move |state| {
            state.validate_focus(&app)?;
            Err(DriverError::CapabilityUnavailable)
        })
        .await
    }

    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> Result<Delivery> {
        let req = req.clone();
        self.call(NATIVE_TIMEOUT, Some(cancel), false, move |state| {
            let (win, _) = action_window(&req)?;
            state.validate_window(win)?;
            Err(DriverError::CapabilityUnavailable)
        })
        .await
    }

    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        #[cfg(target_os = "macos")]
        if self.live_ax.load(Ordering::Acquire) {
            if let Some(pid) = ax::parse_window_pid(&win.id) {
                self.observers.start(pid, self.events.clone());
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = win;
        Some(self.events.subscribe())
    }

    async fn release_all_input(&self) {
        self.restore_toggles();
    }
}

impl Drop for MacosDriver {
    fn drop(&mut self) {
        self.restore_toggles();
        #[cfg(target_os = "macos")]
        self.observers.stop();
    }
}

impl MacosDriver {
    #[cfg(target_os = "macos")]
    async fn snapshot_live(
        &self,
        target: &TreeTarget,
        opts: &TreeOpts,
        cancel: &Cancel,
        remaining: Duration,
    ) -> Result<RawTree> {
        let Some(thread) = self.ax.clone() else {
            return Err(DriverError::Native {
                message: "accessibility tree source is not connected".into(),
                maybe_executed: false,
            });
        };
        if ax::needs_enhanced_accessibility(&target.bundle_id, &target.app_name) {
            let toggles = Arc::new(ax::AccessibilityToggles::new(
                crate::real::AxToggleStore::new(Arc::clone(&thread)),
            ));
            let enabling = Arc::clone(&toggles);
            let pid = target.pid;
            let enabled = tokio::task::spawn_blocking(move || enabling.enable(pid))
                .await
                .map_err(|_| DriverError::Crashed {
                    maybe_executed: false,
                })?;
            enabled?;
            self.track_toggles(ArcToggle(toggles));
        }
        let Some(index) = target.ax_index else {
            return Ok(RawTree { root: None });
        };
        let source = crate::real::RealAxSource::open(thread, target.pid, index).await?;
        source.set_text_limit(opts.text_limit);
        let mut walk = opts.clone();
        walk.timeout = remaining.min(ax::TREE_BUILD_TIMEOUT);
        let root = source.root();
        let tree = ax::build_raw_tree(&source, &root, &walk, cancel, target.scale).await;
        source.reset().await;
        tree
    }
}

/// `Arc` wrapper so tracked toggles restore when the driver drops them.
#[cfg(target_os = "macos")]
struct ArcToggle<S: ax::ToggleStore>(Arc<ax::AccessibilityToggles<S>>);

#[cfg(target_os = "macos")]
impl<S: ax::ToggleStore + 'static> ax::ErasedToggle for ArcToggle<S> {
    fn restore(&self) {
        self.0.restore();
    }
}

fn action_window(action: &DriverAction) -> Result<(&WindowRef, bool)> {
    match action {
        DriverAction::PointerClick { win, route, .. }
        | DriverAction::PointerScroll { win, route, .. }
        | DriverAction::PointerDrag { win, route, .. } => {
            Ok((win, *route == PointerRoute::AllowForeground))
        }
        DriverAction::TypeText { win, route, .. } | DriverAction::PressKeys { win, route, .. } => {
            Ok((win, *route == KeyRoute::AllowForeground))
        }
        DriverAction::Activate { win } => Ok((win, true)),
        DriverAction::Press { el }
        | DriverAction::SecondaryAction { el, .. }
        | DriverAction::SetValue { el, .. }
        | DriverAction::SelectText { el, .. }
        | DriverAction::ScrollElement { el, .. } => {
            let element = el
                .downcast_ref::<MacElement>()
                .ok_or(DriverError::StaleElement)?;
            Ok((&element.window, false))
        }
    }
}
