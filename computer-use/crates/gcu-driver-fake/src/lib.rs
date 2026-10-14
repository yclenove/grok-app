//! Deterministic in-memory desktop driver for offline tests and evaluations.
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod control;
/// One-shot fault descriptions.
pub mod faults;
/// Fixture rasterizer constants for pixel assertions.
pub mod render;
/// Serializable RON scene descriptions.
pub mod scene;
mod world;
pub use world::Oracle;

use async_trait::async_trait;
use faults::Fault;
use futures_core::Stream;
use gcu_core::{driver::*, fingerprint::refresh_tree, model::*};
use gcu_protocol::capabilities::*;
use std::{
    collections::BTreeMap,
    path::Path,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    sync::{mpsc, Mutex, MutexGuard},
    time::Instant,
};
use world::{find, World};

static NEXT_WORLD: AtomicU64 = AtomicU64::new(1);

/// Fake desktop, safe to clone and share. All state is private to this instance.
/// Native work never touches the OS. Faults and delayed effects are deterministic;
/// no background process is created; delayed effects use short-lived Tokio timers.
#[derive(Clone)]
pub struct FakeDriver {
    world: Arc<Mutex<World>>,
    capabilities: Capabilities,
    held_input: Arc<AtomicUsize>,
    perform_calls: Arc<AtomicUsize>,
    release_calls: Arc<AtomicUsize>,
    action_faults: Arc<Mutex<std::collections::VecDeque<Fault>>>,
    incomplete: Option<Incomplete>,
}
impl FakeDriver {
    /// Construct from validated RON data, synchronously, without filesystem access.
    pub fn from_ron(source: &str) -> std::result::Result<Self, scene::SceneError> {
        let scene = scene::Scene::from_ron(source)?;
        Ok(Self {
            world: Arc::new(Mutex::new(World::new(
                scene,
                NEXT_WORLD.fetch_add(1, Ordering::Relaxed),
            ))),
            capabilities: capabilities(),
            held_input: Arc::new(AtomicUsize::new(0)),
            perform_calls: Arc::new(AtomicUsize::new(0)),
            release_calls: Arc::new(AtomicUsize::new(0)),
            action_faults: Arc::new(Mutex::new(Default::default())),
            incomplete: None,
        })
    }
    /// Construct a built-in fixture, synchronously, without OS resources.
    pub fn from_scene(name: &str) -> std::result::Result<Self, scene::SceneError> {
        Self::from_ron(scene::builtin(name).ok_or(scene::SceneError::Invalid("unknown scene"))?)
    }
    /// Report an incomplete native read in offline safety tests. No OS IO or waits;
    /// apply before sharing the driver. The retained fixture nodes stay deterministic.
    pub fn with_incomplete_tree(mut self, incomplete: Incomplete) -> Self {
        self.incomplete = Some(incomplete);
        self
    }
    /// Disable targeted routes to test route ceilings. Accessibility remains available.
    /// Apply before sharing the driver; callers never gain foreground authorization here.
    pub fn with_targeted_input(mut self, pointer: bool, keys: bool) -> Self {
        self.capabilities.delivery.targeted_pointer = pointer;
        self.capabilities.delivery.targeted_keys = keys;
        self.capabilities.background_operation = if pointer && keys {
            BackgroundOperation::Full
        } else {
            BackgroundOperation::Partial
        };
        self
    }
    async fn lock(&self) -> Result<MutexGuard<'_, World>> {
        let mut world = tokio::time::timeout(NATIVE_TIMEOUT, self.world.lock())
            .await
            .map_err(|_| DriverError::Timeout {
                maybe_executed: false,
            })?;
        world.flush();
        Ok(world)
    }
    /// Queue one fault for the next read/capture/action, within NATIVE_TIMEOUT.
    pub async fn inject(&self, fault: Fault) -> Result<()> {
        self.lock().await?.faults.push_back(fault);
        Ok(())
    }
    /// Read the independent fixture oracle, applying any effects whose settle time elapsed.
    pub async fn oracle(&self) -> Result<Oracle> {
        let mut report = self.lock().await?.report();
        report.held_input = self.held_input.load(Ordering::SeqCst);
        Ok(report)
    }
    /// Resolve an exact fixture node handle for tests, within NATIVE_TIMEOUT.
    pub async fn element(&self, win: &WindowRef, identifier: &str) -> Result<NativeHandle> {
        let world = self.lock().await?;
        let root = world
            .window(win)?
            .root
            .as_ref()
            .ok_or(DriverError::StaleElement)?;
        Ok(find(root, identifier)
            .ok_or(DriverError::StaleElement)?
            .handle
            .clone())
    }
    /// Resolve a fixture window by app name/ID/path, without activating it.
    pub async fn fixture_window(&self, query: &str) -> Result<WindowRef> {
        let app = self.resolve_app(&AppQuery::from(query)).await?;
        let info = self.key_window(&app).await?;
        Ok(WindowRef { app, id: info.id })
    }
    async fn prepare(&self, cancel: &Cancel, timeout: Duration) -> Result<Option<Fault>> {
        if cancel.is_cancelled() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        if timeout.is_zero() {
            return Err(DriverError::Timeout {
                maybe_executed: false,
            });
        }
        let fault = {
            let mut world = self.lock().await?;
            let fault = world.faults.pop_front();
            if let Some(fault) = &fault {
                world.structural_fault(fault)?;
            }
            fault
        };
        if let Some(Fault::Delay(delay)) = fault {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(DriverError::Cancelled { maybe_executed: false }),
                result = tokio::time::timeout(timeout, tokio::time::sleep(delay)) => result.map_err(|_| DriverError::Timeout { maybe_executed: false })?,
            }
            return Ok(None);
        }
        Ok(fault)
    }
    fn delivery(&self, action: &DriverAction) -> Result<Delivery> {
        let pointer = match action {
            DriverAction::PointerClick { route, .. }
            | DriverAction::PointerScroll { route, .. }
            | DriverAction::PointerDrag { route, .. } => Some(*route),
            _ => None,
        };
        if let Some(route) = pointer {
            return route_delivery(
                self.capabilities.delivery.targeted_pointer,
                route == PointerRoute::AllowForeground,
            );
        }
        match action {
            DriverAction::TypeText { route, text, .. } => {
                if text.chars().count() > self.capabilities.limits.type_text_max as usize {
                    return Err(DriverError::InvalidRequest);
                }
                route_delivery(
                    self.capabilities.delivery.targeted_keys,
                    *route == KeyRoute::AllowForeground,
                )
            }
            DriverAction::PressKeys { route, .. } => route_delivery(
                self.capabilities.delivery.targeted_keys,
                *route == KeyRoute::AllowForeground,
            ),
            DriverAction::Activate { .. } => Ok(Delivery::Foreground),
            _ => Ok(Delivery::Accessibility),
        }
    }
}
fn route_delivery(targeted: bool, foreground: bool) -> Result<Delivery> {
    if targeted {
        Ok(Delivery::TargetedEvent)
    } else if foreground {
        Ok(Delivery::Foreground)
    } else {
        Err(DriverError::CapabilityUnavailable)
    }
}
fn read_fault(fault: &Option<Fault>) -> Result<()> {
    match fault {
        Some(Fault::Timeout { .. }) => Err(DriverError::Timeout {
            maybe_executed: false,
        }),
        Some(Fault::Crash { .. }) => Err(DriverError::Crashed {
            maybe_executed: false,
        }),
        Some(Fault::Error(e)) => Err(e.clone()),
        Some(Fault::SlowSettle(_)) => Err(DriverError::InvalidRequest),
        _ => Ok(()),
    }
}
fn capabilities() -> Capabilities {
    Capabilities {
        platform: Platform::Linux,
        version: "fake-memory-v1 (no OS input)".into(),
        observe: ObserveCapabilities {
            tree: true,
            screenshot: true,
            events: true,
        },
        delivery: DeliveryCapabilities {
            accessibility: true,
            targeted_pointer: true,
            targeted_keys: true,
            foreground: ForegroundCapability::Policy,
        },
        background_operation: BackgroundOperation::Full,
        user_activity_guard: false,
        overlay_excluded_from_capture: false,
        private_apis: BTreeMap::new(),
        limits: CapabilityLimits {
            max_tree_nodes: 5000,
            max_image_edge: 4096,
            type_text_max: 8192,
        },
    }
}
struct EventStream {
    rx: mpsc::UnboundedReceiver<UiEvent>,
}
impl Stream for EventStream {
    type Item = UiEvent;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<UiEvent>> {
        self.rx.poll_recv(cx)
    }
}
#[async_trait]
impl Driver for FakeDriver {
    fn platform(&self) -> Platform {
        self.capabilities.platform
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    async fn doctor(&self) -> DoctorReport {
        DoctorReport {
            capabilities: self.capabilities.clone(),
            ready: true,
            issues: Vec::new(),
        }
    }
    async fn list_apps(&self) -> Result<Vec<AppInfo>> {
        Ok(self
            .lock()
            .await?
            .apps
            .iter()
            .map(|app| app.info.clone())
            .collect())
    }
    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef> {
        let world = self.lock().await?;
        let mut found = world.apps.iter().filter(|app| {
            let executable = app
                .info
                .identity
                .as_ref()
                .and_then(|id| id.executable.as_deref());
            app.info.id == query.0
                || app.info.name == query.0
                || executable == Some(query.0.as_str())
                || executable
                    .and_then(|exe| Path::new(exe).file_name())
                    .is_some_and(|name| name == query.0.as_str())
        });
        let first = found.next().ok_or(DriverError::AppNotFound)?;
        if found.next().is_some() {
            return Err(DriverError::AmbiguousApp);
        }
        Ok(first.clone())
    }
    async fn launch_app(&self, app: &AppRef) -> Result<AppRef> {
        Ok(self.lock().await?.app(app)?.clone())
    }
    async fn list_windows(&self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>> {
        let world = self.lock().await?;
        if let Some(app) = app {
            world.app(app)?;
        }
        Ok(world
            .windows
            .iter()
            .filter(|w| app.is_none_or(|a| &w.reference.app == a))
            .map(|w| w.info.clone())
            .collect())
    }
    async fn key_window(&self, app: &AppRef) -> Result<WindowInfo> {
        let world = self.lock().await?;
        world.app(app)?;
        world
            .windows
            .iter()
            .find(|w| &w.reference.app == app)
            .map(|w| w.info.clone())
            .ok_or(DriverError::WindowGone)
    }
    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> Result<RawTree> {
        bounded(cancel, opts.timeout, async {
            let fault = self.prepare(cancel, opts.timeout).await?;
            read_fault(&fault)?;
            let incomplete = match fault {
                Some(Fault::IncompleteTree(fact)) => Some(fact),
                _ => self.incomplete,
            };
            let mut root = self.lock().await?.window(win)?.root.clone();
            if cancel.is_cancelled() {
                return Err(DriverError::Cancelled {
                    maybe_executed: false,
                });
            }
            if let Some(root) = &mut root {
                limit_extras(root, opts.text_limit);
                refresh_tree(root, Fingerprint::default(), 0);
            }
            Ok(RawTree { root, incomplete })
        })
        .await
    }

    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture> {
        bounded(cancel, opts.timeout, async {
            read_fault(&self.prepare(cancel, opts.timeout).await?)?;
            let window = self.lock().await?.window(win)?.clone();
            tokio::task::spawn_blocking(move || {
                render::capture(
                    window.root.as_ref(),
                    Geometry {
                        frame: window.info.frame,
                        scale: window.info.scale,
                    },
                )
            })
            .await
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?
        })
        .await
    }

    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        let world = self.lock().await?;
        let info = &world.window(win)?.info;
        Ok(Geometry {
            frame: info.frame,
            scale: info.scale,
        })
    }
    async fn focused_element(&self, app: &AppRef) -> Result<Option<NativeHandle>> {
        let world = self.lock().await?;
        world.app(app)?;
        Ok(world
            .windows
            .iter()
            .filter(|w| &w.reference.app == app)
            .filter_map(|w| w.root.as_ref())
            .find_map(world::focused)
            .map(|node| node.handle.clone()))
    }
    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> Result<Delivery> {
        self.perform_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(fault) = self.action_faults.lock().await.pop_front() {
            self.lock().await?.faults.push_front(fault);
        }
        bounded(cancel, DRIVER_DEADLINE, async {
            let delivery = self.delivery(req)?;
            let fault = self.prepare(cancel, DRIVER_DEADLINE).await?;
            let mut world = self.lock().await?;
            if cancel.is_cancelled() {
                return Err(DriverError::Cancelled {
                    maybe_executed: false,
                });
            }
            match fault {
                Some(Fault::Error(e)) => Err(e),
                Some(Fault::Timeout { maybe_executed } | Fault::Crash { maybe_executed }) => {
                    if maybe_executed {
                        world.apply(req, delivery)?;
                    }
                    if matches!(fault, Some(Fault::Crash { .. })) {
                        if maybe_executed {
                            if let DriverAction::PressKeys { chord, .. } = req {
                                self.held_input
                                    .store(chord.modifiers.len() + 1, Ordering::SeqCst);
                            }
                        }
                        Err(DriverError::Crashed { maybe_executed })
                    } else {
                        Err(DriverError::Timeout { maybe_executed })
                    }
                }
                Some(Fault::SlowSettle(delay)) => {
                    let at = Instant::now()
                        .checked_add(delay)
                        .ok_or(DriverError::InvalidRequest)?;
                    // Validate on an isolated desktop copy; invalid delivery never queues work.
                    let mut trial = World {
                        id: world.id,
                        apps: world.apps.clone(),
                        windows: world.windows.clone(),
                        faults: Default::default(),
                        oracle: world.oracle.clone(),
                        pending: Default::default(),
                        observers: Vec::new(),
                    };
                    trial.apply(req, delivery)?;
                    world.pending.push_back((at, req.clone(), delivery));
                    let desktop = Arc::downgrade(&self.world);
                    // Weak ownership prevents a delayed fixture effect from keeping
                    // an abandoned desktop alive. No worker process is created.
                    drop(tokio::spawn(async move {
                        tokio::time::sleep(delay).await;
                        if let Some(desktop) = desktop.upgrade() {
                            desktop.lock().await.flush();
                        }
                    }));
                    Ok(delivery)
                }
                _ => {
                    world.apply(req, delivery)?;
                    Ok(delivery)
                }
            }
        })
        .await
    }

    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        if !self.capabilities.observe.events {
            return None;
        }
        let mut world = self.world.try_lock().ok()?;
        world.window(win).ok()?;
        let (tx, rx) = mpsc::unbounded_channel();
        world.observers.retain(|(_, sender)| !sender.is_closed());
        world.observers.push((win.id.clone(), tx));
        Some(Box::pin(EventStream { rx }))
    }
    async fn release_all_input(&self) {
        // No physical keys exist. This path remains available even after injected crash.
        self.release_calls.fetch_add(1, Ordering::SeqCst);
        self.held_input.store(0, Ordering::SeqCst);
    }
}
fn limit_extras(node: &mut RawNode, limit: Option<usize>) {
    if let (Some(extra), Some(limit)) = (&mut node.text_extras, limit) {
        if let Some(text) = &mut extra.text {
            *text = text.chars().take(limit).collect();
        }
    }
    for child in &mut node.children {
        limit_extras(child, limit);
    }
}

/// Shared contract fixture containing the form and 2000-row list desktops.
impl FakeDriver {
    /// Create a fresh combined fixture; no filesystem reads or OS state.
    pub fn contract_fixture() -> std::result::Result<Self, scene::SceneError> {
        let mut form = scene::Scene::from_ron(
            scene::builtin("form").ok_or(scene::SceneError::Invalid("missing form"))?,
        )?;
        form.apps.extend(
            scene::Scene::from_ron(
                scene::builtin("list").ok_or(scene::SceneError::Invalid("missing list"))?,
            )?
            .apps,
        );
        Ok(Self {
            world: Arc::new(Mutex::new(World::new(
                form,
                NEXT_WORLD.fetch_add(1, Ordering::Relaxed),
            ))),
            capabilities: capabilities(),
            held_input: Arc::new(AtomicUsize::new(0)),
            perform_calls: Arc::new(AtomicUsize::new(0)),
            release_calls: Arc::new(AtomicUsize::new(0)),
            action_faults: Arc::new(Mutex::new(Default::default())),
            incomplete: None,
        })
    }
}

#[cfg(feature = "testing")]
#[async_trait]
impl gcu_core::testing::ContractFixture for FakeDriver {
    fn driver(&self) -> &dyn Driver {
        self
    }
    fn app_query(&self) -> AppQuery {
        AppQuery::from("Fixture form")
    }
    fn large_app_query(&self) -> AppQuery {
        AppQuery::from("Fixture list")
    }
    async fn oracle(&self) -> Result<gcu_core::testing::ContractOracle> {
        let report = FakeDriver::oracle(self).await?;
        Ok(gcu_core::testing::ContractOracle {
            counter: report.presses.get("submit").copied().unwrap_or(0),
            text: report.text.get("text").cloned().unwrap_or_default(),
            foreground: report.foreground,
            held_input: report.held_input,
        })
    }
    async fn inject(&self, fault: gcu_core::testing::ContractFault) -> Result<()> {
        use gcu_core::testing::ContractFault;
        let fault = match fault {
            ContractFault::Timeout { maybe_executed } => Fault::Timeout { maybe_executed },
            ContractFault::InterruptedInput => Fault::Crash {
                maybe_executed: true,
            },
            ContractFault::Delete { win, identifier } => Fault::ElementDeleted {
                window: win.id,
                identifier,
            },
            ContractFault::Close { win } => Fault::WindowGone { window: win.id },
        };
        FakeDriver::inject(self, fault).await
    }
}

// Mutation effects run synchronously once the lock is acquired and never yield.
// Timing/cancellation before that point can therefore prove zero delivery.
async fn bounded<T>(
    cancel: &Cancel,
    timeout: Duration,
    operation: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(DriverError::Cancelled { maybe_executed: false }),
        result = tokio::time::timeout(timeout, operation) => result.map_err(|_| DriverError::Timeout { maybe_executed: false })?,
    }
}
