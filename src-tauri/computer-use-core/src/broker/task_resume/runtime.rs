//! Native work stays outside the Broker authority lock.
use super::*;

impl HostTaskResume {
    /// Wait for original cleanup without abandoning the one user gesture.
    /// Timeout is an honest paused result; no input or ticket is published.
    pub fn complete(mut self, timeout: Duration) -> Result<HostTaskResumeReceipt, BrokerError> {
        self.prepare_native(timeout)?;
        self.commit()
    }

    /// The owned worker retains this reservation through native preparation.
    /// The grant remains suspended until the Host pins its original endpoint
    /// and calls commit. No ACP or SessionManager lock may span this method.
    pub fn prepare_native(&mut self, timeout: Duration) -> Result<(), BrokerError> {
        if self.admission.is_some() || self.completed {
            return Err(BrokerError::IdentityMismatch("resume already prepared"));
        }
        let start = Instant::now();
        self.await_original_idle(start, timeout)?;
        if self.target.surface == SurfaceKind::ManagedBrowser {
            // Native metadata/capture require the original worker to run. Its
            // revision advances once while the task grant remains suspended.
            let ticket = {
                let inner = self.broker.inner.lock();
                self.recheck_locked(&inner)?;
                self.broker
                    .tabs
                    .prepare_managed_resume(&self.address.run_id)?
                    .ok_or(BrokerError::IdentityMismatch("managed resume owner"))?
            };
            self.broker.tabs.resume_managed_request(&ticket)?;
            self.recheck()?;
        }
        self.revalidate_members()?;
        if self.target.surface == SurfaceKind::Desktop {
            self.broker
                .lease
                .try_acquire(&self.address.run_id, &self.broker.instance_id)
                .map_err(lease_error)?;
        }
        self.recheck()?;
        self.executor
            .adapter
            .claim_target_for_run(&self.address.run_id, &self.target.target_id)
            .map_err(BrokerError::Adapter)?;
        self.revalidate_members()?;
        let capture_start = Instant::now();
        let (cancellation, managed_request) = {
            let mut inner = self.broker.inner.lock();
            self.recheck_locked(&inner)?;
            let run = inner.runs.get_mut(&self.address.run_id).unwrap();
            if run.observe_count >= self.broker.observe_budget {
                return Err(BrokerError::Schema(
                    "observe budget exhausted; start a new run".into(),
                ));
            }
            run.observe_count = run.observe_count.saturating_add(1);
            let cancellation = run.cancellation.clone();
            let request = if self.target.surface == SurfaceKind::ManagedBrowser {
                Some(
                    self.broker
                        .tabs
                        .admit_managed_request(&self.address.run_id, cancellation.clone())?,
                )
            } else {
                None
            };
            (cancellation, request)
        };
        // Private recovery capture. The suspended grant stays non-executable,
        // and generic model/UI capture cannot borrow this reservation.
        self.recheck()?;
        let mut obs = self
            .executor
            .adapter
            .capture_for_run_at_generation(
                &self.address.run_id,
                &self.target.target_id,
                self.generation,
                crate::adapter::CaptureOptions {
                    for_model: true,
                    // ExistingTab has fresh typed DOM refs without an
                    // activeTab screenshot grant. Recovery requires a fresh
                    // observation, not extra browser permission. Explicit
                    // screenshots still fail closed; recovery does not enable
                    // coordinate input. Desktop recovery remains visual.
                    screenshot: !matches!(
                        self.target.surface,
                        SurfaceKind::ExistingTab | SurfaceKind::WebView
                    ),
                    cancellation,
                    managed_request,
                },
            )
            .map_err(BrokerError::Adapter)?;
        self.recheck()?;
        let epoch = self
            .executor
            .adapter
            .current_geometry_revision_for(&self.target.target_id);
        if obs.target_id != self.target.target_id
            || obs.snapshot_id.is_empty()
            || (obs.geometry_revision != 0 && obs.geometry_revision != epoch)
        {
            return Err(BrokerError::IdentityMismatch("recovery observation"));
        }
        crate::protocol::normalize_observation(&mut obs, epoch);
        crate::protocol::clamp_observation(&mut obs);
        obs.run_id = self.address.run_id.clone();
        obs.target_generation = self.generation;
        obs.version = PROTOCOL_VERSION;
        if !image_is_visual(&obs) && obs.nodes.is_empty() && obs.text.trim().is_empty() {
            return Err(BrokerError::Adapter("recovery observation is empty".into()));
        }
        self.revalidate_members()?;
        // A geometry change during the last scope/permission callback also
        // retires the captured coordinates; never publish that stale frame.
        if self
            .executor
            .adapter
            .current_geometry_revision_for(&self.target.target_id)
            != epoch
        {
            return Err(BrokerError::IdentityMismatch(
                "recovery observation geometry",
            ));
        }
        self.prepared = Some(RecoveryObservation {
            observation: obs,
            geometry_revision: epoch,
            capture_ms: capture_start.elapsed().as_millis() as u64,
        });
        Ok(())
    }

    /// Authority-only publication: no adapter query, abort or cleanup runs in
    /// this method. Borrowing keeps error/Drop cleanup outside the Host's locks.
    pub fn commit(&mut self) -> Result<HostTaskResumeReceipt, BrokerError> {
        let _change = self.broker.changes.notice();
        if self.completed {
            return Err(BrokerError::IdentityMismatch("resume already committed"));
        }
        let prepared = self
            .prepared
            .as_ref()
            .ok_or(BrokerError::IdentityMismatch("resume not prepared"))?;
        let mut inner = self.broker.inner.lock();
        self.recheck_locked(&inner)?;
        let enabled = inner.feature_enabled;
        let epoch_feature = inner.feature_epoch;
        let broker = self.broker.clone();
        let mut publish = || {
            // These publications are indivisible to all model and Host readers.
            inner
                .consent
                .publish_recovery(&self.context, &self.binding, enabled, epoch_feature)?;
            let run = inner.runs.get_mut(&self.address.run_id).unwrap();
            super::super::observation::store_model_observation(
                run,
                &prepared.observation,
                prepared.geometry_revision,
                prepared.capture_ms,
            );
            run.paused = false;
            run.unknown_idle_confirmed = false;
            run.recovery = Some(RecoverySource::User);
            if let Some(target) = &mut run.target {
                target.closed = false;
            }
            self.completed = true;
            push_trace(
                &mut inner,
                "task_resume",
                &self.address.run_id,
                "user fresh observation",
                TraceAudience::Model,
            );
            Ok(HostTaskResumeReceipt {
                address: HostExecutionAddress {
                    key: self.address.key.clone(),
                    run_id: self.address.run_id.clone(),
                    generation: self.generation.to_string(),
                },
            })
        };
        if self.target.surface == SurfaceKind::WebView {
            broker.with_scope_members_authority(
                &self.context.app_session_id,
                &self.address.run_id,
                &self.members,
                publish,
            )
        } else {
            publish()
        }
    }

    fn await_original_idle(
        &mut self,
        start: Instant,
        timeout: Duration,
    ) -> Result<(), BrokerError> {
        loop {
            self.recheck()?;
            let ready = {
                let inner = self.broker.inner.lock();
                let run = &inner.runs[&self.address.run_id];
                !run.pause_cleanup_pending
                    && !run.switch_in_flight
                    && !run.in_flight.load(Ordering::SeqCst)
            };
            if ready
                && self.executor.adapter.is_idle(&self.address.run_id)
                && self.broker.tabs.managed_run_idle(&self.address.run_id)
            {
                let mut inner = self.broker.inner.lock();
                self.recheck_locked(&inner)?;
                let run = inner.runs.get_mut(&self.address.run_id).unwrap();
                if run.pause_cleanup_pending
                    || run.switch_in_flight
                    || run.in_flight.load(Ordering::SeqCst)
                {
                    continue;
                }
                run.in_flight.store(true, Ordering::SeqCst);
                self.admission = Some(InFlightClear::new(
                    run.in_flight.clone(),
                    &self.broker.changes,
                ));
                run.generation = run
                    .generation
                    .checked_add(1)
                    .ok_or(BrokerError::IdentityMismatch("generation exhausted"))?;
                self.generation = run.generation;
                if self.target.surface == SurfaceKind::ExistingTab {
                    // Only this original recovery owner can retire a failed
                    // switch fence, after native and model work are idle.
                    // The group remains suspended until fresh publication.
                    run.switch_cleanup_pending = false;
                }
                run.target.as_mut().unwrap().target_generation = run.generation;
                run.cancellation = crate::execution::ActionCancellation::default();
                run.snapshot_id = None;
                run.element_refs.clear();
                run.element_boxes.clear();
                run.last_image_ok = false;
                return Ok(());
            }
            if start.elapsed() >= timeout {
                return Err(BrokerError::LeaseHeld {
                    run_id: self.address.run_id.clone(),
                });
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn revalidate_members(&self) -> Result<(), BrokerError> {
        for member in &self.members {
            self.recheck()?;
            self.broker
                .recheck_scope_members(std::slice::from_ref(member))?;
            self.recheck()?;
        }
        if !self.executor.adapter.input_available() {
            return Err(BrokerError::Schema("session locked; run paused".into()));
        }
        self.recheck()?;
        if !self.executor.adapter.target_alive(&self.target.target_id) {
            return Err(BrokerError::DeadTarget);
        }
        // Liveness/permission callbacks may retire a previously queried native
        // instance. Confirm the exact witnesses again after those callbacks.
        for member in &self.members {
            self.recheck()?;
            self.broker
                .recheck_scope_members(std::slice::from_ref(member))?;
            self.recheck()?;
        }
        Ok(())
    }
}

fn lease_error(error: crate::error::LeaseError) -> BrokerError {
    match error {
        crate::error::LeaseError::Held { run_id, .. } => BrokerError::LeaseHeld { run_id },
        crate::error::LeaseError::Io(message) => BrokerError::Adapter(message),
    }
}
