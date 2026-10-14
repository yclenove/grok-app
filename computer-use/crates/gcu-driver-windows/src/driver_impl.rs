//! Driver dispatch; isolated mode delegates every native operation.
use super::*;
use async_trait::async_trait;
#[async_trait]
impl Driver for WindowsDriver {
    fn platform(&self) -> Platform {
        Platform::Windows
    }
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }
    async fn doctor(&self) -> DoctorReport {
        if let Some(worker) = &self.isolated {
            return worker.doctor().await;
        }
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
            "Pointer/keyboard and foreground unavailable; raw captures require core redaction; protected/all-black frames are refused".into()
        } else if self.observer.is_some() {
            "Capture, pointer/keyboard and foreground unavailable; UIA heuristics are best-effort and non-exhaustive".into()
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
        if let Some(worker) = &self.isolated {
            return worker.discovery_status();
        }
        DiscoveryStatus {
            indexing: self.indexing.load(std::sync::atomic::Ordering::Acquire)
                || self.refreshing.load(std::sync::atomic::Ordering::Acquire),
        }
    }
    async fn list_apps(&self) -> Result<Vec<AppInfo>> {
        if let Some(worker) = &self.isolated {
            return worker.list_apps().await;
        }
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
        if let Some(worker) = &self.isolated {
            return worker.resolve_app(query).await;
        }
        let query = query.clone();
        self.call(NATIVE_TIMEOUT, None, false, move |s| s.resolve(&query))
            .await
    }
    async fn launch_app(&self, app: &AppRef) -> Result<AppRef> {
        if let Some(worker) = &self.isolated {
            return worker.launch_app(app).await;
        }
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
        if let Some(worker) = &self.isolated {
            return worker.list_windows(app).await;
        }
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
        if let Some(worker) = &self.isolated {
            return worker.key_window(app).await;
        }
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
        if let Some(worker) = &self.isolated {
            return worker.snapshot_tree(win, opts, cancel).await;
        }
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
        if let Some(worker) = &self.isolated {
            return worker.capture(win, opts, cancel).await;
        }
        self.capture_window(win, opts, cancel).await
    }
    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        if let Some(worker) = &self.isolated {
            return worker.window_geometry(win).await;
        }
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
        if let Some(worker) = &self.isolated {
            return worker.focused_element(app).await;
        }
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
        if let Some(worker) = &self.isolated {
            return worker.perform(req, cancel).await;
        }
        let deadline = tokio::time::Instant::now() + DRIVER_DEADLINE;
        let request = req.clone();
        let hwnd = self
            .call(DRIVER_DEADLINE, Some(cancel), false, move |s| {
                let (win, foreground) = action_window(&request)?;
                Ok(s.observation_window(win, foreground)?.hwnd)
            })
            .await?;
        if crate::uia::actions::target(req).is_none() {
            return Err(DriverError::CapabilityUnavailable);
        }
        self.observer
            .as_ref()
            .ok_or(DriverError::CapabilityUnavailable)?
            .perform(hwnd, req, cancel, deadline)
            .await
    }
    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        if let Some(worker) = &self.isolated {
            return worker.subscribe(win);
        }
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
    async fn release_all_input(&self) {
        if let Some(worker) = &self.isolated {
            return worker.release_all_input().await;
        } /* Semantic patterns hold no synthetic pointer/key input. */
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
