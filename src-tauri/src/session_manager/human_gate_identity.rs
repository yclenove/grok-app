//! Runtime-only identity and bounded delivery facts for plan/question gates.
use super::*;
use serde::Serialize;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Default)]
pub(super) struct HumanDelivery {
    // 0 = available, 1 = an answer owns the wire, 2 = delivered.
    state: AtomicU8,
    changed: tokio::sync::Notify,
}

impl HumanDelivery {
    pub(super) fn claim(&self) -> bool {
        self.state
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub(super) fn delivered(&self) {
        self.state.store(2, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    pub(super) fn release(&self) {
        let _ = self
            .state
            .compare_exchange(1, 0, Ordering::SeqCst, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    async fn wait_claim(&self) -> bool {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.claim() {
                return true;
            }
            if self.state.load(Ordering::SeqCst) == 2 {
                return false;
            }
            changed.await;
        }
    }
}

pub(super) struct HumanCancellation {
    pub(super) rpc: u64,
    kind: HumanGateKind,
    delivery: Arc<HumanDelivery>,
}

struct CancelClaim(Arc<HumanDelivery>);

impl Drop for CancelClaim {
    fn drop(&mut self) {
        self.0.release();
    }
}

impl HumanCancellation {
    pub(super) async fn respond(
        &self,
        client: &crate::acp_client::AcpClient,
    ) -> Result<(), String> {
        // An answer may have been queued before Stop/Steer took this gate.
        // Wait for that owner; skip a cancellation if it already delivered.
        if !self.delivery.wait_claim().await {
            return Ok(());
        }
        let _claim = CancelClaim(self.delivery.clone());
        match self.kind {
            HumanGateKind::Plan => {
                client
                    .respond_exit_plan_mode(self.rpc, "cancelled", None)
                    .await?
            }
            HumanGateKind::AskUser => {
                client
                    .respond_ask_user_question(
                        self.rpc,
                        crate::acp_client::AskUserOutcome::Cancelled,
                    )
                    .await?
            }
        }
        self.delivery.delivered();
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum HumanGateKind {
    Plan,
    AskUser,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPlanRequest {
    pub session_id: String,
    pub entries: serde_json::Value,
    pub body: Option<String>,
    pub rpc_id: Option<u64>,
    pub tool_call_id: Option<String>,
    pub request_generation: Option<uuid::Uuid>,
    pub chrome_revision: Option<String>,
    pub waiting: bool,
}

#[derive(Clone)]
pub(super) struct HumanGateBinding {
    pub(super) generation: uuid::Uuid,
    pub(super) turn: turn_event_owner::TurnEventOwner,
    binding: workspace_context::Binding,
    context: Option<workspace_context::RuntimeWorkspaceContext>,
    project_path: Option<String>,
    agent: Option<String>,
    store_root: std::path::PathBuf,
}

impl HumanGateBinding {
    pub(super) fn capture(session: &LiveSession) -> Self {
        Self {
            generation: uuid::Uuid::new_v4(),
            turn: turn_event_owner::TurnEventOwner::capture(session, false),
            binding: workspace_context::Binding::from(&session.meta),
            context: session.workspace_context.clone(),
            project_path: session.project_path.clone(),
            agent: session.meta.agent_session_id.clone(),
            store_root: crate::paths::app_data_root(),
        }
    }

    pub(super) fn matches(&self, session: &LiveSession) -> bool {
        self.store_root == crate::paths::app_data_root()
            && self.turn.matches(session)
            && self.binding == workspace_context::Binding::from(&session.meta)
            && self.context == session.workspace_context
            && self.project_path == session.project_path
    }

    pub(super) fn check_store(&self) -> Result<(), String> {
        if self.store_root != crate::paths::app_data_root() {
            return Err("human request store changed".into());
        }
        let row = workspace_context::session(&self.turn.id)?;
        if workspace_context::Binding::from(&row) != self.binding
            || row.agent_session_id != self.agent
        {
            return Err("human request session or workspace binding changed".into());
        }
        if let Some(context) = &self.context {
            context.validate(&self.turn.id)?;
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct HumanResponseKey {
    pub(super) kind: HumanGateKind,
    pub(super) session: String,
    client: usize,
    pub(super) rpc: u64,
    generation: Option<uuid::Uuid>,
}

impl HumanResponseKey {
    pub(super) fn capture(session: &LiveSession, kind: HumanGateKind, rpc: u64) -> Self {
        Self {
            kind,
            session: session.app_session_id.clone(),
            client: session
                .acp
                .as_ref()
                .map_or(0, |client| Arc::as_ptr(client) as usize),
            rpc,
            generation: session.human_gates.generation(kind),
        }
    }

    pub(super) fn matches(&self, session: &LiveSession) -> bool {
        self.session == session.app_session_id
            && session
                .acp
                .as_ref()
                .is_some_and(|client| self.client == Arc::as_ptr(client) as usize)
            && pending_rpc(session, self.kind) == Some(self.rpc)
            && session.human_gates.generation(self.kind) == self.generation
    }
}

#[derive(Default)]
pub(super) struct HumanGateState {
    // Retained across soft decisions; prepared events/decisions share its order.
    pub(super) chrome_clock: Option<crate::plan_chrome::PlanChromeClock>,
    pub(super) plan: Option<UiPlanRequest>,
    plan_owner: Option<HumanGateBinding>,
    ask_owner: Option<HumanGateBinding>,
    plan_reply: Option<HumanResponseKey>,
    ask_reply: Option<HumanResponseKey>,
    plan_delivery: Arc<HumanDelivery>,
    ask_delivery: Arc<HumanDelivery>,
}

impl HumanGateState {
    pub(super) fn delivery(&self, kind: HumanGateKind) -> Arc<HumanDelivery> {
        match kind {
            HumanGateKind::Plan => self.plan_delivery.clone(),
            HumanGateKind::AskUser => self.ask_delivery.clone(),
        }
    }
    pub(super) fn owner(&self, kind: HumanGateKind) -> Option<&HumanGateBinding> {
        match kind {
            HumanGateKind::Plan => self.plan_owner.as_ref(),
            HumanGateKind::AskUser => self.ask_owner.as_ref(),
        }
    }

    pub(super) fn generation(&self, kind: HumanGateKind) -> Option<uuid::Uuid> {
        self.owner(kind).map(|owner| owner.generation)
    }

    pub(super) fn install(&mut self, kind: HumanGateKind, owner: HumanGateBinding) {
        self.clear(kind);
        match kind {
            HumanGateKind::Plan => self.plan_owner = Some(owner),
            HumanGateKind::AskUser => self.ask_owner = Some(owner),
        }
    }

    pub(super) fn reply(&self, kind: HumanGateKind) -> Option<&HumanResponseKey> {
        match kind {
            HumanGateKind::Plan => self.plan_reply.as_ref(),
            HumanGateKind::AskUser => self.ask_reply.as_ref(),
        }
    }

    pub(super) fn record_reply(&mut self, key: HumanResponseKey) {
        match key.kind {
            HumanGateKind::Plan => self.plan_reply = Some(key),
            HumanGateKind::AskUser => self.ask_reply = Some(key),
        }
    }

    pub(super) fn clear(&mut self, kind: HumanGateKind) {
        match kind {
            HumanGateKind::Plan => {
                self.plan = None;
                self.plan_owner = None;
                self.plan_reply = None;
                self.plan_delivery = Arc::default();
            }
            HumanGateKind::AskUser => {
                self.ask_owner = None;
                self.ask_reply = None;
                self.ask_delivery = Arc::default();
            }
        }
    }
}

impl SessionManager {
    pub(super) fn take_human_gates_for_interject(
        session: &mut LiveSession,
    ) -> (
        Option<HumanCancellation>,
        Option<HumanCancellation>,
        Option<super::human_gate_invalidation::PendingGateInvalidation>,
    ) {
        let (ask, plan, mut invalidation) = Self::take_human_gates_for_cancel(session);
        if let Some(pending) = &mut invalidation {
            // Steer only takes plan/question gates; a permission stays pending.
            pending.row["permissionRpcId"] = serde_json::Value::Null;
            pending.row["permissionRequestGeneration"] = serde_json::Value::Null;
        }
        (ask, plan, invalidation)
    }

    pub(super) fn take_human_gates_for_cancel(
        session: &mut LiveSession,
    ) -> (
        Option<HumanCancellation>,
        Option<HumanCancellation>,
        Option<super::human_gate_invalidation::PendingGateInvalidation>,
    ) {
        let invalidation = Self::prepare_gate_invalidation(session);
        let ask = session
            .pending_ask_user_rpc_id
            .take()
            .map(|rpc| HumanCancellation {
                rpc,
                kind: HumanGateKind::AskUser,
                delivery: session.human_gates.delivery(HumanGateKind::AskUser),
            });
        let plan = session
            .pending_plan_rpc_id
            .take()
            .map(|rpc| HumanCancellation {
                rpc,
                kind: HumanGateKind::Plan,
                delivery: session.human_gates.delivery(HumanGateKind::Plan),
            });
        session.pending_ask_user_ui = None;
        session.human_gates = Default::default();
        (ask, plan, invalidation)
    }
}

pub(super) fn pending_rpc(session: &LiveSession, kind: HumanGateKind) -> Option<u64> {
    match kind {
        HumanGateKind::Plan => session.pending_plan_rpc_id,
        HumanGateKind::AskUser => session.pending_ask_user_rpc_id,
    }
}
