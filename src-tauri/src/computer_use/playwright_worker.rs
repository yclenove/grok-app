//! Loopback Playwright worker client. Navigation/download fail closed if the worker is down.

use grok_computer_use_core::adapter::CaptureOptions;
use grok_computer_use_core::browser::{
    bounded_loopback_post, parse_managed_observation, parse_managed_page, parse_worker_run_state,
    ManagedBrowserWorker, ManagedDownload, ManagedObservation, ManagedPage, ManagedProfile,
    ManagedRequestIdentity, ManagedWorkerAction, ManagedWorkerUpload, WorkerError, WorkerRunPhase,
    WorkerRunRevision, WorkerRunState,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

pub struct LoopbackPlaywrightWorker {
    base: String,
    token: String,
    request: Option<ManagedRequestIdentity>,
}

impl LoopbackPlaywrightWorker {
    pub fn new(base: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            token: token.into(),
            request: None,
        }
    }

    fn post(&self, path: &str, mut body: Value) -> Result<Value, WorkerError> {
        let Some(request) = &self.request else {
            return self.post_headers(path, body, &[]);
        };
        if path != "/health" {
            if body.get("owner").and_then(Value::as_str) != Some(request.owner()) {
                return Err(WorkerError::not_started(
                    409,
                    "request_owner_mismatch",
                    "request owner differs from admission",
                ));
            }
            // Authoritative fields always overwrite any action parameters.
            body["runRevision"] = json!(request.revision().get());
        }
        let timeout = std::time::Duration::from_secs(if path == "/open" { 60 } else { 15 });
        grok_computer_use_core::browser::bounded_loopback_post_cancellable(
            &self.base,
            &self.token,
            path,
            &body,
            &[],
            timeout,
            request.cancellation(),
        )
    }

    fn post_headers(
        &self,
        path: &str,
        body: Value,
        extra: &[(&str, &str)],
    ) -> Result<Value, WorkerError> {
        let timeout = if path == "/open" {
            std::time::Duration::from_secs(60)
        } else {
            std::time::Duration::from_secs(15)
        };
        bounded_loopback_post(&self.base, &self.token, path, &body, extra, timeout)
    }

    fn page_from(v: &Value) -> Result<ManagedPage, WorkerError> {
        parse_managed_page(v)
    }
}

impl ManagedBrowserWorker for LoopbackPlaywrightWorker {
    fn bind_request(
        self: Arc<Self>,
        identity: ManagedRequestIdentity,
    ) -> Result<Arc<dyn ManagedBrowserWorker>, WorkerError> {
        identity.cancellation().check().map_err(|_| {
            WorkerError::not_started(409, "request_cancelled", "request cancelled before binding")
        })?;
        let bound = Arc::new(Self {
            base: self.base.clone(),
            token: self.token.clone(),
            request: Some(identity),
        });
        bound.require_run_lifecycle()?;
        Ok(bound)
    }
    fn require_run_lifecycle(&self) -> Result<(), WorkerError> {
        let health = self.post("/health", json!({}))?;
        if health.get("runLifecycle").and_then(Value::as_u64) != Some(1) {
            return Err(WorkerError::not_started(
                409,
                "run_lifecycle_unavailable",
                "repair the managed browser runtime before controlling its run lifecycle",
            ));
        }
        Ok(())
    }

    fn pause_run(
        &self,
        owner: &str,
        revision: WorkerRunRevision,
    ) -> Result<WorkerRunState, WorkerError> {
        self.require_run_lifecycle()?;
        let body = self.post_headers(
            "/pause-run",
            json!({"owner":owner,"runRevision":revision.get()}),
            &[("x-grok-cu-host", "1")],
        )?;
        let state = parse_worker_run_state(&body, revision)?;
        if state.phase != WorkerRunPhase::Paused {
            return Err(WorkerError::invalid_response(200));
        }
        Ok(state)
    }

    fn run_status(
        &self,
        owner: &str,
        revision: WorkerRunRevision,
    ) -> Result<WorkerRunState, WorkerError> {
        self.require_run_lifecycle()?;
        let body = self.post_headers(
            "/run-status",
            json!({"owner":owner,"runRevision":revision.get()}),
            &[("x-grok-cu-host", "1")],
        )?;
        parse_worker_run_state(&body, revision)
    }

    fn resume_run(
        &self,
        owner: &str,
        revision: WorkerRunRevision,
    ) -> Result<WorkerRunState, WorkerError> {
        let next = revision.successor()?;
        self.require_run_lifecycle()?;
        // Cleanup/transition traffic owns an independent transport; a cancelled
        // business token must not suppress the request that fences that work.
        let body = self.post_headers(
            "/resume-run",
            json!({"owner":owner,"runRevision":revision.get(),"nextRunRevision":next.get()}),
            &[("x-grok-cu-host", "1")],
        )?;
        let state = parse_worker_run_state(&body, next)?;
        if state.phase != WorkerRunPhase::Running || !state.is_idle() {
            return Err(WorkerError::invalid_response(200));
        }
        Ok(state)
    }

    fn goto(
        &self,
        owner: &str,
        profile: &str,
        page_id: &str,
        page_generation: u64,
        action_id: &str,
        url: &str,
    ) -> Result<ManagedPage, WorkerError> {
        let body = self.post(
            "/goto",
            json!({
                "owner": owner,
                "profile": profile,
                "url": url,
                "pageId": page_id,
                "pageGeneration": page_generation,
                "actionId": action_id
            }),
        )?;
        Self::page_from(&body)
    }

    #[allow(clippy::too_many_arguments)]
    fn download(
        &self,
        owner: &str,
        profile: &str,
        page_id: &str,
        page_generation: u64,
        snapshot_id: &str,
        action_id: &str,
        filename: &str,
        element_ref: Option<&str>,
    ) -> Result<ManagedDownload, WorkerError> {
        let mut body = json!({
            "owner": owner,
            "profile": profile,
            "filename": filename,
            "pageId": page_id,
            "pageGeneration": page_generation,
            "snapshotId": snapshot_id,
            "actionId": action_id
        });
        if let Some(element_ref) = element_ref {
            body["elementRef"] = json!(element_ref);
        }
        let body = self.post("/download", body)?;
        let path = body
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| WorkerError::invalid_response(200))?;
        Ok(ManagedDownload {
            path: PathBuf::from(path),
            page: Self::page_from(&body)?,
        })
    }

    fn open_profile(&self, owner: &str, profile: &str) -> Result<ManagedProfile, WorkerError> {
        let body = self.post("/open", json!({"owner": owner, "profile": profile}))?;
        let dir = body
            .get("dir")
            .and_then(Value::as_str)
            .ok_or_else(|| WorkerError::invalid_response(200))?;
        Ok(ManagedProfile {
            dir: PathBuf::from(dir),
            page: Self::page_from(&body)?,
        })
    }

    fn list_pages(&self, owner: &str, profile: &str) -> Result<Vec<ManagedPage>, WorkerError> {
        let body = self.post("/tabs", json!({"owner": owner, "profile": profile}))?;
        let pages = body
            .get("pages")
            .and_then(Value::as_array)
            .ok_or_else(|| WorkerError::invalid_response(200))?;
        pages.iter().map(Self::page_from).collect()
    }

    fn new_page(
        &self,
        owner: &str,
        profile: &str,
        action_id: &str,
    ) -> Result<ManagedPage, WorkerError> {
        let body = self.post(
            "/new-tab",
            json!({"owner": owner, "profile": profile, "actionId": action_id}),
        )?;
        Self::page_from(&body)
    }

    fn list_frames(
        &self,
        owner: &str,
        profile: &str,
        page_id: &str,
        page_generation: u64,
    ) -> Result<Vec<String>, WorkerError> {
        let body = self.post(
            "/frames",
            json!({
                "owner": owner,
                "profile": profile,
                "pageId": page_id,
                "pageGeneration": page_generation
            }),
        )?;
        Ok(body
            .get("frames")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect())
    }

    fn clear_profile(&self, profile: &str) -> Result<(), WorkerError> {
        self.post_headers(
            "/clear",
            json!({"profile": profile}),
            &[("x-grok-cu-host", "1")],
        )?;
        Ok(())
    }

    fn observe_page(
        &self,
        owner: &str,
        profile: &str,
        page_id: &str,
        page_generation: u64,
    ) -> Result<ManagedObservation, WorkerError> {
        self.capture_page(
            owner,
            profile,
            page_id,
            page_generation,
            CaptureOptions::model(true),
        )
    }

    fn capture_page(
        &self,
        owner: &str,
        profile: &str,
        page_id: &str,
        page_generation: u64,
        options: CaptureOptions,
    ) -> Result<ManagedObservation, WorkerError> {
        options.cancellation.check().map_err(WorkerError::from)?;
        // Older packaged workers ignore unknown observe fields. Do not send a
        // preview that could silently overwrite their model snapshot.
        if !options.for_model || !options.screenshot {
            let health = self.post("/health", json!({}))?;
            if health
                .pointer("/page/observeOptions")
                .and_then(Value::as_u64)
                != Some(1)
            {
                return Err(WorkerError::not_started(
                    409,
                    "observation_options_unavailable",
                    "repair the managed browser runtime before using observation options",
                ));
            }
            options.cancellation.check().map_err(WorkerError::from)?;
        }
        let body = self.post(
            "/observe",
            json!({
                "owner": owner,
                "profile": profile,
                "pageId": page_id,
                "pageGeneration": page_generation,
                "preview": !options.for_model,
                "screenshot": options.screenshot
            }),
        )?;
        options.cancellation.check().map_err(WorkerError::from)?;
        parse_managed_observation(&body)
    }

    fn act_page(&self, request: ManagedWorkerAction<'_>) -> Result<ManagedPage, WorkerError> {
        if request.kind.eq_ignore_ascii_case("wait") {
            let health = self.post("/health", json!({}))?;
            if health
                .pointer("/page/waitCondition")
                .and_then(Value::as_u64)
                != Some(1)
            {
                return Err(WorkerError::not_started(
                    409,
                    "wait_condition_unavailable",
                    "repair the managed browser runtime before waiting on element conditions",
                ));
            }
        }
        let mut body = request.params.clone();
        if !body.is_object() {
            body = json!({});
        }
        let obj = body.as_object_mut().unwrap();
        obj.insert("owner".into(), json!(request.owner));
        obj.insert("profile".into(), json!(request.profile));
        obj.insert("kind".into(), json!(request.kind));
        obj.insert("pageId".into(), json!(request.page.page_id));
        obj.insert("pageGeneration".into(), json!(request.page.page_generation));
        obj.insert("actionId".into(), json!(request.action_id));
        if !request.snapshot_id.is_empty() {
            obj.insert("snapshotId".into(), json!(request.snapshot_id));
        }
        if let Some(element_ref) = &request.locator.element_ref {
            obj.insert("elementRef".into(), json!(element_ref));
        }
        let body = self.post("/act", body)?;
        Self::page_from(&body)
    }

    fn read_state(
        &self,
        _owner: &str,
        _profile: &str,
        _page_id: &str,
        _page_generation: u64,
        _selector: &str,
    ) -> Result<String, WorkerError> {
        Err(WorkerError::not_started(
            404,
            "invalid_request",
            "read_state is not a production worker route",
        ))
    }

    fn upload_file(&self, request: ManagedWorkerUpload<'_>) -> Result<(), WorkerError> {
        self.post(
            "/upload",
            json!({
                "owner": request.owner,
                "profile": request.profile,
                "path": request.source.to_string_lossy(),
                "pageId": request.page.page_id,
                "pageGeneration": request.page.page_generation,
                "snapshotId": request.snapshot_id,
                "elementRef": request.element_ref,
                "actionId": request.action_id
            }),
        )?;
        Ok(())
    }

    fn close_profile(&self, owner: &str, profile: &str) -> Result<(), WorkerError> {
        self.post("/close", json!({"owner": owner, "profile": profile}))?;
        Ok(())
    }

    fn cancel_run(&self, owner: &str) -> Result<(), WorkerError> {
        self.post_headers(
            "/cancel-run",
            json!({"owner": owner}),
            &[("x-grok-cu-host", "1")],
        )?;
        Ok(())
    }
}

pub fn bind_loopback(
    broker: &super::ComputerUseBroker,
    base: &str,
    token: &str,
    profile_root: &std::path::Path,
) -> Result<(), String> {
    broker
        .register_managed_browser_adapter()
        .map_err(|error| error.to_string())?;
    broker.tabs().set_profile_root(profile_root.to_path_buf());
    broker
        .tabs()
        .set_staging_root(profile_root.join(".staging"));
    broker
        .tabs()
        .set_worker(Arc::new(LoopbackPlaywrightWorker::new(base, token)));
    Ok(())
}
