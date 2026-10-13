//! Serial discovery worker and the portable [`Driver`] implementation.
//!
//! Spotlight indexing uses a separate thread. The first `list_apps` does not
//! wait for it. Tree, capture, and input return [`DriverError::CapabilityUnavailable`]
//! after the same gate as a real observation. Queued calls that time out or
//! cancel before the worker starts them never run.

use crate::{
    api::MacApi, discovery::State, gate::MacGate, identity::TRUST_NOTE, spotlight::SpotlightIndex,
};
use async_trait::async_trait;
use gcu_core::{
    driver::*,
    model::{Geometry, NativeHandle, RawTree},
};
use gcu_protocol::{capabilities::*, AppInfo, WindowInfo};
use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicBool, mpsc, Arc},
    time::Duration,
};
use tokio::sync::oneshot;

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
        })
    }

    /// Start the production adapter. Available only on macOS.
    #[cfg(target_os = "macos")]
    pub async fn new() -> Result<Self> {
        Self::with_api(Arc::new(crate::RealMacApi::new())).await
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
        version: "macOS 14+; discovery only (P1-02)".into(),
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

/// Private element binding. P1-03 will attach an Accessibility node.
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
            "P1-03+: tree, capture, focus, geometry, subscriptions and input unavailable".into(),
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
        let win = win.clone();
        self.call(opts.timeout, Some(cancel), false, move |state| {
            state.validate_window(&win)?;
            Err(DriverError::CapabilityUnavailable)
        })
        .await
    }

    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture> {
        let win = win.clone();
        self.call(opts.timeout, Some(cancel), false, move |state| {
            state.validate_window(&win)?;
            Err(DriverError::CapabilityUnavailable)
        })
        .await
    }

    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        let win = win.clone();
        self.call(NATIVE_TIMEOUT, None, false, move |state| {
            state.validate_window(&win)?;
            Err(DriverError::CapabilityUnavailable)
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

    fn subscribe(&self, _win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        None
    }

    async fn release_all_input(&self) {}
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
