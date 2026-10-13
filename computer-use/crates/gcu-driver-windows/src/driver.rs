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
/// Discovery-only Windows driver. Portable constructors accept a FakeWinApi.
/// COM/OS calls are serialized on one worker with a five-second caller deadline;
/// indexing uses a separate thread. Drop disconnects the worker and stops watching.
pub struct WindowsDriver {
    worker: mpsc::SyncSender<Task>,
    indexing: Arc<std::sync::atomic::AtomicBool>,
    refreshing: Arc<std::sync::atomic::AtomicBool>,
    caps: Capabilities,
    listed: std::sync::atomic::AtomicBool,
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
        tokio::time::timeout(NATIVE_TIMEOUT, ready_rx)
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
        })
    }
    /// Start the production Windows adapter. Available only on Windows.
    #[cfg(windows)]
    pub async fn new() -> Result<Self> {
        Self::with_api(Arc::new(crate::RealWinApi::new()?)).await
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
                // Timed-out/cancelled queued calls must never begin work later.
                if !tx.is_closed() {
                    let result = task(state);
                    let _ = tx.send(result);
                }
            }))
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
        let wait = tokio::time::timeout(timeout.min(NATIVE_TIMEOUT), rx);
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
        issues.push(
            "P2-02+: tree, capture, focus, geometry, subscriptions and input unavailable".into(),
        );
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
        let win = win.clone();
        self.call(opts.timeout, Some(cancel), false, move |s| {
            s.validate_window(&win, false)?;
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
        self.call(opts.timeout, Some(cancel), false, move |s| {
            s.validate_window(&win, false)?;
            Err(DriverError::CapabilityUnavailable)
        })
        .await
    }
    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        let win = win.clone();
        self.call(NATIVE_TIMEOUT, None, false, move |s| {
            s.validate_window(&win, false)?;
            Err(DriverError::CapabilityUnavailable)
        })
        .await
    }
    async fn focused_element(&self, app: &AppRef) -> Result<Option<NativeHandle>> {
        let app = app.clone();
        self.call(NATIVE_TIMEOUT, None, false, move |s| {
            s.validate_app(&app)?;
            Err(DriverError::CapabilityUnavailable)
        })
        .await
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
    fn subscribe(&self, _: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        None
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
            let element = el
                .downcast_ref::<WindowsElement>()
                .ok_or(DriverError::StaleElement)?;
            Ok((&element.window, false))
        }
    }
}
