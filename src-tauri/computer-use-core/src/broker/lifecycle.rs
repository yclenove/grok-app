use super::*;

impl ComputerUseBroker {
    pub fn request_stop(&self, run_id: &str) -> Result<StopState, BrokerError> {
        let Some(ticket) = self.fence_stop(run_id)? else {
            return Ok(StopState::Stopped);
        };
        self.finish_stop_cleanup(&ticket)
    }

    /// Fence authority synchronously without touching an adapter, browser
    /// worker, filesystem, or network.  Callers can therefore publish their
    /// local Stop acknowledgement before starting surface cleanup.
    pub fn fence_stop(&self, run_id: &str) -> Result<Option<StopCleanupTicket>, BrokerError> {
        let generation = {
            let mut inner = self.inner.lock();
            let run = inner.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
            if run.stop == StopState::Stopped {
                return Ok(None);
            }
            if run.stop == StopState::Running {
                run.stop = StopState::StopRequested;
                run.stop_cleanup_pending = true;
                run.stop_cleanup_in_flight = false;
                run.generation = run.generation.saturating_add(1);
                run.cancellation.cancel();
                run.snapshot_id = None;
                run.last_image_ok = false;
            }
            let generation = run.generation;
            push_trace(
                &mut inner,
                "stop_requested",
                run_id,
                "fenced",
                TraceAudience::Model,
            );
            generation
        };
        Ok(Some(StopCleanupTicket {
            run_id: run_id.to_string(),
            generation,
        }))
    }

    /// Complete the cleanup hand-off created by [`Self::fence_stop`].  Only
    /// one caller may own a given pending cleanup at a time.  A failed cleanup
    /// leaves `stop_cleanup_pending` set and clears the in-flight claim so a
    /// later explicit retry can safely try again.
    pub fn finish_stop_cleanup(
        &self,
        ticket: &StopCleanupTicket,
    ) -> Result<StopState, BrokerError> {
        let (target, cleanup_required) = {
            let mut inner = self.inner.lock();
            let run = inner
                .runs
                .get_mut(&ticket.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            if run.generation != ticket.generation || run.stop == StopState::Running {
                return Err(BrokerError::IdentityMismatch("stop generation"));
            }
            if run.stop == StopState::Stopped || !run.stop_cleanup_pending {
                (None, false)
            } else if run.stop_cleanup_in_flight {
                return Ok(StopState::StopRequested);
            } else {
                run.stop_cleanup_in_flight = true;
                (run.target.clone(), true)
            }
        };
        if !cleanup_required {
            return self.stop_state(&ticket.run_id);
        }

        let executor = target
            .as_ref()
            .map(|target| self.executor_for_target(target))
            .transpose();
        let adapter_cleanup = match executor.as_ref() {
            Ok(Some(executor)) => {
                // Terminal cleanup below owns the managed cancel exchange,
                // including a resume whose remote outcome is unknown.
                let result = if target
                    .as_ref()
                    .is_some_and(|t| t.surface == SurfaceKind::ManagedBrowser)
                {
                    Ok(())
                } else {
                    executor.adapter.abort(&ticket.run_id, ticket.generation)
                };
                executor.adapter.stop_periodic_preview();
                result.map_err(|error| format!("surface cancellation did not complete: {error}"))
            }
            Ok(None) => Ok(()),
            Err(error) => Err(format!("surface executor unavailable during stop: {error}")),
        };
        let browser_cleanup = self.tabs.cancel_run(&ticket.run_id).map(|_| ());
        let mut cleanup_errors = Vec::new();
        if let Err(error) = adapter_cleanup {
            cleanup_errors.push(error);
        }
        if let Err(error) = browser_cleanup {
            cleanup_errors.push(format!("browser cancellation did not complete: {error}"));
        }
        if !cleanup_errors.is_empty() {
            if let Some(run) = self.inner.lock().runs.get_mut(&ticket.run_id) {
                if run.generation == ticket.generation {
                    run.stop_cleanup_in_flight = false;
                }
            }
            return Err(BrokerError::Adapter(cleanup_errors.join("; ")));
        }
        if let Some(target) = target.as_ref() {
            if let Ok(executor) = self.executor_for_target(target) {
                executor
                    .adapter
                    .release_target_for_run(&ticket.run_id, &target.target_id);
            }
        }
        if let Some(run) = self.inner.lock().runs.get_mut(&ticket.run_id) {
            if run.generation == ticket.generation && run.stop != StopState::Running {
                run.stop_cleanup_pending = false;
                run.stop_cleanup_in_flight = false;
                run.pause_cleanup_pending = false;
            }
        }
        self.stop_state(&ticket.run_id)
    }

    pub fn stop_cleanup_pending(&self, run_id: &str) -> Result<bool, BrokerError> {
        let inner = self.inner.lock();
        Ok(inner
            .runs
            .get(run_id)
            .ok_or(BrokerError::RunNotFound)?
            .stop_cleanup_pending)
    }

    pub fn wait_stopped(&self, run_id: &str, timeout: Duration) -> Result<StopState, BrokerError> {
        let start = Instant::now();
        loop {
            let state = self.stop_state(run_id)?;
            if state == StopState::Running {
                return Err(BrokerError::Schema("stop not requested".into()));
            }
            if state == StopState::Stopped || start.elapsed() >= timeout {
                return Ok(state);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn stop_state(&self, run_id: &str) -> Result<StopState, BrokerError> {
        let (target, cleanup_pending) = {
            let inner = self.inner.lock();
            let run = inner.runs.get(run_id).ok_or(BrokerError::RunNotFound)?;
            (run.target.clone(), run.stop_cleanup_pending)
        };
        let adapter_idle = if cleanup_pending {
            false
        } else {
            match target.as_ref() {
                Some(target) => self.executor_for_target(target)?.adapter.is_idle(run_id),
                None => true,
            }
        };
        let mut inner = self.inner.lock();
        let run = inner.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
        if !run.stop_cleanup_pending && !run.in_flight.load(Ordering::SeqCst) && adapter_idle {
            if run.stop == StopState::StopRequested {
                run.stop = StopState::Stopped;
                if target.as_ref().map(|t| t.surface) == Some(SurfaceKind::Desktop) {
                    let _ = self.lease.release(run_id);
                }
                push_trace(&mut inner, "stopped", run_id, "", TraceAudience::Model);
                return Ok(StopState::Stopped);
            }
            if run.paused && target.as_ref().map(|t| t.surface) == Some(SurfaceKind::Desktop) {
                let _ = self.lease.release(run_id);
            }
        }
        Ok(run.stop)
    }

    pub fn is_paused(&self, run_id: &str) -> Result<bool, BrokerError> {
        let inner = self.inner.lock();
        Ok(inner
            .runs
            .get(run_id)
            .ok_or(BrokerError::RunNotFound)?
            .paused)
    }

    pub fn pause(&self, run_id: &str) -> Result<(), BrokerError> {
        self.pause_due_to(run_id, "user")
    }

    /// Pause without treating the source as a user resume grant.
    pub fn pause_due_to(&self, run_id: &str, reason: &str) -> Result<(), BrokerError> {
        let (generation, target, cleanup_required) = {
            let mut inner = self.inner.lock();
            let run = inner.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
            if run.stop != StopState::Running {
                return Err(BrokerError::StopRequested);
            }
            if run.paused && !run.resume_in_flight {
                let cleanup_required = run.pause_cleanup_pending;
                let generation = run.generation;
                let target = run.target.clone();
                push_trace(&mut inner, "pause", run_id, reason, TraceAudience::Model);
                (generation, target, cleanup_required)
            } else {
                run.paused = true;
                run.pause_cleanup_pending = true;
                run.cancellation.cancel();
                run.snapshot_id = None;
                run.last_image_ok = false;
                run.generation = run.generation.saturating_add(1);
                if let Some(target) = &mut run.target {
                    target.target_generation = run.generation;
                }
                let generation = run.generation;
                let target = run.target.clone();
                push_trace(&mut inner, "pause", run_id, reason, TraceAudience::Model);
                (generation, target, true)
            }
        };
        if !cleanup_required {
            return Ok(());
        }
        let executor = target
            .as_ref()
            .map(|target| self.executor_for_target(target))
            .transpose();
        let adapter_cleanup = match executor.as_ref() {
            Ok(Some(executor)) => {
                let result = executor.adapter.abort(run_id, generation);
                executor.adapter.stop_periodic_preview();
                result.map_err(|error| format!("surface pause did not complete: {error}"))
            }
            Ok(None) => Ok(()),
            Err(error) => Err(format!(
                "surface executor unavailable during pause: {error}"
            )),
        };
        let browser_cleanup = if target
            .as_ref()
            .is_some_and(|t| t.surface == SurfaceKind::ManagedBrowser)
        {
            self.tabs.cancel_actions(run_id).map(|_| ())
        } else {
            self.tabs
                .pause_managed_run(run_id)
                .and_then(|()| self.tabs.cancel_actions(run_id).map(|_| ()))
        };
        let mut cleanup_errors = Vec::new();
        if let Err(error) = adapter_cleanup {
            cleanup_errors.push(error);
        }
        if let Err(error) = browser_cleanup {
            cleanup_errors.push(format!("browser pause did not complete: {error}"));
        }
        if !cleanup_errors.is_empty() {
            return Err(BrokerError::Adapter(cleanup_errors.join("; ")));
        }
        if let Some(run) = self.inner.lock().runs.get_mut(run_id) {
            if run.generation == generation && run.paused {
                run.pause_cleanup_pending = false;
            }
        }
        self.stop_state(run_id)?;
        Ok(())
    }

    /// Compact / model switch: drop snapshot, geometry, lease and authorization.
    pub fn invalidate_for_context_change(&self, run_id: &str) -> Result<(), BrokerError> {
        self.reconnect(run_id)
    }

    /// Reconnecting invalidates authorization; it never resumes a paused task.
    pub fn reconnect(&self, run_id: &str) -> Result<(), BrokerError> {
        self.pause(run_id)?;
        let mut inner = self.inner.lock();
        let run = inner.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
        drop_execution_identity(run);
        run.recovery = Some(RecoverySource::System);
        push_trace(
            &mut inner,
            "reconnect",
            run_id,
            "system",
            TraceAudience::Model,
        );
        Ok(())
    }

    pub fn takeover(&self, run_id: &str) -> Result<(), BrokerError> {
        self.pause(run_id)
    }

    pub fn fork_run(&self, source_run: &str, new_run: &str) -> Result<(), BrokerError> {
        {
            let inner = self.inner.lock();
            if inner.runs.contains_key(new_run) || new_run.trim().is_empty() {
                return Err(BrokerError::IdentityMismatch("newRunId"));
            }
        }
        self.pause(source_run)?;
        let mut inner = self.inner.lock();
        if inner.runs.contains_key(new_run) {
            return Err(BrokerError::IdentityMismatch("newRunId"));
        }
        let session = inner
            .runs
            .get(source_run)
            .ok_or(BrokerError::RunNotFound)?
            .app_session_id
            .clone();
        inner.runs.insert(
            new_run.to_string(),
            fresh_run(session, Some(RecoverySource::System)),
        );
        push_trace(
            &mut inner,
            "fork",
            new_run,
            source_run,
            TraceAudience::Model,
        );
        Ok(())
    }

    pub fn set_preview_visible(&self, run_id: &str, visible: bool) -> Result<(), BrokerError> {
        let target = {
            let inner = self.inner.lock();
            let run = inner.runs.get(run_id).ok_or(BrokerError::RunNotFound)?;
            if visible && (run.paused || run.stop != StopState::Running) {
                return Err(BrokerError::StopRequested);
            }
            run.target.clone()
        };
        self.preview.set_visible(visible);
        if visible {
            if let Some(target) = target {
                self.executor_for_target(&target)?
                    .adapter
                    .start_periodic_preview(&target.target_id);
            }
        } else if let Some(target) = target {
            self.executor_for_target(&target)?
                .adapter
                .stop_periodic_preview();
        }
        let mut inner = self.inner.lock();
        push_trace(
            &mut inner,
            if visible {
                "preview_shown"
            } else {
                "preview_hidden"
            },
            run_id,
            "",
            TraceAudience::Ui,
        );
        Ok(())
    }
}
