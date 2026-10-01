//! Typed WebView subset. Bind is explicit; no eval and no cookie copy.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use grok_computer_use_core::execution::ActionCancellation;
use parking_lot::Mutex;

use super::adapter::{
    AdapterActResult, Capabilities, ComputerUseAdapter, DispatchRequest, TargetInfo,
};
use super::protocol::Observation;

mod dom;
mod execution;
#[cfg(windows)]
pub(crate) mod isolated_windows;
pub(crate) mod lifecycle;
mod queued_script;
mod script_result;
mod side_script;

pub use execution::ScriptOperation;
use execution::ScriptTracker;
pub(crate) use queued_script::QueuedScript;

/// Live App-owned webview. Probe uses a Host-owned WebView2; product uses side browser.
pub trait LiveWebView: Send + Sync {
    fn label(&self) -> String;
    fn tab_id(&self) -> String;
    fn url(&self) -> Result<String, String>;
    fn title(&self) -> Result<String, String>;
    fn is_alive(&self) -> bool;
    fn navigation_generation(&self) -> u64;
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
    bound: Mutex<Option<Arc<BoundTab>>>,
    scripts: ScriptTracker,
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
            bound: Mutex::new(None),
            scripts: ScriptTracker::default(),
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
        if let Some(old) = self.bound.lock().take() {
            old.binding_cancellation.cancel();
        }
    }

    pub fn unbind_for_run(&self, session: &str, run_id: &str) -> bool {
        let mut bound = self.bound.lock();
        let matches = bound
            .as_ref()
            .is_some_and(|current| current.session == session && current.run_id == run_id);
        if matches {
            if let Some(old) = bound.take() {
                old.binding_cancellation.cancel();
            }
        }
        matches
    }

    /// Explicit bind of the user's currently selected App-owned tab.
    /// Empty selection or a dead view fails closed; cookies/tokens are rejected.
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
        if !view.is_alive() {
            return Err("bind requires a live App-owned webview".into());
        }
        let label = view.label();
        let native_tab_id = view.tab_id();
        let generation = view.navigation_generation();
        if label.is_empty()
            || label.contains('|')
            || native_tab_id.is_empty()
            || native_tab_id.contains('|')
            || generation == 0
        {
            return Err("bind requires a valid native WebView identity".into());
        }
        let url = view.url()?;
        if url.trim().is_empty() {
            return Err("live webview has no url".into());
        }
        let bound = Arc::new(BoundTab {
            view,
            label,
            tab_id: format!("{native_tab_id}@{}", uuid::Uuid::new_v4()),
            native_tab_id,
            generation,
            session: session.into(),
            run_id: run_id.into(),
            observation: Mutex::new(dom::ObservationState::default()),
            execution_epoch: AtomicU64::new(0),
            binding_cancellation: ActionCancellation::default(),
        });
        let title = bound.view.title().unwrap_or_default();
        if !bound.is_current() || bound.target_id().len() > 256 {
            return Err("WebView document changed during bind".into());
        }
        let target = target_info(&bound, if title.trim().is_empty() { url } else { title });
        if let Some(old) = self.bound.lock().replace(bound) {
            old.binding_cancellation.cancel();
        }
        Ok(target)
    }

    /// Product path: bind a labeled side-browser child webview that already exists.
    pub fn bind_side_browser(
        &self,
        app: tauri::AppHandle,
        label: &str,
        session: &str,
        run_id: &str,
    ) -> Result<TargetInfo, String> {
        if !crate::side_browser_host::is_app_owned_side_browser(label) {
            return Err("side browser label must be App-owned".into());
        }
        if !crate::side_browser_host::exists(&app, label) {
            return Err("bind requires a live App-owned webview".into());
        }
        let view = Arc::new(SideBrowserLiveView::open(app, label.to_string())?);
        self.bind(view, session, run_id, &serde_json::json!({}))
    }

    fn bound_target(&self, target_id: &str) -> Result<Arc<BoundTab>, String> {
        let bound = self
            .bound
            .lock()
            .clone()
            .ok_or("webview typed subset not bound to a live tab")?;
        if parse_id(target_id).is_none() || bound.target_id() != target_id || !bound.is_current() {
            return Err("webview target not bound or stale".into());
        }
        Ok(bound)
    }

    fn still_bound(&self, expected: &Arc<BoundTab>) -> bool {
        self.bound
            .lock()
            .as_ref()
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
                .as_ref()
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
    use super::protocol::ActionKind;
    let allowed: &[&str] = match req.action {
        ActionKind::Click => &[],
        ActionKind::SetValue => &["text", "value"],
        ActionKind::Scroll => &["delta", "dy"],
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
    if req.action == ActionKind::SetValue {
        if parameters.len() != 1
            || !parameters
                .get("text")
                .or_else(|| parameters.get("value"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|text| text.encode_utf16().count() <= 16000)
        {
            return Err("typed WebView fill requires exactly one text value".into());
        }
    } else if req.action == ActionKind::Scroll
        && (parameters.len() > 1
            || parameters
                .values()
                .any(|value| !value.as_i64().is_some_and(|n| (-4096..=4096).contains(&n))))
    {
        return Err("typed WebView scroll requires one integer delta".into());
    }
    Ok(())
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
        if self
            .bound
            .lock()
            .as_ref()
            .is_some_and(|bound| bound.is_current())
        {
            caps.observe_ax = true;
            caps.semantic_click = true;
            caps.unicode_text = true;
            caps.chinese_ime = true;
            caps.click = super::adapter::ActionSupport::SEMANTIC;
            caps.set_value = super::adapter::ActionSupport::SEMANTIC;
            caps.scroll = super::adapter::ActionSupport::SEMANTIC;
        }
        caps
    }

    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        let Some(bound) = self.bound.lock().clone() else {
            return Ok(Vec::new());
        };
        if !bound.is_current() {
            return Ok(Vec::new());
        }
        let url = match bound.view.url() {
            Ok(u) if !u.trim().is_empty() => u,
            _ => return Ok(Vec::new()),
        };
        let title = bound.view.title().unwrap_or_default();
        let title = if title.trim().is_empty() { url } else { title };
        if !self.still_bound(&bound) {
            return Ok(Vec::new());
        }
        Ok(vec![target_info(&bound, title)])
    }

    fn target_alive(&self, target_id: &str) -> bool {
        self.bound_target(target_id).is_ok()
    }

    fn list_targets_for_run(&self, run_id: &str) -> Result<Vec<TargetInfo>, String> {
        Ok(self
            .list_targets()?
            .into_iter()
            .filter(|target| {
                parse_id(&target.target_id).is_some_and(|(_, _, _, _, owner)| owner == run_id)
            })
            .collect())
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

    fn abort(&self, run_id: &str, _generation: u64) -> Result<(), String> {
        let bound = self.bound.lock();
        if let Some(bound) = bound.as_ref().filter(|bound| bound.run_id == run_id) {
            bound.observation.lock().snapshot = None;
            if bound
                .execution_epoch
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
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
        let matches = bound
            .as_ref()
            .is_some_and(|current| current.target_id() == target_id);
        if matches {
            if let Some(old) = bound.take() {
                old.binding_cancellation.cancel();
            }
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
            .as_ref()
            .filter(|b| b.target_id() == target_id && b.is_current())
            .map(|b| b.generation)
            .unwrap_or(0)
    }
}

impl WebViewAdapter {
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
                .as_ref()
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
        dom::acknowledge(&snap)?;
        Ok(AdapterActResult {
            applied: true,
            outcome: None,
            // A returned script is not an independently checked page postcondition.
            postcondition_ok: false,
            verifiable: false,
            detail: "typed WebView action dispatched".into(),
        })
    }
}

struct SideBrowserLiveView {
    app: tauri::AppHandle,
    webview: tauri::Webview,
    label: String,
    tab_id: String,
    native: lifecycle::NativeView,
    document: lifecycle::DocumentIdentity,
}

impl SideBrowserLiveView {
    fn open(app: tauri::AppHandle, label: String) -> Result<Self, String> {
        use tauri::Manager;
        let native = lifecycle::current(&label).ok_or("WebView native identity unavailable")?;
        let document = native.document().ok_or("WebView native identity retired")?;
        let webview = app
            .get_webview(&label)
            .ok_or("WebView native instance unavailable")?;
        let url = webview
            .url()
            .map_err(|_| "WebView URL unavailable")?
            .to_string();
        if url.trim().is_empty() {
            return Err("selected tab has no url".into());
        }
        if !native.matches(document) {
            return Err("WebView document changed during bind".into());
        }
        Ok(Self {
            app,
            webview,
            label,
            tab_id: format!("native-{}", document.instance),
            native,
            document,
        })
    }

    fn require_document(&self) -> Result<(), String> {
        if self.is_alive() {
            Ok(())
        } else {
            Err("WebView native document retired".into())
        }
    }
}

impl LiveWebView for SideBrowserLiveView {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn tab_id(&self) -> String {
        self.tab_id.clone()
    }
    fn url(&self) -> Result<String, String> {
        self.require_document()?;
        let url = self
            .webview
            .url()
            .map_err(|_| "WebView URL unavailable")?
            .to_string();
        self.require_document()?;
        Ok(url)
    }
    fn title(&self) -> Result<String, String> {
        self.url()
    }
    fn is_alive(&self) -> bool {
        self.native.matches(self.document)
            && crate::side_browser_host::exists(&self.app, &self.label)
    }
    fn navigation_generation(&self) -> u64 {
        if self.is_alive() {
            self.document.generation
        } else {
            0
        }
    }
    #[cfg(any(test, feature = "computer-use-probe"))]
    fn navigate(&self, url: &str) -> Result<u64, String> {
        self.require_document()?;
        let url = tauri::Url::parse(url).map_err(|_| "invalid WebView URL")?;
        if !matches!(url.scheme(), "http" | "https" | "file") {
            return Err("typed navigate scheme not allowed".into());
        }
        self.native.navigation_started();
        self.webview
            .navigate(url)
            .map_err(|_| "WebView navigation failed")?;
        self.native
            .document()
            .map(|identity| identity.generation)
            .ok_or_else(|| "WebView native instance retired".into())
    }
    fn host_script_owned(
        &self,
        js: &str,
        generation: u64,
        operation: ScriptOperation,
    ) -> Result<String, String> {
        if generation != self.document.generation {
            operation.finished();
            return Err("WebView native document retired".into());
        }
        side_script::run(self, js, operation)
    }
}

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

#[cfg(feature = "computer-use-probe")]
pub use probe::run_webview_gates;

#[cfg(test)]
mod tests;
