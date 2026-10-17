//! Desktop command for one approval decision. The desk enforces the binding.

use tauri::State;

use super::approval::{
    claim_from_body, confirm_from_body, fault_code, handoff_from_body, ConfirmBody, DecisionBody,
    HandoffBody,
};
use super::ComputerUseHost;
use gcu_host_protocol::approval::{ConfirmEnd, HandoffEnd};

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
