//! Async queue orchestration; no store lock crosses dispatch.
use super::{
    idempotency,
    queue_store::{self, Item},
    TurnResult, TurnStatus, QUEUE_FILE,
};
use std::{future::Future, path::Path};

pub(super) async fn enqueue_at(root: &Path, item: Item) -> TurnResult {
    let path = root.join(QUEUE_FILE);
    let sid = item.session_id.clone();
    let saved_item = item.clone();
    let saved = tokio::task::spawn_blocking(move || queue_store::insert(&path, saved_item)).await;
    if !matches!(saved, Ok(Ok(()))) {
        return TurnResult::fail(TurnStatus::Unknown, sid,
            "Queue acceptance could not be confirmed. Inspect the queue and original session; do not automatically resend.");
    }
    TurnResult {
        ok: true,
        status: TurnStatus::Queued,
        session_id: sid,
        idempotency_key: None,
        message: Some("queued until the current turn ends".into()),
        queue_item_id: Some(item.item_id),
        queued_prompt: Some(item.prompt),
    }
}
pub(super) async fn list_at(root: &Path) -> Result<Vec<Item>, String> {
    let path = root.join(QUEUE_FILE);
    tokio::task::spawn_blocking(move || queue_store::list(&path))
        .await
        .map_err(|e| e.to_string())?
}
/// The durable queue claim outlives receipt eviction, cancellation, and restarts.
/// None means stale/already claimed, not permission to send the callback.
pub(super) async fn run_at<F, Fut>(root: &Path, item: &Item, dispatch: F) -> Option<TurnResult>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = TurnResult>,
{
    let lease_root = root.to_owned();
    let lease =
        tokio::task::spawn_blocking(move || super::queue_lease::try_acquire(&lease_root)).await;
    let _lease = match lease {
        Ok(Ok(Some(lease))) => lease,
        Ok(Ok(None)) => return None,
        _ => {
            return Some(idempotency::unknown(
                &item.session_id,
                &item.item_id,
                "Queue dispatch lease unavailable; not dispatched.",
            ))
        }
    };
    let path = root.join(QUEUE_FILE);
    let claim_path = path.clone();
    let claim_item = item.clone();
    let claim =
        tokio::task::spawn_blocking(move || queue_store::claim(&claim_path, &claim_item)).await;
    let nonce = match claim {
        Ok(Ok(Some(nonce))) => nonce,
        Ok(Ok(None)) => return None,
        _ => {
            return Some(idempotency::unknown(
                &item.session_id,
                &item.item_id,
                "Queue claim unavailable; this attempt was not dispatched.",
            ))
        }
    };
    // Also honor the V90 queue-receipt namespace for existing queue entries.
    let mut result = idempotency::run_queued_at(
        root,
        &item.session_id,
        &item.prompt,
        &item.item_id,
        dispatch,
    )
    .await;
    let finished_item = item.clone();
    let finished_result = result.clone();
    let finished = tokio::task::spawn_blocking(move || {
        queue_store::finish(&path, &finished_item, &nonce, &finished_result)
    })
    .await;
    if !matches!(finished, Ok(Ok(true))) {
        result = idempotency::unknown(
            &item.session_id,
            &item.item_id,
            "Queue delivery acknowledgement is unconfirmed. Inspect the original session and durable queue.",
        );
    }
    Some(result)
}
