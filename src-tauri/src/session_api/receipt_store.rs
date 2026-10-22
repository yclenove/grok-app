//! Locked, atomic, versioned receipts. Never evict an unresolved claim.
use super::idempotency::unknown;
use super::{TurnResult, TurnStatus};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const VERSION: u32 = 3;
#[path = "receipt_review_store.rs"]
mod review;
pub(super) use review::{review, snapshot, ReviewBegin};
pub(super) const CAP: usize = 200;

#[derive(Clone)]
pub(super) struct Request {
    path: PathBuf,
    session_id: String,
    key: String,
    fingerprint: String,
}
impl Request {
    pub(super) fn new(path: &Path, session_id: &str, prompt: &str, key: &str) -> Self {
        Self {
            path: path.to_owned(),
            session_id: session_id.to_owned(),
            key: key.to_owned(),
            fingerprint: fingerprint(prompt),
        }
    }
    fn matches(&self, entry: &Entry) -> bool {
        entry.key == self.key && entry.result.session_id == self.session_id
    }
}
fn fingerprint(prompt: &str) -> String {
    hex::encode(Sha256::digest(prompt.trim().as_bytes()))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    #[serde(default)]
    version: Option<u32>,
    entries: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    key: String,
    result: TurnResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prompt_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    claim_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state: Option<ReceiptState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review: Option<super::receipt_review::ReviewNote>,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ReceiptState {
    Pending,
    Completed,
}

pub(super) enum Begin {
    Reserved(String),
    Replay(TurnResult),
    Rejected(TurnResult),
}

fn load(path: &Path) -> Result<Store, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if fs::symlink_metadata(path).is_ok() {
                return Err("unreadable existing receipt path".into());
            }
            return Ok(Store {
                version: Some(VERSION),
                entries: Vec::new(),
            });
        }
        Err(error) => return Err(error.to_string()),
    };
    let raw: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    for entry in raw
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .ok_or("missing receipts")?
    {
        let result = entry
            .get("result")
            .and_then(serde_json::Value::as_object)
            .ok_or("missing receipt result")?;
        if result.keys().any(|key| {
            !matches!(
                key.as_str(),
                "ok" | "status"
                    | "sessionId"
                    | "idempotencyKey"
                    | "message"
                    | "queueItemId"
                    | "queuedPrompt"
            )
        }) {
            return Err("unsupported receipt result fields".into());
        }
    }
    let store: Store = serde_json::from_value(raw).map_err(|error| error.to_string())?;
    if !matches!(store.version, None | Some(1) | Some(2) | Some(VERSION))
        || store.entries.len() > CAP
    {
        return Err("unsupported receipt store".into());
    }
    let mut identities = HashSet::new();
    for entry in &store.entries {
        if entry.key.trim().is_empty()
            || entry.result.session_id.is_empty()
            || !identities.insert((&entry.result.session_id, &entry.key))
        {
            return Err("invalid or duplicate receipt identity".into());
        }
        validate_entry(entry, store.version)?;
    }
    review::validate_links(&store)?;
    Ok(store)
}

fn validate_entry(entry: &Entry, version: Option<u32>) -> Result<(), String> {
    review::validate_note(entry, version)?;
    let result = &entry.result;
    if result
        .idempotency_key
        .as_ref()
        .is_some_and(|key| key != &entry.key)
    {
        return Err("receipt key mismatch".into());
    }
    match (&entry.prompt_sha256, &entry.claim_id, entry.state) {
        (None, None, None) => return Ok(()), // Legacy records are preserved, not replayed.
        (Some(hash), Some(nonce), Some(_))
            if matches!(version, Some(2) | Some(VERSION))
                && hash.len() == 64
                && hex::decode(hash).is_ok()
                && !nonce.is_empty()
                && result.idempotency_key.as_ref() == Some(&entry.key) => {}
        _ => return Err("incomplete receipt identity".into()),
    }
    let success = matches!(result.status, TurnStatus::TurnStarted | TurnStatus::Queued);
    if success != result.ok || result.status == TurnStatus::RetryLater {
        return Err("invalid receipt result".into());
    }
    if entry.state == Some(ReceiptState::Pending) && result.status != TurnStatus::Unknown {
        return Err("invalid pending receipt".into());
    }
    if result.status == TurnStatus::Queued {
        if result.queue_item_id.as_ref().is_none_or(|id| id.is_empty())
            || result
                .queued_prompt
                .as_ref()
                .is_none_or(|prompt| Some(fingerprint(prompt)) != entry.prompt_sha256)
        {
            return Err("queued receipt payload mismatch".into());
        }
    } else if result.queue_item_id.is_some() || result.queued_prompt.is_some() {
        return Err("unexpected queue payload".into());
    }
    Ok(())
}

pub(super) fn begin(request: &Request) -> Result<Begin, String> {
    crate::store_lock::with_exclusive_lock(&request.path, || {
        let mut store = load(&request.path)?;
        if let Some(entry) = store.entries.iter().find(|entry| request.matches(entry)) {
            let Some(hash) = &entry.prompt_sha256 else {
                return Ok(Begin::Rejected(unknown(
                    &request.session_id,
                    &request.key,
                    "Legacy receipt cannot verify this prompt.",
                )));
            };
            if hash != &request.fingerprint {
                let mut conflict = TurnResult::fail(
                    TurnStatus::IdempotencyConflict,
                    &request.session_id,
                    "This session/key is already bound to a different prompt.",
                );
                conflict.idempotency_key = Some(request.key.clone());
                return Ok(Begin::Rejected(conflict));
            }
            if entry.state == Some(ReceiptState::Pending)
                || entry.result.status == TurnStatus::Unknown
            {
                return Ok(Begin::Rejected(entry.result.clone()));
            }
            return Ok(Begin::Replay(entry.result.clone()));
        }
        review::make_room(&mut store)?;
        let nonce = uuid::Uuid::new_v4().to_string();
        store.entries.push(Entry {
            key: request.key.clone(),
            result: unknown(
                &request.session_id,
                &request.key,
                "Dispatch claim is pending.",
            ),
            prompt_sha256: Some(request.fingerprint.clone()),
            claim_id: Some(nonce.clone()),
            state: Some(ReceiptState::Pending),
            review: None,
        });
        save(&request.path, &mut store)?;
        Ok(Begin::Reserved(nonce))
    })
}

pub(super) fn finish(request: &Request, nonce: &str, result: TurnResult) -> Result<(), String> {
    crate::store_lock::with_exclusive_lock(&request.path, || {
        let mut store = load(&request.path)?;
        let entry = store
            .entries
            .iter_mut()
            .find(|entry| request.matches(entry))
            .ok_or("dispatch claim missing")?;
        if entry.review.is_some()
            || entry.claim_id.as_deref() != Some(nonce)
            || entry.prompt_sha256.as_deref() != Some(&request.fingerprint)
            || entry.state != Some(ReceiptState::Pending)
        {
            return Err("dispatch claim changed".into());
        }
        if result.session_id != request.session_id {
            return Err("dispatch session mismatch".into());
        }
        entry.state = Some(ReceiptState::Completed);
        entry.result = result;
        validate_entry(entry, Some(VERSION))?;
        save(&request.path, &mut store)
    })
}

fn save(path: &Path, store: &mut Store) -> Result<(), String> {
    store.version = Some(VERSION);
    let bytes = serde_json::to_vec_pretty(store).map_err(|error| error.to_string())?;
    // Unique create_new prevents overwriting anyone else's stale temp file.
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(|error| error.to_string())?;
    let written = (|| -> std::io::Result<()> {
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
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written.map_err(|error| error.to_string())
}
