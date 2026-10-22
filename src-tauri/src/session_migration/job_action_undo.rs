//! Durable exact cleanup intents committed with content Undo, and immutable
//! completion receipts. A missing completion resumes only its original intent.
use super::*;
use crate::session_migration::{
    jobs::{self, Receipt},
    publication_outcome::undo::cleanup,
};

#[path = "job_action_undo_resume.rs"]
mod resume;
pub(in crate::session_migration) use resume::resume;

#[path = "job_action_undo_recovery.rs"]
mod recovery;
pub(in crate::session_migration) use recovery::recover;

const PENDING_PREFIX: &str = "job_action_cleanup_pending:v1:";

fn pending_key(action: &PreparedAction) -> String {
    format!("{PENDING_PREFIX}{}", action.request.request_id)
}

const INTENT_DOMAIN: &str = "session-import-undo-cleanup-intent-v1";
const DONE_DOMAIN: &str = "session-import-undo-cleanup-done-v1";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Manifest {
    count: usize,
    root_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Intent {
    version: u32,
    request_id: String,
    job_id: String,
    context_digest: String,
    index: usize,
    candidate_id: String,
    identity_digest: String,
    target_digest: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Done {
    version: u32,
    intent_digest: String,
    error: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sealed<T> {
    body: T,
    mac: String,
}

fn address(action: &PreparedAction, index: usize, done: bool) -> String {
    format!(
        "job_action_cleanup{}:v1:{}:{index}",
        if done { "_done" } else { "" },
        action.request.request_id
    )
}

fn seal<T: Serialize>(
    action: &PreparedAction,
    domain: &str,
    body: T,
) -> Result<String, ImportError> {
    let mac = hex::encode(action.auth()?.sign(domain, canonical(&body)?.as_bytes()));
    let encoded = canonical(&Sealed { body, mac })?;
    if encoded.len() > RECORD_BYTES {
        return Err(unavailable());
    }
    Ok(encoded)
}

fn decode<T: Serialize + serde::de::DeserializeOwned>(
    action: &PreparedAction,
    domain: &str,
    encoded: &str,
) -> Result<T, ImportError> {
    if encoded.len() > RECORD_BYTES || encoded.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(unavailable());
    }
    let value: Sealed<T> = serde_json::from_str(encoded).map_err(|_| unavailable())?;
    let mac = hex::decode(&value.mac).map_err(|_| unavailable())?;
    if !action
        .auth()?
        .verify(domain, canonical(&value.body)?.as_bytes(), &mac)
    {
        return Err(unavailable());
    }
    Ok(value.body)
}

fn insert(tx: &Transaction<'_>, key: &str, encoded: &str) -> Result<(), ImportError> {
    let changes = tx.total_changes();
    let inserted = tx
        .execute("INSERT INTO meta(key,value) VALUES(?1,?2)", [key, encoded])
        .map_err(sqlite_storage_error)?;
    if inserted != 1
        || tx.total_changes().checked_sub(changes) != Some(1)
        || PreparedAction::read_at(tx, key)?.as_deref() != Some(encoded)
    {
        return Err(unavailable());
    }
    Ok(())
}

fn legacy_identity(receipt: &Receipt) -> Result<String, ImportError> {
    Ok(hash::sha256_bytes(canonical(receipt)?.as_bytes()))
}

/// Called after the signed Undo final check, before the SAME content COMMIT.
pub(in crate::session_migration) fn record(
    tx: &Transaction<'_>,
    action: &PreparedAction,
    job: &JobRecord,
    links: &[Receipt],
    changes_before: u64,
    expected_changes: u64,
) -> Result<(), ImportError> {
    if action.action != Action::Undo
        || tx.total_changes().checked_sub(changes_before) != Some(expected_changes)
        || links.len() > job.items.len()
    {
        return Err(unavailable());
    }
    let signed =
        PublicationProtocol::for_job_id(&job.job_id)? == PublicationProtocol::SignedOutcomeV1;
    let mut digests = Vec::with_capacity(links.len());
    let mut seen = std::collections::BTreeSet::new();
    for (index, receipt) in links.iter().enumerate() {
        if !seen.insert(&receipt.candidate_id) {
            return Err(unavailable());
        }
        let (identity_digest, target_digest) = if signed {
            let verified = cleanup::capture(tx, job, &receipt.candidate_id, action.crypto())?;
            if canonical(&verified.receipt)? != canonical(receipt)? {
                return Err(unavailable());
            }
            (verified.identity_digest, Some(verified.target_digest))
        } else {
            (legacy_identity(receipt)?, None)
        };
        let intent = Intent {
            version: 1,
            request_id: action.request.request_id.clone(),
            job_id: job.job_id.clone(),
            context_digest: context(job),
            index,
            candidate_id: receipt.candidate_id.clone(),
            identity_digest,
            target_digest,
        };
        let encoded = seal(action, INTENT_DOMAIN, intent)?;
        digests.push(hash::sha256_bytes(encoded.as_bytes()));
        insert(tx, &address(action, index, false), &encoded)?;
    }
    action.record_with_cleanup(
        tx,
        job,
        changes_before,
        expected_changes + links.len() as u64,
        Some(Manifest {
            count: links.len(),
            root_digest: hash::options_hash(&serde_json::json!(digests)),
        }),
    )?;
    if !links.is_empty() {
        let encoded = action.read(tx)?.ok_or_else(unavailable)?;
        insert(tx, &pending_key(action), &encoded)?;
    }
    Ok(())
}

struct Pending {
    intent: Intent,
    digest: String,
    done: Option<Done>,
}

fn pending(
    tx: &Transaction<'_>,
    action: &PreparedAction,
    job: &JobRecord,
) -> Result<Vec<Pending>, ImportError> {
    let record = action.saved(tx, job)?.ok_or_else(unavailable)?;
    let manifest = record.cleanup.ok_or_else(unavailable)?;
    if manifest.count > job.items.len() {
        return Err(unavailable());
    }
    let mut pending = Vec::with_capacity(manifest.count);
    let mut digests = Vec::with_capacity(manifest.count);
    let mut candidates = std::collections::BTreeSet::new();
    for index in 0..manifest.count {
        let raw =
            PreparedAction::read_at(tx, &address(action, index, false))?.ok_or_else(unavailable)?;
        let intent: Intent = decode(action, INTENT_DOMAIN, &raw)?;
        if intent.version != 1
            || intent.index != index
            || intent.job_id != job.job_id
            || intent.request_id != action.request.request_id
            || intent.context_digest != context(job)
            || !candidates.insert(intent.candidate_id.clone())
        {
            return Err(unavailable());
        }
        let digest = hash::sha256_bytes(raw.as_bytes());
        let done = PreparedAction::read_at(tx, &address(action, index, true))?
            .map(|raw| decode::<Done>(action, DONE_DOMAIN, &raw))
            .transpose()?;
        if done
            .as_ref()
            .is_some_and(|done| done.version != 1 || done.intent_digest != digest)
        {
            return Err(unavailable());
        }
        digests.push(digest.clone());
        pending.push(Pending {
            intent,
            digest,
            done,
        });
    }
    if hash::options_hash(&serde_json::json!(digests)) != manifest.root_digest {
        return Err(unavailable());
    }
    let marker = PreparedAction::read_at(tx, &pending_key(action))?;
    if pending.iter().any(|item| item.done.is_none()) {
        if marker != action.read(tx)? {
            return Err(unavailable());
        }
    } else if marker.is_some() {
        return Err(unavailable());
    }
    Ok(pending)
}
