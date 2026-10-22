//! Review + replacement claim commit together in the SAME atomic receipt file.
//! A crash anywhere after reservation leaves Unknown, never an automatic outbox resend.
use super::*;
use crate::session_api::receipt_review::{
    Decision, ReceiptRow, ReceiptSnapshot, ReviewAck, ReviewNote, ReviewRequest,
};
pub(in crate::session_api) enum ReviewBegin {
    Replay(ReviewAck),
    Reserved(ReviewAck, Request, String),
}
fn revision(entry: &Entry) -> String {
    let identity = serde_json::to_vec(&(
        &entry.result.session_id,
        &entry.key,
        &entry.prompt_sha256,
        &entry.claim_id,
    ))
    .expect("string tuple serializes");
    hex::encode(Sha256::digest(identity))
}
pub(super) fn validate_note(entry: &Entry, version: Option<u32>) -> Result<(), String> {
    if let Some(note) = &entry.review {
        if version != Some(VERSION)
            || entry.result.status != TurnStatus::Unknown
            || entry.prompt_sha256.is_none()
            || note.reviewed_at < 0
        {
            return Err("invalid receipt review".into());
        }
        match (
            &note.replacement_key,
            &note.replacement_claim,
            note.decision,
        ) {
            (None, None, Decision::Dismiss) => {}
            (Some(key), Some(claim), Decision::Retry)
                if key != &entry.key
                    && key.starts_with("r-ext-")
                    && uuid::Uuid::parse_str(&key[6..]).is_ok()
                    && uuid::Uuid::parse_str(claim).is_ok() => {}
            _ => return Err("invalid receipt replacement identity".into()),
        }
    }
    Ok(())
}
pub(super) fn validate_links(store: &Store) -> Result<(), String> {
    for entry in &store.entries {
        if let Some(note) = &entry.review {
            if let Some(key) = &note.replacement_key {
                let child = store
                    .entries
                    .iter()
                    .find(|c| &c.key == key && c.result.session_id == entry.result.session_id)
                    .ok_or("review replacement missing")?;
                if child.claim_id != note.replacement_claim
                    || child.prompt_sha256 != entry.prompt_sha256
                {
                    return Err("review replacement changed".into());
                }
            }
        }
    }
    Ok(())
}
pub(super) fn make_room(store: &mut Store) -> Result<(), String> {
    if store.entries.len() < CAP {
        return Ok(());
    }
    let position = store
        .entries
        .iter()
        .position(|entry| {
            entry.state != Some(ReceiptState::Pending)
                && entry.result.status != TurnStatus::Unknown
                && !store.entries.iter().any(|parent| {
                    parent.result.session_id == entry.result.session_id
                        && parent
                            .review
                            .as_ref()
                            .and_then(|n| n.replacement_key.as_ref())
                            == Some(&entry.key)
                })
        })
        .ok_or("unresolved receipt capacity reached")?;
    store.entries.remove(position);
    Ok(())
}
fn ack(store: &Store, entry: &Entry) -> Result<ReviewAck, String> {
    let note = entry.review.as_ref().ok_or("missing review")?;
    let child = note.replacement_key.as_ref().and_then(|key| {
        store
            .entries
            .iter()
            .find(|c| &c.key == key && c.result.session_id == entry.result.session_id)
    });
    Ok(ReviewAck {
        version: 1,
        session_id: entry.result.session_id.clone(),
        key: entry.key.clone(),
        revision: revision(entry),
        decision: note.decision,
        replacement_key: note.replacement_key.clone(),
        replacement_status: child.map(|c| c.result.status.clone()),
        queue_item_id: child.and_then(|c| c.result.queue_item_id.clone()),
    })
}
pub(in crate::session_api) fn snapshot(path: &Path) -> Result<ReceiptSnapshot, String> {
    crate::store_lock::with_exclusive_lock(path, || {
        let store = load(path)?;
        let items = store
            .entries
            .iter()
            .filter(|e| e.result.status == TurnStatus::Unknown)
            .map(|entry| {
                Ok(ReceiptRow {
                    session_id: entry.result.session_id.clone(),
                    key: entry.key.clone(),
                    revision: revision(entry),
                    can_review: entry.prompt_sha256.is_some()
                        && entry.review.is_none()
                        && entry.result.session_id.trim() == entry.result.session_id
                        && entry.result.session_id != "__draft__"
                        && !entry.result.session_id.starts_with("imp:")
                        && entry.key.trim() == entry.key,
                    review: if entry.review.is_some() {
                        Some(ack(&store, entry)?)
                    } else {
                        None
                    },
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(ReceiptSnapshot { version: 1, items })
    })
}
pub(in crate::session_api) fn review(
    path: &Path,
    request: &ReviewRequest,
) -> Result<ReviewBegin, String> {
    crate::store_lock::with_exclusive_lock(path, || {
        let mut store = load(path).map_err(|_| "SESSION_RECEIPT_UNAVAILABLE")?;
        let position = store
            .entries
            .iter()
            .position(|e| e.key == request.key && e.result.session_id == request.session_id)
            .ok_or("SESSION_RECEIPT_CHANGED")?;
        let entry = &store.entries[position];
        if entry.result.status != TurnStatus::Unknown
            || revision(entry) != request.revision
            || entry.prompt_sha256.is_none()
        {
            return Err("SESSION_RECEIPT_CHANGED".into());
        }
        if request.decision == Decision::Retry
            && entry.prompt_sha256.as_deref() != Some(&fingerprint(&request.prompt))
        {
            return Err("SESSION_RECEIPT_PROMPT_MISMATCH".into());
        }
        if let Some(note) = &entry.review {
            if note.decision != request.decision {
                return Err("SESSION_RECEIPT_CHANGED".into());
            }
            return Ok(ReviewBegin::Replay(ack(&store, entry)?));
        }
        let replacement = if request.decision == Decision::Retry {
            make_room(&mut store).map_err(|_| "SESSION_RECEIPT_CAPACITY")?;
            Some((
                format!("r-ext-{}", uuid::Uuid::new_v4()),
                uuid::Uuid::new_v4().to_string(),
            ))
        } else {
            None
        };
        let index = store
            .entries
            .iter()
            .position(|e| e.key == request.key && e.result.session_id == request.session_id)
            .ok_or("SESSION_RECEIPT_CHANGED")?;
        let entry = &mut store.entries[index];
        entry.review = Some(ReviewNote {
            decision: request.decision,
            reviewed_at: chrono::Utc::now().timestamp_millis(),
            replacement_key: replacement.as_ref().map(|v| v.0.clone()),
            replacement_claim: replacement.as_ref().map(|v| v.1.clone()),
        });
        if let Some((key, nonce)) = &replacement {
            store.entries.push(Entry {
                key: key.clone(),
                result: unknown(
                    &request.session_id,
                    key,
                    "Human-authorized replacement claim is pending.",
                ),
                prompt_sha256: Some(fingerprint(&request.prompt)),
                claim_id: Some(nonce.clone()),
                state: Some(ReceiptState::Pending),
                review: None,
            });
        }
        validate_links(&store).map_err(|_| "SESSION_RECEIPT_UNAVAILABLE")?;
        save(path, &mut store).map_err(|_| "SESSION_RECEIPT_UNAVAILABLE")?;
        let result = ack(&store, &store.entries[index])?;
        Ok(match replacement {
            Some((key, nonce)) => ReviewBegin::Reserved(
                result,
                Request::new(path, &request.session_id, &request.prompt, &key),
                nonce,
            ),
            None => ReviewBegin::Replay(result),
        })
    })
}
