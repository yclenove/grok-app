//! Hooks surround the original registry proof; no fake browser authority.
use super::*;
use crate::adapter::*;
use crate::browser::{ExistingBrowserAdapter, ExistingTabHost};

type Hook = (String, Box<dyn FnOnce() + Send>);

pub(super) struct Adapter {
    inner: ExistingBrowserAdapter,
    host: Arc<ExistingTabHost>,
    hook: Mutex<Option<Hook>>,
}

pub(super) fn fixture() -> (Fixture, Arc<Adapter>) {
    let mut gate = None;
    let f = Fixture::with_existing(|broker| {
        let host = broker.tabs.clone();
        let adapter = Arc::new(Adapter {
            inner: ExistingBrowserAdapter::new(host.clone()),
            host,
            hook: Mutex::new(None),
        });
        broker
            .register_surface_adapter(SurfaceKind::ExistingTab, adapter.clone())
            .unwrap();
        gate = Some(adapter);
    });
    (f, gate.unwrap())
}

impl Adapter {
    pub(super) fn after_borrow(&self, tab: &str, hook: impl FnOnce() + Send + 'static) {
        let mut pending = self.hook.lock();
        assert!(pending.is_none());
        *pending = Some((tab.into(), Box::new(hook)));
    }
}

impl ComputerUseAdapter for Adapter {
    fn backend_id(&self) -> &'static str {
        self.inner.backend_id()
    }
    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }
    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        self.inner.list_targets()
    }
    fn scope_candidates(
        &self,
        limit: usize,
        current: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<ScopeCandidatePage, String> {
        self.inner.scope_candidates(limit, current)
    }
    fn target_alive(&self, target: &str) -> bool {
        self.inner.target_alive(target)
    }
    fn scope_identity(&self, target: &str) -> Result<NativeScopeIdentity, String> {
        let hook = {
            let mut pending = self.hook.lock();
            if pending.as_ref().is_some_and(|(tab, _)| {
                target == format!("tab:{tab}") && self.host.existing_target_info(tab).is_some()
            }) {
                pending.take()
            } else {
                None
            }
        };
        if let Some((_, hook)) = hook {
            hook();
        }
        self.inner.scope_identity(target)
    }
    fn scope_target_info(&self, run: &str, target: &str) -> Result<TargetInfo, String> {
        self.inner.scope_target_info(run, target)
    }
    fn observe(&self, target: &str) -> Result<crate::protocol::Observation, String> {
        self.inner.observe(target)
    }
    fn capture_for_run(
        &self,
        run: &str,
        target: &str,
        options: CaptureOptions,
    ) -> Result<crate::protocol::Observation, String> {
        self.inner.capture_for_run(run, target, options)
    }
    fn act(&self, request: &DispatchRequest) -> Result<AdapterActResult, String> {
        self.inner.act(request)
    }
    fn abort(&self, run: &str, generation: u64) -> Result<(), String> {
        self.inner.abort(run, generation)
    }
    fn is_idle(&self, run: &str) -> bool {
        self.inner.is_idle(run)
    }
    fn start_periodic_preview(&self, target: &str) {
        self.inner.start_periodic_preview(target);
    }
    fn stop_periodic_preview(&self) {
        self.inner.stop_periodic_preview();
    }
    fn periodic_preview_active(&self) -> bool {
        self.inner.periodic_preview_active()
    }
    fn current_geometry_revision_for(&self, target: &str) -> u64 {
        self.inner.current_geometry_revision_for(target)
    }
    fn claim_target_for_run(&self, run: &str, target: &str) -> Result<(), String> {
        self.inner.claim_target_for_run(run, target)
    }
    fn release_target_for_run(&self, run: &str, target: &str) {
        self.inner.release_target_for_run(run, target);
    }
}
