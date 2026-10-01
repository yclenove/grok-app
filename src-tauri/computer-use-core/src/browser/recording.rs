//! In-memory worker double for Broker/browser tests.

use parking_lot::Mutex;

use super::types::*;
use super::worker::ManagedBrowserWorker;

fn rec_page(url: impl Into<String>) -> ManagedPage {
    ManagedPage {
        page_id: "rec-page".into(),
        page_generation: 1,
        url: url.into(),
        popup: false,
    }
}

#[cfg(any(test, feature = "test-support"))]
pub type RecordingGotoBarrier = std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>;

#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct RecordingBrowserWorker {
    lifecycle: Mutex<std::collections::HashMap<String, super::WorkerRunState>>,
    pub resume_error: Mutex<Option<WorkerError>>,
    pub resume_calls: Mutex<u32>,
    pub gotos: Mutex<Vec<String>>,
    pub downloads: Mutex<u32>,
    pub act_calls: Mutex<u32>,
    pub uploads: Mutex<u32>,
    pub opens: Mutex<u32>,
    pub capture_options: Mutex<Vec<(bool, bool)>>,
    pub staging: Mutex<std::path::PathBuf>,
    pub actual_url: Mutex<Option<String>>,
    pub act_error: Mutex<Option<WorkerError>>,
    pub cancel_run_error: Mutex<Option<WorkerError>>,
    pub cancel_run_calls: Mutex<u32>,
    pub goto_started: Mutex<Option<RecordingGotoBarrier>>,
    pub goto_release: Mutex<Option<RecordingGotoBarrier>>,
    pub open_started: Mutex<Option<RecordingGotoBarrier>>,
    pub open_release: Mutex<Option<RecordingGotoBarrier>>,
    pub open_error: Mutex<Option<WorkerError>>,
    pub clear_started: Mutex<Option<RecordingGotoBarrier>>,
    pub clear_release: Mutex<Option<RecordingGotoBarrier>>,
    pub clear_error: Mutex<Option<WorkerError>>,
    pub clear_calls: Mutex<u32>,
}

#[cfg(any(test, feature = "test-support"))]
impl RecordingBrowserWorker {
    pub fn new(staging: std::path::PathBuf) -> Self {
        Self {
            lifecycle: Default::default(),
            resume_error: Default::default(),
            resume_calls: Default::default(),
            gotos: Mutex::new(Vec::new()),
            downloads: Mutex::new(0),
            act_calls: Mutex::new(0),
            uploads: Mutex::new(0),
            opens: Mutex::new(0),
            capture_options: Mutex::new(Vec::new()),
            staging: Mutex::new(staging),
            actual_url: Mutex::new(None),
            act_error: Mutex::new(None),
            cancel_run_error: Mutex::new(None),
            cancel_run_calls: Mutex::new(0),
            goto_started: Mutex::new(None),
            goto_release: Mutex::new(None),
            open_started: Mutex::new(None),
            open_release: Mutex::new(None),
            open_error: Mutex::new(None),
            clear_started: Mutex::new(None),
            clear_release: Mutex::new(None),
            clear_error: Mutex::new(None),
            clear_calls: Mutex::new(0),
        }
    }

    pub fn set_actual_url(&self, url: Option<String>) {
        *self.actual_url.lock() = url;
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ManagedBrowserWorker for RecordingBrowserWorker {
    fn bind_request(
        self: std::sync::Arc<Self>,
        identity: super::ManagedRequestIdentity,
    ) -> Result<std::sync::Arc<dyn ManagedBrowserWorker>, WorkerError> {
        identity.cancellation().check().map_err(WorkerError::from)?;
        Ok(self)
    }

    fn pause_run(
        &self,
        owner: &str,
        revision: super::WorkerRunRevision,
    ) -> Result<super::WorkerRunState, WorkerError> {
        let mut states = self.lifecycle.lock();
        let state = states.entry(owner.into()).or_insert(super::WorkerRunState {
            revision,
            phase: super::WorkerRunPhase::Running,
            active_operations: 0,
        });
        if state.revision != revision {
            return Err(WorkerError::not_started(
                409,
                "stale_run_revision",
                "stale revision",
            ));
        }
        state.phase = super::WorkerRunPhase::Paused;
        Ok(*state)
    }

    fn run_status(
        &self,
        owner: &str,
        revision: super::WorkerRunRevision,
    ) -> Result<super::WorkerRunState, WorkerError> {
        let state = self
            .lifecycle
            .lock()
            .get(owner)
            .copied()
            .unwrap_or(super::WorkerRunState {
                revision: super::WorkerRunRevision::INITIAL,
                phase: super::WorkerRunPhase::Running,
                active_operations: 0,
            });
        if state.revision != revision {
            return Err(WorkerError::not_started(
                409,
                "stale_run_revision",
                "stale revision",
            ));
        }
        Ok(state)
    }

    fn resume_run(
        &self,
        owner: &str,
        revision: super::WorkerRunRevision,
    ) -> Result<super::WorkerRunState, WorkerError> {
        *self.resume_calls.lock() += 1;
        let mut states = self.lifecycle.lock();
        let state = states
            .get_mut(owner)
            .ok_or_else(|| WorkerError::not_started(404, "run_unknown", "unknown run"))?;
        if state.revision != revision
            || state.phase != super::WorkerRunPhase::Paused
            || !state.is_idle()
        {
            return Err(WorkerError::not_started(
                409,
                "run_not_quiescent",
                "run not quiescent",
            ));
        }
        state.revision = revision.successor()?;
        state.phase = super::WorkerRunPhase::Running;
        if let Some(error) = self.resume_error.lock().clone() {
            return Err(error);
        }
        Ok(*state)
    }
    fn goto(
        &self,
        owner: &str,
        profile: &str,
        _page_id: &str,
        _page_generation: u64,
        _action_id: &str,
        url: &str,
    ) -> Result<ManagedPage, WorkerError> {
        if let Some(started) = self.goto_started.lock().clone() {
            let (lock, cv) = &*started;
            *lock.lock().unwrap() = true;
            cv.notify_all();
        }
        if let Some(release) = self.goto_release.lock().clone() {
            let (lock, cv) = &*release;
            let mut ready = lock.lock().unwrap();
            while !*ready {
                ready = cv.wait(ready).unwrap();
            }
        }
        self.gotos.lock().push(format!("{owner}:{profile}:{url}"));
        Ok(rec_page(
            self.actual_url
                .lock()
                .clone()
                .unwrap_or_else(|| url.to_string()),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn download(
        &self,
        owner: &str,
        profile: &str,
        _page_id: &str,
        _page_generation: u64,
        snapshot_id: &str,
        _action_id: &str,
        filename: &str,
        element_ref: Option<&str>,
    ) -> Result<ManagedDownload, WorkerError> {
        if snapshot_id.trim().is_empty()
            || element_ref
                .map(str::trim)
                .is_none_or(|value| value.is_empty())
        {
            return Err(WorkerError::not_started(
                400,
                "snapshot_required",
                "snapshotId and elementRef required",
            ));
        }
        *self.downloads.lock() += 1;
        let dest = crate::staging::staging_file(&self.staging.lock(), owner, filename)
            .map_err(|e| e.to_string())?;
        std::fs::write(&dest, format!("worker:{owner}:{profile}")).map_err(|e| e.to_string())?;
        Ok(ManagedDownload {
            path: dest,
            page: rec_page("about:blank"),
        })
    }

    fn open_profile(&self, _owner: &str, profile: &str) -> Result<ManagedProfile, WorkerError> {
        *self.opens.lock() += 1;
        if let Some(started) = self.open_started.lock().clone() {
            let (lock, cv) = &*started;
            *lock.lock().unwrap() = true;
            cv.notify_all();
        }
        if let Some(release) = self.open_release.lock().clone() {
            let (lock, cv) = &*release;
            let (ready, timeout) = cv
                .wait_timeout_while(
                    lock.lock().unwrap(),
                    std::time::Duration::from_secs(5),
                    |ready| !*ready,
                )
                .unwrap();
            if !*ready || timeout.timed_out() {
                return Err(WorkerError::timeout("recording open gate timed out"));
            }
        }
        if let Some(error) = self.open_error.lock().take() {
            return Err(error);
        }
        let dir = self.staging.lock().join("profiles").join(profile);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(ManagedProfile {
            dir,
            page: rec_page("about:blank"),
        })
    }

    fn list_pages(&self, _owner: &str, _profile: &str) -> Result<Vec<ManagedPage>, WorkerError> {
        Ok(vec![rec_page("about:blank")])
    }

    fn new_page(
        &self,
        _owner: &str,
        _profile: &str,
        _action_id: &str,
    ) -> Result<ManagedPage, WorkerError> {
        Ok(rec_page("about:blank"))
    }

    fn clear_profile(&self, profile: &str) -> Result<(), WorkerError> {
        *self.clear_calls.lock() += 1;
        if let Some(started) = self.clear_started.lock().clone() {
            let (lock, cv) = &*started;
            *lock.lock().unwrap() = true;
            cv.notify_all();
        }
        if let Some(release) = self.clear_release.lock().clone() {
            let (lock, cv) = &*release;
            let (ready, _) = cv
                .wait_timeout_while(
                    lock.lock().unwrap(),
                    std::time::Duration::from_secs(5),
                    |ready| !*ready,
                )
                .unwrap();
            if !*ready {
                return Err(WorkerError::timeout("recording clear gate timed out"));
            }
        }
        if let Some(error) = self.clear_error.lock().take() {
            return Err(error);
        }
        let dir = self.staging.lock().join("profiles").join(profile);
        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }

    fn observe_page(
        &self,
        _owner: &str,
        _profile: &str,
        _page_id: &str,
        _page_generation: u64,
    ) -> Result<ManagedObservation, WorkerError> {
        Ok(ManagedObservation {
            page_id: "rec-page".into(),
            page_generation: 1,
            snapshot_id: "rec-snap".into(),
            url: "about:blank".into(),
            title: String::new(),
            aria: "- button \"+1\"".into(),
            nodes: vec![ManagedNode {
                element_ref: "rec-ref".into(),
                role: "button".into(),
                name: "+1".into(),
                disabled: false,
                truncated: false,
            }],
            truncated: false,
            text_only: true,
            image_width: 0,
            image_height: 0,
            image_content_id: String::new(),
            png_base64: None,
            image_omitted_reason: Some("not_captured".into()),
        })
    }

    fn capture_page(
        &self,
        owner: &str,
        profile: &str,
        page_id: &str,
        page_generation: u64,
        options: crate::adapter::CaptureOptions,
    ) -> Result<ManagedObservation, WorkerError> {
        options.cancellation.check().map_err(WorkerError::from)?;
        self.capture_options
            .lock()
            .push((options.for_model, options.screenshot));
        let mut observed = self.observe_page(owner, profile, page_id, page_generation)?;
        if !options.for_model {
            observed.snapshot_id = "rec-preview".into();
        }
        Ok(observed)
    }

    fn act_page(&self, request: ManagedWorkerAction<'_>) -> Result<ManagedPage, WorkerError> {
        *self.act_calls.lock() += 1;
        if let Some(error) = self.act_error.lock().take() {
            return Err(error);
        }
        if let Some(msg) = forbidden_browser_act(request.kind, request.params) {
            return Err(msg.into());
        }
        Ok(ManagedPage {
            page_id: "rec-page".into(),
            page_generation: 1,
            url: String::new(),
            popup: false,
        })
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
        if !request.source.is_file() {
            return Err("upload source missing".into());
        }
        *self.uploads.lock() += 1;
        Ok(())
    }

    fn cancel_run(&self, _owner: &str) -> Result<(), WorkerError> {
        *self.cancel_run_calls.lock() += 1;
        if let Some(error) = self.cancel_run_error.lock().take() {
            return Err(error);
        }
        Ok(())
    }
}
