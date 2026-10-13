//! Switch only original approved shares; preserve the old borrow until commit.
use super::*;
use crate::adapter::NativeScopeIdentity;
use crate::browser::{ExistingTabHost, TabInfo};
use crate::task_consent::ScopeMember;

struct NewBorrow {
    host: Arc<ExistingTabHost>,
    info: TabInfo,
    committed: bool,
}

impl Drop for NewBorrow {
    fn drop(&mut self) {
        if !self.committed {
            // Exact captured generation, not a raw tab/run lookup. A reshare,
            // Stop or foreign successor cannot transfer this cleanup right.
            self.host.release_existing_grant(&self.info);
        }
    }
}

impl ComputerUseBroker {
    pub(in crate::broker) fn switch_existing_member(
        &self,
        mut permit: ExecutionPermit,
        member: ScopeMember,
        title: String,
    ) -> Result<Value, BrokerError> {
        let proof = member
            .native
            .as_ref()
            .ok_or(BrokerError::TargetUnauthorized)?;
        let NativeScopeIdentity::ExistingPage { identity } = &proof.identity else {
            return Err(BrokerError::TargetUnauthorized);
        };
        let executor = self.require_surface_executor(SurfaceKind::ExistingTab)?;
        if executor.generation != proof.executor_generation {
            return Err(BrokerError::IdentityMismatch("scope executor"));
        }
        // The same serialization as original preparation. Stop never waits
        // here, and authority is rechecked after taking this potentially busy lock.
        let _authorization = self.existing_authorization.lock();
        let (old, mut admission) = {
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
            if old.surface != SurfaceKind::ExistingTab {
                return Err(BrokerError::TargetUnauthorized);
            }
            if old.target_id == member.target_id && !run.switch_cleanup_pending {
                return Ok(json!({"runId":permit.run_id,"targetId":old.target_id,
                    "targetGeneration":old.target_generation}));
            }
            // Reserve before acquisition, but do not retire a healthy old
            // observation on a foreign/occupied destination failure.
            run.switch_in_flight = true;
            (
                old,
                SwitchAdmission {
                    broker: self,
                    run_id: permit.run_id.clone(),
                    generation: run.generation,
                },
            )
        };
        self.recheck_scope_call(&permit)?;
        let old_tab = old
            .target_id
            .strip_prefix("tab:")
            .ok_or(BrokerError::TargetUnauthorized)?;
        let original = self
            .tabs
            .existing_target_info(old_tab)
            .ok_or(BrokerError::DeadTarget)?;
        if original.run_id != permit.run_id || original.session != permit.session_id {
            return Err(BrokerError::TargetUnauthorized);
        }
        // Declared after admission: rollback completes before its ownership
        // flag is dropped, so Stop cannot claim terminal before this owner.
        let mut borrowed = if old.target_id != member.target_id {
            let (info, acquired) =
                self.tabs
                    .acquire_scope_tab(&permit.session_id, &permit.run_id, identity)?;
            if !acquired {
                // An inactive resource was not acquired by this transaction.
                // Do not adopt or release an independently borrowed resource.
                return Err(BrokerError::TargetUnauthorized);
            }
            Some(NewBorrow {
                host: self.tabs.clone(),
                info,
                committed: false,
            })
        } else {
            None
        };
        self.recheck_scope_call(&permit)?;
        {
            let mut inner = self.inner.lock();
            Self::recheck_scope_call_locked(&inner, &permit)?;
            let run = inner.runs.get_mut(&permit.run_id).unwrap();
            if run.paused || !run.switch_in_flight || run.resume_in_flight {
                return Err(BrokerError::StopRequested);
            }
            let generation = run
                .generation
                .checked_add(1)
                .ok_or(BrokerError::StopRequested)?;
            run.generation = generation;
            admission.generation = generation;
            permit.execution_generation = generation;
            run.switch_cleanup_pending = true;
            run.cancellation.cancel();
            run.snapshot_id = None;
            run.last_image_ok = false;
            run.element_refs.clear();
            run.element_boxes.clear();
            run.geometry_epoch = 0;
        }
        self.changes.publish();
        executor
            .adapter
            .abort(&permit.run_id, admission.generation)
            .map_err(BrokerError::Adapter)?;
        executor.adapter.stop_periodic_preview();
        if !executor.adapter.is_idle(&permit.run_id) {
            return Err(BrokerError::LeaseHeld {
                run_id: permit.run_id.clone(),
            });
        }
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
        let result = {
            let mut inner = self.inner.lock();
            Self::recheck_scope_call_locked(&inner, &permit)?;
            let run = &inner.runs[&permit.run_id];
            if !run.switch_in_flight
                || !run.switch_cleanup_pending
                || run.paused
                || run.resume_in_flight
                || run.in_flight.load(Ordering::SeqCst)
            {
                return Err(BrokerError::LeaseHeld {
                    run_id: permit.run_id.clone(),
                });
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
            json!({"runId":permit.run_id,"targetId":member.target_id,"targetGeneration":generation})
        };
        if let Some(borrowed) = &mut borrowed {
            borrowed.committed = true;
            self.tabs.release_existing_grant(&original);
        }
        Ok(result)
    }
}
