//! Exact catalog outcomes. A projected path or a candidate alone is not proof.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use super::super::super::{
    errors::ImportError,
    hash, jobs,
    publication_crypto::{JobCrypto, JobCryptos},
};
use super::super::{input_reader, DurableInput};
use super::{deferred, receipt};

struct Input {
    candidate_digest: String,
    phase: String,
    digest: String,
    preview_id: String,
    binding_digest: String,
}

struct Accepted {
    job: jobs::JobRecord,
    inputs: BTreeMap<PathBuf, Input>,
}

#[cfg(test)]
#[path = "job_retention_crypto_tests.rs"]
mod crypto_tests;

fn accepted(
    conn: &Connection,
    job_id: &str,
    crypto: &JobCrypto,
) -> Result<Option<Accepted>, ImportError> {
    if conn.is_autocommit() {
        return Err(deferred("accepted outcome requires its caller's snapshot"));
    }
    // Presence is not a body fetch. In particular a corrupt association must
    // reach its bounded input/proof/header owners before JobRecord decoding.
    let present: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM jobs WHERE job_id=?1)",
            [job_id],
            |row| row.get(0),
        )
        .map_err(deferred)?;
    if !present {
        return Ok(None);
    }
    let mut stmt = conn
        .prepare("SELECT candidate_id,phase,typeof(result_json) FROM job_items WHERE job_id=?1")
        .map_err(deferred)?;
    let rows = stmt
        .query_map([job_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(deferred)?;
    let mut inputs = BTreeMap::new();
    let mut phases = BTreeMap::new();
    let mut associated = 0u64;
    for row in rows {
        let (candidate_id, phase, kind) = row.map_err(deferred)?;
        if kind != "text" || phases.insert(candidate_id.clone(), phase.clone()).is_some() {
            return Err(deferred("accepted input types or identities are invalid"));
        }
        // Point load through the existing codec. Ordinary v2 keeps its original
        // large-input capacity; no global SQL CAST/json_extract or path shortcut.
        let json = input_reader::load(conn, job_id, &candidate_id)?;
        let value = DurableInput::parse(&json)?;
        if let Some(association) = &value.identity_association {
            let record = super::super::super::registration_provenance_store::get(
                conn,
                &association.confirmation.import_id,
            )?;
            let Some(super::super::super::registration_provenance::ProofRecord::Association(proof)) =
                record
            else {
                return Err(deferred(
                    "association retention is missing its accepted proof",
                ));
            };
            proof.verify_retained_input(conn, association, &value.input_identity()?)?;
            associated += 1;
        }
        // Association context was authenticated against its exact canonical
        // input above. Ordinary validate_context was and remains a no-op.
        if value.candidate_id != candidate_id {
            return Err(deferred("accepted job/input membership disagrees"));
        }
        let preview_id = value.selection_set.single_entry()?.preview_id.clone();
        let binding_digest = hash::options_hash(&serde_json::json!([
            value.candidate_id,
            value.source_id,
            value.root_path,
            value.legacy_id,
            value.source_member,
            preview_id,
            value.preview_digest,
            value.preview_file_hashes
        ]));
        let input = Input {
            candidate_digest: hash::sha256_bytes(candidate_id.as_bytes()),
            phase,
            digest: hash::sha256_bytes(json.as_bytes()),
            preview_id,
            binding_digest,
        };
        // Keep only metadata digests, not every adapter/title/body in the cache.
        if inputs.insert(value.preview_path, input).is_some() {
            return Err(deferred("accepted preview membership is duplicated"));
        }
    }
    let prefix = format!("registration_association_accepted:{job_id}:");
    let witnesses: u64 = conn
        .query_row(
            // meta.key uses BINARY ordering. Replacing the terminal ':' by ';'
            // gives the exact prefix range without scanning unrelated markers.
            "SELECT COUNT(*) FROM meta WHERE key>=?1 AND key<?2",
            params![
                prefix,
                format!("registration_association_accepted:{job_id};")
            ],
            |row| row.get(0),
        )
        .map_err(deferred)?;
    // A missing/downgraded row cannot make present accepted association
    // metadata look like an ordinary owner. This is refusal, not a grant
    // derived from the marker count; every association above verifies proof.
    if witnesses != associated {
        return Err(deferred(
            "association acceptance witnesses/input set disagree",
        ));
    }
    // Only now is an association header known to have passed its point byte
    // bound. Ordinary jobs retain their original complete-body capacity.
    let job = jobs::get_job_with_crypto(conn, job_id, crypto)?
        .ok_or_else(|| deferred("accepted input owner is missing its job"))?;
    let columns: (String, String, String, u32, String) = conn
        .query_row(
            "SELECT request_id,plan_hash,state,state_version,created_at FROM jobs WHERE job_id=?1",
            [job_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(deferred)?;
    if job.job_id != job_id
        || columns
            != (
                job.request_id.clone(),
                job.plan_hash.clone(),
                job.state.as_str().into(),
                job.state_version,
                job.created_at.clone(),
            )
    {
        return Err(deferred("accepted job columns/header disagree"));
    }
    let mut members = std::collections::BTreeSet::new();
    for item in &job.items {
        if !members.insert(&item.candidate_id)
            || phases.get(&item.candidate_id).map(String::as_str) != Some(item.phase.as_str())
        {
            return Err(deferred("accepted job/input membership disagrees"));
        }
    }
    if inputs.len() != job.items.len()
        || (job.item_count != 0 && job.item_count as usize != inputs.len())
    {
        return Err(deferred("accepted job input set is incomplete"));
    }
    Ok(Some(Accepted { job, inputs }))
}

/// True only for this complete accepted epoch. No job/header/item/receipt row
/// may survive if its epoch is classified unaccepted.
pub(super) fn committed(
    conn: &Connection,
    recovery: &receipt::Receipt,
    leases: &[(PathBuf, jobs::PreviewLease)],
    cryptos: &JobCryptos,
) -> Result<bool, ImportError> {
    if conn.is_autocommit() {
        return Err(deferred("recovery outcome requires its caller's snapshot"));
    }
    let witness = recovery.accepted_witness(conn)?;
    let Some(accepted) = accepted(conn, &recovery.job_id, cryptos.get(&recovery.job_id)?)? else {
        if witness {
            return Err(deferred("accepted recovery witness is missing its job"));
        }
        for table in ["job_items", "receipts"] {
            let count: u64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE job_id=?1"),
                    [&recovery.job_id],
                    |row| row.get(0),
                )
                .map_err(deferred)?;
            if count != 0 {
                return Err(deferred("partial acceptance survives without its job"));
            }
        }
        return Ok(false);
    };
    if !witness
        || receipt::context_digest(&accepted.job) != recovery.context_digest
        || accepted.inputs.len() != recovery.entries.len()
        || leases.len() != recovery.entries.len()
        || recovery
            .entries
            .iter()
            .zip(leases)
            .any(|(entry, (path, lease))| {
                !accepted.inputs.get(path).is_some_and(|input| {
                    input.candidate_digest == entry.candidate_digest
                        && input.digest == entry.input_digest
                        && *path == receipt::preview_path(entry)
                        && input.preview_id == entry.preview_id
                        && matches_preview(input, lease)
                })
            })
    {
        return Err(deferred("accepted epoch differs from its recovery receipt"));
    }
    Ok(true)
}

fn matches_preview(input: &Input, lease: &jobs::PreviewLease) -> bool {
    input.binding_digest
        == hash::options_hash(&serde_json::json!([
            lease.candidate_id,
            lease.source_id,
            lease.root_path,
            lease.legacy_id,
            lease.source_member,
            lease.preview_id,
            lease.snapshot_digest,
            lease.file_hashes
        ]))
}

/// An unaccepted epoch must not clear a foreign/partially renamed accepted
/// reference. Scan streaming references only on this uncommon recovery path;
/// a matching path still requires full accepted membership authentication.
pub(super) fn foreign_owners(
    conn: &Connection,
    leases: &[(PathBuf, jobs::PreviewLease)],
) -> Result<(), ImportError> {
    let previews: BTreeMap<_, _> = leases.iter().map(|(path, lease)| (path, lease)).collect();
    let mut stmt = conn
        .prepare("SELECT job_id,candidate_id FROM job_items")
        .map_err(deferred)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(deferred)?;
    for row in rows {
        let (job_id, candidate_id) = row.map_err(deferred)?;
        let path = input_reader::retained_preview_path(conn, &job_id, &candidate_id)?;
        if previews
            .get(&path)
            .is_some_and(|lease| lease.candidate_id != candidate_id)
        {
            // A candidate mismatch for the same immutable preview is foreign,
            // not permission to repair a flag or retire its epoch metadata.
            return Err(deferred(
                "foreign accepted input references recovery preview",
            ));
        }
    }
    Ok(())
}

/// None = no exact accepted owner; Some(true) = at least one retryable owner;
/// Some(false) = every authenticated exact owner has finished this input.
/// The candidate index only selects possible jobs; authenticate their complete
/// headers, memberships and input bytes before making any ownership decision.
pub(super) fn owner(
    conn: &Connection,
    path: &Path,
    lease: &jobs::PreviewLease,
) -> Result<Option<bool>, ImportError> {
    let cryptos = JobCryptos::prepare(conn, std::slice::from_ref(&lease.candidate_id), &[])?;
    let tx = conn.unchecked_transaction().map_err(deferred)?;
    owner_cached(&tx, path, lease, &cryptos, &mut BTreeMap::new())
}

pub(super) fn owners(
    conn: &Connection,
    leases: &[(PathBuf, jobs::PreviewLease)],
    cryptos: &JobCryptos,
) -> Result<Vec<Option<bool>>, ImportError> {
    owner_results_in_snapshot(conn, leases, cryptos)?
        .into_iter()
        .collect()
}

/// Reclamation can defer one damaged job while safely completing a different,
/// fully authenticated job. Reconciliation uses the strict owners() wrapper.
pub(super) fn owner_results(
    conn: &Connection,
    leases: &[(PathBuf, jobs::PreviewLease)],
) -> Result<Vec<Result<Option<bool>, ImportError>>, ImportError> {
    let candidates = leases
        .iter()
        .map(|(_, lease)| lease.candidate_id.clone())
        .collect::<Vec<_>>();
    let cryptos = JobCryptos::prepare(conn, &candidates, &[])?;
    let tx = conn.unchecked_transaction().map_err(deferred)?;
    owner_results_in_snapshot(&tx, leases, &cryptos)
}

fn owner_results_in_snapshot(
    conn: &Connection,
    leases: &[(PathBuf, jobs::PreviewLease)],
    cryptos: &JobCryptos,
) -> Result<Vec<Result<Option<bool>, ImportError>>, ImportError> {
    if conn.is_autocommit() {
        return Err(deferred("owners require one read snapshot"));
    }
    let mut cache = BTreeMap::new();
    Ok(leases
        .iter()
        .map(|(path, lease)| owner_cached(conn, path, lease, cryptos, &mut cache))
        .collect())
}

fn owner_cached(
    conn: &Connection,
    path: &Path,
    lease: &jobs::PreviewLease,
    cryptos: &JobCryptos,
    cache: &mut BTreeMap<String, Result<Accepted, ImportError>>,
) -> Result<Option<bool>, ImportError> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT job_id FROM job_items WHERE candidate_id=?1")
        .map_err(deferred)?;
    let ids = stmt
        .query_map([&lease.candidate_id], |row| row.get::<_, String>(0))
        .map_err(deferred)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(deferred)?;
    let mut found = None;
    for id in ids {
        if let std::collections::btree_map::Entry::Vacant(entry) = cache.entry(id.clone()) {
            let accepted = cryptos
                .get(&id)
                .and_then(|crypto| accepted(conn, &id, crypto))
                .and_then(|accepted| {
                    accepted.ok_or_else(|| deferred("input owner is missing its job"))
                });
            entry.insert(accepted);
        }
        let accepted = cache
            .get(&id)
            .expect("accepted outcome was cached")
            .as_ref()
            .map_err(Clone::clone)?;
        if let Some(input) = accepted.inputs.get(path) {
            if !matches_preview(input, lease) {
                return Err(deferred(
                    "accepted input differs from immutable preview identity",
                ));
            }
            let retryable = match input.phase.as_str() {
                "queued" | "running" | "recovery_needed" | "failed" | "cancelled" => true,
                "completed" | "committed" | "undone" | "skipped" | "conflict" | "partial"
                | "tombstoned" => false,
                _ => return Err(deferred("unknown accepted input phase")),
            };
            found = Some(found.unwrap_or(false) || retryable);
        }
    }
    Ok(found)
}

pub(super) fn existing(
    conn: &Connection,
    request: &jobs::JobRecord,
) -> Result<Option<jobs::JobRecord>, ImportError> {
    let prepared = jobs::request_reader::PreparedRequest::prepare(conn, &request.request_id)?;
    let tx = conn.unchecked_transaction().map_err(deferred)?;
    // Select only the durable owner ID; do not allocate its JSON as a shortcut
    // to the family authentication performed by accepted().
    let Some((job_id, crypto)) = prepared.selected(&tx, &request.request_id)? else {
        return Ok(None);
    };
    let accepted =
        accepted(&tx, job_id, crypto)?.ok_or_else(|| deferred("accepted request is incomplete"))?;
    let job = &accepted.job;
    if job.plan_id != request.plan_id || job.plan_hash != request.plan_hash {
        return Err(super::super::super::errors::ImportError::new(
            super::super::super::errors::ErrorCode::PlanStale,
            "request ID belongs to a different plan",
        ));
    }
    if accepted.job.request_id != request.request_id {
        return Err(deferred("accepted request header disagrees"));
    }
    Ok(Some(accepted.job))
}
