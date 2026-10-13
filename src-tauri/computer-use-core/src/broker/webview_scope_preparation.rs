//! Original pending owner precedes candidate binding. Failure cleans exact IDs.
use super::*;
use crate::adapter::NativeScopeIdentity;
use crate::task_consent::{NativeScopeProof, PrepareTicket, ScopeMember, WebViewSelectionTicket};
use std::collections::HashSet;

struct Bindings {
    adapter: Arc<dyn ComputerUseAdapter>,
    run: String,
    targets: Vec<String>,
    retained: bool,
}
impl Drop for Bindings {
    fn drop(&mut self) {
        if !self.retained {
            for target in &self.targets {
                self.adapter.release_target_for_run(&self.run, target);
            }
        }
    }
}

impl ComputerUseBroker {
    pub(super) fn prepare_webview_task_scope(
        &self,
        ticket: &WebViewSelectionTicket,
    ) -> Result<PrepareTicket, BrokerError> {
        let _change = self.changes.notice();
        let executor = self.require_surface_executor(SurfaceKind::WebView)?;
        self.recheck_scope_members(&ticket.members)?;
        let mut native = HashSet::new();
        for member in &ticket.members {
            let proof = member
                .native
                .as_ref()
                .ok_or(BrokerError::TargetUnauthorized)?;
            let NativeScopeIdentity::WebViewSelected { identity } = &proof.identity else {
                return Err(BrokerError::TargetUnauthorized);
            };
            if proof.executor_generation != executor.generation
                || !native.insert(identity.native_tab_id.clone())
            {
                return Err(BrokerError::TargetUnauthorized);
            }
        }
        let (generation, _admission) = {
            let mut inner = self.inner.lock();
            if !inner.feature_enabled
                || !inner.consent.webview_selection_current(
                    ticket,
                    inner.feature_enabled,
                    inner.feature_epoch,
                )
            {
                return Err(BrokerError::StopRequested);
            }
            if inner.runs.contains_key(&ticket.run_id) {
                return Err(BrokerError::IdentityMismatch(
                    "WebView selection owner already opened",
                ));
            }
            let mut run = fresh_run(ticket.session_id.clone(), None);
            run.consent = ConsentGate::Pending;
            run.in_flight.store(true, Ordering::SeqCst);
            let generation = run.generation;
            let admission = InFlightClear::new(run.in_flight.clone(), &self.changes);
            inner.runs.insert(ticket.run_id.clone(), run);
            inner.consent.note_opened_run(ticket.id);
            (generation, admission)
        };
        let mut bindings = Bindings {
            adapter: executor.adapter.clone(),
            run: ticket.run_id.clone(),
            targets: Vec::new(),
            retained: false,
        };
        let mut members = Vec::with_capacity(ticket.members.len());
        for selected in &ticket.members {
            {
                let inner = self.inner.lock();
                if !inner.consent.webview_selection_current(
                    ticket,
                    inner.feature_enabled,
                    inner.feature_epoch,
                ) || !inner.runs.get(&ticket.run_id).is_some_and(|run| {
                    run.generation == generation
                        && run.stop == StopState::Running
                        && !run.paused
                        && matches!(run.consent, ConsentGate::Pending)
                }) {
                    return Err(BrokerError::StopRequested);
                }
            }
            let proof = selected
                .native
                .as_ref()
                .ok_or(BrokerError::TargetUnauthorized)?;
            let bound = executor
                .adapter
                .bind_scope_candidate(&proof.identity, &ticket.session_id, &ticket.run_id)
                .map_err(BrokerError::Adapter)?;
            // Record ownership before validating the returned metadata/proof.
            // A late or malformed result still needs its exact cleanup owner.
            bindings.targets.push(bound.info.target_id.clone());
            let member = ScopeMember {
                target_id: bound.info.target_id,
                surface: SurfaceKind::WebView,
                display_name: selected.display_name.clone(),
                native: Some(NativeScopeProof {
                    executor_generation: executor.generation,
                    identity: bound.identity,
                }),
            };
            self.recheck_scope_members(std::slice::from_ref(&member))?;
            members.push(member);
        }
        let identities: Vec<_> = members
            .iter()
            .filter_map(|member| member.native.as_ref().map(|proof| proof.identity.clone()))
            .collect();
        let mut inner = self.inner.lock();
        let run = inner
            .runs
            .get(&ticket.run_id)
            .ok_or(BrokerError::RunNotFound)?;
        if run.generation != generation
            || run.stop != StopState::Running
            || run.paused
            || !matches!(run.consent, ConsentGate::Pending)
        {
            return Err(BrokerError::StopRequested);
        }
        let enabled = inner.feature_enabled;
        let epoch = inner.feature_epoch;
        let mut result = None;
        let mut original = Some(members);
        executor
            .adapter
            .with_scope_authority(&identities, &mut || {
                let members = original
                    .take()
                    .ok_or(BrokerError::IdentityMismatch("scope bind repeated"))?;
                result = Some(
                    inner
                        .consent
                        .bind_webview_selection_scope(ticket, members, enabled, epoch)?,
                );
                inner.runs.get_mut(&ticket.run_id).unwrap().webview_targets =
                    bindings.targets.clone();
                Ok(())
            })?;
        drop(inner);
        let result = result.ok_or(BrokerError::IdentityMismatch("scope bind missing"))?;
        bindings.retained = true;
        Ok(result)
    }
}
