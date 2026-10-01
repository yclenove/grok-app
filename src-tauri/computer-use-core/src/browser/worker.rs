//! Loopback Playwright worker contract. In-memory URL writes are not navigation.

use super::types::*;
use super::{WorkerRunRevision, WorkerRunState};
use crate::adapter::CaptureOptions;

/// Loopback Playwright worker (or a test double). In-memory URL writes are not navigation.
pub trait ManagedBrowserWorker: Send + Sync {
    /// Return a request-local client. Never mutate a shared client's authority.
    fn bind_request(
        self: std::sync::Arc<Self>,
        _identity: super::ManagedRequestIdentity,
    ) -> Result<std::sync::Arc<dyn ManagedBrowserWorker>, WorkerError> {
        Err(super::run_lifecycle::lifecycle_unavailable())
    }
    fn require_run_lifecycle(&self) -> Result<(), WorkerError> {
        Err(super::run_lifecycle::lifecycle_unavailable())
    }

    fn pause_run(
        &self,
        _owner: &str,
        _revision: WorkerRunRevision,
    ) -> Result<WorkerRunState, WorkerError> {
        Err(super::run_lifecycle::lifecycle_unavailable())
    }

    fn run_status(
        &self,
        _owner: &str,
        _revision: WorkerRunRevision,
    ) -> Result<WorkerRunState, WorkerError> {
        Err(super::run_lifecycle::lifecycle_unavailable())
    }

    /// Advance exactly once. A lost acknowledgement is unknown, never a reason
    /// to replay this transition or to grant business requests a newer revision.
    fn resume_run(
        &self,
        _owner: &str,
        _revision: WorkerRunRevision,
    ) -> Result<WorkerRunState, WorkerError> {
        Err(super::run_lifecycle::lifecycle_unavailable())
    }

    fn goto(
        &self,
        owner: &str,
        profile: &str,
        page_id: &str,
        page_generation: u64,
        action_id: &str,
        url: &str,
    ) -> Result<ManagedPage, WorkerError>;
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
    ) -> Result<ManagedDownload, WorkerError>;
    fn open_profile(&self, owner: &str, profile: &str) -> Result<ManagedProfile, WorkerError> {
        let _ = (owner, profile);
        Err("managed profile open unavailable".into())
    }
    fn list_pages(&self, owner: &str, profile: &str) -> Result<Vec<ManagedPage>, WorkerError> {
        let _ = (owner, profile);
        Err("list pages unavailable".into())
    }
    fn new_page(
        &self,
        owner: &str,
        profile: &str,
        action_id: &str,
    ) -> Result<ManagedPage, WorkerError> {
        let _ = (owner, profile, action_id);
        Err("new tab unavailable".into())
    }
    fn open_popup(
        &self,
        owner: &str,
        profile: &str,
        page_id: &str,
        page_generation: u64,
        action_id: &str,
        element_ref: &str,
    ) -> Result<ManagedPage, WorkerError> {
        let _ = (
            owner,
            profile,
            page_id,
            page_generation,
            action_id,
            element_ref,
        );
        Err("popup is opened by clicking an observed element".into())
    }
    fn list_frames(
        &self,
        owner: &str,
        profile: &str,
        page_id: &str,
        page_generation: u64,
    ) -> Result<Vec<String>, WorkerError> {
        let _ = (owner, profile, page_id, page_generation);
        Err("frames unavailable".into())
    }
    fn clear_profile(&self, profile: &str) -> Result<(), WorkerError> {
        let _ = profile;
        Err("clear profile unavailable".into())
    }
    fn observe_page(
        &self,
        owner: &str,
        profile: &str,
        page_id: &str,
        page_generation: u64,
    ) -> Result<ManagedObservation, WorkerError> {
        let _ = (owner, profile, page_id, page_generation);
        Err("observe page unavailable".into())
    }
    fn act_page(&self, request: ManagedWorkerAction<'_>) -> Result<ManagedPage, WorkerError> {
        let _ = request;
        Err("act page unavailable".into())
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
        if !options.for_model {
            return Err(WorkerError::not_started(
                409,
                "preview_unavailable",
                "worker does not support isolated preview",
            ));
        }
        let mut observation = self.observe_page(owner, profile, page_id, page_generation)?;
        options.cancellation.check().map_err(WorkerError::from)?;
        if !options.screenshot {
            observation.png_base64 = None;
            observation.text_only = true;
        }
        Ok(observation)
    }
    fn read_state(
        &self,
        owner: &str,
        profile: &str,
        page_id: &str,
        page_generation: u64,
        selector: &str,
    ) -> Result<String, WorkerError> {
        let _ = (owner, profile, page_id, page_generation, selector);
        Err("read state unavailable".into())
    }
    fn upload_file(&self, request: ManagedWorkerUpload<'_>) -> Result<(), WorkerError> {
        let _ = request;
        Err("upload unavailable".into())
    }
    fn close_profile(&self, owner: &str, profile: &str) -> Result<(), WorkerError> {
        let _ = (owner, profile);
        Ok(())
    }
    fn cancel_run(&self, owner: &str) -> Result<(), WorkerError> {
        let _ = owner;
        Err("managed browser cancellation unavailable".into())
    }
}
