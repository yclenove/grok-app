//! Host-only decision delivery. Scheduling never establishes CU authority.
use super::*;
use crate::acp_client::computer_use::{DecisionDelivery, DeliveryAttempt};
use futures_util::future::BoxFuture;
use serde_json::json;
use std::sync::atomic::AtomicBool;
use tokio::sync::Notify;

type DispatcherTask = Arc<tokio::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>;

#[derive(Default)]
pub(in crate::session_manager) struct DispatchWorker {
    stopping: AtomicBool,
    join_failed: AtomicBool,
    wake: Notify,
    task: Mutex<Option<DispatcherTask>>,
    #[cfg(test)]
    join_entered: Notify,
}

impl DispatchWorker {
    async fn stopped(&self) {
        // Subscribe before checking the flag so shutdown between the check and
        // the first poll cannot lose its broadcast and strand an RPC waiter.
        let notified = self.wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !self.stopping.load(Ordering::SeqCst) {
            notified.await;
        }
    }
}

fn ready_for_delivery(session: &LiveSession, endpoint: &McpEndpoint) -> bool {
    session.process_id == endpoint.process_id
        && session.meta.agent_session_id.as_deref() == Some(endpoint.agent_session_id.as_str())
        && session
            .acp
            .as_ref()
            .is_some_and(|acp| Arc::ptr_eq(acp, &endpoint.acp))
        && session.fsm.state() == SessionState::Ready
        && !session.prompt_in_flight
        && !endpoint
            .acp
            .prompt_rpc_in_flight(&endpoint.agent_session_id)
        && session.pending_permission_rpc_id.is_none()
        && session.pending_plan_rpc_id.is_none()
        && session.pending_ask_user_rpc_id.is_none()
        && session.open_tool_ids.is_empty()
        && session.deferred_prompt_complete.is_none()
        && !session.needs_history_bootstrap
}

fn open_host_turn(session: &mut LiveSession) -> Option<String> {
    session.fsm.begin_stream().ok()?;
    let turn = uuid::Uuid::new_v4().to_string();
    session.active_turn_id = Some(turn.clone());
    session.prompt_in_flight = true;
    session.sent_prompt_this_visit = true;
    session.streaming_message_id = Some(uuid::Uuid::new_v4().to_string());
    session.stream_message_id_locked = false;
    session.stream_buf.clear();
    session.stream_thought.clear();
    session.stream_attachments.clear();
    session.stream_last_was_assistant = false;
    session.journal_throttle.reset();
    session.terminal_tool_ids.clear();
    session.open_tool_seen_at.clear();
    session.last_stall_emit = None;
    session.stall_soft_emits = 0;
    session.saw_model_output = false;
    session.tools_this_turn = 0;
    session.provider_retry_attempt = 0;
    session.provider_retry_aborted = false;
    SessionManager::touch_stream_progress_locked(session);
    crate::turn_lease::begin_active(
        &session.app_session_id,
        session.meta.agent_session_id.as_deref(),
        Some(&turn),
    );
    Some(turn)
}

impl SessionManager {
    pub(crate) fn start_computer_use_dispatcher(self: &Arc<Self>, app: tauri::AppHandle) {
        let mut task = self.computer_use_dispatcher.task.lock();
        if task.is_some() || self.computer_use_dispatcher.stopping.load(Ordering::SeqCst) {
            return;
        }
        let weak = Arc::downgrade(self);
        *task = Some(Arc::new(tokio::sync::Mutex::new(Some(
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_millis(250));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut jobs: FuturesUnordered<BoxFuture<'static, ()>> = FuturesUnordered::new();
                loop {
                    let Some(manager) = weak.upgrade() else { break };
                    if manager
                        .computer_use_dispatcher
                        .stopping
                        .load(Ordering::SeqCst)
                    {
                        break;
                    }
                    tokio::select! {
                        _ = manager.computer_use_dispatcher.wake.notified() => {},
                        _ = jobs.next(), if !jobs.is_empty() => {},
                        _ = ticker.tick() => {
                            // One scheduler and a bounded set of actual RPC jobs.
                            // A slow chat cannot block decisions for another chat.
                            if jobs.len() < 32 {
                                if let Ok(_connection) = manager.connect_lock.try_lock() {
                                    for session in manager.mcp_session_ids() {
                                        if jobs.len() >= 32 { break; }
                                        if let Some((endpoint, delivery, turn)) = manager.reserve_computer_use_delivery(&session) {
                                            let manager = Arc::clone(&manager);
                                            let app = app.clone();
                                            jobs.push(Box::pin(async move {
                                                manager.deliver_computer_use_decision(app, endpoint, delivery, turn).await;
                                            }));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Each job observes the same stopping flag. Join its original
                // future after cancellation; do not abort and claim it joined.
                while jobs.next().await.is_some() {}
            }),
        ))));
    }

    pub(super) fn reserve_computer_use_delivery(
        &self,
        session: &str,
    ) -> Option<(McpEndpoint, DecisionDelivery, String)> {
        if self.computer_use_dispatcher.stopping.load(Ordering::SeqCst)
            || self.pending_soft_respawn.lock().contains_key(session)
            || self
                .pending_spawn_flag_invalidations
                .lock()
                .contains_key(session)
            || self
                .mcp_catalog_status(session)
                .is_some_and(|status| status.pending || status.resource_cleanup_pending)
        {
            return None;
        }
        let endpoint = self.mcp_endpoint(session)?;
        let delivery = endpoint
            .acp
            .computer_use_decision(session, &endpoint.agent_session_id)?;
        // Automatic continuation never steals focus. Unpark the exact owner
        // into background, even when another focused chat is currently idle.
        if self.parked.lock().contains_key(session) {
            let live = self.unpark_to_live(session)?;
            self.background.lock().insert(session.into(), live);
        }
        let turn = self
            .with_session_mut(session, |live| {
                ready_for_delivery(live, &endpoint)
                    .then(|| open_host_turn(live))
                    .flatten()
            })
            .flatten()?;
        Some((endpoint, delivery, turn))
    }

    async fn deliver_computer_use_decision(
        self: &Arc<Self>,
        app: tauri::AppHandle,
        endpoint: McpEndpoint,
        delivery: DecisionDelivery,
        turn: String,
    ) {
        let session = delivery.app_session().to_string();
        self.emit_for_session(&app, &session);
        let check = || {
            let active = !self.computer_use_dispatcher.stopping.load(Ordering::SeqCst)
                && self.endpoint_is_current(&session, &endpoint)
                && self
                    .with_session_mut(&session, |live| {
                        live.prompt_in_flight
                            && live.active_turn_id.as_deref() == Some(turn.as_str())
                            && live.pending_permission_rpc_id.is_none()
                            && live.pending_plan_rpc_id.is_none()
                            && live.pending_ask_user_rpc_id.is_none()
                    })
                    .unwrap_or(false);
            active
                .then_some(())
                .ok_or_else(|| "Computer Use Host turn retired".into())
        };
        let attempt = if self.computer_use_dispatcher.stopping.load(Ordering::SeqCst) {
            DeliveryAttempt::NotStarted
        } else {
            tokio::select! {
                attempt = endpoint.acp.prompt_computer_use(&delivery, &turn, &check) => attempt,
                _ = self.computer_use_dispatcher.stopped() => {
                    endpoint.acp.computer_use_chat_stopped(&session);
                    let _ = endpoint.acp.cancel_for(&endpoint.agent_session_id).await;
                    if delivery.was_admitted() {
                        DeliveryAttempt::Unknown(crate::error::AgentError::new(
                            crate::error::AgentErrorCode::AgentCrashed,
                            "Host decision interrupted during App shutdown; outcome unknown",
                        ))
                    } else { DeliveryAttempt::NotStarted }
                }
            }
        };
        self.settle_computer_use_delivery(&app, &session, &endpoint, &turn, &attempt);
        if matches!(attempt, DeliveryAttempt::Completed) {
            let changed =
                super::super::post_turn_reconcile::reconcile_linked_session(self, &session, &turn)
                    .await;
            if changed > 0 {
                use tauri::Emitter;
                let _ = app.emit(
                    "session://journal_reconciled",
                    json!({"sessionId":session,"changed":changed}),
                );
            }
        }
    }

    fn settle_computer_use_delivery(
        &self,
        app: &tauri::AppHandle,
        session: &str,
        endpoint: &McpEndpoint,
        turn: &str,
        attempt: &DeliveryAttempt,
    ) {
        if !self.endpoint_is_current(session, endpoint) {
            return;
        }
        let mut emits = Vec::new();
        let mut persists = Vec::new();
        self.with_session_mut(session, |live| {
            if live.active_turn_id.as_deref() != Some(turn) {
                return;
            }
            live.prompt_in_flight = false;
            match attempt {
                DeliveryAttempt::Unknown(error) => {
                    persists.push(PendingSessionPersist::TurnBoundary(Box::new(
                        Self::prepare_turn_error(live, error, &mut emits),
                    )));
                    let _ = live.fsm.fail_with(error.clone());
                }
                DeliveryAttempt::Completed => {
                    live.deferred_prompt_complete
                        .get_or_insert_with(|| "end_turn".into());
                    Self::try_finish_deferred_prompt_complete(
                        live,
                        Some(&mut emits),
                        Some(&mut persists),
                    );
                }
                DeliveryAttempt::NotStarted => {
                    let _ = live.fsm.end_stream();
                    live.active_turn_id = None;
                    live.streaming_message_id = None;
                    crate::turn_lease::clear_lease(session);
                }
            }
        });
        Self::emit_stream_payloads(app, emits);
        Self::commit_session_persists(Some(app), persists);
        self.emit_for_session(app, session);
    }

    pub(crate) fn stop_computer_use_dispatcher(&self) {
        self.computer_use_dispatcher
            .stopping
            .store(true, Ordering::SeqCst);
        self.computer_use_dispatcher.wake.notify_waiters();
    }

    pub(crate) async fn join_computer_use_dispatcher(&self) -> Result<(), String> {
        self.stop_computer_use_dispatcher();
        let state = &self.computer_use_dispatcher;
        // The registry owns the original task through its actual join. A
        // cancelled observer drops only a borrow, never the running handle.
        let original = state.task.lock().clone();
        if let Some(original) = original {
            let mut slot = original.lock().await;
            #[cfg(test)]
            state.join_entered.notify_one();
            if let Some(task) = slot.as_mut() {
                if task.await.is_err() {
                    state.join_failed.store(true, Ordering::SeqCst);
                }
                slot.take();
            }
            drop(slot);
            let mut registered = state.task.lock();
            if registered
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &original))
            {
                registered.take();
            }
        } else {
            #[cfg(test)]
            state.join_entered.notify_one();
        }
        if state.join_failed.load(Ordering::SeqCst) {
            Err("Computer Use decision dispatcher did not join".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod ownership;

    #[tokio::test]
    async fn ready_fsm_does_not_bypass_real_rpc_or_permission_plan_ask_gates() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (acp, _events) = AcpClient::connect_tcp(
            &listener.local_addr().unwrap().to_string(),
            std::env::temp_dir(),
        )
        .await
        .unwrap();
        let (_peer, _) = listener.accept().await.unwrap();
        let endpoint = McpEndpoint {
            acp: acp.clone(),
            agent_session_id: "gate-agent".into(),
            process_id: "gate-process".into(),
            cwd: None,
        };
        let fresh = || {
            super::super::tests::ready_live("gate-app", "gate-agent", "gate-process", acp.clone())
        };
        let mut live = fresh();
        assert!(ready_for_delivery(&live, &endpoint));
        live.prompt_in_flight = true;
        assert!(!ready_for_delivery(&live, &endpoint));
        let in_flight = crate::acp_client::computer_use::PromptCallGuard::begin(
            &acp,
            "gate-agent",
            Some("rpc-turn"),
        )
        .unwrap();
        assert!(!ready_for_delivery(&fresh(), &endpoint));
        drop(in_flight);
        for gate in 0..6 {
            let mut live = fresh();
            match gate {
                0 => live.pending_permission_rpc_id = Some(1),
                1 => live.pending_plan_rpc_id = Some(2),
                2 => live.pending_ask_user_rpc_id = Some(3),
                3 => {
                    live.open_tool_ids.insert("owned-tool".into());
                }
                4 => live.deferred_prompt_complete = Some("end_turn".into()),
                _ => live.needs_history_bootstrap = true,
            }
            assert!(!ready_for_delivery(&live, &endpoint));
        }
        let mut old_agent = fresh();
        old_agent.meta.agent_session_id = Some("old-agent".into());
        assert!(!ready_for_delivery(&old_agent, &endpoint));
        let mut old_process = fresh();
        old_process.process_id = "old-process".into();
        assert!(!ready_for_delivery(&old_process, &endpoint));
        acp.kill().await;
    }

    #[tokio::test]
    async fn shutdown_subscription_handles_notification_before_and_after_poll() {
        let worker = DispatchWorker::default();
        worker.stopping.store(true, Ordering::SeqCst);
        worker.wake.notify_waiters();
        tokio::time::timeout(Duration::from_secs(1), worker.stopped())
            .await
            .unwrap();
        let worker = Arc::new(DispatchWorker::default());
        let subscriber = {
            let worker = worker.clone();
            tokio::spawn(async move {
                worker.stopped().await;
            })
        };
        tokio::task::yield_now().await;
        worker.stopping.store(true, Ordering::SeqCst);
        worker.wake.notify_waiters();
        tokio::time::timeout(Duration::from_secs(1), subscriber)
            .await
            .unwrap()
            .unwrap();
    }
}
