//! Broker routing over retained selections. No ambient/current-run fallback.
use crate::{adapter::portal_capabilities, PortalAdapter, PortalRegistry};
use grok_computer_use_core::{
    adapter::{
        AdapterActResult, Capabilities, CaptureOptions, ComputerUseAdapter, DispatchRequest,
        TargetInfo,
    },
    protocol::Observation,
};
use std::sync::Arc;

impl PortalRegistry {
    fn for_target(&self, run: &str, target: &str) -> Result<Arc<PortalAdapter>, String> {
        self.entry(run)
            .and_then(|e| e.adapter())
            .filter(|a| a.target_id() == target)
            .ok_or_else(|| "run does not own an active portal target".into())
    }
    fn target(&self, target: &str) -> Option<Arc<PortalAdapter>> {
        self.all_entries()
            .into_iter()
            .filter_map(|e| e.adapter())
            .find(|a| a.target_id() == target)
    }
}

impl ComputerUseAdapter for PortalRegistry {
    fn backend_id(&self) -> &'static str {
        "wayland-portal"
    }
    fn capabilities(&self) -> Capabilities {
        portal_capabilities()
    }
    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        Err("portal target discovery requires its owning run".into())
    }
    fn list_targets_for_run(&self, run: &str) -> Result<Vec<TargetInfo>, String> {
        match self.entry(run).and_then(|e| e.adapter()) {
            Some(adapter) => adapter.list_targets_for_run(run),
            None => Ok(Vec::new()),
        }
    }
    fn target_alive(&self, target: &str) -> bool {
        self.target(target).is_some_and(|a| a.target_alive(target))
    }
    fn observe(&self, _: &str) -> Result<Observation, String> {
        Err("portal capture requires run and Broker generation".into())
    }
    fn capture_for_run_at_generation(
        &self,
        run: &str,
        target: &str,
        generation: u64,
        options: CaptureOptions,
    ) -> Result<Observation, String> {
        self.for_target(run, target)?
            .capture_for_run_at_generation(run, target, generation, options)
    }
    fn act(&self, request: &DispatchRequest) -> Result<AdapterActResult, String> {
        self.for_target(&request.run_id, &request.target_id)?
            .act(request)
    }
    fn claim_target(&self, _: &str) -> Result<(), String> {
        Err("portal claim requires its owning run".into())
    }
    fn claim_target_for_run(&self, run: &str, target: &str) -> Result<(), String> {
        self.for_target(run, target)?
            .claim_target_for_run(run, target)
    }
    fn release_target_for_run(&self, run: &str, target: &str) {
        if let Some(entry) = self.entry(run) {
            if entry
                .adapter()
                .is_some_and(|adapter| adapter.target_id() == target)
            {
                entry.close();
            }
        }
    }
    fn abort(&self, run: &str, generation: u64) -> Result<(), String> {
        if let Some(entry) = self.entry(run) {
            entry.abort(generation);
        }
        Ok(())
    }
    fn is_idle(&self, run: &str) -> bool {
        self.idle_for_run(run)
    }
    fn input_available(&self) -> bool {
        self.all_entries()
            .into_iter()
            .filter_map(|e| e.adapter())
            .any(|a| a.input_available())
    }
    fn foreground_input_available(&self, target: &str) -> bool {
        self.target(target)
            .is_some_and(|a| a.foreground_input_available(target))
    }
    fn user_input_active(&self) -> bool {
        self.policy.user_input_active()
    }
    fn current_geometry_revision_for(&self, target: &str) -> u64 {
        self.target(target)
            .map(|a| a.current_geometry_revision_for(target))
            .unwrap_or(0)
    }
    fn start_periodic_preview(&self, _: &str) {}
    fn stop_periodic_preview(&self) {}
    fn periodic_preview_active(&self) -> bool {
        false
    }
}
