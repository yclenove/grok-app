use grok_computer_use_core::adapter::{
    AdapterActResult, Capabilities, ComputerUseAdapter, DispatchRequest, SurfaceKind, TargetInfo,
};
use grok_computer_use_core::protocol::Observation;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const COUNTING_TARGET: &str = "test:window:computer-use";

#[derive(Default)]
pub(crate) struct CountingAdapter {
    aborts: AtomicU64,
    abort_blocked: std::sync::atomic::AtomicBool,
    releases: AtomicU64,
    listed_runs: parking_lot::Mutex<Vec<String>>,
}

impl CountingAdapter {
    pub(crate) fn listed_runs(&self) -> Vec<String> {
        self.listed_runs.lock().clone()
    }
    pub(crate) fn set_abort_blocked(&self, blocked: bool) {
        self.abort_blocked.store(blocked, Ordering::SeqCst);
    }

    pub(crate) fn aborts(&self) -> u64 {
        self.aborts.load(Ordering::SeqCst)
    }

    pub(crate) fn releases(&self) -> u64 {
        self.releases.load(Ordering::SeqCst)
    }
}

impl ComputerUseAdapter for CountingAdapter {
    fn backend_id(&self) -> &'static str {
        "counting-test"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::for_surface(SurfaceKind::Desktop, self.backend_id())
    }

    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        Ok(vec![TargetInfo {
            target_id: COUNTING_TARGET.into(),
            title: "Computer Use counting target".into(),
            app_name: "counting-test".into(),
            kind: "window".into(),
            pid: Some(std::process::id()),
            backend: self.backend_id().into(),
            execution_mode: "exclusive".into(),
            replay_policy: "never".into(),
            lifecycle_stamp: 1,
            display_id: "counting-display".into(),
            coordinate_space: "image-pixels".into(),
            scope_label: "Computer Use lifecycle test target".into(),
        }])
    }

    fn target_alive(&self, target_id: &str) -> bool {
        target_id == COUNTING_TARGET
    }

    fn list_targets_for_run(&self, run_id: &str) -> Result<Vec<TargetInfo>, String> {
        self.listed_runs.lock().push(run_id.to_owned());
        self.list_targets()
    }

    fn observe(&self, _target_id: &str) -> Result<Observation, String> {
        Err("unused in Computer Use lifecycle test".into())
    }

    fn act(&self, _request: &DispatchRequest) -> Result<AdapterActResult, String> {
        Err("unused in Computer Use lifecycle test".into())
    }

    fn abort(&self, _run_id: &str, _generation: u64) -> Result<(), String> {
        self.aborts.fetch_add(1, Ordering::SeqCst);
        while self.abort_blocked.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }

    fn is_idle(&self, _run_id: &str) -> bool {
        true
    }

    fn start_periodic_preview(&self, _target_id: &str) {}

    fn stop_periodic_preview(&self) {}

    fn periodic_preview_active(&self) -> bool {
        false
    }

    fn release_target_for_run(&self, _run_id: &str, target_id: &str) {
        if target_id == COUNTING_TARGET {
            self.releases.fetch_add(1, Ordering::SeqCst);
        }
    }
}
