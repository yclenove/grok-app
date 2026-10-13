//! One-shot App WebView downloads staged at a Broker-owned destination.

use super::super::adapter::{DownloadError, DownloadRequest, DownloadResult, StagedDownload};
use super::super::BrokerError;
use super::*;
use std::path::Path;

#[cfg(all(windows, feature = "computer-use-probe"))]
pub(super) mod publication_probe;
#[cfg(windows)]
mod windows;
#[cfg(all(windows, feature = "computer-use-probe"))]
pub(super) use windows::probe;
#[cfg(windows)]
pub(crate) use windows::{
    cancel_instance, cancel_instance_now, claim_wry_requested, download_live, finish_wry,
    maintain_download_buffers, retire_instance, schedule_handler, uninstall_instance, wait_retired,
};
#[cfg(all(windows, feature = "computer-use-probe"))]
pub(crate) use windows::{
    cancel_probe_download_now, download_live_host, install_probe_handlers, probe_download_pending,
    probe_resources_retired, retire_instance_now, uninstall_instance_now,
};

struct WebViewStagedDownload {
    bound: Arc<Mutex<BTreeMap<String, Arc<BoundTab>>>>,
    expected: Arc<BoundTab>,
    epoch: u64,
    cancellation: ActionCancellation,
    result: DownloadResult,
    published: bool,
}

impl Drop for WebViewStagedDownload {
    fn drop(&mut self) {
        if !self.published {
            cleanup_exact_download(&self.result.path);
        }
    }
}

impl StagedDownload for WebViewStagedDownload {
    fn result(&self) -> &DownloadResult {
        &self.result
    }

    fn commit(
        mut self: Box<Self>,
        commit: &mut dyn FnMut() -> Result<(), BrokerError>,
    ) -> Result<(), BrokerError> {
        let target_id = self.expected.target_id();
        let current = self.bound.lock();
        if !current
            .get(&target_id)
            .is_some_and(|candidate| Arc::ptr_eq(candidate, &self.expected))
        {
            return Err(BrokerError::DeadTarget);
        }

        let expected = self.expected.clone();
        let epoch = self.epoch;
        let cancellation = self.cancellation.clone();
        let mut authority_commit = || {
            if expected.execution_epoch.load(Ordering::SeqCst) != epoch {
                return Err(BrokerError::StopRequested);
            }
            cancellation.check().map_err(BrokerError::Adapter)?;
            expected
                .binding_cancellation
                .check()
                .map_err(|_| BrokerError::DeadTarget)?;
            commit()
        };
        expected
            .view
            .with_document_authority(expected.generation, &mut authority_commit)?;
        drop(current);
        self.published = true;
        Ok(())
    }
}

impl WebViewAdapter {
    pub(super) fn stage_download_owned(
        &self,
        request: &DownloadRequest,
    ) -> Result<Box<dyn StagedDownload>, DownloadError> {
        request
            .cancellation
            .check()
            .map_err(DownloadError::not_dispatched)?;
        if request.destination.exists() {
            return Err(DownloadError::not_dispatched(
                "WebView download staging destination is not empty",
            ));
        }

        let expected = self
            .bound_target(&request.target_id)
            .map_err(DownloadError::not_dispatched)?;
        if expected.run_id != request.run_id {
            return Err(DownloadError::not_dispatched(
                "WebView run identity changed",
            ));
        }
        let epoch = expected.execution_epoch.load(Ordering::SeqCst);
        expected
            .observation
            .lock()
            .validate_download(&request.snapshot_id, &request.element_ref)
            .map_err(DownloadError::not_dispatched)?;

        // The resolved URL remains Host-private. It is used only to match the
        // ensuing native DownloadStarting callback and never enters a model
        // observation, trace or adapter error.
        let prepare =
            dom::download_prepare(&expected.tab_id, &request.snapshot_id, &request.element_ref);
        let prepared = self
            .host_script(&expected, epoch, &request.cancellation, &prepare)
            .and_then(|raw| parse_script_json(&raw))
            .and_then(dom::prepared_download_url)
            .map_err(|_| DownloadError::not_dispatched("WebView download preparation failed"))?;

        request
            .cancellation
            .check()
            .map_err(DownloadError::not_dispatched)?;
        if !self.still_bound(&expected) || expected.execution_epoch.load(Ordering::SeqCst) != epoch
        {
            return Err(DownloadError::not_dispatched(
                "WebView document changed before download dispatch",
            ));
        }

        let dispatch =
            dom::download_dispatch(&expected.tab_id, &request.snapshot_id, &request.element_ref);
        let operation = {
            // Linearize one-shot consumption with abort and binding replacement.
            // The long-lived native operation is admitted before the retained
            // snapshot is consumed, so a rejected admission remains retry-safe.
            let current = self.bound.lock();
            if !current
                .get(&expected.target_id())
                .is_some_and(|candidate| Arc::ptr_eq(candidate, &expected))
                || expected.execution_epoch.load(Ordering::SeqCst) != epoch
                || !expected.is_current()
            {
                return Err(DownloadError::not_dispatched(
                    "WebView binding changed before download dispatch",
                ));
            }
            request
                .cancellation
                .check()
                .map_err(DownloadError::not_dispatched)?;
            let mut observation = expected.observation.lock();
            observation
                .validate_download(&request.snapshot_id, &request.element_ref)
                .map_err(DownloadError::not_dispatched)?;
            let operation = self
                .scripts
                .begin(
                    &expected.run_id,
                    &expected.native_key(),
                    &request.cancellation,
                    &expected.binding_cancellation,
                )
                .map_err(DownloadError::not_dispatched)?;
            observation.snapshot = None;
            operation
        };

        let result = expected.view.download_for_run(
            request,
            expected.generation,
            &prepared,
            &dispatch,
            operation,
        )?;
        if result.path != request.destination {
            cleanup_exact_download(&request.destination);
            return Err(DownloadError::dispatched_rejected(
                "WebView download did not use the Broker staging destination",
            ));
        }

        let staged = Box::new(WebViewStagedDownload {
            bound: self.bound.clone(),
            expected,
            epoch,
            cancellation: request.cancellation.clone(),
            result,
            published: false,
        });
        // Fixture-only hold outside every Broker/binding/native authority lock.
        // Keep the authentic staged object alive so its original commit/Drop
        // still decides publication after the control under test.
        #[cfg(all(windows, feature = "computer-use-probe"))]
        publication_probe::before_return(
            &request.run_id,
            &staged.expected.native_key(),
            &staged.result,
        );
        Ok(staged)
    }
}

/// Remove only the exact staged file and its action-private directory. Never
/// recurse into the run staging root or remove sibling download attempts.
fn cleanup_exact_download(path: &Path) {
    let _ = std::fs::remove_file(path);
    if let Some(directory) = path.parent() {
        let _ = std::fs::remove_dir(directory);
    }
}
