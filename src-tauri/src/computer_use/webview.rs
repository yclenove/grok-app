//! Typed WebView subset. Bind is explicit; no eval and no cookie copy.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use grok_computer_use_core::execution::ActionCancellation;
use parking_lot::Mutex;

use super::adapter::{
    AdapterActResult, Capabilities, ComputerUseAdapter, DispatchRequest, DownloadError,
    DownloadRequest, DownloadResult, NavigationError, NavigationRequest, StagedDownload,
    StagedNavigation, StagedUpload, TargetInfo, UploadError, UploadRequest,
};
use super::protocol::{ActionKind, Observation};

mod authority;
mod candidates;
mod dom;
mod download;
#[cfg(windows)]
pub(crate) use download::maintain_download_buffers;
mod execution;
#[cfg(windows)]
pub(crate) mod isolated_windows;
pub(crate) mod lifecycle;
mod navigation;
mod queued_script;
mod script_result;
mod selection;
mod side_script;
mod upload;

pub use execution::ScriptOperation;
use execution::ScriptTracker;
pub(crate) use queued_script::QueuedScript;
pub use selection::WebViewSelection;

#[derive(Debug, Clone)]
pub struct LiveNavigationDocument {
    pub generation: u64,
    pub url: String,
    pub title: String,
}

#[cfg(any(test, feature = "computer-use-probe"))]
type ScopeFixture = (Weak<WebViewAdapter>, Vec<Arc<dyn LiveWebView>>);

/// Live App-owned webview. Probe uses a Host-owned WebView2; product uses side browser.
pub trait LiveWebView: Send + Sync {
    fn label(&self) -> String;
    fn tab_id(&self) -> String;
    fn url(&self) -> Result<String, String>;
    fn title(&self) -> Result<String, String>;
    fn is_alive(&self) -> bool;
    fn navigation_generation(&self) -> u64;
    /// Retained native document lock, not a native query or an execution grant.
    fn with_document_authority(
        &self,
        _generation: u64,
        _commit: &mut dyn FnMut() -> Result<(), super::BrokerError>,
    ) -> Result<(), super::BrokerError> {
        Err(super::BrokerError::TargetUnauthorized)
    }
    #[cfg(any(test, feature = "computer-use-probe"))]
    fn navigate(&self, url: &str) -> Result<u64, String>;
    #[cfg(any(test, feature = "computer-use-probe"))]
    fn host_script(&self, js: &str) -> Result<String, String> {
        let _ = js;
        Err("typed host script unavailable".into())
    }
    /// Settle only on completion, proven native exit or non-dispatch.
    /// The raw probe helper above is never an implicit product fallback.
    fn host_script_owned(
        &self,
        _js: &str,
        _generation: u64,
        operation: ScriptOperation,
    ) -> Result<String, String> {
        operation.finished();
        Err("owned typed host script unavailable".into())
    }
    fn navigate_for_run(
        &self,
        _request: &NavigationRequest,
        _expected_generation: u64,
    ) -> Result<LiveNavigationDocument, NavigationError> {
        Err(NavigationError::not_dispatched(
            "typed WebView navigation unavailable",
        ))
    }
    fn download_for_run(
        &self,
        _request: &DownloadRequest,
        _expected_generation: u64,
        _prepared_url: &str,
        _dispatch_script: &str,
        operation: ScriptOperation,
    ) -> Result<DownloadResult, DownloadError> {
        operation.finished();
        Err(DownloadError::not_dispatched(
            "typed WebView download unavailable",
        ))
    }
}

struct BoundTab {
    view: Arc<dyn LiveWebView>,
    label: String,
    native_tab_id: String,
    tab_id: String,
    generation: u64,
    session: String,
    run_id: String,
    observation: Mutex<dom::ObservationState>,
    execution_epoch: AtomicU64,
    binding_cancellation: ActionCancellation,
}

pub struct WebViewAdapter {
    bound: Arc<Mutex<BTreeMap<String, Arc<BoundTab>>>>,
    scripts: ScriptTracker,
    scope_host: OnceLock<(tauri::AppHandle, Weak<WebViewAdapter>)>,
    #[cfg(any(test, feature = "computer-use-probe"))]
    scope_fixture: Mutex<Option<ScopeFixture>>,
}

impl BoundTab {
    fn native_key(&self) -> String {
        format!("{}|{}", self.label, self.native_tab_id)
    }

    fn is_current(&self) -> bool {
        self.binding_cancellation.check().is_ok()
            && self.view.is_alive()
            && self.view.label() == self.label
            && self.view.tab_id() == self.native_tab_id
            && self.view.navigation_generation() == self.generation
    }

    fn target_id(&self) -> String {
        encode_id(
            &self.label,
            &self.tab_id,
            self.generation,
            &self.session,
            &self.run_id,
        )
    }
}

impl WebViewAdapter {
    pub fn new() -> Self {
        Self {
            bound: Arc::new(Mutex::new(BTreeMap::new())),
            scripts: ScriptTracker::default(),
            scope_host: OnceLock::new(),
            #[cfg(any(test, feature = "computer-use-probe"))]
            scope_fixture: Mutex::new(None),
        }
    }

    #[cfg(feature = "computer-use-probe")]
    pub fn import_surface_auth(&self, _cookies_or_token: &str) -> Result<(), String> {
        Err("cookie/auth reuse from other App surfaces is forbidden".into())
    }

    #[cfg(feature = "computer-use-probe")]
    pub fn migrate_auth_to_managed(&self) -> Result<(), String> {
        Err("cannot migrate WebView auth into a managed browser".into())
    }

    pub fn unbind(&self) {
        for old in std::mem::take(&mut *self.bound.lock()).into_values() {
            old.binding_cancellation.cancel();
        }
    }

    pub fn unbind_for_run(&self, session: &str, run_id: &str) -> bool {
        let mut bound = self.bound.lock();
        let mut removed = false;
        bound.retain(|_, old| {
            if old.session == session && old.run_id == run_id {
                old.binding_cancellation.cancel();
                removed = true;
                false
            } else {
                true
            }
        });
        removed
    }

    /// Explicit bind of the user's currently selected App-owned tab.
    /// Empty selection or a dead view fails closed; cookies/tokens are rejected.
    #[cfg(any(test, feature = "computer-use-probe"))]
    pub fn bind(
        &self,
        view: Arc<dyn LiveWebView>,
        session: &str,
        run_id: &str,
        extras: &serde_json::Value,
    ) -> Result<TargetInfo, String> {
        if extras.get("cookies").is_some()
            || extras.get("token").is_some()
            || extras.get("auth").is_some()
            || extras.get("album").is_some()
        {
            return Err("cookie/auth reuse from other App surfaces is forbidden".into());
        }
        let session = session.trim();
        let run_id = run_id.trim();
        if session.is_empty() || run_id.is_empty() || session.contains('|') || run_id.contains('|')
        {
            return Err("bind requires session and run".into());
        }
        let selection = WebViewSelection::capture(view)?;
        self.bind_selected(&selection, session, run_id)
    }

    /// Debug harness convenience; product tasks retain an explicit selection.
    #[cfg(debug_assertions)]
    pub fn bind_side_browser(
        &self,
        app: tauri::AppHandle,
        label: &str,
        session: &str,
        run_id: &str,
    ) -> Result<TargetInfo, String> {
        let selection = Self::select_side_browser(app, label)?;
        self.bind_selected(&selection, session, run_id)
    }

    fn bound_target(&self, target_id: &str) -> Result<Arc<BoundTab>, String> {
        let bound = self
            .bound
            .lock()
            .get(target_id)
            .cloned()
            .ok_or("webview typed subset not bound to a live tab")?;
        if parse_id(target_id).is_none() || bound.target_id() != target_id || !bound.is_current() {
            return Err("webview target not bound or stale".into());
        }
        Ok(bound)
    }

    fn still_bound(&self, expected: &Arc<BoundTab>) -> bool {
        self.bound
            .lock()
            .get(&expected.target_id())
            .is_some_and(|current| Arc::ptr_eq(current, expected))
            && expected.is_current()
    }

    fn host_script(
        &self,
        expected: &Arc<BoundTab>,
        epoch: u64,
        cancellation: &ActionCancellation,
        script: &str,
    ) -> Result<String, String> {
        let operation = {
            // Share this short admission lock with abort. A completed preflight
            // from before Pause/Stop cannot admit a later side-effect script.
            let bound = self.bound.lock();
            if !bound
                .get(&expected.target_id())
                .is_some_and(|current| Arc::ptr_eq(current, expected))
                || expected.execution_epoch.load(Ordering::SeqCst) != epoch
                || !expected.is_current()
            {
                return Err("WebView binding changed before dispatch".into());
            }
            self.scripts.begin(
                &expected.run_id,
                &expected.native_key(),
                cancellation,
                &expected.binding_cancellation,
            )?
        };
        expected
            .view
            .host_script_owned(script, expected.generation, operation)
    }
}

impl Default for WebViewAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn encode_id(label: &str, tab: &str, gen: u64, session: &str, run: &str) -> String {
    format!("wv|{label}|{tab}|{gen}|{session}|{run}")
}

fn parse_id(id: &str) -> Option<(String, String, u64, String, String)> {
    let mut parts = id.split('|');
    if parts.next() != Some("wv") {
        return None;
    }
    let parsed = (
        parts.next()?.into(),
        parts.next()?.into(),
        parts.next()?.parse().ok()?,
        parts.next()?.into(),
        parts.next()?.into(),
    );
    if parts.next().is_some() {
        return None;
    }
    Some(parsed)
}

fn parse_script_json(raw: &str) -> Result<serde_json::Value, String> {
    script_result::parse(raw)
}

fn validate_action_parameters(req: &DispatchRequest) -> Result<(), String> {
    let allowed: &[&str] = match req.action {
        ActionKind::Click => &[],
        ActionKind::SetValue => &["text", "value"],
        ActionKind::TypeText => &["text", "via"],
        ActionKind::Scroll => &["delta", "dy"],
        ActionKind::Wait => &["nameEquals", "timeoutMs"],
        _ => return Err("typed WebView subset does not implement this action".into()),
    };
    let parameters = req
        .parameters
        .as_object()
        .ok_or("typed WebView action parameters must be an object")?;
    if parameters
        .keys()
        .any(|key| !allowed.contains(&key.as_str()))
    {
        return Err("unsupported typed WebView action parameter".into());
    }
    if matches!(req.action, ActionKind::SetValue | ActionKind::TypeText) {
        if req.action == ActionKind::TypeText && parameters.get("via").is_some() {
            return Err("typed WebView type_text does not support via=clipboard".into());
        }
        let text = if req.action == ActionKind::SetValue {
            parameters.get("text").or_else(|| parameters.get("value"))
        } else {
            parameters.get("text")
        }
        .and_then(serde_json::Value::as_str);
        if parameters.len() != 1
            || !text.is_some_and(|text| {
                text.chars().count() <= super::protocol::TEXT_MAX_CHARS && !text.contains('\0')
            })
        {
            return Err("typed WebView text action requires exactly one bounded text value".into());
        }
    } else if req.action == ActionKind::Scroll
        && (parameters.len() > 1
            || parameters
                .values()
                .any(|value| !value.as_i64().is_some_and(|n| (-4096..=4096).contains(&n))))
    {
        return Err("typed WebView scroll requires one integer delta".into());
    } else if req.action == ActionKind::Wait {
        wait_parameters(req)?;
    }
    Ok(())
}

fn wait_parameters(req: &DispatchRequest) -> Result<(String, u64), String> {
    let parameters = req
        .parameters
        .as_object()
        .ok_or("typed WebView action parameters must be an object")?;
    if !(parameters.len() == 1 || parameters.len() == 2) {
        return Err("typed WebView wait requires nameEquals and optional timeoutMs".into());
    }
    let name = parameters
        .get("nameEquals")
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.trim().is_empty() && name.encode_utf16().count() <= 256)
        .ok_or("typed WebView wait requires a bounded nameEquals")?;
    let timeout = match parameters.get("timeoutMs") {
        None => 2000,
        Some(value) => value
            .as_u64()
            .filter(|timeout| (1..=10_000).contains(timeout))
            .ok_or("typed WebView wait timeoutMs must be between 1 and 10000")?,
    };
    Ok((name.to_string(), timeout))
}

fn target_info(bound: &BoundTab, title: String) -> TargetInfo {
    let gen = bound.generation;
    TargetInfo {
        target_id: bound.target_id(),
        title,
        app_name: "side-browser".into(),
        kind: "webview".into(),
        pid: None,
        backend: "webview".into(),
        execution_mode: "host-owned".into(),
        replay_policy: "never".into(),
        lifecycle_stamp: gen,
        display_id: bound.label.clone(),
        coordinate_space: "image-pixels".into(),
        scope_label: format!("{}/{}", bound.session, bound.run_id),
    }
}

impl ComputerUseAdapter for WebViewAdapter {
    fn backend_id(&self) -> &'static str {
        "webview"
    }

    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::for_surface(super::adapter::SurfaceKind::WebView, "webview");
        caps.notes = vec![
            "side_browser_host typed snapshot/actions only; no eval in Computer Use".into(),
            "Cross-origin iframe / downloads are not claimed equivalent to Playwright".into(),
        ];
        if self.bound.lock().values().any(|bound| bound.is_current()) {
            caps.observe_ax = true;
            caps.semantic_click = true;
            caps.unicode_text = true;
            caps.chinese_ime = true;
            caps.click = super::adapter::ActionSupport::SEMANTIC;
            caps.set_value = super::adapter::ActionSupport::SEMANTIC;
            caps.type_text = super::adapter::ActionSupport::SEMANTIC;
            caps.scroll = super::adapter::ActionSupport::SEMANTIC;
            caps.wait = super::adapter::ActionSupport::SEMANTIC;
        }
        caps
    }

    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        self.list_bound_targets(None)
    }

    fn target_alive(&self, target_id: &str) -> bool {
        self.bound_target(target_id).is_ok()
    }
    fn scope_identity(
        &self,
        target_id: &str,
    ) -> Result<super::adapter::NativeScopeIdentity, String> {
        self.bound_scope_identity(target_id)
    }
    fn scope_candidates_for_task(
        &self,
        limit: usize,
        is_current: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<super::adapter::ScopeCandidatePage, String> {
        self.discover_scope_candidates(limit, is_current)
    }
    fn bind_scope_candidate(
        &self,
        identity: &super::adapter::NativeScopeIdentity,
        session: &str,
        run: &str,
    ) -> Result<super::adapter::ScopeCandidate, String> {
        let super::adapter::NativeScopeIdentity::WebViewSelected { identity } = identity else {
            return Err("WebView binding requires the original unbound receipt".into());
        };
        identity.receipt.bind(self, session, run)
    }
    fn scope_target_info(&self, run_id: &str, target_id: &str) -> Result<TargetInfo, String> {
        self.bound_scope_target_info(run_id, target_id)
    }
    fn with_scope_authority(
        &self,
        identities: &[super::adapter::NativeScopeIdentity],
        commit: &mut dyn FnMut() -> Result<(), super::BrokerError>,
    ) -> Result<(), super::BrokerError> {
        self.with_bound_authority(identities, commit)
    }

    fn list_targets_for_run(&self, run_id: &str) -> Result<Vec<TargetInfo>, String> {
        self.list_bound_targets(Some(run_id))
    }

    fn claim_target_for_run(&self, run_id: &str, target_id: &str) -> Result<(), String> {
        let bound = self.bound_target(target_id)?;
        if bound.run_id != run_id {
            return Err("WebView run identity changed".into());
        }
        if !self.scripts.native_is_idle(&bound.native_key()) {
            return Err("WebView native execution is still pending".into());
        }
        Ok(())
    }

    fn observe(&self, target_id: &str) -> Result<Observation, String> {
        self.observe_owned(target_id, true, &ActionCancellation::default())
    }

    fn observe_for_run(&self, run_id: &str, target_id: &str) -> Result<Observation, String> {
        if self.bound_target(target_id)?.run_id != run_id {
            return Err("WebView run identity changed".into());
        }
        self.observe(target_id)
    }

    fn capture_for_run(
        &self,
        run_id: &str,
        target_id: &str,
        options: super::adapter::CaptureOptions,
    ) -> Result<Observation, String> {
        if self.bound_target(target_id)?.run_id != run_id {
            return Err("WebView run identity changed".into());
        }
        options.cancellation.check()?;
        let observation =
            self.observe_owned(target_id, options.for_model, &options.cancellation)?;
        options.cancellation.check()?;
        Ok(observation)
    }

    fn act(&self, req: &DispatchRequest) -> Result<AdapterActResult, String> {
        self.act_owned(req)
    }

    fn stage_navigation(
        &self,
        req: &NavigationRequest,
    ) -> Result<Box<dyn StagedNavigation>, NavigationError> {
        self.stage_navigation_owned(req)
    }

    fn stage_download(
        &self,
        req: &DownloadRequest,
    ) -> Result<Box<dyn StagedDownload>, DownloadError> {
        self.stage_download_owned(req)
    }

    fn stage_upload(&self, req: &UploadRequest) -> Result<Box<dyn StagedUpload>, UploadError> {
        self.stage_upload_owned(req)
    }

    fn wait_uses_retained_reference(&self) -> bool {
        true
    }

    fn abort(&self, run_id: &str, _generation: u64) -> Result<(), String> {
        let bound = self.bound.lock();
        for bound in bound.values().filter(|bound| bound.run_id == run_id) {
            bound.observation.lock().snapshot = None;
            if bound
                .execution_epoch
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
                .is_err()
            {
                // Never wrap an old dispatch epoch back into authority.
                bound.binding_cancellation.cancel();
            }
        }
        self.scripts.cancel(run_id);
        // Revokes queued work; already dispatched native work keeps occupancy.
        // This acknowledgement is not a claim of physical interruption.
        Ok(())
    }

    fn is_idle(&self, run_id: &str) -> bool {
        self.scripts.is_idle(run_id)
    }

    fn start_periodic_preview(&self, _target_id: &str) {}
    fn stop_periodic_preview(&self) {}
    fn periodic_preview_active(&self) -> bool {
        false
    }

    fn release_target(&self, target_id: &str) {
        let Some(_) = parse_id(target_id) else {
            return;
        };
        let mut bound = self.bound.lock();
        if let Some(old) = bound.remove(target_id) {
            old.binding_cancellation.cancel();
        }
    }

    fn release_target_for_run(&self, run_id: &str, target_id: &str) {
        if parse_id(target_id).is_some_and(|(_, _, _, _, owner)| owner == run_id) {
            self.release_target(target_id);
        }
    }

    fn current_geometry_revision_for(&self, target_id: &str) -> u64 {
        self.bound
            .lock()
            .get(target_id)
            .filter(|b| b.is_current())
            .map(|b| b.generation)
            .unwrap_or(0)
    }
}

impl WebViewAdapter {
    fn list_bound_targets(&self, run_id: Option<&str>) -> Result<Vec<TargetInfo>, String> {
        // Do not read another run's metadata and filter it afterwards.
        let selected: Vec<_> = self
            .bound
            .lock()
            .values()
            .filter(|bound| run_id.is_none_or(|run| bound.run_id == run))
            .cloned()
            .collect();
        let mut targets = Vec::with_capacity(selected.len());
        for bound in selected {
            if !bound.is_current() {
                continue;
            }
            let url = match bound.view.url() {
                Ok(url) if !url.trim().is_empty() => url,
                _ => continue,
            };
            let title = bound.view.title().unwrap_or_default();
            let title = if title.trim().is_empty() { url } else { title };
            if self.still_bound(&bound) {
                targets.push(target_info(
                    &bound,
                    title.chars().take(selection::MAX_METADATA_CHARS).collect(),
                ));
            }
        }
        Ok(targets)
    }

    fn observe_owned(
        &self,
        target_id: &str,
        for_model: bool,
        cancellation: &ActionCancellation,
    ) -> Result<Observation, String> {
        let bound = self.bound_target(target_id)?;
        let nav = bound.generation;
        let epoch = bound.execution_epoch.load(Ordering::SeqCst);
        let revision = if for_model {
            Some(bound.observation.lock().begin()?)
        } else {
            None
        };
        let snapshot_id = format!("wvsnap:{}", uuid::Uuid::new_v4());
        let script = dom::observe(&bound.tab_id, &snapshot_id, for_model);
        let raw = self.host_script(&bound, epoch, cancellation, &script)?;
        if !self.still_bound(&bound) {
            return Err("WebView document changed during observation".into());
        }
        if bound.execution_epoch.load(Ordering::SeqCst) != epoch {
            return Err("WebView observation retired during cancellation".into());
        }
        cancellation.check()?;
        let snapshot = dom::Snapshot::parse(parse_script_json(&raw)?, &snapshot_id)?;
        if let Some(revision) = revision {
            // Synchronize with abort and binding replacement before publishing.
            let current = self.bound.lock();
            if !current
                .get(&bound.target_id())
                .is_some_and(|current| Arc::ptr_eq(current, &bound))
                || !bound.is_current()
                || bound.execution_epoch.load(Ordering::SeqCst) != epoch
            {
                return Err("WebView observation retired before publication".into());
            }
            cancellation.check()?;
            bound
                .observation
                .lock()
                .publish(revision, snapshot.published())?;
        }
        Ok(Observation {
            text: snapshot.text,
            version: super::protocol::PROTOCOL_VERSION,
            run_id: bound.run_id.clone(),
            target_id: target_id.into(),
            target_generation: nav,
            snapshot_id,
            captured_at: chrono::Utc::now().to_rfc3339(),
            geometry_revision: nav,
            coordinate_space: "image-pixels".into(),
            image: super::protocol::ObservationImage {
                width: snapshot.width,
                height: snapshot.height,
                content_id: "webview-typed".into(),
                png_base64: None,
            },
            nodes: snapshot.nodes,
            truncated: snapshot.truncated,
            crop_x: 0,
            crop_y: 0,
            crop_width: snapshot.width,
            crop_height: snapshot.height,
            scale: 1.0,
            dpi: 96.0,
            origin_x: 0,
            origin_y: 0,
            topology_revision: nav,
        })
    }

    fn act_owned(&self, req: &DispatchRequest) -> Result<AdapterActResult, String> {
        if req.parameters.get("eval").is_some() || req.parameters.get("script").is_some() {
            return Err("arbitrary eval is not in the typed WebView subset".into());
        }
        if req.parameters.get("cookies").is_some() || req.parameters.get("auth").is_some() {
            return Err("cookie/auth reuse from other App surfaces is forbidden".into());
        }
        validate_action_parameters(req)?;
        let bound = self.bound_target(&req.target_id)?;
        if bound.run_id != req.run_id {
            return Err("WebView run identity changed".into());
        }
        req.admit(self)?;
        let epoch = bound.execution_epoch.load(Ordering::SeqCst);
        bound.observation.lock().validate(req)?;
        if req.action == ActionKind::Wait {
            return self.wait_owned(&bound, req, epoch);
        }
        {
            let preflight = dom::action(&bound.tab_id, req, true)?;
            let raw = self.host_script(&bound, epoch, &req.cancellation, &preflight)?;
            if !self.still_bound(&bound) {
                return Err("WebView document changed before dispatch".into());
            }
            req.cancellation.check()?;
            dom::acknowledge(&parse_script_json(&raw)?)?;
        }
        let js = dom::action(&bound.tab_id, req, false)?;
        {
            // Consume only this exact model snapshot before dispatch. Unknown
            // completion never restores it, and a new observation is not cleared.
            let mut observation = bound.observation.lock();
            observation.validate(req)?;
            observation.snapshot = None;
        }
        if !self.still_bound(&bound) {
            return Err("WebView binding changed before dispatch".into());
        }
        req.cancellation.check()?;
        let raw = self.host_script(&bound, epoch, &req.cancellation, &js)?;
        if !self.still_bound(&bound) {
            return Err("WebView action outcome unknown after document change".into());
        }
        if bound.execution_epoch.load(Ordering::SeqCst) != epoch {
            return Err("WebView action outcome unknown after cancellation".into());
        }
        req.cancellation.check()?;
        let snap = parse_script_json(&raw)?;
        let verified = if req.action == ActionKind::TypeText {
            dom::action_verified(snap)?
        } else {
            dom::acknowledge(&snap)?;
            false
        };
        Ok(AdapterActResult {
            applied: true,
            outcome: None,
            postcondition_ok: verified,
            verifiable: verified,
            detail: if verified {
                "typed WebView text inserted and field postcondition verified".into()
            } else {
                // A returned click/fill/scroll script is not an independently
                // checked page postcondition.
                "typed WebView action dispatched".into()
            },
        })
    }

    fn wait_owned(
        &self,
        bound: &Arc<BoundTab>,
        req: &DispatchRequest,
        epoch: u64,
    ) -> Result<AdapterActResult, String> {
        let (name_equals, timeout_ms) = wait_parameters(req)?;
        let started = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);
        loop {
            req.cancellation.check()?;
            if !self.still_bound(bound) || bound.execution_epoch.load(Ordering::SeqCst) != epoch {
                return Err("WebView wait retired during cancellation or document change".into());
            }
            bound.observation.lock().validate(req)?;
            let js = dom::wait_check(&bound.tab_id, req, &name_equals)?;
            let raw = self.host_script(bound, epoch, &req.cancellation, &js)?;
            if !self.still_bound(bound) {
                return Err("WebView document changed during wait".into());
            }
            if bound.execution_epoch.load(Ordering::SeqCst) != epoch {
                return Err("WebView wait retired during cancellation".into());
            }
            req.cancellation.check()?;
            bound.observation.lock().validate(req)?;
            if dom::wait_matched(parse_script_json(&raw)?)? {
                self.commit_wait_match(bound, req, epoch)?;
                return Ok(AdapterActResult {
                    applied: true,
                    outcome: None,
                    postcondition_ok: true,
                    verifiable: true,
                    detail: "observed WebView element name matched; no input dispatched".into(),
                });
            }
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                return Err("typed WebView wait timed out".into());
            }
            std::thread::sleep(Duration::from_millis(50).min(timeout - elapsed));
        }
    }

    /// Linearize a matched reply against the original native document. The
    /// binding map is acquired before native document authority, matching the
    /// publication and dispatch lock order. Do not call `is_current()` from
    /// the callback: it would query the native document recursively.
    fn commit_wait_match(
        &self,
        bound: &Arc<BoundTab>,
        req: &DispatchRequest,
        epoch: u64,
    ) -> Result<(), String> {
        let current = self.bound.lock();
        if !current
            .get(&bound.target_id())
            .is_some_and(|candidate| Arc::ptr_eq(candidate, bound))
        {
            return Err("WebView wait binding retired before verification".into());
        }
        let mut commit = || {
            if bound.execution_epoch.load(Ordering::SeqCst) != epoch {
                return Err(super::BrokerError::StopRequested);
            }
            req.cancellation
                .check()
                .map_err(super::BrokerError::Adapter)?;
            bound
                .binding_cancellation
                .check()
                .map_err(|_| super::BrokerError::DeadTarget)?;
            bound
                .observation
                .lock()
                .validate(req)
                .map_err(super::BrokerError::Adapter)
        };
        let result = bound
            .view
            .with_document_authority(bound.generation, &mut commit);
        drop(current);
        result.map_err(|error| format!("WebView wait retired before verification: {error}"))
    }
}

mod native_view;
use native_view::SideBrowserLiveView;

#[cfg(windows)]
pub(crate) use download::{
    cancel_instance as cancel_side_browser_downloads,
    cancel_instance_now as cancel_side_browser_downloads_now,
    claim_wry_requested as claim_computer_use_download, finish_wry as finish_computer_use_download,
    retire_instance as retire_side_browser_downloads,
    schedule_handler as schedule_computer_use_download_handler,
    uninstall_instance as uninstall_side_browser_downloads,
    wait_retired as wait_side_browser_downloads_retired,
};

#[cfg(all(windows, feature = "computer-use-probe"))]
pub(crate) use download::{
    cancel_probe_download_now, download_live_host as download_live_probe,
    install_probe_handlers as install_probe_download_handlers,
};

#[cfg(feature = "computer-use-probe")]
mod probe;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod probe_timeout;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod probe_deadline;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod probe_setup;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod probe_process;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod probe_close;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod probe_close_baseline;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_close_baseline::run as run_native_close_baseline;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_close::run as run_native_close_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_process::run as run_renderer_exit_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod probe_isolation;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod probe_dom;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod probe_download;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod probe_upload;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_upload::run as run_product_upload_gate;
#[cfg(all(windows, feature = "computer-use-probe"))]
pub(crate) use probe_upload::run_mcp as run_product_upload_mcp_gate;
#[cfg(all(windows, feature = "computer-use-probe"))]
pub(crate) use probe_upload::run_task as run_product_upload_task_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod probe_navigation;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_navigation::run as run_navigation_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_download::run as run_download_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_download::lifecycle::run as run_download_lifecycle_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_download::lifecycle::shared::run as run_download_shared_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_download::lifecycle::app::run as run_product_download_close_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_download::lifecycle::app::idle_exit::run as run_product_idle_exit_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_download::lifecycle::app::error_unwind::run as run_product_download_error_unwind_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_download::lifecycle::app::before_start::run as run_product_download_before_start_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_download::lifecycle::app::buffer_retirement::run as run_product_download_buffer_retirement_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_download::lifecycle::app::buffer_crash::run as run_product_download_buffer_crash_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use probe_download::lifecycle::app::publication::run as run_product_download_publication_gate;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use download::probe_download_pending;
#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use download::probe_resources_retired as probe_download_resources_retired;
#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use download::retire_instance_now as retire_probe_downloads_now;
#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
pub(crate) use download::uninstall_instance_now as uninstall_probe_downloads_now;

#[cfg(feature = "computer-use-probe")]
pub use probe::run_webview_gates;

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::upload_permission::fixture as upload_permission_fixture;
