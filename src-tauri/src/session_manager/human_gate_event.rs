//! Actual Plan/AskUser consumer; native effects run after runtime guards drop.
use super::human_gate_identity::{HumanGateBinding, HumanGateKind, UiPlanRequest};
use super::*;
use crate::acp_client::{AcpEvent, AskUserQuestionItem};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

pub(super) enum HumanGateEvent {
    Plan {
        entries: Value,
        body: Option<String>,
        rpc_id: Option<u64>,
        tool_call_id: Option<String>,
    },
    AskUser {
        rpc_id: u64,
        tool_call_id: Option<String>,
        questions: Vec<AskUserQuestionItem>,
    },
}

impl HumanGateEvent {
    pub(super) fn from_acp(event: AcpEvent) -> Option<Self> {
        match event {
            AcpEvent::Plan {
                entries,
                body,
                rpc_id,
                tool_call_id,
            } => Some(Self::Plan {
                entries,
                body,
                rpc_id,
                tool_call_id,
            }),
            AcpEvent::AskUserQuestion {
                rpc_id,
                tool_call_id,
                questions,
                ..
            } => Some(Self::AskUser {
                rpc_id,
                tool_call_id,
                questions,
            }),
            _ => None,
        }
    }
}

pub(super) enum HumanGatePayload {
    Plan(UiPlanRequest),
    AskUser(UiAskUserRequest),
}

pub(super) struct HumanGateUpdate {
    pub(super) session_id: String,
    pub(super) payload: HumanGatePayload,
    pub(super) background: Option<SessionSnapshot>,
    chrome: Option<crate::plan_chrome::PreparedPlanChrome>,
}

impl HumanGateUpdate {
    #[cfg(test)]
    pub(super) fn persist(&self) {
        self.publish(|| {});
    }

    pub(super) fn publish(&self, effect: impl FnOnce()) {
        if let Some(write) = &self.chrome {
            if let Err(error) = write.commit_then(effect) {
                tracing::warn!(%error, "plan chrome persistence failed");
            }
        } else {
            effect();
        }
    }

    fn emit(&self, app: &AppHandle) {
        match &self.payload {
            HumanGatePayload::Plan(plan) => {
                crate::mirror::fanout_event(app, "session://plan", plan);
            }
            HumanGatePayload::AskUser(pending) => {
                crate::mirror::fanout_event(app, "session://ask_user", pending);
                if self.background.is_some() {
                    let _ = app.emit(
                        "session://background_permission",
                        serde_json::json!({ "sessionId": self.session_id, "kind": "ask_user" }),
                    );
                }
            }
        }
        if let Some(snapshot) = &self.background {
            SessionManager::emit_runtime(app, snapshot);
        }
    }
}

impl SessionManager {
    pub(super) fn handle_human_gate_event(
        &self,
        app: &AppHandle,
        process: &str,
        agent: Option<&str>,
        event: HumanGateEvent,
    ) {
        if let Some(update) = self.prepare_human_gate_event(process, agent, event) {
            update.publish(|| update.emit(app));
        }
    }

    pub(super) fn prepare_human_gate_event(
        &self,
        process: &str,
        agent: Option<&str>,
        event: HumanGateEvent,
    ) -> Option<HumanGateUpdate> {
        // Capture route and generation under the existing pool-transfer guard.
        // This captures idle owners too: restored human gates are not replay.
        let owner = self.capture_turn_event_owner(process, agent)?;
        #[cfg(test)]
        super::command_spawn_options_tests::human_gate_consumer_tests::pause_after_route(&owner.id);
        // Locate that App owner wherever it moved and validate ACP/epoch/turn
        // inside the final mutation guard. Capture attention/runtime there too.
        self.with_turn_event_owner_mut(&owner, |session, background| {
            Self::update_human_gate_locked(session, event, background)
        })
        .flatten()
    }

    fn update_human_gate_locked(
        session: &mut LiveSession,
        event: HumanGateEvent,
        background: bool,
    ) -> Option<HumanGateUpdate> {
        let mut chrome = None;
        let payload = match event {
            HumanGateEvent::Plan {
                entries,
                body,
                rpc_id,
                tool_call_id,
            } => {
                if Self::should_drop_plan_event(
                    session.prompt_in_flight,
                    session.deferred_prompt_complete.is_some(),
                    session.pending_plan_rpc_id.is_some(),
                    rpc_id.is_some(),
                ) {
                    tracing::debug!(session = %session.app_session_id, "plan dropped: idle replay without a gate");
                    return None;
                }
                let clock = session.human_gates.chrome_clock.get_or_insert_with(|| {
                    crate::plan_chrome::PlanChromeClock::for_session(&session.app_session_id)
                });
                if !clock.matches_store(&session.app_session_id) {
                    return None;
                }
                let fresh_gate = rpc_id.is_some();
                if let Some(id) = rpc_id {
                    let owner = HumanGateBinding::capture(session);
                    session.human_gates.install(HumanGateKind::Plan, owner);
                    session.pending_plan_rpc_id = Some(id);
                    Self::touch_activity_locked(session);
                }
                // Progress updates retain an existing gate; they do not create
                // a new reverse request or remove its live restore identity.
                let rpc_id = session.pending_plan_rpc_id.or(rpc_id);
                let mut plan = UiPlanRequest {
                    session_id: session.app_session_id.clone(),
                    entries,
                    body,
                    rpc_id,
                    tool_call_id,
                    request_generation: session.human_gates.generation(HumanGateKind::Plan),
                    chrome_revision: None,
                    waiting: rpc_id.is_none(),
                };
                let clock = session.human_gates.chrome_clock.get_or_insert_with(|| {
                    crate::plan_chrome::PlanChromeClock::for_session(&session.app_session_id)
                });
                let write = clock.prepare(
                    &session.app_session_id,
                    crate::plan_chrome::PlanChromeMutation::Event {
                        entries: plan.entries.clone(),
                        body: plan.body.clone(),
                        rpc_id: plan.rpc_id,
                        tool_call_id: plan.tool_call_id.clone(),
                        fresh_gate,
                    },
                )?;
                plan.chrome_revision = Some(write.ui_revision());
                chrome = Some(write);
                // Keep progress too: Stop/exit can precede its first disk write.
                session.human_gates.plan = Some(plan.clone());
                HumanGatePayload::Plan(plan)
            }
            HumanGateEvent::AskUser {
                rpc_id,
                tool_call_id,
                questions,
            } => {
                // Reverse RPCs remain valid when session/load restores an idle gate.
                if Self::should_drop_ask_user_event(session.prompt_in_flight) {
                    return None;
                }
                let owner = HumanGateBinding::capture(session);
                let pending = UiAskUserRequest {
                    request_generation: owner.generation,
                    rpc_id,
                    session_id: session.app_session_id.clone(),
                    tool_call_id,
                    questions,
                };
                session.pending_ask_user_rpc_id = Some(rpc_id);
                session.pending_ask_user_ui = Some(pending.clone());
                session.human_gates.install(HumanGateKind::AskUser, owner);
                Self::touch_activity_locked(session);
                HumanGatePayload::AskUser(pending)
            }
        };
        Some(HumanGateUpdate {
            session_id: session.app_session_id.clone(),
            payload,
            background: background.then(|| Self::snapshot_from_live(session)),
            chrome,
        })
    }
}
