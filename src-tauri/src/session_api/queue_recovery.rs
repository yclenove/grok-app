//! Human-reviewed recovery is a separate operation, not cancellation or claim reset.
use super::{
    queue_lease,
    queue_store::{self, Item},
    QUEUE_FILE,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Decision {
    Dismiss,
    Retry,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RecoveryRequest {
    session_id: String,
    item_id: String,
    prompt: String,
    created_at: i64,
    decision: Decision,
    reviewed: bool,
    confirmed: bool,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoveryAck {
    version: u8,
    decision: Decision,
    session_id: String,
    item_id: String,
    replacement_id: Option<String>,
}
impl RecoveryAck {
    pub(super) fn new(item: &Item, decision: Decision, replacement_id: Option<String>) -> Self {
        Self {
            version: 1,
            decision,
            session_id: item.session_id.clone(),
            item_id: item.item_id.clone(),
            replacement_id,
        }
    }
}
pub(super) async fn recover_at(
    root: &Path,
    request: RecoveryRequest,
) -> Result<RecoveryAck, String> {
    if !request.reviewed
        || !request.confirmed
        || request.session_id.trim().is_empty()
        || request.session_id.trim() != request.session_id
        || request.session_id == "__draft__"
        || request.session_id.starts_with("imp:")
        || request.item_id.trim().is_empty()
        || request.item_id.trim() != request.item_id
        || request.prompt.trim().is_empty()
    {
        return Err("SESSION_QUEUE_INVALID".into());
    }
    let root = root.to_owned();
    tokio::task::spawn_blocking(move || {
        // A live cooperating dispatch cannot be retired or replaced.
        let _lease = queue_lease::try_acquire(&root)
            .map_err(|_| "SESSION_QUEUE_UNAVAILABLE")?
            .ok_or("SESSION_QUEUE_ACTIVE")?;
        let expected = Item::new(
            &request.session_id,
            &request.item_id,
            &request.prompt,
            request.created_at,
        );
        queue_store::recover(&root.join(QUEUE_FILE), &expected, request.decision).map_err(|e| {
            if e == "SESSION_QUEUE_CHANGED" {
                e
            } else {
                "SESSION_QUEUE_UNAVAILABLE".into()
            }
        })
    })
    .await
    .map_err(|_| "SESSION_QUEUE_UNAVAILABLE".to_string())?
}
#[tauri::command]
pub(crate) async fn session_api_queue_recover(
    request: RecoveryRequest,
) -> Result<RecoveryAck, String> {
    recover_at(&crate::paths::app_data_root(), request).await
}
