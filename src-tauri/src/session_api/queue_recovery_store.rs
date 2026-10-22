//! Explicit manual disposition. Never pretends an uncertain attempt was unsent.
use super::*;
use crate::session_api::queue_recovery::{Decision, RecoveryAck};
pub(in crate::session_api) fn recover(
    path: &Path,
    expected: &Item,
    decision: Decision,
) -> Result<RecoveryAck, String> {
    validate_item(expected, Some(VERSION))?;
    crate::store_lock::with_exclusive_lock(path, || {
        let mut store = load(path)?;
        let status = match decision {
            Decision::Dismiss => Status::Reviewed,
            Decision::Retry => Status::RetryAuthorized,
        };
        let Some(index) = store
            .items
            .iter()
            .position(|i| i.item_id == expected.item_id)
        else {
            let previous = store
                .outcomes
                .iter()
                .find(|o| o.status == status && o.matches_item(expected))
                .ok_or("SESSION_QUEUE_CHANGED")?;
            return Ok(RecoveryAck::new(
                expected,
                decision,
                previous.replacement_id.clone(),
            ));
        };
        let current = &store.items[index];
        if !current.same_payload(expected) || current.delivery.is_none() {
            return Err("SESSION_QUEUE_CHANGED".into());
        }
        let replacement_id = if decision == Decision::Retry {
            let id = format!("q-ext-{}", uuid::Uuid::new_v4());
            // Preserve the accepted order, but use a distinct attempt/receipt identity.
            store.items[index] = Item::new(
                &expected.session_id,
                &id,
                &expected.prompt,
                chrono::Utc::now().timestamp_millis(),
            );
            Some(id)
        } else {
            store.items.remove(index);
            None
        };
        record(
            &mut store,
            Outcome::new(expected, status, replacement_id.clone()),
        );
        save(path, &mut store)?;
        Ok(RecoveryAck::new(expected, decision, replacement_id))
    })
}
