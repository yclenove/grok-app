//! Desktop command for one approval decision. The desk enforces the binding.

use tauri::State;

use super::approval::{claim_from_body, fault_code, DecisionBody};
use super::ComputerUseHost;

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
