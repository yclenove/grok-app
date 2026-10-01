//! User resume is admitted once; surface work never holds Broker state.
use super::*;

struct ResumeAdmission<'a> {
    broker: &'a ComputerUseBroker,
    run_id: &'a str,
    // Held through surface queries and final publication, including failures.
    _in_flight: InFlightClear,
}

impl Drop for ResumeAdmission<'_> {
    fn drop(&mut self) {
        if let Some(run) = self.broker.inner.lock().runs.get_mut(self.run_id) {
            run.resume_in_flight = false;
        }
    }
}

impl ComputerUseBroker {
    /// Host/user-only. IPC never exposes this control as an agent tool.
    pub fn resume(&self, run_id: &str) -> Result<(), BrokerError> {
        let (target, generation, managed_resume, _admission) = {
            let mut inner = self.inner.lock();
            if !inner.feature_enabled {
                return Err(BrokerError::FeatureDisabled);
            }
            let run = inner.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
            if run.stop != StopState::Running {
                return Err(BrokerError::StopRequested);
            }
            if run.pause_cleanup_pending {
                return Err(BrokerError::Schema("pause cleanup still pending".into()));
            }
            if run.in_flight.load(Ordering::SeqCst) {
                return Err(BrokerError::LeaseHeld {
                    run_id: run_id.into(),
                });
            }
            if !run.paused {
                return Err(BrokerError::Schema("run is not paused".into()));
            }
            let managed_resume = self.tabs.prepare_managed_resume(run_id)?;
            run.in_flight.store(true, Ordering::SeqCst);
            run.resume_in_flight = true;
            let admission = ResumeAdmission {
                broker: self,
                run_id,
                _in_flight: InFlightClear(run.in_flight.clone()),
            };
            (
                run.target.clone(),
                run.generation,
                managed_resume,
                admission,
            )
        };

        let executor = target
            .as_ref()
            .map(|target| self.executor_for_target(target))
            .transpose()?;
        if let Some(executor) = executor.as_ref() {
            if !executor.adapter.is_idle(run_id) {
                return Err(BrokerError::LeaseHeld {
                    run_id: run_id.into(),
                });
            }
            if let Some(target) = &target {
                if !executor.adapter.target_alive(&target.target_id) {
                    return Err(BrokerError::DeadTarget);
                }
            }
        }

        if !self.tabs.managed_run_idle(run_id) {
            return Err(BrokerError::LeaseHeld {
                run_id: run_id.into(),
            });
        }
        if let Some(ticket) = managed_resume.as_ref() {
            self.tabs.resume_managed_request(ticket)?;
        }

        let mut inner = self.inner.lock();
        if !inner.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        let run = inner.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
        if run.stop != StopState::Running {
            return Err(BrokerError::StopRequested);
        }
        // Pause/Stop/reconnect can revoke while a surface query is blocked.
        // Admission excludes reauthorization and a second resume; a later
        // explicit Pause bumps generation even though the run was already paused.
        if run.generation != generation || !run.paused || run.pause_cleanup_pending {
            return Err(BrokerError::IdentityMismatch("resume generation"));
        }
        run.paused = false;
        drop_execution_identity(run);
        run.cancellation = crate::execution::ActionCancellation::default();
        run.generation = run.generation.saturating_add(1);
        run.recovery = Some(RecoverySource::User);
        if target.as_ref().map(|target| target.surface) == Some(SurfaceKind::Desktop) {
            let _ = self.lease.release(run_id);
        }
        push_trace(&mut inner, "resume", run_id, "user", TraceAudience::Model);
        Ok(())
    }
}
