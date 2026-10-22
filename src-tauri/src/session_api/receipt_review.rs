//! Direct Unknown receipts: explicit human review, never reuse an old dispatch identity.
use super::{idempotency, receipt_lease, receipt_store, TurnResult, TurnStatus};
use serde::{Deserialize, Serialize};
use std::{future::Future, path::Path, sync::Arc};
use tauri::Manager;
pub(super) const FILE: &str = "session-api-idempotency.json";
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Decision {
    Dismiss,
    Retry,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReviewNote {
    pub decision: Decision,
    pub reviewed_at: i64,
    pub replacement_key: Option<String>,
    pub replacement_claim: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewRequest {
    pub session_id: String,
    pub key: String,
    pub revision: String,
    pub prompt: String,
    pub decision: Decision,
    pub reviewed: bool,
    pub confirmed: bool,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewAck {
    pub version: u8,
    pub session_id: String,
    pub key: String,
    pub revision: String,
    pub decision: Decision,
    pub replacement_key: Option<String>,
    pub replacement_status: Option<TurnStatus>,
    pub queue_item_id: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReceiptRow {
    pub session_id: String,
    pub key: String,
    pub revision: String,
    pub can_review: bool,
    pub review: Option<ReviewAck>,
}
#[derive(Debug, Serialize)]
pub(crate) struct ReceiptSnapshot {
    pub version: u8,
    pub items: Vec<ReceiptRow>,
}
fn validate(r: &ReviewRequest) -> Result<(), String> {
    if !r.reviewed
        || !r.confirmed
        || r.session_id.trim().is_empty()
        || r.session_id.trim() != r.session_id
        || r.session_id == "__draft__"
        || r.session_id.starts_with("imp:")
        || r.key.trim().is_empty()
        || r.key.trim() != r.key
        || r.revision.len() != 64
        || hex::decode(&r.revision).is_err()
        || (r.decision == Decision::Retry && r.prompt.trim().is_empty())
    {
        return Err("SESSION_RECEIPT_INVALID".into());
    }
    Ok(())
}
pub(super) async fn review_at<F, Fut>(
    path: &Path,
    request: ReviewRequest,
    dispatch: F,
) -> Result<ReviewAck, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = TurnResult>,
{
    validate(&request)?;
    let lease_path = path.to_owned();
    let _lease = tokio::task::spawn_blocking(move || receipt_lease::try_acquire(&lease_path, true))
        .await
        .map_err(|_| "SESSION_RECEIPT_UNAVAILABLE")?
        .map_err(|_| "SESSION_RECEIPT_UNAVAILABLE")?
        .ok_or("SESSION_RECEIPT_ACTIVE")?;
    let store_path = path.to_owned();
    let sid = request.session_id.clone();
    let begin = tokio::task::spawn_blocking(move || receipt_store::review(&store_path, &request))
        .await
        .map_err(|_| "SESSION_RECEIPT_UNAVAILABLE")??;
    let (ack, child, nonce) = match begin {
        receipt_store::ReviewBegin::Replay(ack) => return Ok(ack),
        receipt_store::ReviewBegin::Reserved(ack, child, nonce) => (ack, child, nonce),
    };
    let key = ack
        .replacement_key
        .as_deref()
        .ok_or("SESSION_RECEIPT_UNAVAILABLE")?;
    let mut result = dispatch().await;
    result.idempotency_key = Some(key.to_owned());
    if result.session_id != sid || result.status == TurnStatus::RetryLater {
        result = idempotency::unknown(
            &sid,
            key,
            "Reviewed dispatch did not return a confirmed outcome.",
        );
    }
    let saved_result = result.clone();
    let saved =
        tokio::task::spawn_blocking(move || receipt_store::finish(&child, &nonce, saved_result))
            .await;
    if !matches!(saved, Ok(Ok(()))) {
        return Err("SESSION_RECEIPT_UNAVAILABLE".into());
    }
    Ok(ReviewAck {
        replacement_status: Some(result.status),
        queue_item_id: result.queue_item_id,
        ..ack
    })
}
#[tauri::command]
pub(crate) async fn session_api_receipt_snapshot() -> Result<ReceiptSnapshot, String> {
    let path = crate::paths::app_data_root().join(FILE);
    tokio::task::spawn_blocking(move || receipt_store::snapshot(&path))
        .await
        .map_err(|_| "SESSION_RECEIPT_UNAVAILABLE")?
        .map_err(|_| "SESSION_RECEIPT_UNAVAILABLE".into())
}
pub(crate) async fn review_with_host(
    request: ReviewRequest,
    app: &tauri::AppHandle,
    mgr: &Arc<crate::session_manager::SessionManager>,
) -> Result<ReviewAck, String> {
    let sid = request.session_id.clone();
    let prompt = request.prompt.clone();
    review_at(
        &crate::paths::app_data_root().join(FILE),
        request,
        || async {
            match super::prepare_send(&sid, &prompt, "external") {
                Ok(prepared) => super::dispatch_turn_or_timeout(app, mgr, prepared).await,
                Err(result) => *result,
            }
        },
    )
    .await
}
#[tauri::command]
pub(crate) async fn session_api_receipt_review(
    app: tauri::AppHandle,
    request: ReviewRequest,
) -> Result<ReviewAck, String> {
    let mgr = app.state::<Arc<crate::session_manager::SessionManager>>();
    review_with_host(request, &app, mgr.inner()).await
}
