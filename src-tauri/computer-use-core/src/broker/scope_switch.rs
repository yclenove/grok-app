//! Scope membership is stable; the execution binding is replaceable.
//! Cleanup uses the retained executor and never holds Broker state.
use super::*;
use crate::task_consent::ExecutionPermit;
use serde_json::{json, Value};

mod existing;

struct SwitchAdmission<'a> {
    broker: &'a ComputerUseBroker,
    run_id: String,
    generation: u64,
}

impl Drop for SwitchAdmission<'_> {
    fn drop(&mut self) {
        let _change = self.broker.changes.notice();
        if let Some(run) = self.broker.inner.lock().runs.get_mut(&self.run_id) {
            // No successor can enter while this flag is held. Stop/Pause may
            // advance generation, but that does not transfer this ownership.
            run.switch_in_flight = false;
        }
    }
}

impl ComputerUseBroker {
    /// Preflight neither claims a target nor retires the current observation.
    pub(super) fn preflight_scope_switch(
        &self,
        permit: &ExecutionPermit,
    ) -> Result<(), BrokerError> {
        let member = self.recheck_scope_call(permit)?;
        let inner = self.inner.lock();
        Self::recheck_scope_call_locked(&inner, permit)?;
        let run = inner
            .runs
            .get(&permit.run_id)
            .ok_or(BrokerError::RunNotFound)?;
        let target = run.target.as_ref().ok_or(BrokerError::TargetUnauthorized)?;
        if run.paused || run.resume_in_flight || run.switch_in_flight {
            return Err(BrokerError::StopRequested);
        }
        // Cross-surface bindings need a separate runtime-owner transaction.
        // Preserve the legacy surface and browser ownership fences.
        if (member.surface != target.surface
            || !matches!(
                member.surface,
                SurfaceKind::Desktop | SurfaceKind::ExistingTab | SurfaceKind::WebView
            ))
            && (member.target_id != target.target_id || run.switch_cleanup_pending)
        {
            return Err(BrokerError::Schema(
                "scope switching is unavailable on this surface".into(),
            ));
        }
        Ok(())
    }

    pub fn open_approved_task_target(
        &self,
        token: &str,
        args: &Value,
    ) -> Result<Value, BrokerError> {
        let _change = self.changes.notice();
        let options = args
            .as_object()
            .ok_or_else(|| BrokerError::Schema("invalid target selection".into()))?;
        if options
            .keys()
            .any(|key| !matches!(key.as_str(), "runId" | "run_id" | "targetId" | "target_id"))
        {
            return Err(BrokerError::Schema("invalid target selection".into()));
        }
        let claim = crate::tools_claim::execution_claim(args)?;
        if claim.target_id.is_none() {
            return Err(BrokerError::IdentityMismatch("targetId"));
        }
        let mut permit = self.begin_model_call_claiming(
            token,
            "computer_open_target",
            claim.run_id,
            claim.target_id,
        )?;
        self.preflight_scope_switch(&permit)?;
        let member = self.recheck_scope_call(&permit)?;
        let title = if member.native.is_some() {
            self.scope_member_info(&permit.run_id, &member, || {
                Self::recheck_scope_call_locked(&self.inner.lock(), &permit)
            })?
            .title
        } else {
            member.target_id.clone()
        };
        if member.surface == SurfaceKind::ExistingTab {
            return self.switch_existing_member(permit, member, title);
        }
        let (old, admission) = {
            let mut inner = self.inner.lock();
            Self::recheck_scope_call_locked(&inner, &permit)?;
            let run = inner
                .runs
                .get_mut(&permit.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            if run.paused || run.switch_in_flight || run.resume_in_flight {
                return Err(BrokerError::StopRequested);
            }
            let old = run.target.clone().ok_or(BrokerError::TargetUnauthorized)?;
            if old.target_id == member.target_id && !run.switch_cleanup_pending {
                return Ok(
                    json!({"runId":permit.run_id,"targetId":old.target_id,"targetGeneration":old.target_generation}),
                );
            }
            let generation = run
                .generation
                .checked_add(1)
                .ok_or(BrokerError::StopRequested)?;
            run.switch_in_flight = true;
            run.switch_cleanup_pending = true;
            run.cancellation.cancel();
            run.generation = generation;
            permit.execution_generation = generation;
            run.snapshot_id = None;
            run.last_image_ok = false;
            run.element_refs.clear();
            run.element_boxes.clear();
            run.geometry_epoch = 0;
            (
                old,
                SwitchAdmission {
                    broker: self,
                    run_id: permit.run_id.clone(),
                    generation,
                },
            )
        };
        let old_executor = self.executor_for_target(&old)?;
        self.changes.publish();
        old_executor
            .adapter
            .abort(&permit.run_id, admission.generation)
            .map_err(BrokerError::Adapter)?;
        old_executor.adapter.stop_periodic_preview();
        if !old_executor.adapter.is_idle(&permit.run_id) {
            return Err(BrokerError::LeaseHeld {
                run_id: permit.run_id.clone(),
            });
        }
        {
            let inner = self.inner.lock();
            Self::recheck_scope_call_locked(&inner, &permit)?;
            if inner.runs[&permit.run_id].in_flight.load(Ordering::SeqCst) {
                return Err(BrokerError::LeaseHeld {
                    run_id: permit.run_id.clone(),
                });
            }
        }
        if old.surface != SurfaceKind::WebView {
            old_executor
                .adapter
                .release_target_for_run(&permit.run_id, &old.target_id);
        }
        // Old work is terminal. Respect another session's file lease while
        // reacquiring instead of keeping an input owner through the switch.
        if old.surface == SurfaceKind::Desktop {
            self.lease
                .release(&permit.run_id)
                .map_err(|error| match error {
                    crate::error::LeaseError::Held { run_id, .. } => {
                        BrokerError::LeaseHeld { run_id }
                    }
                    crate::error::LeaseError::Io(message) => BrokerError::Adapter(message),
                })?;
        }
        self.recheck_scope_call(&permit)?;
        let executor = self.require_surface_executor(member.surface)?;
        if member.surface == SurfaceKind::Desktop {
            self.lease
                .try_acquire(&permit.run_id, &self.instance_id)
                .map_err(|error| match error {
                    crate::error::LeaseError::Held { run_id, .. } => {
                        BrokerError::LeaseHeld { run_id }
                    }
                    crate::error::LeaseError::Io(message) => BrokerError::Adapter(message),
                })?;
        }
        let result = (|| {
            self.recheck_scope_call(&permit)?;
            executor
                .adapter
                .claim_target_for_run(&permit.run_id, &member.target_id)
                .map_err(BrokerError::Adapter)?;
            self.recheck_scope_call(&permit)?;
            if !executor.adapter.target_alive(&member.target_id) {
                return Err(BrokerError::DeadTarget);
            }
            self.recheck_scope_call(&permit)?;
            let mut inner = self.inner.lock();
            Self::recheck_scope_call_locked(&inner, &permit)?;
            let run = inner
                .runs
                .get(&permit.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            if !run.switch_in_flight
                || !run.switch_cleanup_pending
                || run.paused
                || run.in_flight.load(Ordering::SeqCst)
            {
                return Err(BrokerError::StopRequested);
            }
            let binding = inner.consent.select_active_member(&permit)?;
            let run = inner.runs.get_mut(&permit.run_id).unwrap();
            run.target = Some(AuthorizedTarget {
                target_id: member.target_id.clone(),
                title,
                backend: executor.adapter.backend_id().into(),
                closed: false,
                target_generation: run.generation,
                surface: member.surface,
                executor_generation: executor.generation,
            });
            run.consent = ConsentGate::Bound(binding);
            run.switch_cleanup_pending = false;
            run.cancellation = crate::execution::ActionCancellation::default();
            run.geometry_revision = 0;
            run.image_size = (0, 0);
            run.last_unknown = false;
            run.unknown_idle_confirmed = false;
            let generation = run.generation;
            push_trace(
                &mut inner,
                "scope_switch",
                &permit.run_id,
                &member.target_id,
                TraceAudience::Model,
            );
            Ok(
                json!({"runId":permit.run_id,"targetId":member.target_id,"targetGeneration":generation}),
            )
        })();
        if result.is_err() {
            if member.surface != SurfaceKind::WebView {
                executor
                    .adapter
                    .release_target_for_run(&permit.run_id, &member.target_id);
            }
            if member.surface == SurfaceKind::Desktop {
                let _ = self.lease.release(&permit.run_id);
            }
        }
        result
    }
}
