//! Scope external request identity to a session and a normalized prompt.
//! Persist a claim before entering dispatch. A dropped future or a host crash
//! leaves an Unknown receipt: retrying that identity must never dispatch twice.
use super::receipt_store::{self, Begin, Request};
use super::{TurnResult, TurnStatus};
use std::{future::Future, path::Path};

pub(super) fn unknown(session_id: &str, key: &str, detail: &str) -> TurnResult {
    let mut result = TurnResult::fail(
        TurnStatus::Unknown,
        session_id,
        format!("{detail} Outcome unknown; inspect the original session before any new request. This key will not be automatically redispatched."),
    );
    result.idempotency_key = Some(key.to_owned());
    result
}

pub(super) async fn run_at<F, Fut>(
    path: &Path,
    session_id: &str,
    prompt: &str,
    key: Option<&str>,
    dispatch: F,
) -> (TurnResult, bool)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = TurnResult>,
{
    let key = key.map(str::trim).filter(|key| !key.is_empty());
    // No cached result may bypass the imported-history read-only boundary.
    if let Err(error) = crate::session_migration::reject_imported_history_id(Some(session_id)) {
        let mut result = TurnResult::fail(TurnStatus::Error, session_id, error);
        result.idempotency_key = key.map(str::to_owned);
        return (result, false);
    }
    let Some(key) = key else {
        let mut result = dispatch().await;
        result.idempotency_key = None;
        return (result, false);
    };
    let lease_path = path.to_owned();
    let lease =
        tokio::task::spawn_blocking(move || super::receipt_lease::try_acquire(&lease_path, false))
            .await;
    let _lease = match lease {
        Ok(Ok(Some(lease))) => lease,
        _ => {
            return (
                unknown(
                    session_id,
                    key,
                    "Receipt review/lease unavailable; this attempt was not dispatched.",
                ),
                false,
            )
        }
    };
    let request = Request::new(path, session_id, prompt, key);
    let claim_request = request.clone();
    // Store locks/file I/O must not block the HTTP/UI async executor.
    let begin = tokio::task::spawn_blocking(move || receipt_store::begin(&claim_request)).await;
    let nonce = match begin {
        Ok(Ok(Begin::Reserved(nonce))) => nonce,
        Ok(Ok(Begin::Replay(result))) => return (result, true),
        Ok(Ok(Begin::Rejected(result))) => return (result, false),
        _ => {
            return (
                unknown(
                    session_id,
                    key,
                    "Receipt unavailable; this attempt was not dispatched.",
                ),
                false,
            )
        }
    };
    let mut result = dispatch().await;
    result.idempotency_key = Some(key.to_owned());
    if result.session_id != session_id || result.status == TurnStatus::RetryLater {
        result = unknown(
            session_id,
            key,
            "Dispatch did not return a confirmed outcome.",
        );
    }
    let receipt = result.clone();
    let saved =
        tokio::task::spawn_blocking(move || receipt_store::finish(&request, &nonce, receipt)).await;
    if !matches!(saved, Ok(Ok(()))) {
        result = unknown(
            session_id,
            key,
            "Dispatch returned but its receipt could not be confirmed.",
        );
    }
    (result, false)
}

/// Queue retries have their own identity namespace, distinct from client keys.
/// An uncertain drain may be scheduled again, but never dispatched again.
pub(super) async fn run_queued_at<F, Fut>(
    root: &Path,
    session_id: &str,
    prompt: &str,
    item_id: &str,
    dispatch: F,
) -> TurnResult
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = TurnResult>,
{
    if item_id.trim().is_empty() {
        return TurnResult::fail(
            TurnStatus::Error,
            session_id,
            "queue item identity is missing; not dispatched",
        );
    }
    run_at(
        &root.join("session-api-queue-dispatch-receipts.json"),
        session_id,
        prompt,
        Some(item_id),
        dispatch,
    )
    .await
    .0
}
