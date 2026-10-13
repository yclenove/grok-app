//! Host-selected immutable content for a retained App WebView file input.
//!
//! These Rust-only candidates are not model tools or file/task approval. The
//! product caller must obtain both approvals before dispatch; the forthcoming
//! Broker/Host file-selection flow must not deserialize one from model JSON.

use super::*;
use grok_computer_use_core::adapter::HostSelectedUploadFile;
use std::sync::atomic::AtomicBool;

/// No Clone/Deserialize: it retains the actual binding, epoch and DOM ref,
/// rather than granting access to another file field with the same selector.
struct WebViewUpload {
    expected: Arc<BoundTab>,
    epoch: u64,
    snapshot_id: String,
    element_ref: String,
    file: HostSelectedUploadFile,
    consumed: AtomicBool,
}

struct WebViewStagedUpload {
    bound: Arc<Mutex<BTreeMap<String, Arc<BoundTab>>>>,
    expected: Arc<BoundTab>,
    epoch: u64,
    cancellation: ActionCancellation,
    result: AdapterActResult,
}

impl StagedUpload for WebViewStagedUpload {
    fn result(&self) -> &AdapterActResult {
        &self.result
    }

    fn commit(
        self: Box<Self>,
        commit: &mut dyn FnMut() -> Result<(), super::super::BrokerError>,
    ) -> Result<(), super::super::BrokerError> {
        let current = self.bound.lock();
        if !current
            .get(&self.expected.target_id())
            .is_some_and(|bound| Arc::ptr_eq(bound, &self.expected))
        {
            return Err(super::super::BrokerError::DeadTarget);
        }
        let mut guarded = || {
            if self.expected.execution_epoch.load(Ordering::SeqCst) != self.epoch {
                return Err(super::super::BrokerError::StopRequested);
            }
            self.cancellation
                .check()
                .map_err(super::super::BrokerError::Adapter)?;
            self.expected
                .binding_cancellation
                .check()
                .map_err(|_| super::super::BrokerError::DeadTarget)?;
            commit()
        };
        self.expected
            .view
            .with_document_authority(self.expected.generation, &mut guarded)
    }
}

impl WebViewAdapter {
    fn prepare_host_upload(
        &self,
        run_id: &str,
        target_id: &str,
        target_generation: u64,
        snapshot_id: &str,
        element_ref: &str,
        file: HostSelectedUploadFile,
    ) -> Result<WebViewUpload, String> {
        let expected = self.bound_target(target_id)?;
        if expected.run_id != run_id {
            return Err("WebView upload run identity changed".into());
        }
        if expected.generation != target_generation {
            return Err("WebView upload document generation changed".into());
        }
        expected
            .observation
            .lock()
            .validate_upload(snapshot_id, element_ref)?;
        let epoch = expected.execution_epoch.load(Ordering::SeqCst);
        Ok(WebViewUpload {
            expected,
            epoch,
            snapshot_id: snapshot_id.into(),
            element_ref: element_ref.into(),
            file,
            consumed: AtomicBool::new(false),
        })
    }

    fn check_host_upload(
        &self,
        upload: &WebViewUpload,
        cancellation: &ActionCancellation,
    ) -> Result<(), String> {
        cancellation.check()?;
        if upload.consumed.load(Ordering::SeqCst) {
            return Err("WebView upload already consumed; do not replay".into());
        }
        if !self.still_bound(&upload.expected)
            || upload.expected.execution_epoch.load(Ordering::SeqCst) != upload.epoch
        {
            return Err("WebView upload binding retired".into());
        }
        upload
            .expected
            .observation
            .lock()
            .validate_upload(&upload.snapshot_id, &upload.element_ref)
    }

    /// Completion proves the file-field assignment, not the website's eventual
    /// upload/submission. The model must observe/verify that separate action.
    fn dispatch_host_upload(
        &self,
        upload: &WebViewUpload,
        cancellation: &ActionCancellation,
    ) -> Result<AdapterActResult, String> {
        self.check_host_upload(upload, cancellation)?;
        let expected = &upload.expected;
        let preflight =
            dom::upload_prepare(&expected.tab_id, &upload.snapshot_id, &upload.element_ref);
        let raw = self.host_script(expected, upload.epoch, cancellation, &preflight)?;
        dom::acknowledge(&parse_script_json(&raw)?)?;
        self.check_host_upload(upload, cancellation)?;
        let script = dom::upload_dispatch(
            &expected.tab_id,
            &upload.snapshot_id,
            &upload.element_ref,
            &upload.file,
        );
        let operation = {
            // Admit before consuming, under the same binding lock as abort.
            // Native completion owns the occupancy after this block returns.
            let current = self.bound.lock();
            if !current
                .get(&expected.target_id())
                .is_some_and(|candidate| Arc::ptr_eq(candidate, expected))
                || !expected.is_current()
                || expected.execution_epoch.load(Ordering::SeqCst) != upload.epoch
            {
                return Err("WebView upload retired before dispatch".into());
            }
            let mut observation = expected.observation.lock();
            observation.validate_upload(&upload.snapshot_id, &upload.element_ref)?;
            let operation = self.scripts.begin(
                &expected.run_id,
                &expected.native_key(),
                cancellation,
                &expected.binding_cancellation,
            )?;
            if upload
                .consumed
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                operation.finished(); // This operation has not been dispatched.
                return Err("WebView upload already consumed; do not replay".into());
            }
            observation.snapshot = None;
            operation
        };
        // Do not clear occupancy or restore a file ticket on timeout/cancel.
        let raw = expected
            .view
            .host_script_owned(&script, expected.generation, operation)?;
        if !self.still_bound(expected)
            || expected.execution_epoch.load(Ordering::SeqCst) != upload.epoch
            || cancellation.check().is_err()
        {
            return Err("WebView upload outcome unknown after retirement; do not replay".into());
        }
        dom::action_verified(parse_script_json(&raw)?)?;
        Ok(AdapterActResult {
            applied: true,
            outcome: None,
            postcondition_ok: true,
            verifiable: true,
            detail: "Host-selected file assigned to the retained WebView file input".into(),
        })
    }

    pub(super) fn stage_upload_owned(
        &self,
        request: &UploadRequest,
    ) -> Result<Box<dyn StagedUpload>, UploadError> {
        request
            .cancellation
            .check()
            .map_err(UploadError::NotDispatched)?;
        if request.target_generation == 0
            || request.action_id.trim().is_empty()
            || request.action_id.len() > 256
        {
            return Err(UploadError::NotDispatched(
                "invalid Host upload identity".into(),
            ));
        }
        let upload = self
            .prepare_host_upload(
                &request.run_id,
                &request.target_id,
                request.target_generation,
                &request.snapshot_id,
                &request.element_ref,
                request.file.clone(),
            )
            .map_err(UploadError::NotDispatched)?;
        let result = self
            .dispatch_host_upload(&upload, &request.cancellation)
            .map_err(UploadError::DispatchedUnknown)?;
        Ok(Box::new(WebViewStagedUpload {
            bound: self.bound.clone(),
            expected: upload.expected,
            epoch: upload.epoch,
            cancellation: request.cancellation.clone(),
            result,
        }))
    }
}
