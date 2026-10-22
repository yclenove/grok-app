//! Invalidation is reserved under the owning runtime guard, committed outside it.
use super::human_gate_identity::HumanGateKind;
use super::*;
use crate::plan_chrome::{
    PlanChromeClock, PlanChromeDraft, PlanChromeMutation, PreparedPlanChrome,
};
use serde_json::{json, Value};
use tauri::{AppHandle, Runtime};

pub(crate) struct PendingGateInvalidation {
    pub(super) row: Value,
    chrome: Option<PreparedPlanChrome>,
}

impl PendingGateInvalidation {
    pub(super) fn publish(&self, effect: impl FnOnce(&Value)) {
        if let Some(write) = &self.chrome {
            let mut effect = Some(effect);
            if let Err(error) = write.commit_then(|| effect.take().unwrap()(&self.row)) {
                tracing::warn!(%error, "plan invalidation persistence failed");
            }
            // A later Plan can supersede only the Plan effect; its arrival
            // does not restore the old independent permission/question gates.
            if let Some(effect) = effect {
                if self.row["permissionRpcId"].is_u64() || self.row["askUserRpcId"].is_u64() {
                    let mut row = self.row.clone();
                    for field in [
                        "planRpcId",
                        "planRequestGeneration",
                        "previousPlanChromeRevision",
                        "publishedPlanChromeRevision",
                        "planChromeRevision",
                    ] {
                        row[field] = Value::Null;
                    }
                    effect(&row);
                }
            }
        } else {
            effect(&self.row);
        }
    }

    pub(super) fn emit<R: Runtime>(&self, app: &AppHandle<R>, reason: &str) {
        self.publish(|row| {
            crate::mirror::fanout_event(
                app,
                "session://permissions_invalidated",
                json!({"reason": reason, "sessions": [row]}),
            );
        });
    }
}

impl SessionManager {
    /// Includes non-RPC progress: its prepared write must die with the turn too.
    pub(super) fn prepare_gate_invalidation(s: &LiveSession) -> Option<PendingGateInvalidation> {
        if s.pending_permission_rpc_id.is_none()
            && s.pending_plan_rpc_id.is_none()
            && s.pending_ask_user_rpc_id.is_none()
            && s.human_gates.chrome_clock.is_none()
        {
            return None;
        }
        let clock = s.human_gates.chrome_clock.clone().or_else(|| {
            s.pending_plan_rpc_id
                .map(|_| PlanChromeClock::for_session(&s.app_session_id))
        });
        let previous_revision = clock.as_ref().map(PlanChromeClock::current_ui_revision);
        let published_revision = clock.as_ref().map(PlanChromeClock::published_ui_revision);
        let chrome = clock.and_then(|clock| {
            clock.prepare(
                &s.app_session_id,
                PlanChromeMutation::Stale {
                    plan: s.human_gates.plan.as_ref().map(|plan| PlanChromeDraft {
                        entries: plan.entries.clone(),
                        body: plan.body.clone(),
                        rpc_id: plan.rpc_id,
                        tool_call_id: plan.tool_call_id.clone(),
                    }),
                },
            )
        });
        Some(PendingGateInvalidation {
            row: json!({
                "sessionId": s.app_session_id,
                "identityVersion": 1,
                "permissionRpcId": s.pending_permission_rpc_id,
                "permissionRequestGeneration": s.pending_permission_ui.as_ref().map(|gate| gate.request_generation),
                "planRpcId": s.pending_plan_rpc_id,
                "planRequestGeneration": s.human_gates.generation(HumanGateKind::Plan),
                "previousPlanChromeRevision": previous_revision,
                "publishedPlanChromeRevision": published_revision,
                "planChromeRevision": chrome.as_ref().map(PreparedPlanChrome::ui_revision),
                "askUserRpcId": s.pending_ask_user_rpc_id,
                "askUserRequestGeneration": s.pending_ask_user_ui.as_ref().map(|gate| gate.request_generation),
            }),
            chrome,
        })
    }

    pub(super) fn take_pending_gate_invalidation(
        s: &mut LiveSession,
    ) -> Option<PendingGateInvalidation> {
        let pending = Self::prepare_gate_invalidation(s);
        s.pending_permission_rpc_id = None;
        s.pending_permission_reply = None;
        s.pending_permission_options = None;
        s.pending_permission_tool_name = None;
        s.pending_permission_ui = None;
        s.pending_plan_rpc_id = None;
        s.pending_ask_user_rpc_id = None;
        s.pending_ask_user_ui = None;
        s.human_gates = Default::default();
        pending
    }
}
