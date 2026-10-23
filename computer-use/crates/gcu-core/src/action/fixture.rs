//! Trusted fixture-scope assembly. Model tools cannot call this.

use super::{router::ActionRouter, ToolOutcome};
use crate::browser::Hub;
use crate::driver::Driver;
use crate::scope::{InstanceEvidence, ScopeLease, ScopedDriver};
use crate::session::WindowLocks;
use gcu_host_api::{ApprovalProvider, EventSink, PolicyStore, UserActivity};
use std::sync::Arc;

impl ActionRouter {
    /// Route every call through [`ScopedDriver`].
    ///
    /// The caller is the trusted assembler. This does not read the platform,
    /// wait, or install a browser hub. No timeout or cancellation applies to
    /// construction. The same `activity` must be given to [`crate::session::Session::open`]
    /// so a latched Stop wins over scope denial.
    pub fn with_fixture_scope<D>(
        driver: D,
        lease: ScopeLease,
        activity: Arc<dyn UserActivity>,
        approvals: Arc<dyn ApprovalProvider>,
        policy: Arc<dyn PolicyStore>,
        events: Arc<dyn EventSink>,
        locks: WindowLocks,
    ) -> Self
    where
        D: Driver + InstanceEvidence + 'static,
    {
        let wrapped = Arc::new(ScopedDriver::new(driver, lease.clone(), activity));
        let mut router = Self::new(wrapped, approvals, policy, events, locks);
        router.fixture_scoped = true;
        router.lease = Some(lease);
        router
    }

    pub(crate) fn scope_binding_current(&self, run: Option<&str>, generation: Option<u64>) -> bool {
        match (&self.lease, run, generation) {
            (None, None, None) => true,
            (Some(lease), Some(run), Some(generation)) => lease.same_epoch(run, generation),
            _ => false,
        }
    }

    pub(super) fn scrub_unpublished_target(&self, out: &mut ToolOutcome) {
        // Labels and screenshot references require a committed scoped observation.
        // Clear the whole target before finish clones it into state or the ledger.
        // A successful commit survives later revoke; do not recheck the lease here.
        if self.lease.is_some() && out.state.is_none() {
            out.action.target = None;
        }
    }

    /// Lease installed by the trusted assembler.
    ///
    /// Memory only. No blocking, timeout, or cancellation. `None` when this
    /// router was not built with [`Self::with_fixture_scope`].
    pub fn fixture_lease(&self) -> Option<ScopeLease> {
        self.lease.clone()
    }

    pub(crate) fn scope_stamp(&self) -> (Option<String>, Option<u64>) {
        self.lease
            .as_ref()
            .and_then(|lease| lease.authorized_current())
            .map(|scope| (Some(scope.run_id().to_owned()), Some(scope.generation())))
            .unwrap_or((None, None))
    }

    /// Driver this router calls. In scoped mode it is the wrapper.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn bound_driver(&self) -> Arc<dyn Driver> {
        self.driver.clone()
    }

    pub(crate) fn browser_hub(&self) -> Option<Arc<Hub>> {
        if self.fixture_scoped {
            None
        } else {
            self.browser.clone()
        }
    }
}
