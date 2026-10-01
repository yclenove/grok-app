//! Product desktop path: Host-owned worker admission plus exclusive target ownership.
use crate::adapter::{
    AdapterActResult, Capabilities, CaptureOptions, ComputerUseAdapter, DispatchRequest, TargetInfo,
};
use crate::driver::PrivateWorker;
use crate::execution::ActionCancellation;
use crate::protocol::Observation;
use parking_lot::Mutex;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[path = "owned_ownership.rs"]
mod ownership;

#[cfg(test)]
#[path = "owned_contract.rs"]
mod contract;

pub struct HostOwnedAdapter {
    inner: Arc<dyn ComputerUseAdapter>,
    ownership_id: &'static str,
    instance_id: uuid::Uuid,
    owners: Arc<Mutex<HashMap<String, ownership::Ownership>>>,
    worker: Mutex<Option<Arc<PrivateWorker>>>,
}

impl HostOwnedAdapter {
    pub fn new(inner: Arc<dyn ComputerUseAdapter>) -> Self {
        let ownership_id = inner.backend_id();
        Self {
            inner,
            ownership_id,
            instance_id: uuid::Uuid::new_v4(),
            owners: Arc::new(Mutex::new(HashMap::new())),
            worker: Mutex::new(None),
        }
    }

    pub fn with_ownership_id(mut self, ownership_id: &'static str) -> Self {
        self.ownership_id = ownership_id;
        self
    }

    pub fn share_owners(mut self, other: &Self) -> Self {
        self.owners = other.owners.clone();
        self
    }

    pub fn attach_worker(&self, worker: Arc<PrivateWorker>) {
        *self.worker.lock() = Some(worker);
    }

    fn admit(&self, run_id: &str, cancellation: &ActionCancellation) -> Result<(), String> {
        let Some(worker) = self.worker.lock().clone() else {
            return Ok(());
        };
        worker
            .call(
                "prepare_dispatch",
                json!({}),
                run_id,
                None,
                cancellation,
                Duration::from_secs(2),
            )
            .map(|_| ())
            .map_err(|e| format!("private worker refused dispatch: {e}"))
    }

    fn worker_exec(
        &self,
        name: &str,
        args: serde_json::Value,
        run_id: &str,
        cancellation: &ActionCancellation,
    ) -> Result<serde_json::Value, String> {
        let Some(worker) = self.worker.lock().clone() else {
            return Err("private worker not attached".into());
        };
        let result = worker
            .call(
                name,
                args,
                run_id,
                None,
                cancellation,
                Duration::from_secs(8),
            )
            .map_err(|e| format!("private worker {name}: {e}"))?;
        if result.get("executed") != Some(&json!(true)) {
            return Err(format!(
                "private worker {name} did not execute; prepare_dispatch is not enough"
            ));
        }
        Ok(result)
    }

    fn checked_listing(
        &self,
        run: &str,
        listed: Vec<TargetInfo>,
    ) -> Result<Vec<TargetInfo>, String> {
        if self.worker.lock().is_some() {
            let ids: Vec<&str> = listed.iter().map(|t| t.target_id.as_str()).collect();
            self.worker_exec(
                "list_windows",
                json!({"ids": ids}),
                run,
                &ActionCancellation::default(),
            )?;
        }
        Ok(listed)
    }

    fn capture_owned(
        &self,
        run_id: &str,
        target_id: &str,
        generation: Option<u64>,
        options: CaptureOptions,
    ) -> Result<Observation, String> {
        self.claim_target_for_run(run_id, target_id)?;
        if self.worker.lock().is_some() {
            let admission = self
                .worker_exec(
                    "observe",
                    json!({"targetId": target_id}),
                    run_id,
                    &options.cancellation,
                )
                .and_then(|result| {
                    if result.get("targetId").and_then(|v| v.as_str()) == Some(target_id) {
                        Ok(())
                    } else {
                        Err("private worker observe identity mismatch".into())
                    }
                });
            if let Err(error) = admission {
                // An admitted model replacement may not leave native authority
                // alive when the private-worker stage prevents it reaching the
                // inner capture. No retry/replay; retain ownership until idle.
                if options.for_model {
                    let _ = self.inner.abort(run_id, u64::MAX);
                    self.release_target_for_run(run_id, target_id);
                }
                return Err(error);
            }
        }
        // The native adapter must see even a cancelled replacement, so it can
        // retire its old model ticket before returning the cancellation error.
        // Preserve options verbatim; preview must not become model capture.
        match generation {
            Some(generation) => self
                .inner
                .capture_for_run_at_generation(run_id, target_id, generation, options),
            None => self.inner.capture_for_run(run_id, target_id, options),
        }
    }
}

impl ComputerUseAdapter for HostOwnedAdapter {
    fn backend_id(&self) -> &'static str {
        self.inner.backend_id()
    }

    fn capabilities(&self) -> Capabilities {
        let mut caps = self.inner.capabilities();
        if let Some(worker) = self.worker.lock().as_ref() {
            caps.notes.push(format!(
                "host-owned private worker generation={}",
                worker.generation()
            ));
        } else {
            caps.notes
                .push("host-owned adapter; private worker not attached".into());
        }
        caps
    }

    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        self.checked_listing("host-list", self.inner.list_targets()?)
    }

    fn list_targets_for_run(&self, run_id: &str) -> Result<Vec<TargetInfo>, String> {
        self.checked_listing(run_id, self.inner.list_targets_for_run(run_id)?)
    }

    fn target_alive(&self, target_id: &str) -> bool {
        self.inner.target_alive(target_id)
    }

    fn observe(&self, target_id: &str) -> Result<Observation, String> {
        self.claim_target(target_id)?;
        if self.worker.lock().is_some() {
            let result = self.worker_exec(
                "observe",
                json!({ "targetId": target_id }),
                "host-observe",
                &ActionCancellation::default(),
            )?;
            if result.get("targetId").and_then(|v| v.as_str()) != Some(target_id) {
                return Err("private worker observe identity mismatch".into());
            }
        }
        self.inner.observe(target_id)
    }

    fn act(&self, req: &DispatchRequest) -> Result<AdapterActResult, String> {
        self.claim_target_for_run(&req.run_id, &req.target_id)?;
        req.admit(self)?;
        self.admit(&req.run_id, &req.cancellation)?;
        if self.worker.lock().is_some() {
            let result = self.worker_exec(
                "act",
                json!({
                    "targetId": req.target_id,
                    "actionId": req.action_id,
                }),
                &req.run_id,
                &req.cancellation,
            )?;
            if result.get("targetId").and_then(|v| v.as_str()) != Some(req.target_id.as_str()) {
                return Err("private worker act identity mismatch".into());
            }
        }
        self.inner.act(req)
    }

    fn observe_for_run(&self, run_id: &str, target_id: &str) -> Result<Observation, String> {
        self.capture_for_run(run_id, target_id, CaptureOptions::model(true))
    }

    fn capture_for_run(
        &self,
        run_id: &str,
        target_id: &str,
        options: CaptureOptions,
    ) -> Result<Observation, String> {
        self.capture_owned(run_id, target_id, None, options)
    }

    fn capture_for_run_at_generation(
        &self,
        run_id: &str,
        target_id: &str,
        generation: u64,
        options: CaptureOptions,
    ) -> Result<Observation, String> {
        self.capture_owned(run_id, target_id, Some(generation), options)
    }

    fn wait_uses_retained_reference(&self) -> bool {
        self.inner.wait_uses_retained_reference()
    }

    fn abort(&self, run_id: &str, generation: u64) -> Result<(), String> {
        // Fence native input before any optional worker IPC can wait.
        let native = self.inner.abort(run_id, generation);
        if self.worker.lock().is_some() {
            let _ = self.worker_exec(
                "abort",
                json!({ "generation": generation }),
                run_id,
                &ActionCancellation::default(),
            );
        }
        native
    }

    fn is_idle(&self, run_id: &str) -> bool {
        self.owner_idle(Some(run_id))
    }

    fn start_periodic_preview(&self, target_id: &str) {
        self.inner.start_periodic_preview(target_id);
    }

    fn stop_periodic_preview(&self) {
        self.inner.stop_periodic_preview();
    }

    fn periodic_preview_active(&self) -> bool {
        self.inner.periodic_preview_active()
    }

    fn input_available(&self) -> bool {
        self.inner.input_available()
    }

    fn foreground_input_available(&self, target_id: &str) -> bool {
        self.inner.foreground_input_available(target_id)
    }

    fn current_geometry_revision(&self) -> u64 {
        self.inner.current_geometry_revision()
    }

    fn current_geometry_revision_for(&self, target_id: &str) -> u64 {
        self.inner.current_geometry_revision_for(target_id)
    }

    fn user_input_active(&self) -> bool {
        self.inner.user_input_active()
    }

    fn claim_target(&self, target_id: &str) -> Result<(), String> {
        self.claim_owned(None, target_id)
    }

    fn claim_target_for_run(&self, run_id: &str, target_id: &str) -> Result<(), String> {
        self.claim_owned(Some(run_id), target_id)
    }

    fn release_target(&self, target_id: &str) {
        self.release_owned(None, target_id);
    }

    fn release_target_for_run(&self, run_id: &str, target_id: &str) {
        self.release_owned(Some(run_id), target_id);
    }

    fn worker_attached(&self) -> bool {
        self.worker.lock().is_some()
    }

    fn worker_generation(&self) -> Option<String> {
        self.worker
            .lock()
            .as_ref()
            .map(|w| w.generation().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeAdapter;

    #[test]
    fn host_owned_adapter_rejects_backend_switch_without_release() {
        let fake = Arc::new(FakeAdapter::new());
        let windows = HostOwnedAdapter::new(fake.clone()).with_ownership_id("windows");
        let webview = HostOwnedAdapter::new(fake)
            .with_ownership_id("webview")
            .share_owners(&windows);
        let target = windows.list_targets().unwrap()[0].target_id.clone();
        windows.claim_target(&target).unwrap();
        let err = webview.claim_target(&target).unwrap_err();
        assert!(err.contains("stop and re-authorize"), "{err}");
        windows.release_target(&target);
        webview.claim_target(&target).unwrap();
    }

    #[test]
    fn host_owned_adapter_same_backend_can_reclaim() {
        let fake = Arc::new(FakeAdapter::new());
        let owned = HostOwnedAdapter::new(fake);
        let target = owned.list_targets().unwrap()[0].target_id.clone();
        owned.claim_target(&target).unwrap();
        owned.claim_target(&target).unwrap();
        assert!(!owned.worker_attached());
    }
}
