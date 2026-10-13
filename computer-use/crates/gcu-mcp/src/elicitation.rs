//! Optional client UI, never the only authorization route.
use async_trait::async_trait;
use gcu_host_api::{
    ActionSummary, AppInfo, ApprovalDecision, ApprovalProvider, Cancel, HandoffReason, ImageRef,
    ModelReason, Risk, SessionCtx,
};
use rmcp::{
    model::{ElicitRequestParams, ElicitationAction, ElicitationSchema},
    Peer, RoleServer,
};
use serde_json::json;
use std::sync::Arc;

tokio::task_local! { pub(crate) static REQUEST_PEER: Peer<RoleServer>; }
pub(crate) struct Approvals(pub Arc<dyn ApprovalProvider>);
impl Approvals {
    async fn elicit(&self, app: &AppInfo, cancel: &Cancel) -> Option<ApprovalDecision> {
        let peer = REQUEST_PEER.try_with(Clone::clone).ok()?;
        let capability = peer.peer_info()?.capabilities.elicitation.clone()?;
        capability.form.as_ref()?;
        let schema: ElicitationSchema = serde_json::from_value(json!({
            "type":"object", "properties":{"decision":{"type":"string","enum":["turn","session","always","deny"]}},
            "required":["decision"]
        })).ok()?;
        // Still inside tools/call and rmcp's originating-request task scope.
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Some(ApprovalDecision::Deny),
            result = peer.create_elicitation(ElicitRequestParams::FormElicitationParams {
                meta:None, message:format!("Authorize {} for computer use? App identity: {}", app.name, app.identity.key), requested_schema:schema,
            }) => result.ok()?,
        };
        match result.action {
            ElicitationAction::Accept => match result.content?.get("decision")?.as_str()? {
                "turn" => Some(ApprovalDecision::Turn),
                "session" => Some(ApprovalDecision::Session),
                "deny" => Some(ApprovalDecision::Deny),
                // Only the trusted Host can persist Always. Ask that Host to
                // collect/persist the decision, never manufacture a grant here.
                _ => None,
            },
            ElicitationAction::Decline => Some(ApprovalDecision::Deny),
            // Grok headless immediately cancels: this is NOT authorization.
            ElicitationAction::Cancel => None,
            _ => None,
        }
    }
}
#[async_trait]
impl ApprovalProvider for Approvals {
    async fn request_app(
        &self,
        session: &SessionCtx,
        app: &AppInfo,
        reason: Option<&ModelReason>,
        risk: Risk,
        cancel: &Cancel,
    ) -> ApprovalDecision {
        if let Some(decision) = self.elicit(app, cancel).await {
            return decision;
        }
        if cancel.is_cancelled() {
            return ApprovalDecision::Deny;
        }
        self.0.request_app(session, app, reason, risk, cancel).await
    }
    async fn confirm_action(
        &self,
        session: &SessionCtx,
        app: &AppInfo,
        summary: &ActionSummary,
        crop: Option<&ImageRef>,
        cancel: &Cancel,
    ) -> bool {
        self.0
            .confirm_action(session, app, summary, crop, cancel)
            .await
    }
    async fn handoff(
        &self,
        session: &SessionCtx,
        app: &AppInfo,
        reason: HandoffReason,
        cancel: &Cancel,
    ) {
        self.0.handoff(session, app, reason, cancel).await;
    }
}
