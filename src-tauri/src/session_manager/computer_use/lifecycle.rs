//! Fenced Stop, handshake termination, and process-exit cleanup.

use super::*;

impl SessionManager {
    /// Synchronous authority fence for Stop.  This intentionally performs no
    /// ACP, adapter, browser-worker, or filesystem work.  Errors are retained
    /// in the plan so callers can still acknowledge the local Stop and expose
    /// a retryable cleanup state.
    pub(crate) fn fence_computer_use_stop(&self, session_id: &str) -> FencedComputerUseStop {
        let fenced = sessions::fence_revoke_checked(session_id);
        self.fenced_computer_use_plan(session_id, fenced)
    }

    /// Variant for lifecycle coordinators that already captured the exact
    /// process Broker. Cleanup remains bound to that Broker even if a test or
    /// recovery path replaces the global runtime slot later.
    pub(crate) fn fence_computer_use_stop_with_broker(
        &self,
        broker: Arc<crate::computer_use::ComputerUseBroker>,
        session_id: &str,
    ) -> FencedComputerUseStop {
        let fenced = sessions::fence_revoke_checked_with_broker(broker, session_id);
        self.fenced_computer_use_plan(session_id, fenced)
    }

    /// Fence compensation for one exact failed authorization. A stale failure
    /// cannot stop a newer attempt because the grant layer matches the full
    /// Host ticket before publishing desired-absent.
    pub(crate) fn fence_failed_computer_use_authorization(
        &self,
        broker: Arc<crate::computer_use::ComputerUseBroker>,
        ticket: &sessions::AuthorizationTicket,
    ) -> FencedComputerUseStop {
        let session_id = ticket.session.as_str();
        let fenced = sessions::fence_failed_authorization(broker, ticket);
        self.fenced_computer_use_plan(session_id, fenced)
    }

    pub(crate) fn fence_cancelled_computer_use_authorization(
        &self,
        broker: Arc<crate::computer_use::ComputerUseBroker>,
        session_id: &str,
        attempt_id: &str,
        selector_revision: u64,
    ) -> Result<(bool, Option<FencedComputerUseStop>), String> {
        let fenced = sessions::fence_cancel_attempt_checked(
            broker,
            session_id,
            attempt_id,
            selector_revision,
        )?;
        if !fenced.cancelled {
            return Ok((false, None));
        }
        Ok((
            true,
            Some(self.fenced_computer_use_plan(session_id, fenced.revoke)),
        ))
    }

    fn fenced_computer_use_plan(
        &self,
        session_id: &str,
        fenced: sessions::FencedRevoke,
    ) -> FencedComputerUseStop {
        let sessions::FencedRevoke {
            matched,
            cleanup,
            error: fence_error,
        } = fenced;
        if !matched && cleanup.is_none() && fence_error.is_none() {
            return FencedComputerUseStop {
                desired: sessions::McpDesiredState {
                    generation: 0,
                    run_id: None,
                    attempt_generation: None,
                },
                cleanup: None,
                fence_error: None,
            };
        }
        let desired = sessions::mcp_desired(session_id);
        // Broker fencing can only fail before producing a cleanup ticket (for
        // example, an already-missing run). That is a terminal consistency
        // error, not a retryable resource operation. Never advertise a Retry
        // action that has no generation-bound hand-off to execute.
        let resource_cleanup_pending = cleanup.is_some()
            || sessions::pending_fenced_cleanup(session_id, desired.generation).is_some();
        self.record_fenced_cleanup(session_id, &desired, resource_cleanup_pending);
        FencedComputerUseStop {
            desired,
            cleanup,
            fence_error,
        }
    }

    /// Finish one fenced Stop after local acknowledgement.  The ACP catalog
    /// is reconciled only if the exact absent generation is still current; a
    /// newer authorization is never consumed by an older cleanup.  Surface
    /// cleanup runs behind `spawn_blocking` because managed workers and native
    /// adapters may perform bounded blocking I/O.
    pub(crate) async fn finish_fenced_computer_use_stop(
        &self,
        session_id: &str,
        plan: FencedComputerUseStop,
    ) -> Result<(), String> {
        if plan.is_noop() {
            return Ok(());
        }
        let mut errors = Vec::new();
        if let Some(error) = plan.fence_error.as_ref() {
            errors.push(error.clone());
        }

        let current = sessions::mcp_desired(session_id);
        let detach = if current == plan.desired && plan.desired.run_id.is_none() {
            self.reconcile_session_mcp_scoped(
                session_id,
                McpReconcileDeps::default(),
                McpReconcileIntent::CleanupOnly,
            )
            .await
        } else if current != plan.desired {
            if current.run_id.is_some() {
                Err("Computer Use cleanup was superseded by a newer authorization".into())
            } else {
                // A newer absent generation owns the catalog now.  Do not
                // let this older Stop overwrite its status or send a stale
                // replacement.
                Ok(())
            }
        } else {
            Ok(())
        };
        if let Err(error) = detach {
            errors.push(format!("Computer Use cleanup pending: {error}"));
        }

        if let Some(cleanup) = plan.cleanup {
            let cleanup_lock = self.computer_use_cleanup_lock(session_id);
            let _serial = cleanup_lock.lock().await;
            let result = tauri::async_runtime::spawn_blocking(move || {
                sessions::finish_fenced_cleanup_blocking(cleanup)
            })
            .await
            .map_err(|error| error.to_string())?;
            if let Err(error) = result {
                self.mark_resource_cleanup_result(
                    session_id,
                    plan.desired.generation,
                    Some(&error),
                );
                errors.push(error);
            } else {
                self.mark_resource_cleanup_result(session_id, plan.desired.generation, None);
            }
        } else if plan.fence_error.is_none() {
            self.mark_resource_cleanup_result(session_id, plan.desired.generation, None);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    pub(super) async fn finish_fenced_computer_use_stop_task(
        &self,
        session_id: &str,
        plan: FencedComputerUseStop,
        acp_termination: Option<FencedAcpTermination>,
    ) -> Result<(), String> {
        if let Some(termination) = acp_termination {
            debug_assert_eq!(termination.app_session_id, session_id);
            if self.has_other_process_tenant(&termination.process_id, &termination.app_session_id) {
                tracing::info!(
                    session = %termination.app_session_id,
                    process = %termination.process_id,
                    "handshake Stop detached a shared ACP tenant"
                );
            } else {
                // Termination is independent from catalog/surface cleanup. In
                // particular, a no-op plan or a failed cleanup must not leave
                // the cancelled handshake process alive.
                Self::kill_acp_bounded(&termination.acp).await;
                tracing::info!(
                    session = %termination.app_session_id,
                    process = %termination.process_id,
                    "handshake Stop terminated the captured ACP"
                );
            }
        }

        self.finish_fenced_computer_use_stop(session_id, plan).await
    }

    /// Schedule the slow half of a fenced Stop without holding up the Host
    /// command or session event loop. A handshake termination carries its
    /// exact detached ACP incarnation; delayed work never looks up the
    /// session's current endpoint.
    pub(crate) fn spawn_fenced_computer_use_stop(
        self: &Arc<Self>,
        session_id: String,
        plan: FencedComputerUseStop,
        acp_termination: Option<FencedAcpTermination>,
    ) {
        if plan.is_noop() && acp_termination.is_none() {
            return;
        }
        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let result = manager
                .finish_fenced_computer_use_stop_task(&session_id, plan, acp_termination)
                .await;
            if let Err(error) = &result {
                tracing::warn!(
                    session = %session_id,
                    "Computer Use cleanup after local Stop is pending: {error}"
                );
            }
        });
    }

    /// Fence every Computer Use tenant owned by one exact ACP incarnation.
    /// This is deliberately synchronous and must run before the session maps
    /// discard the dead endpoint. The returned plans contain only the
    /// generation-bound surface cleanup hand-offs; catalog transport is known
    /// to be gone and is therefore marked settled without an ACP request.
    pub(crate) fn fence_computer_use_for_process_exit(
        &self,
        process_id: &str,
    ) -> Vec<(String, FencedComputerUseStop)> {
        self.fence_computer_use_for_process_exit_with(
            process_id,
            crate::computer_use::global_broker(),
        )
    }

    pub(super) fn fence_computer_use_for_process_exit_with(
        &self,
        process_id: &str,
        broker: Option<Arc<crate::computer_use::ComputerUseBroker>>,
    ) -> Vec<(String, FencedComputerUseStop)> {
        let mut fenced_sessions = BTreeSet::new();
        let mut plans = Vec::new();
        {
            let live = self.inner.lock();
            if let Some(session) = live
                .as_ref()
                .filter(|session| session.process_id == process_id)
            {
                self.fence_process_exit_session(
                    &session.app_session_id,
                    broker.as_ref(),
                    &mut fenced_sessions,
                    &mut plans,
                );
            }
        }
        {
            let background = self.background.lock();
            for session in background
                .values()
                .filter(|session| session.process_id == process_id)
            {
                self.fence_process_exit_session(
                    &session.app_session_id,
                    broker.as_ref(),
                    &mut fenced_sessions,
                    &mut plans,
                );
            }
        }
        {
            let parked = self.parked.lock();
            for session in parked
                .values()
                .filter(|session| session.process_id == process_id)
            {
                self.fence_process_exit_session(
                    &session.app_session_id,
                    broker.as_ref(),
                    &mut fenced_sessions,
                    &mut plans,
                );
            }
        }
        plans
    }

    fn fence_process_exit_session(
        &self,
        session_id: &str,
        broker: Option<&Arc<crate::computer_use::ComputerUseBroker>>,
        fenced_sessions: &mut BTreeSet<String>,
        plans: &mut Vec<(String, FencedComputerUseStop)>,
    ) {
        if !fenced_sessions.insert(session_id.to_string()) {
            return;
        }
        let desired = sessions::mcp_desired(session_id);
        if desired.run_id.is_none()
            && desired.generation != 0
            && self.mcp_catalog_status(session_id).is_some_and(|status| {
                !status.pending
                    && !status.desired_present
                    && status.applied_generation == Some(desired.generation)
            })
        {
            // A duplicate ProcessExited notification must not replay a
            // retained surface cleanup hand-off.
            return;
        }
        let plan = match broker {
            Some(broker) => {
                self.fence_computer_use_stop_with_broker(Arc::clone(broker), session_id)
            }
            None => self.fence_computer_use_stop(session_id),
        };
        if plan.is_noop() {
            return;
        }
        self.mark_mcp_transport_gone(session_id, &plan);
        plans.push((session_id.to_string(), plan));
    }

    /// A dead ACP cannot receive a catalog replacement. Keep the desired
    /// absent generation authoritative so a future reconnect starts from the
    /// base catalog, while retaining any adapter/browser cleanup bit.
    fn mark_mcp_transport_gone(&self, session_id: &str, plan: &FencedComputerUseStop) {
        if plan.desired.generation == 0 {
            return;
        }
        let previous = self.mcp_catalog_status(session_id).unwrap_or_default();
        self.set_mcp_status(
            session_id,
            McpCatalogStatus {
                desired_generation: plan.desired.generation,
                applied_generation: Some(plan.desired.generation),
                desired_present: false,
                pending: false,
                resource_cleanup_pending: previous.resource_cleanup_pending
                    || plan.cleanup.is_some(),
                last_error: plan.fence_error.as_deref().map(safe_error),
            },
        );
    }

    pub(super) async fn finish_process_exit_cleanup(
        &self,
        session_id: &str,
        plan: FencedComputerUseStop,
    ) -> Result<(), String> {
        let mut errors = plan.fence_error.into_iter().collect::<Vec<_>>();
        if let Some(cleanup) = plan.cleanup {
            let cleanup_lock = self.computer_use_cleanup_lock(session_id);
            let _serial = cleanup_lock.lock().await;
            let result = tauri::async_runtime::spawn_blocking(move || {
                sessions::finish_fenced_cleanup_blocking(cleanup)
            })
            .await
            .map_err(|error| error.to_string())?;
            if let Err(error) = result {
                self.mark_resource_cleanup_result(
                    session_id,
                    plan.desired.generation,
                    Some(&error),
                );
                errors.push(error);
            } else {
                self.mark_resource_cleanup_result(session_id, plan.desired.generation, None);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Finish only generation-bound adapter/browser cleanup after an ACP exit.
    /// No task in this path may attempt to address the dead ACP endpoint.
    pub(crate) fn spawn_process_exit_computer_use_cleanup(
        self: &Arc<Self>,
        cleanups: Vec<(String, FencedComputerUseStop)>,
    ) {
        for (session_id, plan) in cleanups {
            let manager = Arc::clone(self);
            tauri::async_runtime::spawn(async move {
                if let Err(error) = manager.finish_process_exit_cleanup(&session_id, plan).await {
                    tracing::warn!(
                        session = %session_id,
                        "Computer Use cleanup after ACP exit is pending: {error}"
                    );
                }
            });
        }
    }
}
