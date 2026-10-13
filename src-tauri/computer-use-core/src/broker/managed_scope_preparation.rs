//! A resource preparation opens only its own blank context/tab, not a grant.
use super::*;
use crate::adapter::NativeScopeIdentity;
use crate::task_consent::{ManagedResourceTicket, NativeScopeProof, PrepareTicket, ScopeMember};

impl ComputerUseBroker {
    pub(super) fn prepare_managed_task_scope(
        &self,
        ticket: &ManagedResourceTicket,
    ) -> Result<PrepareTicket, BrokerError> {
        let _change = self.changes.notice();
        let executor = self.require_surface_executor(SurfaceKind::ManagedBrowser)?;
        let (request, generation, _admission) = {
            let mut inner = self.inner.lock();
            if !inner.feature_enabled
                || !inner.consent.managed_resource_current(
                    ticket,
                    inner.feature_enabled,
                    inner.feature_epoch,
                )
            {
                return Err(BrokerError::StopRequested);
            }
            // The original pending owner exists before the first Host admission,
            // spawn or HTTP call. Never create a Legacy run and mark it later.
            if inner.runs.contains_key(&ticket.run_id) {
                return Err(BrokerError::IdentityMismatch(
                    "managed resource owner already opened",
                ));
            }
            let mut run = fresh_run(ticket.session_id.clone(), None);
            run.consent = ConsentGate::Pending;
            let generation = run.generation;
            run.in_flight.store(true, Ordering::SeqCst);
            let admission = InFlightClear::new(run.in_flight.clone(), &self.changes);
            let cancellation = run.cancellation.clone();
            inner.runs.insert(ticket.run_id.clone(), run);
            inner.consent.note_opened_run(ticket.id);
            let request = self
                .tabs
                .admit_managed_request(&ticket.run_id, cancellation)?;
            (request, generation, admission)
        };
        // All potentially blocking work stays outside the Broker authority lock.
        let scoped = Arc::new(self.tabs.for_managed_request(&request)?);
        let tab =
            scoped.open_managed_profile(&ticket.session_id, &ticket.run_id, &ticket.profile)?;
        if tab.url != "about:blank"
            || tab.user_owned
            || tab.run_id != ticket.run_id
            || tab.session != ticket.session_id
        {
            return Err(BrokerError::IdentityMismatch("managed initial tab"));
        }
        let identity = ManagedBrowserAdapter::new(scoped)
            .scope_identity(&tab.tab_id)
            .map_err(BrokerError::Adapter)?;
        let member = ScopeMember {
            target_id: tab.tab_id,
            surface: SurfaceKind::ManagedBrowser,
            display_name: None,
            native: Some(NativeScopeProof {
                executor_generation: executor.generation,
                identity,
            }),
        };
        let mut inner = self.inner.lock();
        let run = inner
            .runs
            .get(&ticket.run_id)
            .ok_or(BrokerError::RunNotFound)?;
        if run.generation != generation
            || run.stop != StopState::Running
            || run.paused
            || run.resume_in_flight
            || run.switch_in_flight
            || run.target.is_some()
            || !matches!(run.consent, ConsentGate::Pending)
        {
            return Err(BrokerError::StopRequested);
        }
        let enabled = inner.feature_enabled;
        let epoch = inner.feature_epoch;
        inner
            .consent
            .bind_managed_resource_scope(ticket, member, enabled, epoch)
    }

    /// A proved task tab never uses legacy profile provisioning or page discovery.
    pub(super) fn authorize_prepared_managed(
        &self,
        ticket: &PrepareTicket,
    ) -> Result<u64, BrokerError> {
        let proof = ticket
            .native
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?;
        if !matches!(&proof.identity, NativeScopeIdentity::ManagedPage { identity } if identity.owner == ticket.run_id)
        {
            return Err(BrokerError::IdentityMismatch("managed scope run"));
        }
        self.prepare_authorize(&ticket.run_id)?;
        let executor = self.require_surface_executor(SurfaceKind::ManagedBrowser)?;
        if executor.generation != proof.executor_generation {
            return Err(BrokerError::IdentityMismatch("managed executor"));
        }
        let (generation, admission) = {
            let inner = self.inner.lock();
            if !inner.consent.preparation_matches(
                ticket,
                inner.feature_enabled,
                inner.feature_epoch,
            ) {
                return Err(BrokerError::StopRequested);
            }
            let run = inner
                .runs
                .get(&ticket.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            if run.stop != StopState::Running
                || run.paused
                || run.in_flight.load(Ordering::SeqCst)
                || run.app_session_id != ticket.session_id
                || !matches!(run.consent, ConsentGate::Pending)
            {
                return Err(BrokerError::TargetUnauthorized);
            }
            run.in_flight.store(true, Ordering::SeqCst);
            (
                run.generation,
                InFlightClear::new(run.in_flight.clone(), &self.changes),
            )
        };
        executor
            .adapter
            .claim_target_for_run(&ticket.run_id, &ticket.target_id)
            .map_err(BrokerError::Adapter)?;
        let info = executor
            .adapter
            .scope_target_info(&ticket.run_id, &ticket.target_id)
            .map_err(BrokerError::Adapter)?;
        if info.target_id != ticket.target_id {
            return Err(BrokerError::TargetUnauthorized);
        }
        self.commit_authorize(
            &ticket.run_id,
            &ticket.target_id,
            info.title,
            SurfaceKind::ManagedBrowser,
            executor.generation,
            Some((generation, &admission)),
        )
    }
}
