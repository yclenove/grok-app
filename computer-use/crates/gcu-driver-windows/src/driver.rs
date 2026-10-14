//! Serial discovery worker and the portable Driver implementation.
use crate::{
    api::WinApi, apps::AppIndex, desktop::HwndStamp, discovery::State, gate::check_engine,
    identity::identity_for_path,
};
use async_trait::async_trait;
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
/// Windows discovery, optional UIA and optional capture. Portable sources inject OS data.
/// COM/OS calls are serialized on one worker with a five-second caller deadline;
/// indexing uses a separate thread. Drop disconnects the worker and stops watching.
pub struct WindowsDriver {
    worker: mpsc::SyncSender<Task>,
    indexing: Arc<std::sync::atomic::AtomicBool>,
    refreshing: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) caps: Capabilities,
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
            worker: tx,
            indexing,
            refreshing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            caps: discovery_capabilities(),
            listed: std::sync::atomic::AtomicBool::new(false),
            observer: None,
            capture_backend: None,
        })
    }
    /// Start the production Windows adapter. Available only on Windows.
    #[cfg(windows)]
    pub async fn new() -> Result<Self> {
        let driver = Self::with_api(Arc::new(crate::RealWinApi::new()?)).await?;
        let uia = Arc::new(crate::uia::RealUia::start().await?);
        let wgc = Arc::new(crate::capture::WgcCapture::new().await?);
        let print = Arc::new(crate::capture::PrintWindowCapture::new().await?);
        Ok(driver
            .attach_uia(uia.clone(), uia)
            .with_capture(Arc::new(crate::capture::CaptureBackend::new(wgc, print))))
    }
    /// Start discovery with portable/native UIA sources. Startup validates the
    /// engine before enabling tree/events; capture and input remain unavailable.
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
        self.caps.limits.max_tree_nodes = crate::uia::tree::NODE_CAP as u32;
        self.caps.version = "Windows 10 1903+; discovery and UIA observation".into();
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
#[async_trait]
impl Driver for WindowsDriver {
    fn platform(&self) -> Platform {
        Platform::Windows
    }
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }
    async fn doctor(&self) -> DoctorReport {
        let result = self
            .call(NATIVE_TIMEOUT, None, false, |s| {
                s.check()?;
                let mut issues = s.index.issues();
                issues.extend(s.issues.clone());
                Ok(issues)
            })
            .await;
        let (ready, mut issues) = match result {
            Ok(issues) => (true, issues),
            Err(e) => (false, vec![e.to_string()]),
        };
        issues.push(if self.capture_backend.is_some() {
            "Input unavailable; raw captures require core redaction; protected/all-black frames are refused".into()
        } else if self.observer.is_some() {
            "P2-03+: capture and input unavailable; UIA heuristics are best-effort and non-exhaustive".into()
        } else {
            "P2-02+: UIA source unavailable; capture and input unavailable".into()
        });
        issues.push(
            "Entry signatures do not attest loaded DLL/plugins or writable loading paths".into(),
        );
        DoctorReport {
            capabilities: self.caps.clone(),
            ready,
            issues,
        }
    }
    fn discovery_status(&self) -> DiscoveryStatus {
        DiscoveryStatus {
            indexing: self.indexing.load(std::sync::atomic::Ordering::Acquire)
                || self.refreshing.load(std::sync::atomic::Ordering::Acquire),
        }
    }
    async fn list_apps(&self) -> Result<Vec<AppInfo>> {
        // Return in-memory hints without signature hashing/directory traversal.
        // Running-app refresh follows the response on the dedicated worker.
        let timeout = if !self.listed.swap(true, std::sync::atomic::Ordering::AcqRel) {
            Duration::from_millis(100)
        } else {
            NATIVE_TIMEOUT
        };
        let apps = self.call(timeout, None, false, State::list).await?;
        if self
            .refreshing
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
        {
            let refreshing = self.refreshing.clone();
            if self
                .worker
                .try_send(Box::new(move |s| {
                    if let Err(error) = s.enumerate() {
                        s.issues.push(error.to_string());
                        s.issues.truncate(8);
                    }
                    refreshing.store(false, std::sync::atomic::Ordering::Release);
                }))
                .is_err()
            {
                self.refreshing
                    .store(false, std::sync::atomic::Ordering::Release);
            }
        }
        Ok(apps)
    }
    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef> {
        let query = query.clone();
        self.call(NATIVE_TIMEOUT, None, false, move |s| s.resolve(&query))
            .await
    }
    async fn launch_app(&self, app: &AppRef) -> Result<AppRef> {
        let app = app.clone();
        self.call(NATIVE_TIMEOUT, None, true, move |s| {
            let entry = s.validate_app(&app)?;
            if app.info.pid.is_some() {
                return Ok(app);
            }
            let shortcut = entry.shortcut.ok_or(DriverError::CapabilityUnavailable)?;
            let actual = if shortcut.link.is_empty() {
                shortcut.clone()
            } else {
                s.api.read_link(&shortcut.link)?
            };
            if actual != shortcut
                || identity_for_path(s.api.as_ref(), &actual.target)? != entry.identity
            {
                return Err(DriverError::AppNotFound);
            }
            s.check()?;
            s.api.launch(&actual, &entry.identity)?;
            Ok(app)
        })
        .await
    }
    async fn list_windows(&self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>> {
        let app = app.cloned();
        self.call(NATIVE_TIMEOUT, None, false, move |s| {
            if let Some(app) = &app {
                s.validate_app(app)?;
            }
            Ok(s.enumerate()?
                .into_iter()
                .filter(|(_, id)| app.as_ref().is_none_or(|a| &a.info.id == id))
                .map(|(w, _)| w)
                .collect())
        })
        .await
    }
    async fn key_window(&self, app: &AppRef) -> Result<WindowInfo> {
        let app = app.clone();
        self.call(NATIVE_TIMEOUT, None, true, move |s| {
            s.validate_app(&app)?;
            let windows = s.enumerate()?;
            let (mut info, _) = windows
                .into_iter()
                .filter(|(_, id)| id == &app.info.id)
                .max_by_key(|(w, _)| {
                    s.windows
                        .get(&w.id)
                        .is_some_and(|(w, _, _, _)| w.foreground)
                })
                .ok_or(DriverError::WindowGone)?;
            let win = WindowRef {
                app,
                id: info.id.clone(),
            };
            s.validate_window(&win, false)?;
            let hwnd = s
                .windows
                .get(&info.id)
                .ok_or(DriverError::WindowGone)?
                .0
                .hwnd;
            if s.windows
                .get(&info.id)
                .is_some_and(|(w, _, _, _)| w.minimized)
            {
                // Restoring an off-desktop window may switch desktops; refuse.
                s.validate_window(&win, true)?;
                let expected = s
                    .windows
                    .get(&info.id)
                    .ok_or(DriverError::WindowGone)?
                    .0
                    .clone();
                s.api.restore(&expected)?;
                let current = s
                    .api
                    .enum_windows()?
                    .into_iter()
                    .find(|w| w.hwnd == hwnd)
                    .ok_or(DriverError::WindowGone)?;
                HwndStamp::from(&s.windows.get(&info.id).ok_or(DriverError::WindowGone)?.0)
                    .validate(&current)?;
                if current.minimized {
                    return Err(DriverError::WindowMinimized);
                }
                info.frame = current.frame;
            }
            Ok(info)
        })
        .await
    }
    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> Result<RawTree> {
        let deadline = tokio::time::Instant::now() + opts.timeout.min(DRIVER_DEADLINE);
        let operation = async {
            let target = win.clone();
            let window = self
                .call(opts.timeout, Some(cancel), false, move |s| {
                    s.observation_window(&target, false)
                })
                .await?;
            let observer = self
                .observer
                .as_ref()
                .ok_or(DriverError::CapabilityUnavailable)?;
            let mut remaining = opts.clone();
            remaining.timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
            let raw = observer
                .snapshot(window.hwnd, win, &remaining, cancel)
                .await?;
            // Do not release tree payloads if identity/security changed during UIA.
            let target = win.clone();
            self.call(DRIVER_DEADLINE, Some(cancel), false, move |s| {
                s.validate_window(&target, false)
            })
            .await?;
            observer.remember(win, window.hwnd);
            Ok(raw)
        };
        tokio::select! { biased;
            _ = cancel.cancelled() => Err(DriverError::Cancelled { maybe_executed: false }),
            result = tokio::time::timeout_at(deadline, operation) => result.map_err(|_| DriverError::Timeout { maybe_executed: false })?,
        }
    }
    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture> {
        self.capture_window(win, opts, cancel).await
    }
    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        let win = win.clone();
        self.call(NATIVE_TIMEOUT, None, false, move |s| {
            let window = s.observation_window(&win, false)?;
            Ok(Geometry {
                frame: window.frame,
                scale: window.scale,
            })
        })
        .await
    }
    async fn focused_element(&self, app: &AppRef) -> Result<Option<NativeHandle>> {
        let operation = async {
            let target = app.clone();
            let win = self
                .call(DRIVER_DEADLINE, None, false, move |s| {
                    s.validate_app(&target)?;
                    s.enumerate()?;
                    Ok(s.windows
                        .iter()
                        .find(|(_, (w, _, _, identity))| {
                            w.foreground && identity.path == target.info.id
                        })
                        .map(|(id, _)| WindowRef {
                            app: target.clone(),
                            id: id.clone(),
                        }))
                })
                .await?;
            self.observer
                .as_ref()
                .ok_or(DriverError::CapabilityUnavailable)?;
            let Some(win) = win else {
                return Ok(None);
            };
            let tree = self
                .snapshot_tree(
                    &win,
                    &TreeOpts {
                        max_tree_nodes: crate::uia::tree::NODE_CAP,
                        ..Default::default()
                    },
                    &Cancel::default(),
                )
                .await?;
            fn focused(node: &gcu_core::model::RawNode) -> Option<NativeHandle> {
                if node.states.contains(gcu_core::model::States::FOCUSED) {
                    return Some(node.handle.clone());
                }
                node.children.iter().find_map(focused)
            }
            let focus = tree.root.as_ref().and_then(focused);
            if focus.is_none() && tree.incomplete.is_some() {
                return Err(DriverError::Native {
                    message: "UIA focus unresolved in incomplete tree".into(),
                    maybe_executed: false,
                });
            }
            Ok(focus)
        };
        tokio::time::timeout(DRIVER_DEADLINE, operation)
            .await
            .map_err(|_| DriverError::Timeout {
                maybe_executed: false,
            })?
    }
    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> Result<Delivery> {
        let req = req.clone();
        self.call(NATIVE_TIMEOUT, Some(cancel), false, move |s| {
            let (win, foreground) = action_window(&req)?;
            s.validate_window(win, foreground)?;
            Err(DriverError::CapabilityUnavailable)
        })
        .await
    }
    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        let (hwnd, events) = self.observer.as_ref()?.subscription(win)?;
        let target = win.clone();
        let expires = std::time::Instant::now() + DRIVER_DEADLINE;
        let (tx, rx) = oneshot::channel();
        self.worker
            .try_send(Box::new(move |s| {
                if tx.is_closed()
                    || std::time::Instant::now() >= expires
                    || s.validate_window(&target, false).is_err()
                    || tx.is_closed()
                    || std::time::Instant::now() >= expires
                {
                    return;
                }
                if let Some(stream) = crate::uia::events::subscribe(events.as_ref(), hwnd) {
                    let _ = tx.send(stream);
                }
            }))
            .ok()?;
        Some(crate::uia::events::deferred(rx))
    }
    async fn release_all_input(&self) { /* No input is held or delivered in P2-01. */
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
            let win = el
                .downcast_ref::<WindowsElement>()
                .map(|e| &e.window)
                .or_else(|| {
                    el.downcast_ref::<crate::uia::UiaElement>()
                        .map(|e| &e.window)
                })
                .ok_or(DriverError::StaleElement)?;
            Ok((win, false))
        }
    }
}
