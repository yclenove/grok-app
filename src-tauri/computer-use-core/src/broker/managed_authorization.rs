//! Profile provisioning and grant publication share one Broker admission.
use super::*;

impl ComputerUseBroker {
    pub(super) fn authorize_managed_target(
        &self,
        session: &str,
        run_id: &str,
        selector: &str,
    ) -> Result<u64, BrokerError> {
        let executor = self.require_surface_executor(SurfaceKind::ManagedBrowser)?;
        let (request, generation, admission) = {
            let mut g = self.inner.lock();
            if !g.feature_enabled {
                return Err(BrokerError::FeatureDisabled);
            }
            let run = g.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
            if run.app_session_id != session {
                return Err(BrokerError::IdentityMismatch("appSessionId"));
            }
            if run.stop != StopState::Running {
                return Err(BrokerError::StopRequested);
            }
            if run.paused {
                return Err(BrokerError::Schema("run is paused".into()));
            }
            if run.in_flight.load(Ordering::SeqCst) {
                return Err(BrokerError::LeaseHeld {
                    run_id: run_id.into(),
                });
            }
            if run
                .target
                .as_ref()
                .is_some_and(|t| t.surface != SurfaceKind::ManagedBrowser)
            {
                return Err(BrokerError::Schema(
                    "session cannot reuse a target across surfaces".into(),
                ));
            }
            let request = self
                .tabs
                .admit_managed_request(run_id, run.cancellation.clone())?;
            run.in_flight.store(true, Ordering::SeqCst);
            (
                request,
                run.generation,
                InFlightClear(run.in_flight.clone()),
            )
        };
        let tabs = self.tabs.for_managed_request(&request)?;
        let target = if let Some(profile) = selector.strip_prefix("managed-profile:") {
            tabs.open_managed_profile(session, run_id, profile)?.tab_id
        } else {
            selector.to_string()
        };
        if !executor.adapter.target_alive(&target) {
            return Err(BrokerError::DeadTarget);
        }
        executor
            .adapter
            .claim_target_for_run(run_id, &target)
            .map_err(BrokerError::Adapter)?;
        // Reject a foreign target locally before requesting its page list.
        // An explicit reauthorization can then refresh a navigation whose
        // cancelled response was correctly prevented from publishing state.
        if !selector.starts_with("managed-profile:") && tabs.managed_target_info(&target).is_some()
        {
            tabs.refresh_managed_target(run_id, &target)?;
        }
        let title = executor
            .adapter
            .list_targets_for_run(run_id)
            .map_err(BrokerError::Adapter)?
            .into_iter()
            .find(|t| t.target_id == target)
            .ok_or(BrokerError::TargetUnauthorized)?
            .title;
        self.commit_authorize(
            run_id,
            &target,
            title,
            SurfaceKind::ManagedBrowser,
            executor.generation,
            Some((generation, &admission)),
        )
    }
}
