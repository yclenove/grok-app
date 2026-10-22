//! Actual questionnaire/plan reply core; native effects live in control.rs.
pub(super) use super::human_gate_identity::HumanGateKind;
use super::human_gate_identity::{pending_rpc, HumanDelivery, HumanGateBinding, HumanResponseKey};
use super::*;
use crate::acp_client::AskUserOutcome;
use serde_json::Value;

pub(super) struct HumanGateAnswer {
    pub(super) kind: HumanGateKind,
    pub(super) rpc_id: Option<u64>,
    pub(super) request_generation: Option<uuid::Uuid>,
    pub(super) decision: String,
    pub(super) feedback: Option<String>,
    pub(super) answers: Option<Value>,
    pub(super) session_id: Option<String>,
}

pub(super) struct HumanGateResolution {
    pub(super) target: String,
    pub(super) snapshot: SessionSnapshot,
    pub(super) emits: Vec<StreamEmitPayload>,
    pub(super) persists: Vec<PendingSessionPersist>,
    pub(super) empty: Option<(String, String, String)>,
    plan_chrome: Option<crate::plan_chrome::PreparedPlanChrome>,
}

impl HumanGateResolution {
    pub(super) fn persist_plan_chrome(&self) {
        if let Some(write) = &self.plan_chrome {
            if let Err(error) = write.commit() {
                tracing::warn!(%error, "plan decision persistence failed");
            }
        }
    }
}

struct HumanOwner {
    binding: HumanGateBinding,
    key: HumanResponseKey,
    client: Arc<crate::acp_client::AcpClient>,
    delivery: Arc<HumanDelivery>,
}

struct HumanReservation<'a> {
    responses: &'a Mutex<HashSet<HumanResponseKey>>,
    key: HumanResponseKey,
    delivery: Arc<HumanDelivery>,
}

impl Drop for HumanReservation<'_> {
    fn drop(&mut self) {
        self.responses.lock().remove(&self.key);
        self.delivery.release();
    }
}

impl HumanOwner {
    fn capture(session: &LiveSession, answer: &HumanGateAnswer) -> Result<Self, String> {
        let rpc = pending_rpc(session, answer.kind).ok_or("no pending human request")?;
        if answer.rpc_id.is_some_and(|id| id != rpc) {
            return Err("stale human request RPC; refresh the pending card".into());
        }
        let binding = match (
            session.human_gates.owner(answer.kind),
            answer.request_generation,
        ) {
            (Some(owner), Some(generation))
                if owner.generation == generation && answer.rpc_id == Some(rpc) =>
            {
                let card_matches = match answer.kind {
                    HumanGateKind::Plan => session.human_gates.plan.as_ref().is_some_and(|card| {
                        card.session_id == session.app_session_id
                            && card.rpc_id == Some(rpc)
                            && card.request_generation == Some(generation)
                    }),
                    HumanGateKind::AskUser => {
                        session.pending_ask_user_ui.as_ref().is_some_and(|card| {
                            card.session_id == session.app_session_id
                                && card.rpc_id == rpc
                                && card.request_generation == generation
                        })
                    }
                };
                if !card_matches || !owner.matches(session) {
                    return Err("human request owner or card changed".into());
                }
                owner.clone()
            }
            (None, None)
                if match answer.kind {
                    HumanGateKind::Plan => session.human_gates.plan.is_none(),
                    HumanGateKind::AskUser => session.pending_ask_user_ui.is_none(),
                } =>
            {
                HumanGateBinding::capture(session)
            }
            _ => return Err("stale human request identity; refresh the pending card".into()),
        };
        if session
            .human_gates
            .reply(answer.kind)
            .is_some_and(|key| key.matches(session))
        {
            return Err("this human response has already been sent".into());
        }
        Ok(Self {
            binding,
            key: HumanResponseKey::capture(session, answer.kind, rpc),
            client: session.acp.clone().ok_or("ACP client missing")?,
            delivery: session.human_gates.delivery(answer.kind),
        })
    }

    fn matches(&self, session: &LiveSession) -> bool {
        self.binding.matches(session) && self.key.matches(session)
    }

    fn check(&self, manager: &SessionManager, sent: bool) -> Result<(), String> {
        if !self.client.is_alive() {
            return Err("human request agent process exited".into());
        }
        self.binding.check_store()?;
        if !manager
            .with_session_mut(&self.key.session, |session| {
                self.matches(session)
                    && (sent
                        || !session
                            .human_gates
                            .reply(self.key.kind)
                            .is_some_and(|key| key.matches(session)))
            })
            .unwrap_or(false)
        {
            return Err("human request owner changed; the pending gate was preserved".into());
        }
        Ok(())
    }

    fn reserve<'a>(&self, manager: &'a SessionManager) -> Result<HumanReservation<'a>, String> {
        if !manager.human_responses.lock().insert(self.key.clone()) {
            return Err("this human request is already being answered".into());
        }
        if !self.delivery.claim() {
            manager.human_responses.lock().remove(&self.key);
            return Err("this human response is in flight or already delivered".into());
        }
        Ok(HumanReservation {
            responses: &manager.human_responses,
            key: self.key.clone(),
            delivery: self.delivery.clone(),
        })
    }

    fn record_delivery(&self, manager: &SessionManager) {
        self.delivery.delivered();
        // Keep a successful wire delivery even if a following store read fails.
        // This is bounded by the original gate and does not adopt a new request.
        manager.with_session_mut(&self.key.session, |session| {
            if self.key.matches(session) {
                session.human_gates.record_reply(self.key.clone());
            }
        });
    }
}

impl SessionManager {
    pub(super) async fn resolve_human_gate_core(
        &self,
        answer: HumanGateAnswer,
    ) -> Result<HumanGateResolution, String> {
        let target = self.resolve_target_session(answer.session_id.clone())?;
        let owner = self
            .with_session_mut(&target, |session| HumanOwner::capture(session, &answer))
            .ok_or("no session")??;
        let _reservation = owner.reserve(self)?;
        owner.check(self, false)?;
        #[cfg(test)]
        super::command_spawn_options_tests::human_gate_resolution_tests::pause(
            &target,
            "before_write",
        )
        .await;
        owner.check(self, false)?;
        match answer.kind {
            HumanGateKind::Plan => owner
                .client
                .respond_exit_plan_mode_checked(
                    owner.key.rpc,
                    &answer.decision,
                    answer.feedback,
                    || owner.check(self, false),
                )
                .await
                .map_err(|failure| failure.message)?,
            HumanGateKind::AskUser => {
                let outcome = match answer.decision.as_str() {
                    "accepted" | "answered" | "accept" => AskUserOutcome::Accepted {
                        answers: answer.answers.unwrap_or_else(|| serde_json::json!({})),
                    },
                    _ => AskUserOutcome::Cancelled,
                };
                owner
                    .client
                    .respond_ask_user_question_checked(owner.key.rpc, outcome, || {
                        owner.check(self, false)
                    })
                    .await
                    .map_err(|failure| failure.message)?;
            }
        }
        owner.record_delivery(self);
        #[cfg(test)]
        super::command_spawn_options_tests::human_gate_resolution_tests::pause(
            &target,
            "after_write",
        )
        .await;
        owner.check(self, true)?;
        let mut emits = Vec::new();
        let mut persists = Vec::new();
        let mut plan_chrome = None;
        let (mut snapshot, empty) = self
            .with_turn_event_owner_mut(&owner.binding.turn, |session, _| {
                if !owner.matches(session) || !owner.client.is_alive() {
                    return None;
                }
                Self::touch_activity_locked(session);
                if answer.kind == HumanGateKind::Plan {
                    let clock = session.human_gates.chrome_clock.get_or_insert_with(|| {
                        crate::plan_chrome::PlanChromeClock::for_session(&session.app_session_id)
                    });
                    plan_chrome = Some(clock.prepare(
                        &session.app_session_id,
                        crate::plan_chrome::PlanChromeMutation::Decision {
                            decision: answer.decision.clone(),
                            keep_body: true,
                            plan: session.human_gates.plan.as_ref().map(|plan| {
                                crate::plan_chrome::PlanChromeDraft {
                                    entries: plan.entries.clone(),
                                    body: plan.body.clone(),
                                    rpc_id: plan.rpc_id,
                                    tool_call_id: plan.tool_call_id.clone(),
                                }
                            }),
                        },
                    )?);
                }
                match answer.kind {
                    HumanGateKind::Plan => session.pending_plan_rpc_id = None,
                    HumanGateKind::AskUser => {
                        session.pending_ask_user_rpc_id = None;
                        session.pending_ask_user_ui = None;
                    }
                }
                session.human_gates.clear(answer.kind);
                let empty = Self::try_finish_deferred_prompt_complete(
                    session,
                    Some(&mut emits),
                    Some(&mut persists),
                )
                .flatten();
                Some((Self::snapshot_from_live(session), empty))
            })
            .flatten()
            .ok_or("human request owner changed; the pending gate was preserved")?;
        snapshot.plan_chrome_revision = plan_chrome.as_ref().map(|write| write.ui_revision());
        Ok(HumanGateResolution {
            target,
            snapshot,
            emits,
            persists,
            empty,
            plan_chrome,
        })
    }
}
