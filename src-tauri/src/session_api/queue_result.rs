//! Resolve the original queued receipt without rewriting its identity binding.
use super::{queue_outcomes::Resolved, queue_store, TurnResult, TurnStatus, QUEUE_FILE};
use std::path::Path;
pub(super) async fn resolve_at(root: &Path, result: TurnResult) -> TurnResult {
    if result.status != TurnStatus::Queued {
        return result;
    }
    let path = root.join(QUEUE_FILE);
    let source = result.clone();
    let resolved =
        tokio::task::spawn_blocking(
            move || match (&source.queue_item_id, &source.queued_prompt) {
                (Some(id), Some(prompt)) => {
                    queue_store::resolve(&path, &source.session_id, id, prompt)
                }
                _ => Ok(Resolved::Unknown),
            },
        )
        .await;
    let mut updated=match resolved {
        Ok(Ok(Resolved::Queued(item))) => TurnResult {
            queue_item_id:Some(item.item_id),queued_prompt:Some(item.prompt),..result.clone()
        },
        Ok(Ok(Resolved::Started)) => TurnResult::started(&result.session_id),
        Ok(Ok(Resolved::Cancelled)) => TurnResult::fail(TurnStatus::Cancelled,&result.session_id,
            "Queued request was cancelled before dispatch."),
        _ => TurnResult::fail(TurnStatus::Unknown,&result.session_id,
            "Queue outcome cannot be confirmed. Inspect the original session; do not automatically resend.")
    };
    updated.idempotency_key = result.idempotency_key;
    updated
}
