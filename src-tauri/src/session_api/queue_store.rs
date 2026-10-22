//! Durable external queue: retain payload until confirmed delivery.
//! A pending/held claim is never implicitly reset on restart.
use super::{
    queue_outcomes::{Outcome, Resolved, Status},
    TurnResult, TurnStatus, QUEUE_CAP,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io::Write, path::Path};

const VERSION: u32 = 4;
#[path = "queue_recovery_store.rs"]
mod recovery;
pub(super) use recovery::recover;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Item {
    pub session_id: String,
    pub item_id: String,
    pub prompt: String,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<Delivery>,
}
impl Item {
    pub(super) fn new(session_id: &str, item_id: &str, prompt: &str, created_at: i64) -> Self {
        Self {
            session_id: session_id.into(),
            item_id: item_id.into(),
            prompt: prompt.into(),
            created_at,
            delivery: None,
        }
    }
    pub(super) fn display_state(&self) -> &'static str {
        match self.delivery.as_ref().map(|d| d.state) {
            None => "queued",
            Some(DeliveryState::Pending) => "pending",
            Some(DeliveryState::Held) => "held",
        }
    }
    fn same_payload(&self, other: &Self) -> bool {
        self.session_id == other.session_id
            && self.item_id == other.item_id
            && self.prompt == other.prompt
            && self.created_at == other.created_at
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Delivery {
    nonce: String,
    state: DeliveryState,
    detail: String,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DeliveryState {
    Pending,
    Held,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    #[serde(default)]
    version: Option<u32>,
    items: Vec<Item>,
    #[serde(default)]
    outcomes: Vec<Outcome>,
}
fn validate_item(item: &Item, version: Option<u32>) -> Result<(), String> {
    if item.session_id == "__draft__"
        || item.session_id.starts_with("imp:")
        || item.session_id.trim().is_empty()
        || item.session_id != item.session_id.trim()
        || item.item_id.trim().is_empty()
        || item.item_id != item.item_id.trim()
        || item.prompt.trim().is_empty()
    {
        return Err("invalid queue identity or empty prompt".into());
    }
    crate::session_migration::reject_imported_history_id(Some(&item.session_id))?;
    if let Some(delivery) = &item.delivery {
        if !matches!(version, Some(2) | Some(3) | Some(VERSION))
            || uuid::Uuid::parse_str(&delivery.nonce).is_err()
            || delivery.detail.trim().is_empty()
        {
            return Err("invalid queue delivery claim".into());
        }
    }
    Ok(())
}
fn load(path: &Path) -> Result<Store, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if fs::symlink_metadata(path).is_ok() {
                return Err("unreadable existing queue path".into());
            }
            return Ok(Store {
                version: Some(VERSION),
                items: Vec::new(),
                outcomes: Vec::new(),
            });
        }
        Err(e) => return Err(e.to_string()),
    };
    let store: Store = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !matches!(store.version, None | Some(2) | Some(3) | Some(VERSION))
        || store.items.len() > QUEUE_CAP
        || store.outcomes.len() > QUEUE_CAP
        || (!matches!(store.version, Some(3) | Some(VERSION)) && raw.get("outcomes").is_some())
        || (matches!(store.version, Some(3) | Some(VERSION)) && raw.get("outcomes").is_none())
    {
        return Err("unsupported queue version or capacity".into());
    }
    let mut ids = HashSet::new();
    for item in &store.items {
        validate_item(item, store.version)?;
        if !ids.insert(&item.item_id) {
            return Err("duplicate queue identity".into());
        }
    }
    for outcome in &store.outcomes {
        outcome.validate()?;
        if store.version != Some(VERSION)
            && matches!(outcome.status, Status::Reviewed | Status::RetryAuthorized)
        {
            return Err("manual recovery requires queue version 4".into());
        }
        if !ids.insert(&outcome.item_id) {
            return Err("duplicate queue outcome identity".into());
        }
    }
    Ok(store)
}
pub(super) fn list(path: &Path) -> Result<Vec<Item>, String> {
    crate::store_lock::with_exclusive_lock(path, || Ok(load(path)?.items))
}
pub(super) fn insert(path: &Path, item: Item) -> Result<(), String> {
    validate_item(&item, Some(VERSION))?;
    if item.delivery.is_some() {
        return Err("cannot insert a preclaimed item".into());
    }
    crate::store_lock::with_exclusive_lock(path, || {
        let mut store = load(path)?;
        if let Some(existing) = store.items.iter().find(|e| e.item_id == item.item_id) {
            if existing.same_payload(&item) {
                return Ok(());
            }
            return Err("queue identity already binds another payload".into());
        }
        if store.outcomes.iter().any(|e| e.item_id == item.item_id) {
            return Err("queue identity already completed".into());
        }
        if store.items.len() >= QUEUE_CAP {
            return Err("queue capacity reached; no accepted prompt was removed".into());
        }
        store.items.push(item);
        save(path, &mut store)
    })
}
/// Claim only the same, still-ready first item for this session.
pub(super) fn claim(path: &Path, expected: &Item) -> Result<Option<String>, String> {
    validate_item(expected, Some(VERSION))?;
    crate::store_lock::with_exclusive_lock(path, || {
        let mut store = load(path)?;
        let Some(item) = store
            .items
            .iter_mut()
            .find(|i| i.session_id == expected.session_id)
        else {
            return Ok(None);
        };
        if item.item_id != expected.item_id || item.delivery.is_some() {
            return Ok(None);
        }
        if !item.same_payload(expected) {
            return Err("queue payload changed before claim".into());
        }
        let nonce = uuid::Uuid::new_v4().to_string();
        item.delivery = Some(Delivery {
            nonce: nonce.clone(),
            state: DeliveryState::Pending,
            detail: "Dispatch outcome pending; do not automatically resend.".into(),
        });
        save(path, &mut store)?;
        Ok(Some(nonce))
    })
}
/// Return true only when an accepted dispatch or durable replacement is confirmed.
pub(super) fn finish(
    path: &Path,
    expected: &Item,
    nonce: &str,
    result: &TurnResult,
) -> Result<bool, String> {
    crate::store_lock::with_exclusive_lock(path, || {
        let mut store = load(path)?;
        let index = store
            .items
            .iter()
            .position(|i| i.item_id == expected.item_id)
            .ok_or("queue item missing")?;
        let item = &store.items[index];
        if !item.same_payload(expected)
            || !item
                .delivery
                .as_ref()
                .is_some_and(|d| d.nonce == nonce && d.state == DeliveryState::Pending)
        {
            return Err("queue claim changed".into());
        }
        let same_session = result.session_id == expected.session_id;
        let started = result.status == TurnStatus::TurnStarted
            && result.queue_item_id.is_none()
            && result.queued_prompt.is_none();
        let handed_off = result.status == TurnStatus::Queued
            && result
                .queued_prompt
                .as_deref()
                .is_some_and(|p| p == expected.prompt)
            && result.queue_item_id.as_ref().is_some_and(|id| {
                id != &expected.item_id
                    && store.items.iter().any(|replacement| {
                        &replacement.item_id == id
                            && replacement.session_id == expected.session_id
                            && replacement.prompt == expected.prompt
                    })
            });
        let confirmed = result.ok && same_session && (started || handed_off);
        if confirmed {
            if handed_off {
                // A busy race creates a replacement at the tail. Move it into
                // the original slot atomically so younger prompts cannot pass it.
                let replacement_index = store
                    .items
                    .iter()
                    .position(|item| Some(&item.item_id) == result.queue_item_id.as_ref())
                    .ok_or("queue replacement missing")?;
                if replacement_index <= index {
                    return Err("queue replacement is not after its original claim".into());
                }
                let replacement = store.items.remove(replacement_index);
                store.items[index] = replacement;
            } else {
                store.items.remove(index);
            }
            record(
                &mut store,
                Outcome::new(
                    expected,
                    if handed_off {
                        Status::HandedOff
                    } else {
                        Status::Started
                    },
                    if handed_off {
                        result.queue_item_id.clone()
                    } else {
                        None
                    },
                ),
            );
        } else {
            let delivery = store.items[index]
                .delivery
                .as_mut()
                .ok_or("queue claim missing")?;
            delivery.state = DeliveryState::Held;
            delivery.detail = "Delivery was not confirmed. Inspect the original session; no automatic redispatch.".into();
        }
        save(path, &mut store)?;
        Ok(confirmed)
    })
}
fn record(store: &mut Store, outcome: Outcome) {
    if store.outcomes.len() == QUEUE_CAP {
        store.outcomes.remove(0);
    }
    store.outcomes.push(outcome);
}
/// Cancellation and dispatch claim share one lock and one atomic store file.
pub(super) fn cancel(path: &Path, expected: &Item) -> Result<(), String> {
    validate_item(expected, Some(VERSION))?;
    crate::store_lock::with_exclusive_lock(path, || {
        let mut store = load(path)?;
        let Some(index) = store
            .items
            .iter()
            .position(|i| i.item_id == expected.item_id)
        else {
            return if store
                .outcomes
                .iter()
                .any(|o| o.status == Status::Cancelled && o.matches_item(expected))
            {
                Ok(())
            } else {
                Err("SESSION_QUEUE_CHANGED".into())
            };
        };
        let item = &store.items[index];
        if !item.same_payload(expected) {
            return Err("SESSION_QUEUE_CHANGED".into());
        }
        if item.delivery.is_some() {
            return Err("SESSION_QUEUE_CLAIMED".into());
        }
        store.items.remove(index);
        record(&mut store, Outcome::new(expected, Status::Cancelled, None));
        save(path, &mut store)
    })
}
/// Missing/evicted lineage never means delivered and never authorizes a resend.
pub(super) fn resolve(
    path: &Path,
    session: &str,
    id: &str,
    prompt: &str,
) -> Result<Resolved, String> {
    crate::store_lock::with_exclusive_lock(path, || {
        let store = load(path)?;
        let mut current = id;
        let mut visited = HashSet::new();
        while visited.insert(current) {
            if let Some(item) = store.items.iter().find(|i| i.item_id == current) {
                return Ok(
                    if item.session_id == session
                        && item.prompt == prompt
                        && item.delivery.is_none()
                    {
                        Resolved::Queued(item.clone())
                    } else {
                        Resolved::Unknown
                    },
                );
            }
            let Some(outcome) = store.outcomes.iter().find(|o| o.item_id == current) else {
                break;
            };
            if !outcome.matches(session, prompt) {
                break;
            }
            match outcome.status {
                Status::Started => return Ok(Resolved::Started),
                Status::Cancelled => return Ok(Resolved::Cancelled),
                // Human disposition is not proof that the original attempt succeeded.
                Status::Reviewed | Status::RetryAuthorized => return Ok(Resolved::Unknown),
                Status::HandedOff => {
                    current = outcome
                        .replacement_id
                        .as_deref()
                        .ok_or("invalid queue lineage")?
                }
            }
        }
        Ok(Resolved::Unknown)
    })
}
fn save(path: &Path, store: &mut Store) -> Result<(), String> {
    store.version = Some(VERSION);
    let bytes = serde_json::to_vec_pretty(store).map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(|e| e.to_string())?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        #[cfg(unix)]
        if let Some(parent) = path.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result.map_err(|e| e.to_string())
}
