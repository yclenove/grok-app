//! Compare-and-cancel only unclaimed external work. No dispatch or claim release.
use super::{
    queue_store::{self, Item},
    QUEUE_FILE,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct QueueCancelRequest {
    session_id: String,
    item_id: String,
    prompt: String,
    created_at: i64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueueCancelResult {
    version: u8,
    status: &'static str,
    session_id: String,
    item_id: String,
}
pub(super) async fn cancel_at(
    root: &Path,
    request: QueueCancelRequest,
) -> Result<QueueCancelResult, String> {
    if request.session_id.trim().is_empty()
        || request.session_id.trim() != request.session_id
        || request.session_id == "__draft__"
        || request.session_id.starts_with("imp:")
        || crate::session_migration::reject_imported_history_id(Some(&request.session_id)).is_err()
        || request.item_id.trim().is_empty()
        || request.item_id.trim() != request.item_id
        || request.prompt.trim().is_empty()
    {
        return Err("SESSION_QUEUE_INVALID".into());
    }
    let path = root.join(QUEUE_FILE);
    let expected = Item::new(
        &request.session_id,
        &request.item_id,
        &request.prompt,
        request.created_at,
    );
    tokio::task::spawn_blocking(move || queue_store::cancel(&path, &expected))
        .await
        .map_err(|_| "SESSION_QUEUE_UNAVAILABLE".to_string())?
        .map_err(|e| match e.as_str() {
            "SESSION_QUEUE_CHANGED" | "SESSION_QUEUE_CLAIMED" => e,
            _ => "SESSION_QUEUE_UNAVAILABLE".into(),
        })?;
    Ok(QueueCancelResult {
        version: 1,
        status: "cancelled",
        session_id: request.session_id,
        item_id: request.item_id,
    })
}
#[tauri::command]
pub(crate) async fn session_api_queue_cancel(
    request: QueueCancelRequest,
) -> Result<QueueCancelResult, String> {
    cancel_at(&crate::paths::app_data_root(), request).await
}
