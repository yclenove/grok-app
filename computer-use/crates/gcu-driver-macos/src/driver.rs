//! Independent discovery workers and the portable [`Driver`] implementation.
//!
//! `list_apps` reads an in-memory snapshot while a catalog thread enriches
//! running-app hints. Spotlight, running-app resolution, and installed-app
//! lookup use independent queues. No index IO holds a cache lock. Screenshots
//! use [`crate::capture`] and stop at
//! [`gcu_core::driver::DRIVER_DEADLINE`]. Semantic accessibility actions use
//! [`crate::ax`] and do not activate the target app. Pointer and keyboard
//! input use [`crate::input`] when an event backend is connected. Without one,
//! they stay [`DriverError::CapabilityUnavailable`]. Focus queries stay
//! unavailable. Tree reads use a
//! separate `gcu-macos-ax` thread on a live driver. Queued calls that time out
//! or cancel before the worker starts them never run.

use crate::{
    api::MacApi,
    ax::{self, EventHub, FakeAxNode, FakeAxSource},
    capture::{self, CaptureSource},
    catalog::Catalog,
    discovery::State,
    gate::MacGate,
    identity::TRUST_NOTE,
    spotlight::SpotlightIndex,
    worker::{self, Task},
};
use async_trait::async_trait;
use gcu_core::{
    driver::*,
    model::{Geometry, NativeHandle, RawTree},
};
use gcu_protocol::{AppInfo, WindowInfo};
use std::{
    sync::{atomic::AtomicBool, mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;

#[derive(Clone)]
struct InjectedTree {
    source: Arc<FakeAxSource>,
    root: FakeAxNode,
}

/// Discovery-only macOS driver. Portable tests pass a [`crate::testing::FakeMacApi`].
///
/// `new` exists only on macOS. Both constructors refuse a root engine with
/// [`DriverError::ElevationRequired`] before any Spotlight work.
pub struct MacosDriver {
    worker: mpsc::SyncSender<Task>,
    resolver: mpsc::SyncSender<Task>,
    lookup: mpsc::SyncSender<Task>,
    catalog: Catalog,
    spotlight: Arc<SpotlightIndex>,
    pub(crate) caps: Capabilities,
    pub(crate) guarded_caps: [Capabilities; 4],
    pub(crate) guard_runtime: Option<crate::input::watchdog::GuardRuntime>,
    injected: Mutex<Option<InjectedTree>>,
    capture: Mutex<Option<Arc<dyn CaptureSource>>>,
    /// Declared before the AX thread so Drop restores toggles while the thread
    /// still exists.
    toggles: Mutex<Vec<Box<dyn ax::ErasedToggle>>>,
    events: EventHub,
    pub(crate) input: Mutex<Option<Arc<dyn crate::input::EventApi>>>,
    pub(crate) ledger: Arc<crate::input::InputLedger>,
    pub(crate) text: Mutex<Option<crate::text_input::TextBackends>>,
    snapshot: tokio::sync::Mutex<()>,
    /// True only after the macOS constructor. Portable drivers never set it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) live_ax: AtomicBool,
    /// Identity evidence. Declared before the AX thread so its drop releases
    /// held elements while that thread still exists.
    pub(crate) evidence: Arc<crate::scope_os::Evidence>,
    #[cfg(target_os = "macos")]
    pub(crate) ax: Option<Arc<crate::real::AxThread>>,
    #[cfg(target_os = "macos")]
    observers: crate::ax::ObserverPool,
}

impl MacosDriver {
    /// Start the native queues and background catalog/Spotlight threads.
    /// Startup is bounded by [`NATIVE_TIMEOUT`] and does not wait for indexing.
    /// Lists return cached, possibly empty/partial hints with no native IO;
    /// `discovery_status` reports unfinished indexing. Resolution verifies fresh
    /// facts on independent queues with a shared five-second caller budget.
    pub async fn with_api(api: Arc<dyn MacApi>) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Task>(32);
        let (ready_tx, ready_rx) = oneshot::channel();
        std::thread::Builder::new()
            .name("gcu-macos-discovery".into())
            .spawn(move || {
                let started = (|| {
                    MacGate::engine(api.engine_euid()?)?;
                    let spotlight = Arc::new(SpotlightIndex::start(api.clone())?);
                    let catalog = Catalog::start(api.clone())?;
                    let state = State::new(api, spotlight.clone());
                    let resolver = worker::start("gcu-macos-resolve", state.fork())?;
                    let lookup = worker::start("gcu-macos-lookup", state.fork())?;
                    Ok((state, catalog, spotlight, resolver, lookup))
                })();
                match started {
                    Ok((mut state, catalog, spotlight, resolver, lookup)) => {
                        if ready_tx
                            .send(Ok((catalog, spotlight, resolver, lookup)))
                            .is_err()
                        {
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
        let (catalog, spotlight, resolver, lookup) = tokio::time::timeout(NATIVE_TIMEOUT, ready_rx)
            .await
            .map_err(|_| DriverError::Timeout {
                maybe_executed: false,
            })?
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })??;
        Ok(Self {
            worker: tx,
            resolver,
            lookup,
            catalog,
            spotlight,
            caps: crate::guards::discovery_capabilities(),
            guarded_caps: std::array::from_fn(|_| crate::guards::discovery_capabilities()),
            guard_runtime: None,
            injected: Mutex::new(None),
            capture: Mutex::new(None),
            toggles: Mutex::new(Vec::new()),
            events: EventHub::new(),
            input: Mutex::new(None),
            ledger: Arc::new(crate::input::InputLedger::new()),
            text: Mutex::new(None),
            snapshot: tokio::sync::Mutex::new(()),
            live_ax: AtomicBool::new(false),
            evidence: crate::scope_os::Evidence::closed(),
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
        Self::with_stop_latch(
            Arc::new(crate::input::hotkey::SharedStopLatch::new(true)),
            crate::input::hotkey::HotkeyConfig::default(),
            3000,
        )
        .await
    }

    /// Attach the trusted Host's persistent Stop latch before enabling input.
    /// New engines must stay stopped until that Host restores their state.
    #[cfg(target_os = "macos")]
    pub async fn with_stop_latch(
        latch: Arc<dyn crate::input::hotkey::StopLatch>,
        config: crate::input::hotkey::HotkeyConfig,
        idle_ms: u64,
    ) -> Result<Self> {
        let mut driver = Self::with_api(Arc::new(crate::RealMacApi::new())).await?;
        driver.live_ax.store(true, Ordering::Release);
        let thread = Arc::new(crate::real::AxThread::spawn()?);
        driver.evidence = crate::scope_os::live_evidence(Arc::clone(&thread));
        driver.ax = Some(thread);
        driver.inject_capture(Arc::new(capture::SckCapture::new()));
        let events = Arc::new(crate::input::RealEvents::with_stop_latch(
            latch, config, idle_ms,
        ));
        let clock = crate::input::EventApi::guards(events.as_ref())
            .ok_or(DriverError::CapabilityUnavailable)?
            .clock
            .clone();
        driver.inject_events(events);
        if let Some(thread) = &driver.ax {
            driver.inject_text(
                Arc::new(crate::real::RealText::new(
                    Arc::clone(thread),
                    clock.clone(),
                    Arc::clone(&driver.evidence),
                )),
                Arc::new(crate::input::NativePasteboard::general()),
                clock,
            );
        }
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
        source.bind_root(root.clone());
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

    pub(crate) async fn call<T: Send + 'static>(
        &self,
        timeout: Duration,
        cancel: Option<&Cancel>,
        mutation: bool,
        task: impl FnOnce(&mut State) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        worker::call(&self.worker, timeout, cancel, mutation, task).await
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
        self.guard_capabilities()
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
            "Focus queries are unavailable. Semantic accessibility actions do not activate the target app. Tree reads need Accessibility. Screenshots need Screen Recording. Background clicks need a passing field-51 probe and are unavailable for Chromium-class windows. Foreground input runs only when the route allows it.".into(),
        );
        if let Some(guards) = self.input_guards() {
            issues.push(format!(
                "stop_hotkey: {}; user_activity_guard: {}; stopped: {}",
                guards.stop_available(),
                guards.ms_since_physical_input().is_some(),
                guards.is_stopped()
            ));
        }
        issues.extend(self.catalog.issues());
        issues.push(TRUST_NOTE.into());
        issues.push(crate::text_input::CLIPBOARD_NOTE.into());
        DoctorReport {
            capabilities: self.capabilities().clone(),
            ready,
            issues,
        }
    }

    fn discovery_status(&self) -> DiscoveryStatus {
        DiscoveryStatus {
            indexing: self.catalog.indexing()
                || self
                    .spotlight
                    .flag()
                    .load(std::sync::atomic::Ordering::Acquire),
        }
    }

    async fn list_apps(&self) -> Result<Vec<AppInfo>> {
        self.catalog.list(&self.spotlight)
    }

    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef> {
        let started = Instant::now();
        let running_query = query.clone();
        if let Some(app) = worker::call(&self.resolver, NATIVE_TIMEOUT, None, false, move |state| {
            state.resolve_running(&running_query)
        })
        .await?
        {
            return Ok(app);
        }
        let query = query.clone();
        worker::call(
            &self.lookup,
            NATIVE_TIMEOUT.saturating_sub(started.elapsed()),
            None,
            false,
            move |state| state.resolve(&query),
        )
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
        if let Some(guards) = self.input_guards() {
            if guards.is_stopped() {
                crate::diagnostics::note(
                    "CANCEL_REASON",
                    "host_stop_latch; before AX dispatch; maybe_executed=false",
                );
            }
            guards.check(false)?;
        }
        if matches!(req, DriverAction::TypeText { .. }) {
            return self
                .perform_text(req, crate::input::text::TextOptions::default(), cancel)
                .await;
        }
        if ax::is_semantic(req) {
            let work = self.perform_semantic(req, cancel);
            if let Some(guards) = self.input_guards() {
                return tokio::select! {
                    biased;
                    _ = guards.stopped() => Err(DriverError::Cancelled { maybe_executed: true }),
                    result = work => result,
                };
            }
            return work.await;
        }
        self.perform_events(req, cancel).await
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
        if let Some(api) = self.event_api() {
            let clock = api
                .guards()
                .map(|g| g.clock.clone())
                .unwrap_or_else(crate::input::clock::runtime_clock);
            let deadline = clock.now() + Duration::from_millis(100);
            tokio::select! {
                biased;
                _ = crate::input::release_held(api.as_ref(), &self.ledger) => {},
                _ = clock.wait_until(deadline) => {},
            }
        }
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
    async fn perform_semantic(&self, req: &DriverAction, cancel: &Cancel) -> Result<Delivery> {
        if cancel.is_cancelled() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        self.call(DRIVER_DEADLINE, Some(cancel), false, |state| {
            state.action_gate()
        })
        .await?;
        if let Some(injected) = self.injected.lock().ok().and_then(|slot| slot.clone()) {
            return ax::perform(&*injected.source, req, cancel, DRIVER_DEADLINE).await;
        }
        self.perform_live_action(req, cancel).await
    }
}
