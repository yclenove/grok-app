//! Host Computer Use broker. All tool paths enter here.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::adapter::{ActionScope, ComputerUseAdapter, DispatchRequest, SurfaceKind, TargetInfo};
use super::browser::{ExistingBrowserAdapter, ExistingTabHost, ManagedBrowserAdapter};
use super::error::BrokerError;
use super::lease::DesktopLease;
use super::preview::PreviewController;
use super::protocol::{ActionOutcome, ActionRequest, Observation, OutcomeKind, PROTOCOL_VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopState {
    Running,
    StopRequested,
    Stopped,
}

/// Opaque hand-off from the synchronous stop fence to the potentially
/// blocking surface cleanup.  The generation binds cleanup to the run that
/// was fenced; a later lifecycle operation cannot make an old cleanup release
/// a newer execution target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopCleanupTicket {
    pub(crate) run_id: String,
    pub(crate) generation: u64,
}

impl StopCleanupTicket {
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

impl StopState {
    pub fn as_str(self) -> &'static str {
        match self {
            StopState::Running => "running",
            StopState::StopRequested => "stop_requested",
            StopState::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoverySource {
    User,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TraceAudience {
    Model,
    Ui,
    Support,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceEvent {
    pub kind: String,
    pub run_id: String,
    pub detail: String,
    pub ms: u64,
    pub audience: TraceAudience,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedRun {
    pub app_session_id: String,
    pub run_id: String,
    pub traces: Vec<TraceEvent>,
    pub last_target_id: Option<String>,
    /// Opaque consumed actionIds only. Never grant, snapshot, URL, or form values.
    #[serde(default)]
    pub consumed_action_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTimings {
    pub observe_ms: u64,
    pub act_ms: u64,
    pub verify_ms: u64,
}

const TRACE_CAP: usize = 256;
// Keep all action IDs for a run: eviction would allow an old side effect to replay.
const ACTIONS_PER_RUN: usize = 10_000;
const OBSERVES_PER_RUN: usize = 10_000;

#[derive(Clone)]
struct AuthorizedTarget {
    target_id: String,
    title: String,
    target_generation: u64,
    surface: SurfaceKind,
    executor_generation: u64,
}

struct RunState {
    app_session_id: String,
    generation: u64,
    target: Option<AuthorizedTarget>,
    snapshot_id: Option<String>,
    geometry_revision: u64,
    stop: StopState,
    seen: HashMap<String, ActionOutcome>,
    paused: bool,
    recovery: Option<RecoverySource>,
    in_flight: Arc<AtomicBool>,
    cancellation: crate::execution::ActionCancellation,
    last_image_ok: bool,
    image_size: (u32, u32),
    element_refs: std::collections::HashSet<String>,
    element_boxes: Vec<crate::protocol::ObservationNode>,
    last_unknown: bool,
    timings: RunTimings,
    geometry_epoch: u64,
    observe_count: usize,
    stop_cleanup_pending: bool,
    stop_cleanup_in_flight: bool,
    pause_cleanup_pending: bool,
    resume_in_flight: bool,
    fingerprints: HashMap<String, [u8; 32]>,
    browser_replay: HashMap<String, browser_dispatch::BrowserReplay>,
}

fn drop_execution_identity(run: &mut RunState) {
    run.target = None;
    run.snapshot_id = None;
    run.last_image_ok = false;
    run.geometry_revision = 0;
    run.geometry_epoch = 0;
    run.element_refs.clear();
    run.element_boxes.clear();
    run.last_unknown = false;
    run.image_size = (0, 0);
}

fn fresh_run(app_session_id: String, recovery: Option<RecoverySource>) -> RunState {
    RunState {
        app_session_id,
        generation: 1,
        target: None,
        snapshot_id: None,
        geometry_revision: 0,
        stop: StopState::Running,
        seen: HashMap::new(),
        paused: false,
        recovery,
        in_flight: Arc::new(AtomicBool::new(false)),
        cancellation: crate::execution::ActionCancellation::default(),
        last_image_ok: false,
        image_size: (0, 0),
        element_refs: Default::default(),
        element_boxes: Vec::new(),
        last_unknown: false,
        timings: RunTimings::default(),
        geometry_epoch: 0,
        observe_count: 0,
        stop_cleanup_pending: false,
        stop_cleanup_in_flight: false,
        pause_cleanup_pending: false,
        resume_in_flight: false,
        fingerprints: HashMap::new(),
        browser_replay: HashMap::new(),
    }
}

fn push_trace(inner: &mut Inner, kind: &str, run_id: &str, detail: &str, audience: TraceAudience) {
    let detail = crate::privacy::sanitize_trace_detail(detail, audience);
    inner.traces.push(TraceEvent {
        kind: kind.to_string(),
        run_id: run_id.to_string(),
        detail,
        ms: inner.started.elapsed().as_millis() as u64,
        audience,
    });
    if inner.traces.len() > TRACE_CAP {
        let drop_n = inner.traces.len() - TRACE_CAP;
        inner.traces.drain(0..drop_n);
    }
}

fn image_is_visual(obs: &Observation) -> bool {
    obs.image.width > 0
        && obs.image.height > 0
        && obs
            .image
            .png_base64
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
}

pub struct BrokerOptions {
    pub instance_id: String,
    pub lease_path: PathBuf,
    pub feature_enabled: bool,
    pub action_timeout: Duration,
    pub action_budget: usize,
    pub observe_budget: usize,
}

impl Default for BrokerOptions {
    fn default() -> Self {
        let instance_id = Uuid::new_v4().to_string();
        Self {
            instance_id: instance_id.clone(),
            lease_path: std::env::temp_dir().join("grok-computer-use-desktop.lease"),
            feature_enabled: false,
            action_timeout: Duration::from_secs(8),
            action_budget: ACTIONS_PER_RUN,
            observe_budget: OBSERVES_PER_RUN,
        }
    }
}

struct Inner {
    feature_enabled: bool,
    runs: HashMap<String, RunState>,
    dropped_late: u64,
    traces: Vec<TraceEvent>,
    started: Instant,
}

pub struct ComputerUseBroker {
    inner: Mutex<Inner>,
    executors: surface_executor::SurfaceExecutorRegistry,
    lease: DesktopLease,
    preview: PreviewController,
    tabs: Arc<ExistingTabHost>,
    managed_browser: Arc<ManagedBrowserAdapter>,
    existing_browser: Arc<ExistingBrowserAdapter>,
    existing_authorization: Mutex<()>,
    instance_id: String,
    action_timeout: Duration,
    action_budget: usize,
    observe_budget: usize,
}

struct InFlightClear(Arc<AtomicBool>);

impl Drop for InFlightClear {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl ComputerUseBroker {
    pub fn new(adapter: Arc<dyn ComputerUseAdapter>, opts: BrokerOptions) -> Self {
        let tabs = Arc::new(ExistingTabHost::new());
        let managed_browser = Arc::new(ManagedBrowserAdapter::new(tabs.clone()));
        let existing_browser = Arc::new(ExistingBrowserAdapter::new(tabs.clone()));
        Self {
            inner: Mutex::new(Inner {
                feature_enabled: opts.feature_enabled,
                runs: HashMap::new(),
                dropped_late: 0,
                traces: Vec::new(),
                started: Instant::now(),
            }),
            executors: surface_executor::SurfaceExecutorRegistry::new(adapter),
            lease: DesktopLease::new(opts.lease_path),
            preview: PreviewController::new(),
            tabs,
            managed_browser,
            existing_browser,
            existing_authorization: Mutex::new(()),
            instance_id: opts.instance_id,
            action_timeout: opts.action_timeout,
            action_budget: opts.action_budget.max(1),
            observe_budget: opts.observe_budget.max(1),
        }
    }

    pub fn tabs(&self) -> &ExistingTabHost {
        self.tabs.as_ref()
    }

    pub fn register_managed_browser_adapter(&self) -> Result<u64, BrokerError> {
        self.register_surface_adapter(SurfaceKind::ManagedBrowser, self.managed_browser.clone())
    }

    pub fn register_existing_browser_adapter(&self) -> Result<u64, BrokerError> {
        self.register_surface_adapter(SurfaceKind::ExistingTab, self.existing_browser.clone())
    }

    /// Compatibility accessor for Desktop-only probes. Product execution uses
    /// the authorized run's surface executor.
    pub fn adapter(&self) -> Arc<dyn ComputerUseAdapter> {
        self.executors
            .resolve(SurfaceKind::Desktop)
            .expect("Desktop executor is registered by ComputerUseBroker::new")
            .adapter
    }

    pub fn register_surface_adapter(
        &self,
        surface: SurfaceKind,
        adapter: Arc<dyn ComputerUseAdapter>,
    ) -> Result<u64, BrokerError> {
        self.executors.register(surface, adapter)
    }

    pub(crate) fn require_surface_executor(
        &self,
        surface: SurfaceKind,
    ) -> Result<surface_executor::SurfaceExecutor, BrokerError> {
        self.executors.resolve(surface)
    }

    fn executor_for_target(
        &self,
        target: &AuthorizedTarget,
    ) -> Result<surface_executor::SurfaceExecutor, BrokerError> {
        let executor = self.require_surface_executor(target.surface)?;
        if executor.generation != target.executor_generation {
            return Err(BrokerError::IdentityMismatch("surfaceExecutorGeneration"));
        }
        Ok(executor)
    }

    fn authorized_binding(
        &self,
        run_id: &str,
    ) -> Result<(AuthorizedTarget, surface_executor::SurfaceExecutor), BrokerError> {
        let target = {
            let inner = self.inner.lock();
            if !inner.feature_enabled {
                return Err(BrokerError::FeatureDisabled);
            }
            let run = inner.runs.get(run_id).ok_or(BrokerError::RunNotFound)?;
            run.target.clone().ok_or(BrokerError::TargetUnauthorized)?
        };
        let executor = self.executor_for_target(&target)?;
        Ok((target, executor))
    }

    pub fn set_feature_enabled(&self, on: bool) {
        for ticket in self.fence_feature_enabled(on) {
            let _ = self.finish_stop_cleanup(&ticket);
        }
    }

    /// Change the feature gate and synchronously fence every run without
    /// touching a surface adapter or browser worker. Host applications retain
    /// the returned tickets and finish cleanup on their blocking boundary.
    pub fn fence_feature_enabled(&self, on: bool) -> Vec<StopCleanupTicket> {
        let runs = {
            let mut inner = self.inner.lock();
            inner.feature_enabled = on;
            if on {
                return Vec::new();
            }
            self.tabs.revoke_pairing();
            inner.runs.keys().cloned().collect::<Vec<_>>()
        };
        runs.into_iter()
            .filter_map(|run_id| self.fence_stop(&run_id).ok().flatten())
            .collect()
    }

    pub fn feature_enabled(&self) -> bool {
        self.inner.lock().feature_enabled
    }

    #[cfg(test)]
    pub(crate) fn test_run_count(&self) -> usize {
        self.inner.lock().runs.len()
    }

    pub fn open_run(&self, app_session_id: &str, run_id: &str) -> Result<(), BrokerError> {
        if app_session_id.trim().is_empty() || run_id.trim().is_empty() {
            return Err(BrokerError::IdentityMismatch("session/run"));
        }
        let mut g = self.inner.lock();
        if !g.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        if let Some(run) = g.runs.get(run_id) {
            if run.app_session_id != app_session_id {
                return Err(BrokerError::IdentityMismatch("appSessionId"));
            }
            if run.stop != StopState::Running {
                return Err(BrokerError::StopRequested);
            }
        } else {
            g.runs.insert(
                run_id.to_string(),
                fresh_run(app_session_id.to_string(), None),
            );
            push_trace(&mut g, "open", run_id, app_session_id, TraceAudience::Model);
        }
        Ok(())
    }

    pub fn has_authorized_target(&self, run_id: &str) -> bool {
        self.inner
            .lock()
            .runs
            .get(run_id)
            .is_some_and(|run| run.target.is_some() && run.stop == StopState::Running)
    }

    pub fn require_owner(&self, session: &str, run_id: &str) -> Result<(), BrokerError> {
        let inner = self.inner.lock();
        let run = inner.runs.get(run_id).ok_or(BrokerError::RunNotFound)?;
        if session.is_empty() || run.app_session_id != session {
            return Err(BrokerError::IdentityMismatch("appSessionId"));
        }
        Ok(())
    }

    pub fn authorize_target(&self, run_id: &str, target_id: &str) -> Result<u64, BrokerError> {
        let classified = surface_router::SurfaceRouter::classify_target_id(target_id);
        let _existing_guard =
            (classified == SurfaceKind::ExistingTab).then(|| self.existing_authorization.lock());
        match classified {
            SurfaceKind::Desktop => self.authorize_desktop_window(run_id, target_id),
            SurfaceKind::ManagedBrowser => {
                let session = self
                    .inner
                    .lock()
                    .runs
                    .get(run_id)
                    .ok_or(BrokerError::RunNotFound)?
                    .app_session_id
                    .clone();
                self.authorize_managed_target(&session, run_id, target_id)
            }
            SurfaceKind::ExistingTab | SurfaceKind::WebView => {
                self.authorize_non_desktop(run_id, target_id, classified)
            }
        }
    }

    pub fn authorize_on_surface(
        &self,
        session: &str,
        run_id: &str,
        surface: SurfaceKind,
        target_id: &str,
    ) -> Result<u64, BrokerError> {
        surface_router::SurfaceRouter::authorize(self, session, run_id, surface, target_id)
    }

    pub(crate) fn reject_cross_surface(
        &self,
        run_id: &str,
        surface: SurfaceKind,
    ) -> Result<(), BrokerError> {
        let g = self.inner.lock();
        let run = g.runs.get(run_id).ok_or(BrokerError::RunNotFound)?;
        if let Some(existing) = &run.target {
            if existing.surface != surface {
                return Err(BrokerError::Schema(
                    "session cannot reuse a target across surfaces".into(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn authorize_desktop_window(
        &self,
        run_id: &str,
        target_id: &str,
    ) -> Result<u64, BrokerError> {
        if surface_router::SurfaceRouter::classify_target_id(target_id) != SurfaceKind::Desktop {
            return Err(BrokerError::Schema(
                "profile id is not a desktop window; open_managed_profile first".into(),
            ));
        }
        self.prepare_authorize(run_id)?;
        let executor = self.require_surface_executor(SurfaceKind::Desktop)?;
        if !executor.adapter.target_alive(target_id) {
            return Err(BrokerError::DeadTarget);
        }
        executor
            .adapter
            .claim_target_for_run(run_id, target_id)
            .map_err(BrokerError::Adapter)?;
        let title = executor
            .adapter
            .list_targets_for_run(run_id)
            .map_err(BrokerError::Adapter)?
            .into_iter()
            .filter(|t| !crate::adapter::skip_desktop_target(&t.title, &t.app_name))
            .find(|t| t.target_id == target_id)
            .map(|t| t.title);
        let Some(title) = title else {
            executor.adapter.release_target_for_run(run_id, target_id);
            return Err(BrokerError::TargetUnauthorized);
        };
        let result = self.commit_authorize(
            run_id,
            target_id,
            title,
            SurfaceKind::Desktop,
            executor.generation,
            None,
        );
        if result.is_err() {
            executor.adapter.release_target_for_run(run_id, target_id);
        }
        result
    }

    pub(crate) fn authorize_non_desktop(
        &self,
        run_id: &str,
        target_id: &str,
        surface: SurfaceKind,
    ) -> Result<u64, BrokerError> {
        self.prepare_authorize(run_id)?;
        if surface == SurfaceKind::Desktop {
            return Err(BrokerError::Schema(
                "desktop must use desktop adapter".into(),
            ));
        }
        let executor = self.require_surface_executor(surface)?;
        if !executor.adapter.target_alive(target_id) {
            return Err(BrokerError::DeadTarget);
        }
        executor
            .adapter
            .claim_target_for_run(run_id, target_id)
            .map_err(BrokerError::Adapter)?;
        let title = executor
            .adapter
            .list_targets_for_run(run_id)
            .map_err(BrokerError::Adapter)?
            .into_iter()
            .find(|target| target.target_id == target_id)
            .map(|target| target.title);
        let Some(title) = title else {
            if surface != SurfaceKind::ExistingTab {
                executor.adapter.release_target_for_run(run_id, target_id);
            }
            return Err(BrokerError::TargetUnauthorized);
        };
        let result =
            self.commit_authorize(run_id, target_id, title, surface, executor.generation, None);
        // Existing-tab picker transactions own their exact grant rollback. A
        // failed reauthorization must not revoke a previously accepted grant.
        if result.is_err() && surface != SurfaceKind::ExistingTab {
            executor.adapter.release_target_for_run(run_id, target_id);
        }
        result
    }

    fn prepare_authorize(&self, run_id: &str) -> Result<(), BrokerError> {
        let g = self.inner.lock();
        if !g.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        let run = g.runs.get(run_id).ok_or(BrokerError::RunNotFound)?;
        if run.stop != StopState::Running {
            return Err(BrokerError::StopRequested);
        }
        if run.in_flight.load(Ordering::SeqCst) {
            return Err(BrokerError::LeaseHeld {
                run_id: run_id.into(),
            });
        }
        Ok(())
    }

    fn commit_authorize(
        &self,
        run_id: &str,
        target_id: &str,
        title: String,
        surface: SurfaceKind,
        executor_generation: u64,
        admission: Option<(u64, &InFlightClear)>,
    ) -> Result<u64, BrokerError> {
        let mut g = self.inner.lock();
        if !g.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        let gen = {
            let run = g.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
            if run.stop != StopState::Running {
                return Err(BrokerError::StopRequested);
            }
            let owns_admission = admission.is_some_and(|(generation, permit)| {
                generation == run.generation && Arc::ptr_eq(&permit.0, &run.in_flight)
            });
            if admission.is_some() && (!owns_admission || run.paused) {
                return Err(BrokerError::IdentityMismatch("authorization admission"));
            }
            if run.in_flight.load(Ordering::SeqCst) != owns_admission {
                return Err(BrokerError::LeaseHeld {
                    run_id: run_id.into(),
                });
            }
            if let Some(existing) = &run.target {
                if existing.surface != surface {
                    return Err(BrokerError::Schema(
                        "session cannot reuse a target across surfaces".into(),
                    ));
                }
            }
            run.cancellation.cancel();
            run.cancellation = crate::execution::ActionCancellation::default();
            run.generation = run.generation.saturating_add(1);
            let gen = run.generation;
            run.target = Some(AuthorizedTarget {
                target_id: target_id.to_string(),
                title,
                target_generation: gen,
                surface,
                executor_generation,
            });
            run.snapshot_id = None;
            run.last_image_ok = false;
            run.geometry_epoch = 0;
            gen
        };
        push_trace(
            &mut g,
            "authorize",
            run_id,
            &format!("{}:{}", surface.as_wire(), target_id),
            TraceAudience::Model,
        );
        Ok(gen)
    }

    pub fn list_targets(&self, run_id: &str) -> Result<Vec<TargetInfo>, BrokerError> {
        self.require_run(run_id)?;
        let authorized = self
            .inner
            .lock()
            .runs
            .get(run_id)
            .and_then(|run| run.target.clone());
        if let Some(target) = authorized.filter(|target| target.surface != SurfaceKind::Desktop) {
            // A browser/WebView run must not enumerate the desktop or inherit its
            // availability/permission failures. Discovery has a separate Host API.
            return self
                .executor_for_target(&target)?
                .adapter
                .list_targets_for_run(run_id)
                .map_err(BrokerError::Adapter);
        }
        let mut rows = self.visible_targets(Some(run_id))?;
        for tab in self.tabs().list_for_run(run_id) {
            if tab.borrowed || tab.user_owned {
                continue;
            }
            rows.push(TargetInfo {
                target_id: tab.tab_id,
                title: tab.title,
                app_name: if tab.borrowed {
                    "existing-tabs".into()
                } else {
                    "managed-browser".into()
                },
                kind: if tab.borrowed {
                    "existing-tab".into()
                } else {
                    "managed-tab".into()
                },
                pid: None,
                backend: if tab.borrowed {
                    "existing-tabs".into()
                } else {
                    "managed-browser".into()
                },
                execution_mode: "exclusive".into(),
                replay_policy: "never".into(),
                lifecycle_stamp: tab.document_generation.max(1),
                display_id: "browser".into(),
                coordinate_space: "css-pixels".into(),
                scope_label: "session-owned browser tab".into(),
            });
        }
        Ok(rows)
    }

    /// Host target picker may discover metadata before creating a task/grant.
    pub fn discover_targets(&self) -> Result<Vec<TargetInfo>, BrokerError> {
        self.list_targets_for_surface(SurfaceKind::Desktop)
    }

    /// List targets for one product surface. Never falls back to Desktop.
    pub fn list_targets_for_surface(
        &self,
        surface: SurfaceKind,
    ) -> Result<Vec<TargetInfo>, BrokerError> {
        if !self.feature_enabled() {
            return Err(BrokerError::FeatureDisabled);
        }
        match surface {
            SurfaceKind::Desktop => self.visible_targets(None),
            SurfaceKind::ManagedBrowser => Ok(self.managed_browser_picker_targets()),
            SurfaceKind::ExistingTab => Ok(self.existing_tab_picker_targets()),
            SurfaceKind::WebView => self
                .require_surface_executor(SurfaceKind::WebView)?
                .adapter
                .list_targets()
                .map_err(BrokerError::Adapter),
        }
    }

    /// Host picker refresh for a known run. Desktop/retained WebView targets
    /// must preserve native run ownership. Browser profile/shared-tab choices
    /// are pre-authorization candidates, not existing agent-owned resources.
    /// The App must also validate that this run belongs to its selected chat.
    pub fn list_targets_for_surface_in_run(
        &self,
        surface: SurfaceKind,
        run_id: &str,
    ) -> Result<Vec<TargetInfo>, BrokerError> {
        self.require_run(run_id)?;
        match surface {
            SurfaceKind::Desktop => self.visible_targets(Some(run_id)),
            SurfaceKind::WebView => self
                .require_surface_executor(SurfaceKind::WebView)?
                .adapter
                .list_targets_for_run(run_id)
                .map_err(BrokerError::Adapter),
            SurfaceKind::ManagedBrowser | SurfaceKind::ExistingTab => {
                self.list_targets_for_surface(surface)
            }
        }
    }

    fn managed_browser_picker_targets(&self) -> Vec<TargetInfo> {
        self.tabs()
            .list_managed_profiles()
            .into_iter()
            .map(|name| TargetInfo {
                target_id: format!("managed-profile:{name}"),
                title: name,
                app_name: "managed-browser".into(),
                kind: "managed-profile".into(),
                pid: None,
                backend: "managed-browser".into(),
                execution_mode: "exclusive".into(),
                replay_policy: "never".into(),
                lifecycle_stamp: 1,
                display_id: "managed-browser".into(),
                coordinate_space: "css-pixels".into(),
                scope_label: "App-owned managed browser profile".into(),
            })
            .collect()
    }

    fn existing_tab_picker_targets(&self) -> Vec<TargetInfo> {
        self.tabs()
            .list_shared_candidates()
            .into_iter()
            .map(|(id, title, _url)| TargetInfo {
                target_id: format!("existing-tab:{id}"),
                title,
                app_name: "existing-tabs".into(),
                kind: "existing-tab".into(),
                pid: None,
                backend: "existing-tabs".into(),
                execution_mode: "borrow".into(),
                replay_policy: "never".into(),
                lifecycle_stamp: 1,
                display_id: "existing-tabs".into(),
                coordinate_space: "css-pixels".into(),
                scope_label: "User-shared existing tab".into(),
            })
            .collect()
    }

    fn visible_targets(&self, run_id: Option<&str>) -> Result<Vec<TargetInfo>, BrokerError> {
        let executor = self.require_surface_executor(SurfaceKind::Desktop)?;
        let listed = match run_id {
            Some(run_id) => executor.adapter.list_targets_for_run(run_id),
            None => executor.adapter.list_targets(),
        }
        .map_err(BrokerError::Adapter)?;
        Ok(listed
            .into_iter()
            .filter(|t| !crate::adapter::skip_desktop_target(&t.title, &t.app_name))
            .collect())
    }

    pub fn report_late(&self, run_id: &str, generation: u64, action_id: &str) -> bool {
        let mut g = self.inner.lock();
        let Some(run) = g.runs.get(run_id) else {
            g.dropped_late += 1;
            return false;
        };
        if run.generation != generation {
            g.dropped_late += 1;
            return false;
        }
        run.seen.contains_key(action_id)
    }

    pub fn dropped_late(&self) -> u64 {
        self.inner.lock().dropped_late
    }

    pub fn traces(&self, run_id: Option<&str>, model_only: bool) -> Vec<TraceEvent> {
        let g = self.inner.lock();
        g.traces
            .iter()
            .filter(|t| {
                if model_only && t.audience != TraceAudience::Model {
                    return false;
                }
                match run_id {
                    Some(id) => t.run_id == id,
                    None => true,
                }
            })
            .cloned()
            .collect()
    }

    pub fn timings(&self, run_id: &str) -> Result<RunTimings, BrokerError> {
        Ok(self
            .inner
            .lock()
            .runs
            .get(run_id)
            .ok_or(BrokerError::RunNotFound)?
            .timings
            .clone())
    }

    pub fn act_defaults(&self, run_id: &str) -> Result<(String, u64, String, u64), BrokerError> {
        let g = self.inner.lock();
        let run = g.runs.get(run_id).ok_or(BrokerError::RunNotFound)?;
        let t = run.target.as_ref().ok_or(BrokerError::TargetUnauthorized)?;
        let snap = run
            .snapshot_id
            .clone()
            .ok_or(BrokerError::IdentityMismatch("snapshotId"))?;
        Ok((
            t.target_id.clone(),
            t.target_generation,
            snap,
            run.geometry_revision,
        ))
    }

    pub fn authorized_target(&self, run_id: &str) -> Result<(String, u64), BrokerError> {
        let inner = self.inner.lock();
        if !inner.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        let run = inner.runs.get(run_id).ok_or(BrokerError::RunNotFound)?;
        if run.stop != StopState::Running {
            return Err(BrokerError::StopRequested);
        }
        let target = run.target.as_ref().ok_or(BrokerError::TargetUnauthorized)?;
        Ok((target.target_id.clone(), target.target_generation))
    }

    pub fn model_snapshot_id(&self, run_id: &str) -> Option<String> {
        self.inner
            .lock()
            .runs
            .get(run_id)
            .and_then(|run| run.snapshot_id.clone())
    }

    pub fn target_title(&self, run_id: &str) -> Option<String> {
        self.inner
            .lock()
            .runs
            .get(run_id)
            .and_then(|run| run.target.as_ref())
            .map(|t| t.title.clone())
    }

    pub fn authorized_target_alive(&self, run_id: &str) -> bool {
        let Ok((target, executor)) = self.authorized_binding(run_id) else {
            return false;
        };
        executor.adapter.target_alive(&target.target_id)
    }

    pub fn recovery_source(&self, run_id: &str) -> Option<RecoverySource> {
        self.inner.lock().runs.get(run_id).and_then(|r| r.recovery)
    }

    pub fn capabilities(&self) -> super::adapter::Capabilities {
        self.require_surface_executor(SurfaceKind::Desktop)
            .expect("Desktop executor is registered by ComputerUseBroker::new")
            .adapter
            .capabilities()
    }

    pub fn capabilities_for_run(
        &self,
        run_id: &str,
    ) -> Result<super::adapter::Capabilities, BrokerError> {
        let (_, executor) = self.authorized_binding(run_id)?;
        Ok(executor.adapter.capabilities())
    }

    fn require_run(&self, run_id: &str) -> Result<(), BrokerError> {
        let g = self.inner.lock();
        if !g.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        if !g.runs.contains_key(run_id) {
            return Err(BrokerError::RunNotFound);
        }
        Ok(())
    }

    pub fn persist_run(&self, run_id: &str) -> Result<PersistedRun, BrokerError> {
        let g = self.inner.lock();
        let run = g.runs.get(run_id).ok_or(BrokerError::RunNotFound)?;
        let mut consumed_action_ids: Vec<String> = run.seen.keys().cloned().collect();
        consumed_action_ids.sort();
        Ok(PersistedRun {
            app_session_id: run.app_session_id.clone(),
            run_id: run_id.to_string(),
            traces: g
                .traces
                .iter()
                .filter(|t| t.run_id == run_id)
                .cloned()
                .collect(),
            last_target_id: run.target.as_ref().map(|t| t.target_id.clone()),
            consumed_action_ids,
        })
    }

    /// Restores traces and run identity. Does not restore authorization or snapshot.
    pub fn restore_run(&self, snap: PersistedRun) -> Result<(), BrokerError> {
        if snap.app_session_id.trim().is_empty() || snap.run_id.trim().is_empty() {
            return Err(BrokerError::IdentityMismatch("session/run"));
        }
        self.open_run(&snap.app_session_id, &snap.run_id)?;
        let mut g = self.inner.lock();
        {
            let run = g
                .runs
                .get_mut(&snap.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            run.target = None;
            run.snapshot_id = None;
            run.last_image_ok = false;
            run.geometry_epoch = 0;
            run.paused = false;
            run.observe_count = 0;
            run.browser_replay.clear();
            for action_id in &snap.consumed_action_ids {
                if action_id.trim().is_empty() {
                    continue;
                }
                run.seen.entry(action_id.clone()).or_insert(ActionOutcome {
                    action_id: action_id.clone(),
                    run_id: snap.run_id.clone(),
                    kind: OutcomeKind::Rejected,
                    executed: false,
                    reason: Some("restored run does not replay actions".into()),
                    generation: run.generation,
                });
            }
        }
        for event in snap.traces {
            g.traces.push(event);
        }
        if g.traces.len() > TRACE_CAP {
            let drop_n = g.traces.len() - TRACE_CAP;
            g.traces.drain(0..drop_n);
        }
        Ok(())
    }

    pub fn persist_run_to_path(
        &self,
        run_id: &str,
        path: &std::path::Path,
    ) -> Result<(), BrokerError> {
        let snap = self.persist_run(run_id)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| BrokerError::Adapter(e.to_string()))?;
        }
        let raw = serde_json::to_vec(&snap).map_err(|e| BrokerError::Adapter(e.to_string()))?;
        std::fs::write(path, raw).map_err(|e| BrokerError::Adapter(e.to_string()))
    }

    pub fn restore_run_from_path(
        &self,
        path: &std::path::Path,
    ) -> Result<PersistedRun, BrokerError> {
        let raw = std::fs::read(path).map_err(|e| BrokerError::Adapter(e.to_string()))?;
        let snap: PersistedRun =
            serde_json::from_slice(&raw).map_err(|e| BrokerError::Adapter(e.to_string()))?;
        self.restore_run(snap.clone())?;
        Ok(snap)
    }

    pub fn browser_navigate(
        &self,
        run_id: &str,
        tab_id: &str,
        url: &str,
        action_id: &str,
    ) -> Result<crate::browser::TabInfo, BrokerError> {
        match self.dispatch_browser(BrowserWrite {
            session_id: None,
            run_id,
            tab_id,
            action_id,
            page_generation: 0,
            op: BrowserOp::Navigate { url },
        })? {
            BrowserWriteResult::Tab(info) => Ok(info),
            _ => Err(BrokerError::Adapter(
                "navigate returned an unexpected result".into(),
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn browser_download(
        &self,
        run_id: &str,
        tab_id: &str,
        filename: &str,
        model_path: Option<&str>,
        action_id: &str,
        snapshot_id: &str,
        element_ref: Option<&str>,
    ) -> Result<std::path::PathBuf, BrokerError> {
        match self.dispatch_browser(BrowserWrite {
            session_id: None,
            run_id,
            tab_id,
            action_id,
            page_generation: 0,
            op: BrowserOp::Download {
                filename,
                model_path,
                snapshot_id,
                element_ref: element_ref.unwrap_or(""),
            },
        })? {
            BrowserWriteResult::Download(path) => Ok(path),
            _ => Err(BrokerError::Adapter(
                "download returned an unexpected result".into(),
            )),
        }
    }

    pub fn browser_act(
        &self,
        run_id: &str,
        tab_id: &str,
        action: crate::browser::ManagedTabAction<'_>,
    ) -> Result<crate::browser::ManagedPage, BrokerError> {
        match self.dispatch_browser(BrowserWrite {
            session_id: None,
            run_id,
            tab_id,
            action_id: action.action_id,
            page_generation: action.page_generation,
            op: BrowserOp::Act {
                kind: action.kind,
                snapshot_id: action.snapshot_id,
                locator: action.locator,
                params: action.params,
            },
        })? {
            BrowserWriteResult::Page(page) => Ok(page),
            _ => Err(BrokerError::Adapter(
                "act returned an unexpected result".into(),
            )),
        }
    }

    pub fn browser_act_managed(
        &self,
        run_id: &str,
        profile: &str,
        action_id: &str,
        kind: &str,
        locator: crate::browser::ManagedLocator,
        params: serde_json::Value,
    ) -> Result<(), BrokerError> {
        let (tab_id, page_generation) = self.tabs.managed_primary_tab_identity(run_id, profile)?;
        let snapshot_id = self
            .tabs
            .tab_write_identity(run_id, &tab_id)?
            .snapshot_id
            .unwrap_or_default();
        let _ = self.browser_act(
            run_id,
            &tab_id,
            crate::browser::ManagedTabAction {
                page_generation,
                snapshot_id: &snapshot_id,
                action_id,
                kind,
                locator,
                params,
            },
        )?;
        Ok(())
    }

    pub fn browser_upload(
        &self,
        run_id: &str,
        tab_id: &str,
        action_id: &str,
        element_ref: &str,
        source: &std::path::Path,
    ) -> Result<(), BrokerError> {
        let identity = self.tabs.tab_write_identity(run_id, tab_id)?;
        let snapshot_id = identity.snapshot_id.unwrap_or_default();
        match self.dispatch_browser(BrowserWrite {
            session_id: None,
            run_id,
            tab_id,
            action_id,
            page_generation: identity.page_generation,
            op: BrowserOp::Upload {
                snapshot_id: &snapshot_id,
                element_ref,
                source,
            },
        })? {
            BrowserWriteResult::Uploaded => Ok(()),
            _ => Err(BrokerError::Adapter(
                "upload returned an unexpected result".into(),
            )),
        }
    }

    pub fn browser_open_tab(
        &self,
        session: &str,
        run_id: &str,
        profile: &str,
        action_id: &str,
    ) -> Result<crate::browser::TabInfo, BrokerError> {
        match self.dispatch_browser(BrowserWrite {
            session_id: Some(session),
            run_id,
            tab_id: "",
            action_id,
            page_generation: 0,
            op: BrowserOp::NewTab { profile },
        })? {
            BrowserWriteResult::Tab(info) => Ok(info),
            _ => Err(BrokerError::Adapter(
                "open tab returned an unexpected result".into(),
            )),
        }
    }

    pub fn browser_popup(
        &self,
        run_id: &str,
        profile: &str,
        action_id: &str,
        element_ref: &str,
    ) -> Result<crate::browser::ManagedPage, BrokerError> {
        let (tab_id, page_generation) = self.tabs.managed_primary_tab_identity(run_id, profile)?;
        match self.dispatch_browser(BrowserWrite {
            session_id: None,
            run_id,
            tab_id: &tab_id,
            action_id,
            page_generation,
            op: BrowserOp::Popup {
                profile,
                element_ref,
            },
        })? {
            BrowserWriteResult::Page(page) => Ok(page),
            _ => Err(BrokerError::Adapter(
                "popup returned an unexpected result".into(),
            )),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
mod gates;
#[cfg(any(test, feature = "test-support"))]
pub use gates::run_broker_gates;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_async_stop;
#[cfg(test)]
mod tests_browser_dispatch;
#[cfg(test)]
mod tests_browser_tools;
#[cfg(test)]
mod tests_cancel;
#[cfg(test)]
mod tests_click_nav;
#[cfg(test)]
mod tests_host_ledger;
#[cfg(test)]
mod tests_ipc_headers;
#[cfg(test)]
mod tests_keys_wait;
#[cfg(test)]
mod tests_lease_schema;
#[cfg(test)]
mod tests_managed_admission;
#[cfg(test)]
mod tests_managed_surface;
#[cfg(test)]
mod tests_metadata;
#[cfg(test)]
mod tests_observation_transactions;
#[cfg(test)]
mod tests_observe_bounds;
#[cfg(test)]
mod tests_observe_normalize;
#[cfg(test)]
mod tests_product_surface;
#[cfg(test)]
mod tests_races;
#[cfg(test)]
mod tests_status_overlay;
#[cfg(test)]
mod tests_typed_actions;
#[cfg(test)]
mod tests_wait_tabs;

mod actions;
mod browser_dispatch;
mod extension_actions;
mod observation;
mod pairing;
mod surface_executor;
mod surface_router;
mod target_readback;
pub use browser_dispatch::{BrowserOp, BrowserWrite, BrowserWriteResult};
pub use surface_router::SurfaceRouter;
mod lifecycle;
mod managed_authorization;
mod resume;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_auth;
#[cfg(test)]
mod tests_identity;
#[cfg(test)]
mod tests_outcomes;
#[cfg(test)]
mod tests_postcondition;
#[cfg(any(test, feature = "test-support"))]
mod verification_probe;
