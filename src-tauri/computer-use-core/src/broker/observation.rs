//! Per-run observation admission spans adapter work and snapshot publication.
use super::*;

fn admit_capture(
    run: &mut RunState,
    run_id: &str,
    for_model: bool,
    budget: usize,
    inherited: Option<&InFlightClear>,
) -> Result<Option<InFlightClear>, BrokerError> {
    if run.stop != StopState::Running {
        return Err(BrokerError::StopRequested);
    }
    if run.paused {
        return Err(BrokerError::Schema("run is paused".into()));
    }
    // Only the synchronous Wait action may reuse its own admission. Public
    // observations cannot bypass an outstanding action (including a timeout).
    let owns_admission = inherited.is_some_and(|permit| Arc::ptr_eq(&permit.0, &run.in_flight));
    if (inherited.is_some() && !owns_admission)
        || run.in_flight.load(Ordering::SeqCst) != owns_admission
    {
        return Err(BrokerError::LeaseHeld {
            run_id: run_id.into(),
        });
    }
    if for_model && run.observe_count >= budget {
        return Err(BrokerError::Schema(
            "observe budget exhausted; start a new run".into(),
        ));
    }
    let guard = if owns_admission {
        None
    } else {
        run.in_flight.store(true, Ordering::SeqCst);
        Some(InFlightClear(run.in_flight.clone()))
    };
    // Charge admitted attempts, not just successes; failed adapters cannot
    // create an unbounded retry loop. UI previews use no model budget.
    if for_model {
        run.observe_count = run.observe_count.saturating_add(1);
    }
    Ok(guard)
}

impl ComputerUseBroker {
    pub fn observe(&self, run_id: &str) -> Result<Observation, BrokerError> {
        self.observe_with_screenshot(run_id, true)
    }

    pub fn observe_with_screenshot(
        &self,
        run_id: &str,
        screenshot: bool,
    ) -> Result<Observation, BrokerError> {
        self.capture(run_id, true, screenshot, None)
    }

    pub fn observe_preview(&self, run_id: &str) -> Result<Observation, BrokerError> {
        self.capture(run_id, false, true, None)
    }

    pub(super) fn capture(
        &self,
        run_id: &str,
        for_model: bool,
        screenshot: bool,
        inherited: Option<&InFlightClear>,
    ) -> Result<Observation, BrokerError> {
        let t0 = Instant::now();
        let (target, gen, cancellation, managed_request, _guard) = {
            let mut g = self.inner.lock();
            if !g.feature_enabled {
                return Err(BrokerError::FeatureDisabled);
            }
            let run = g.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
            let target = run.target.clone().ok_or(BrokerError::TargetUnauthorized)?;
            let guard = admit_capture(run, run_id, for_model, self.observe_budget, inherited)?;
            let managed = if target.surface == SurfaceKind::ManagedBrowser {
                Some(
                    self.tabs
                        .admit_managed_request(run_id, run.cancellation.clone())?,
                )
            } else {
                None
            };
            (
                target,
                run.generation,
                run.cancellation.clone(),
                managed,
                guard,
            )
        };
        let executor = self.executor_for_target(&target)?;
        let adapter = &executor.adapter;
        let target_id = target.target_id.clone();
        if !adapter.input_available() {
            let _ = self.pause_due_to(run_id, "session locked");
            return Err(BrokerError::Schema("session locked; run paused".into()));
        }
        if !adapter.target_alive(&target_id) {
            return Err(BrokerError::DeadTarget);
        }
        let mut obs = adapter
            .capture_for_run_at_generation(
                run_id,
                &target_id,
                gen,
                crate::adapter::CaptureOptions {
                    for_model,
                    screenshot,
                    cancellation,
                    managed_request,
                },
            )
            .map_err(BrokerError::Adapter)?;
        let epoch = adapter.current_geometry_revision_for(&target_id);
        if obs.geometry_revision != 0 && obs.geometry_revision != epoch {
            return Err(BrokerError::IdentityMismatch("observation revision"));
        }
        crate::protocol::normalize_observation(&mut obs, epoch);
        crate::protocol::clamp_observation(&mut obs);
        obs.run_id = run_id.to_string();
        obs.target_generation = gen;
        obs.version = PROTOCOL_VERSION;
        let visual = image_is_visual(&obs);
        let elapsed = t0.elapsed().as_millis() as u64;
        let mut g = self.inner.lock();
        if !g.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        {
            let run = g.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
            if run.stop != StopState::Running
                || run.paused
                || run.generation != gen
                || run.target.as_ref().is_none_or(|t| {
                    t.target_id != target_id
                        || t.surface != target.surface
                        || t.executor_generation != executor.generation
                })
            {
                return Err(BrokerError::IdentityMismatch("observation generation"));
            }
            if obs.target_id != target_id {
                return Err(BrokerError::IdentityMismatch("observation target"));
            }
            if !for_model {
                return Ok(obs);
            }
            run.snapshot_id = Some(obs.snapshot_id.clone());
            run.geometry_revision = obs.geometry_revision;
            run.geometry_epoch = epoch;
            run.last_image_ok = visual;
            run.image_size = (obs.image.width, obs.image.height);
            run.element_refs = obs
                .nodes
                .iter()
                .filter(|node| !node.truncated)
                .map(|node| node.node_ref.clone())
                .collect();
            run.element_boxes = obs
                .nodes
                .iter()
                .filter(|node| !node.truncated)
                .filter(|node| {
                    node.x.is_some()
                        && node.y.is_some()
                        && node.width.is_some()
                        && node.height.is_some()
                })
                .cloned()
                .collect();
            run.last_unknown = false;
            run.timings.observe_ms = elapsed;
        }
        push_trace(
            &mut g,
            "observe",
            run_id,
            &format!(
                "surface={} backend={} snap={} image={}",
                target.surface.as_wire(),
                adapter.backend_id(),
                obs.snapshot_id,
                visual
            ),
            TraceAudience::Model,
        );
        Ok(obs)
    }

    /// Legacy browser tool shares the same admission and observation budget.
    /// Host validates tab ownership; this path does not create or change grants.
    pub(crate) fn observe_managed_tab(
        &self,
        run_id: &str,
        tab_id: &str,
        page_generation: u64,
    ) -> Result<crate::browser::ManagedObservation, BrokerError> {
        let (generation, cancellation, managed_request, _guard) = {
            let mut g = self.inner.lock();
            if !g.feature_enabled {
                return Err(BrokerError::FeatureDisabled);
            }
            let run = g.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
            let guard = admit_capture(run, run_id, true, self.observe_budget, None)?;
            let managed = self
                .tabs
                .admit_managed_request(run_id, run.cancellation.clone())?;
            (run.generation, run.cancellation.clone(), managed, guard)
        };
        let obs = self
            .tabs
            .for_managed_request(&managed_request)?
            .capture_managed_tab(
                run_id,
                tab_id,
                page_generation,
                crate::adapter::CaptureOptions {
                    for_model: true,
                    screenshot: true,
                    cancellation,
                    managed_request: Some(managed_request),
                },
            )?;
        let mut g = self.inner.lock();
        if !g.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        let run = g.runs.get_mut(run_id).ok_or(BrokerError::RunNotFound)?;
        if run.stop != StopState::Running || run.paused || run.generation != generation {
            return Err(BrokerError::IdentityMismatch("observation generation"));
        }
        // A legacy observation changed Host refs; require a new generic
        // observation before computer_act can reuse its generic snapshot.
        run.snapshot_id = None;
        run.element_refs.clear();
        run.element_boxes.clear();
        run.last_image_ok = false;
        run.last_unknown = false;
        Ok(obs)
    }
}
