//! Closed [`crate::driver::Driver`] wrapper.
//!
//! Every trait method is implemented in this file. A new `Driver` method
//! without a body fails compilation here. Do not add a blanket forwarder.

use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use async_trait::async_trait;
use futures_core::Stream;
use gcu_host_api::UserActivity;
use gcu_protocol::Capabilities;

use super::guard::{self, PostCheck, SealEntry};
use super::model::{FixtureScope, InstanceEvidence, ScopeLease};
use crate::driver::{
    AppQuery, AppRef, BoxStream, Cancel, Capture, CaptureOpts, Delivery, DiscoveryStatus,
    DoctorReport, Driver, DriverAction, DriverError, Platform, Result, TreeOpts, UiEvent,
    WindowRef,
};
use crate::model::{NativeHandle, RawTree};

const DOCTOR_ALLOW: [&str; 2] = [
    "permission missing: accessibility",
    "permission missing: screen_recording",
];

/// Driver that admits only the targets in its [`ScopeLease`].
///
/// The lease is shared with the assembler that installed it. This type does
/// not widen the lease. `release_all_input` is the only method that ignores
/// both scope and the stop latch, because it is cleanup.
pub struct ScopedDriver<D> {
    inner: D,
    lease: ScopeLease,
    activity: std::sync::Arc<dyn UserActivity>,
    capabilities: Capabilities,
    seals: Mutex<Vec<SealEntry>>,
}

impl<D> ScopedDriver<D>
where
    D: Driver + InstanceEvidence,
{
    /// Wrap `inner`. Private-API names on the capability snapshot are cleared.
    ///
    /// Memory only. No blocking, timeout, or cancellation. The activity object
    /// is the same stop latch the session uses.
    pub fn new(inner: D, lease: ScopeLease, activity: std::sync::Arc<dyn UserActivity>) -> Self {
        let mut capabilities = inner.capabilities().clone();
        capabilities.private_apis.clear();
        capabilities.version = guard::normalize_version(&capabilities.version).to_owned();
        Self {
            inner,
            lease,
            activity,
            capabilities,
            seals: Mutex::new(Vec::new()),
        }
    }

    fn open(&self) -> Result<FixtureScope> {
        if self.activity.stop_requested() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        self.lease.current().ok_or(DriverError::ScopeDenied)
    }

    /// Stop wins. A generation or record change drops the payload.
    fn handoff(&self, opened: &FixtureScope) -> Result<()> {
        if self.activity.stop_requested() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        if self.lease.matches(opened) {
            Ok(())
        } else {
            Err(DriverError::ScopeDenied)
        }
    }
}

#[async_trait]
impl<D> Driver for ScopedDriver<D>
where
    D: Driver + InstanceEvidence,
{
    fn platform(&self) -> Platform {
        self.inner.platform()
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn doctor(&self) -> DoctorReport {
        if self.activity.stop_requested() || self.lease.current().is_none() {
            return DoctorReport {
                capabilities: self.capabilities.clone(),
                ready: false,
                issues: Vec::new(),
            };
        }
        let mut report = self.inner.doctor().await;
        report.capabilities = self.capabilities.clone();
        report
            .issues
            .retain(|issue| DOCTOR_ALLOW.contains(&issue.as_str()));
        report
    }

    async fn list_apps(&self) -> Result<Vec<gcu_protocol::AppInfo>> {
        let scope = self.open()?;
        let apps = self
            .inner
            .list_apps()
            .await
            .map_err(|_| DriverError::ScopeDenied)?;
        let mut kept = Vec::new();
        for app in apps {
            let reference = AppRef { info: app.clone() };
            if guard::admit_app(&scope, &self.inner, &reference).is_ok() {
                kept.push(app);
            }
        }
        self.handoff(&scope)?;
        Ok(kept)
    }

    fn discovery_status(&self) -> DiscoveryStatus {
        if self.activity.stop_requested() || self.lease.current().is_none() {
            return DiscoveryStatus::default();
        }
        self.inner.discovery_status()
    }

    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef> {
        let apps = self.list_apps().await?;
        let mut found = apps
            .into_iter()
            .filter(|app| guard::matches_query(app, &query.0));
        let Some(first) = found.next() else {
            return Err(DriverError::ScopeDenied);
        };
        if found.next().is_some() {
            return Err(DriverError::AmbiguousApp);
        }
        Ok(AppRef { info: first })
    }

    async fn launch_app(&self, _app: &AppRef) -> Result<AppRef> {
        let _scope = self.open()?;
        Err(DriverError::ScopeDenied)
    }

    async fn list_windows(&self, app: Option<&AppRef>) -> Result<Vec<gcu_protocol::WindowInfo>> {
        let scope = self.open()?;
        if let Some(app) = app {
            guard::admit_app(&scope, &self.inner, app)?;
            let windows = self
                .inner
                .list_windows(Some(app))
                .await
                .map_err(guard::scrub)?;
            let kept = guard::filter_windows(&scope, &self.inner, app, windows);
            self.handoff(&scope)?;
            return Ok(kept);
        }
        let apps = self.list_apps().await?;
        let mut all = Vec::new();
        for info in apps {
            let app = AppRef { info };
            let windows = self
                .inner
                .list_windows(Some(&app))
                .await
                .map_err(guard::scrub)?;
            all.extend(guard::filter_windows(&scope, &self.inner, &app, windows));
        }
        self.handoff(&scope)?;
        Ok(all)
    }

    async fn key_window(&self, app: &AppRef) -> Result<gcu_protocol::WindowInfo> {
        let scope = self.open()?;
        guard::admit_app(&scope, &self.inner, app)?;
        let windows = self
            .inner
            .list_windows(Some(app))
            .await
            .map_err(guard::scrub)?;
        let kept = guard::filter_windows(&scope, &self.inner, app, windows);
        self.handoff(&scope)?;
        let info = kept.into_iter().next().ok_or(DriverError::ScopeDenied)?;
        let window = WindowRef {
            app: app.clone(),
            id: info.id.clone(),
        };
        let current = self.lease.current().ok_or(DriverError::ScopeDenied)?;
        guard::admit_window(&current, &self.inner, &window)?;
        if self.activity.stop_requested() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        Ok(info)
    }

    async fn snapshot_tree(
        &self,
        win: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> Result<RawTree> {
        let scope = self.open()?;
        let binding = guard::admit_window(&scope, &self.inner, win)?;
        let mut raw = self
            .inner
            .snapshot_tree(win, opts, cancel)
            .await
            .map_err(guard::scrub)?;
        let mut cache = self.seals.lock().unwrap_or_else(|err| err.into_inner());
        guard::prepare_cache(&mut cache, &scope);
        guard::seal_tree(&mut raw, &scope, &binding, &self.inner, &mut cache)?;
        drop(cache);
        self.handoff(&scope)?;
        Ok(raw)
    }

    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture> {
        let scope = self.open()?;
        guard::admit_window(&scope, &self.inner, win)?;
        let shot = self
            .inner
            .capture(win, opts, cancel)
            .await
            .map_err(guard::scrub)?;
        self.handoff(&scope)?;
        Ok(shot)
    }

    async fn window_geometry(&self, win: &WindowRef) -> Result<crate::model::Geometry> {
        let scope = self.open()?;
        guard::admit_window(&scope, &self.inner, win)?;
        let geometry = self
            .inner
            .window_geometry(win)
            .await
            .map_err(guard::scrub)?;
        self.handoff(&scope)?;
        Ok(geometry)
    }

    async fn focused_element(&self, app: &AppRef) -> Result<Option<NativeHandle>> {
        let scope = self.open()?;
        let instance = guard::admit_app(&scope, &self.inner, app)?;
        let found = self
            .inner
            .focused_element(app)
            .await
            .map_err(guard::scrub)?;
        let Some(handle) = found else {
            self.handoff(&scope)?;
            return Ok(None);
        };
        let live = self
            .inner
            .element_binding(&handle)
            .ok_or(DriverError::ScopeDenied)?;
        if live.window().owner() != &instance || !scope.admits_window(live.window()) {
            return Err(DriverError::ScopeDenied);
        }
        let window = live.window().clone();
        let mut cache = self.seals.lock().unwrap_or_else(|err| err.into_inner());
        guard::prepare_cache(&mut cache, &scope);
        let sealed = guard::seal_handle(&handle, &scope, &window, &self.inner, &mut cache)?;
        drop(cache);
        self.handoff(&scope)?;
        Ok(Some(sealed))
    }

    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> Result<Delivery, DriverError> {
        let scope = self.open()?;
        let (action, post) = guard::rewrite(req, &scope, &self.inner)?;
        let mut epoch = self.lease.subscribe();
        epoch.borrow_and_update();
        let work = self.inner.perform(&action, cancel);
        tokio::pin!(work);
        let result = tokio::select! {
            biased;
            _ = epoch.changed() => {
                cancel.cancel();
                self.inner.release_all_input().await;
                work.await
            }
            result = &mut work => result,
        };
        self.finish_perform(&scope, post, result)
    }

    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        let scope = self.open().ok()?;
        guard::admit_window(&scope, &self.inner, win).ok()?;
        let inner = self.inner.subscribe(win)?;
        Some(Box::pin(ScopedEvents {
            inner,
            run_id: scope.run_id().to_owned(),
            generation: scope.generation(),
            lease: self.lease.clone(),
            activity: ArcActivity(self.activity.clone()),
        }))
    }

    async fn release_all_input(&self) {
        self.inner.release_all_input().await;
    }
}

impl<D> ScopedDriver<D>
where
    D: Driver + InstanceEvidence,
{
    fn finish_perform(
        &self,
        opened: &FixtureScope,
        post: PostCheck,
        result: Result<Delivery, DriverError>,
    ) -> Result<Delivery, DriverError> {
        let maybe_executed = match &result {
            Ok(_) => true,
            Err(err) => err.maybe_executed(),
        };
        if self.activity.stop_requested() {
            return Err(DriverError::Cancelled { maybe_executed });
        }
        if !self.lease.matches(opened) {
            return if maybe_executed {
                Err(DriverError::Native {
                    message: "evaluation scope changed during delivery".into(),
                    maybe_executed: true,
                })
            } else {
                Err(DriverError::ScopeDenied)
            };
        }
        if guard::identity_changed(&self.inner, &post) {
            return Err(DriverError::Native {
                message: "target identity changed during delivery".into(),
                maybe_executed: true,
            });
        }
        result.map_err(guard::scrub)
    }
}

struct ArcActivity(std::sync::Arc<dyn UserActivity>);

struct ScopedEvents {
    inner: BoxStream<'static, UiEvent>,
    run_id: String,
    generation: u64,
    lease: ScopeLease,
    activity: ArcActivity,
}

impl Stream for ScopedEvents {
    type Item = UiEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<UiEvent>> {
        let this = self.get_mut();
        if this.activity.0.stop_requested() || !this.lease.same_epoch(&this.run_id, this.generation)
        {
            return Poll::Ready(None);
        }
        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(event)) => {
                if this.activity.0.stop_requested()
                    || !this.lease.same_epoch(&this.run_id, this.generation)
                {
                    Poll::Ready(None)
                } else {
                    Poll::Ready(Some(event))
                }
            }
            other => other,
        }
    }
}
