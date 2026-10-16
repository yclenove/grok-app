//! Optional client UI, never the only authorization route.
use crate::ui_text::{self, fill, sanitize_plain};
use async_trait::async_trait;
use gcu_host_api::{
    ActionSummary, AppInfo, ApprovalDecision, ApprovalProvider, Cancel, HandoffReason, ImageRef,
    ModelReason, Platform, Risk, SessionCtx, SignatureState,
};
use rmcp::{
    model::{ElicitRequestParams, ElicitationAction, ElicitationSchema, RequestMetaObject},
    Peer, RoleServer,
};
use serde_json::{json, Value};
use std::sync::Arc;

tokio::task_local! { pub(crate) static REQUEST_PEER: Peer<RoleServer>; }

/// Formal decision schema sent inside `tools/call`.
pub fn app_decision_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "decision": {
                "type": "string",
                "enum": ["turn", "session", "always", "deny"]
            }
        },
        "required": ["decision"]
    })
}

/// Elicitation `_meta`: OS identity, risk, and a sanitized model reason. No icon bytes.
pub fn approval_meta(app: &AppInfo, reason: Option<&ModelReason>, risk: Risk) -> Value {
    json!({
        "gcu/app": app_value(app),
        "gcu/risk": risk_name(risk),
        "gcu/reason": reason_value(reason),
    })
}

/// Localized elicitation question. The name is plain text with controls removed.
pub fn approval_message(name: &str) -> String {
    let safe = sanitize_plain(name);
    fill(
        ui_text::active_locale(),
        "approval.elicitation",
        &[("name", safe.as_str())],
    )
}

pub(crate) struct Approvals(pub Arc<dyn ApprovalProvider>);

impl Approvals {
    async fn elicit(
        &self,
        app: &AppInfo,
        reason: Option<&ModelReason>,
        risk: Risk,
        cancel: &Cancel,
    ) -> Option<ApprovalDecision> {
        let peer = REQUEST_PEER.try_with(Clone::clone).ok()?;
        let capability = peer.peer_info()?.capabilities.elicitation.clone()?;
        capability.form.as_ref()?;
        let schema: ElicitationSchema = serde_json::from_value(app_decision_schema()).ok()?;
        let meta = meta_object(app, reason, risk);
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Some(ApprovalDecision::Deny),
            result = peer.create_elicitation(ElicitRequestParams::FormElicitationParams {
                meta: Some(meta),
                message: approval_message(&app.name),
                requested_schema: schema,
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

fn meta_object(app: &AppInfo, reason: Option<&ModelReason>, risk: Risk) -> RequestMetaObject {
    match approval_meta(app, reason, risk) {
        Value::Object(map) => RequestMetaObject::from(map),
        _ => RequestMetaObject::new(),
    }
}

fn app_value(app: &AppInfo) -> Value {
    let id = &app.identity;
    json!({
        "name": sanitize_plain(&app.name),
        "platform": platform_name(id.platform),
        "key": sanitize_plain(&id.key),
        "signature": signature_name(id.signature),
        "publisher": id.publisher.as_deref().map(sanitize_plain),
        "team_id": id.team_id.as_deref().map(sanitize_plain),
        "image_path": sanitize_plain(&id.image_path),
        "path_hash": id.sha256.map(hex_encode),
    })
}

fn reason_value(reason: Option<&ModelReason>) -> Value {
    match reason {
        Some(reason) => {
            let text = sanitize_plain(reason.as_str());
            if text.is_empty() {
                Value::Null
            } else {
                Value::String(text)
            }
        }
        None => Value::Null,
    }
}

fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Macos => "macos",
        Platform::Windows => "windows",
        Platform::Linux => "linux",
    }
}

fn signature_name(signature: SignatureState) -> &'static str {
    match signature {
        SignatureState::Valid => "valid",
        SignatureState::Invalid => "invalid",
        SignatureState::Unsigned => "unsigned",
        SignatureState::Unknown => "unknown",
    }
}

fn risk_name(risk: Risk) -> &'static str {
    match risk {
        Risk::High => "high",
        Risk::Low => "low",
    }
}

fn hex_encode(bytes: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
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
        if let Some(decision) = self.elicit(app, reason, risk, cancel).await {
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
