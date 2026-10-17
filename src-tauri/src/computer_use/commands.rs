//! Desktop commands for an approval decision and one redacted screenshot.

use serde::Deserialize;
use tauri::State;

use super::approval::{
    claim_from_body, confirm_from_body, fault_code, handoff_from_body, ConfirmBody, DecisionBody,
    HandoffBody,
};
use super::events::{tool_shot, ShotDto};
use super::ComputerUseHost;
use gcu_host_protocol::approval::{ConfirmEnd, HandoffEnd};

/// Body of `cu_tool_shot`. The id is the screenshot id from tool JSON, not a path.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShotQuery {
    session_id: String,
    id: String,
}

/// Consume the card the user is looking at. Errors are `absent`, `identity`,
/// `replay`, or `failed` — no paths or reasons.
#[tauri::command]
pub async fn cu_approval_decide(
    host: State<'_, ComputerUseHost>,
    body: DecisionBody,
) -> Result<(), String> {
    let office = host.office();
    let claim = claim_from_body(body)?;
    office.decide(claim).await.map_err(fault_code)
}

/// Consume one sensitive confirmation. The desk returns whether it still matches.
#[tauri::command]
pub async fn cu_confirm_decide(
    host: State<'_, ComputerUseHost>,
    body: ConfirmBody,
) -> Result<ConfirmEnd, String> {
    let claim = confirm_from_body(body)?;
    host.office().decide_confirm(&claim).map_err(fault_code)
}

/// Mark a handoff handled. This does not grant and does not clear Stop.
#[tauri::command]
pub async fn cu_handoff_decide(
    host: State<'_, ComputerUseHost>,
    body: HandoffBody,
) -> Result<HandoffEnd, String> {
    let claim = handoff_from_body(body)?;
    host.office().decide_handoff(&claim).map_err(fault_code)
}

/// Redacted thumbnail and full image for the chat tool card.
///
/// Errors are `foreign`, `missing`, or `rejected` — no paths.
#[tauri::command]
pub async fn cu_tool_shot(
    host: State<'_, ComputerUseHost>,
    body: ShotQuery,
) -> Result<ShotDto, String> {
    tool_shot(&host, &body.session_id, &body.id).await
}
