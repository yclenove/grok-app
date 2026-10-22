//! Optional accepted routing facts owned by the existing enqueue transaction.
//! Neither a scope, its digest nor a plan alone grants association job access.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{association_acceptance::Identity, association_context, DurableInput};
use crate::session_migration::{
    catalog, engine,
    errors::{sqlite_storage_error, ErrorCode, ImportError},
    hash, job_input_association,
    jobs::{JobRecord, PreparedRevision},
    registration_provenance::{old_owner, AssociationProof, ProofRecord},
    registration_provenance_store as store,
};

// Applies only to the new routing metadata, not ordinary job/input bodies.
const SCOPE_BYTES: usize = 16 * 1024 * 1024;

#[path = "job_provenance_compatibility.rs"]
pub(super) mod compatibility;
#[path = "job_provenance_intent.rs"]
mod intent;
pub(in crate::session_migration::job_control) use intent::publication_plan_protocol;
#[path = "job_publication_exclusion.rs"]
pub(in crate::session_migration) mod publication;

#[cfg(test)]
pub(in crate::session_migration) use compatibility::test_after_capture;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reference {
    version: u32,
    import_id: String,
    digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    version: u32,
    job_id: String,
    context_digest: String,
    plan_id: String,
    plan_hash: String,
    plan_json_digest: String,
    references: Vec<Reference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    intent_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publication_digest: Option<String>,
}

/// Private, non-serializable detached evidence from genuine registered preflight
/// plus complete validated plan/input membership, never a caller's allow flag.
pub(super) struct PreparedScope {
    record: Record,
    plan_json: String,
    intent: Option<intent::PreparedIntent>,
    publication: Option<publication::PreparedExclusion>,
    acceptance: Option<super::publication_acceptance::PreparedAcceptance>,
    publication_protocol: crate::session_migration::publication_protocol::PublicationProtocol,
}

fn refusal() -> ImportError {
    ImportError::new(
        ErrorCode::HistoryAccessUnavailable,
        "accepted job provenance scope is missing or inconsistent",
    )
    .job_local()
}

fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message).job_local()
}

fn metadata_read_error(cause: std::io::Error) -> ImportError {
    if cause.kind() == std::io::ErrorKind::InvalidData {
        // read_to_string's invalid UTF8 belongs to this exact metadata point.
        refusal()
    } else {
        // Genuine SQLite BLOB I/O is shared, not a job-specific codec refusal.
        crate::session_migration::errors::io_storage_error(cause)
    }
}

fn scope_key(job_id: &str) -> String {
    format!("job_entry_scope:{job_id}")
}

fn accepted_key(job_id: &str) -> String {
    format!("job_entry_scope_accepted:{job_id}")
}

fn context_digest(job: &JobRecord) -> String {
    hash::options_hash(&serde_json::json!([
        job.job_id,
        job.request_id,
        job.plan_id,
        job.plan_hash,
        job.created_at
    ]))
}

fn digest_shape(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl Record {
    fn validate(&self, job_id: &str) -> Result<(), ImportError> {
        let mut previous: Option<&str> = None;
        if self.version != 1
            || self.job_id != job_id
            || !uuid::Uuid::parse_str(job_id).is_ok_and(|id| id.to_string() == job_id)
            || !uuid::Uuid::parse_str(&self.plan_id).is_ok_and(|id| id.to_string() == self.plan_id)
            || !digest_shape(&self.context_digest)
            || !digest_shape(&self.plan_hash)
            || !digest_shape(&self.plan_json_digest)
            || self
                .intent_digest
                .as_deref()
                .is_some_and(|digest| !digest_shape(digest))
            || self
                .publication_digest
                .as_deref()
                .is_some_and(|digest| !digest_shape(digest))
        {
            return Err(refusal());
        }
        for reference in &self.references {
            if !matches!(reference.version, 1 | 2)
                || !crate::session_migration::ids::parse_import_uuid(&reference.import_id)
                    .is_ok_and(|id| format!("imp:{id}") == reference.import_id)
                || !digest_shape(&reference.digest)
                || previous.is_some_and(|prior| prior >= reference.import_id.as_str())
            {
                return Err(refusal());
            }
            previous = Some(&reference.import_id);
        }
        Ok(())
    }
}

pub(super) fn capture(
    conn: &Connection,
    job: &JobRecord,
    prepared: &[(DurableInput, Arc<PreparedRevision>)],
    identities: &[Identity],
) -> Result<Option<PreparedScope>, ImportError> {
    let mut scope = if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        capture_snapshot(&tx, job, prepared, identities)?
    } else {
        capture_snapshot(conn, job, prepared, identities)?
    };
    if let Some(scope) = &mut scope {
        if scope.publication_protocol
            == crate::session_migration::publication_protocol::PublicationProtocol::SignedOutcomeV1
        {
            // Native project reads and key loading happen only after dropping
            // the SQL snapshot above. The acceptance writer rechecks all facts.
            scope.acceptance = Some(super::publication_acceptance::PreparedAcceptance::capture(
                conn,
                job,
                &scope.plan_json,
                &hash::sha256_bytes(
                    serde_json::to_string(&scope.record)
                        .map_err(|_| refusal())?
                        .as_bytes(),
                ),
                prepared,
                identities,
            )?);
        }
    }
    Ok(scope)
}

fn capture_snapshot(
    conn: &Connection,
    job: &JobRecord,
    prepared: &[(DurableInput, Arc<PreparedRevision>)],
    identities: &[Identity],
) -> Result<Option<PreparedScope>, ImportError> {
    // The test-only unregistered enqueue lane does not manufacture this fact.
    // Older/low-level producers without a retained plan keep the strict legacy
    // closure; absence here never turns their jobs ordinary automatically.
    let has_plan: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM plans WHERE plan_id=?1)",
            [&job.plan_id],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if identities.len() != prepared.len() || !has_plan {
        return Ok(None);
    }
    let plan = engine::job_entry_plan(conn, &job.plan_id, &job.plan_hash)?;
    plan.publication_protocol.require_job_id(&job.job_id)?;
    let (_, plan_json) = catalog::load_plan_json(conn, &job.plan_id)?;
    let selections: BTreeMap<_, _> = plan
        .selection_set
        .entries
        .iter()
        .map(|entry| (entry.preview_id.as_str(), entry))
        .collect();
    let confirmations: BTreeMap<_, _> = plan
        .identity_associations
        .iter()
        .map(|confirmation| (confirmation.preview_id.as_str(), confirmation))
        .collect();
    let mut covered = BTreeSet::new();
    for (input, _) in prepared {
        let selected = input.selection_set.single_entry()?;
        let expected = selections
            .get(selected.preview_id.as_str())
            .copied()
            .ok_or_else(refusal)?;
        let selection_matches = match (
            &input.identity_association,
            confirmations.get(selected.preview_id.as_str()),
        ) {
            (Some(association), Some(confirmation)) => {
                association.require_job(job)?;
                &association.confirmation == *confirmation && selected == expected
            }
            (None, None) => {
                input.selection_set
                    == crate::session_migration::selection::SelectionSet::single(expected.clone())
                        .canonicalized()
            }
            _ => false,
        };
        if !selection_matches || !covered.insert(selected.preview_id.as_str()) {
            return Err(refusal());
        }
        plan.snapshots
            .get(&selected.preview_id)
            .ok_or_else(refusal)?
            .require_metadata(&input.preview_digest, input.source_member.as_ref())?;
        association_context::validate_mapping(input, &plan)?;
    }
    if covered.len() != plan.preview_ids.len()
        || plan
            .preview_ids
            .iter()
            .any(|preview| !covered.contains(preview.as_str()))
    {
        return Err(refusal());
    }
    let mut references = Vec::new();
    for identity in identities {
        let proof = match identity {
            Identity::Legacy(_, None) => continue,
            Identity::Legacy(_, Some(proof)) => ProofRecord::Legacy(proof.clone()),
            Identity::Association(_, proof, _) => ProofRecord::Association(proof.clone()),
        };
        references.push(Reference {
            version: if matches!(proof, ProofRecord::Association(_)) {
                2
            } else {
                1
            },
            import_id: proof.import_id().into(),
            digest: proof.digest()?,
        });
    }
    references.sort_by(|a, b| a.import_id.cmp(&b.import_id));
    let intent = intent::PreparedIntent::capture(job, &plan, &plan_json)?;
    let mut record = Record {
        version: 1,
        job_id: job.job_id.clone(),
        context_digest: context_digest(job),
        plan_id: job.plan_id.clone(),
        plan_hash: job.plan_hash.clone(),
        plan_json_digest: hash::sha256_bytes(plan_json.as_bytes()),
        references,
        intent_digest: Some(intent.digest()),
        publication_digest: None,
    };
    record.validate(&job.job_id)?;
    let publication = publication::capture(conn, job, prepared, identities)?;
    record.publication_digest = Some(publication.digest());
    Ok(Some(PreparedScope {
        record,
        plan_json,
        intent: Some(intent),
        publication: Some(publication),
        acceptance: None,
        publication_protocol: plan.publication_protocol,
    }))
}

impl PreparedScope {
    pub(super) fn verify_before_acceptance(&self, conn: &Connection) -> Result<(), ImportError> {
        if let Some(publication) = &self.publication {
            publication.verify_before_acceptance(conn)?;
        }
        Ok(())
    }

    pub(super) fn record(
        &self,
        conn: &rusqlite::Transaction<'_>,
        job: &JobRecord,
    ) -> Result<(), ImportError> {
        if conn.is_autocommit() || self.record.context_digest != context_digest(job) {
            return Err(refusal());
        }
        let (hash, raw) = catalog::load_plan_json(conn, &self.record.plan_id)?;
        if hash != self.record.plan_hash || raw != self.plan_json {
            return Err(stale("job provenance scope plan changed after preflight"));
        }
        if let Some(publication) = &self.publication {
            publication.record(conn)?;
        }
        if let Some(intent) = &self.intent {
            intent.record(conn)?;
        }
        let encoded = serde_json::to_string(&self.record).map_err(|_| refusal())?;
        if encoded.len() > SCOPE_BYTES {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "job provenance scope exceeds metadata budget",
            ));
        }
        conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)",
            params![scope_key(&job.job_id), encoded],
        )
        .map_err(sqlite_storage_error)?;
        conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)",
            params![
                accepted_key(&job.job_id),
                hash::sha256_bytes(encoded.as_bytes())
            ],
        )
        .map_err(sqlite_storage_error)?;
        if let Some(acceptance) = &self.acceptance {
            acceptance.record(conn, job)?;
        }
        Ok(())
    }
}

fn load(conn: &Connection, job_id: &str) -> Result<Option<Record>, ImportError> {
    let accepted = catalog::retention_metadata_text(conn, &accepted_key(job_id), 64)?;
    let Some(encoded) = metadata_record(conn, &scope_key(job_id))? else {
        return if accepted.is_some() || intent::present(conn, job_id)? {
            Err(refusal())
        } else {
            Ok(None)
        };
    };
    if accepted.as_deref() != Some(hash::sha256_bytes(encoded.as_bytes()).as_str()) {
        return Err(refusal());
    }
    let record: Record = serde_json::from_str(&encoded).map_err(|_| refusal())?;
    record.validate(job_id)?;
    if record.intent_digest.is_none() && intent::present(conn, job_id)? {
        return Err(refusal());
    }
    Ok(Some(record))
}

fn metadata_record(conn: &Connection, key: &str) -> Result<Option<String>, ImportError> {
    metadata_record_with_budget(conn, key, Some(SCOPE_BYTES))
}

fn metadata_record_with_budget(
    conn: &Connection,
    key: &str,
    budget: Option<usize>,
) -> Result<Option<String>, ImportError> {
    let row: Option<(i64, String)> = conn
        .query_row(
            "SELECT rowid,typeof(value) FROM meta WHERE key=?1",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    let Some((row, kind)) = row else {
        return Ok(None);
    };
    if kind != "text" {
        return Err(refusal());
    }
    let mut blob = conn
        .blob_open(rusqlite::DatabaseName::Main, "meta", "value", row, true)
        .map_err(sqlite_storage_error)?;
    if budget.is_some_and(|budget| blob.len() > budget) {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "job provenance scope exceeds metadata budget",
        ));
    }
    let mut encoded = String::with_capacity(blob.len());
    blob.read_to_string(&mut encoded)
        .map_err(metadata_read_error)?;
    Ok(Some(encoded))
}

/// Same-snapshot scoped selection, not an accepted-member witness. The caller
/// still authenticates complete proof2/header/input/retained membership.
pub(in crate::session_migration) fn job_candidates(
    conn: &Connection,
    job_id: &str,
) -> Result<Option<Vec<AssociationProof>>, ImportError> {
    if conn.is_autocommit() {
        return Err(refusal());
    }
    let Some(record) = load(conn, job_id)? else {
        return Ok(None);
    };
    // New producer facts preserve ORIGINAL routing intent independently of a
    // consumed ordinary draft. Older scopes keep their strict retained-plan
    // route; neither absence nor a caller's ordinary flag creates a shortcut.
    let expected = match &record.intent_digest {
        Some(digest) => intent::associations(conn, &record, digest)?,
        None => retained_associations(conn, &record)?,
    };
    let selected: BTreeSet<_> = record
        .references
        .iter()
        .filter(|reference| reference.version == 2)
        .map(|reference| reference.import_id.clone())
        .collect();
    if expected != selected {
        return Err(refusal());
    }
    if record.intent_digest.is_some()
        && !expected.is_empty()
        && retained_associations(conn, &record)? != expected
    {
        return Err(refusal());
    }
    // Choose bounds only after comparing the hash-validated plan intent with
    // this producer's selected references. Forging an empty/version1 list must
    // not route an association's header through the uncapped ordinary reader.
    let job = if expected.is_empty() {
        job_input_association::ordinary_job_metadata(conn, job_id)?
    } else {
        job_input_association::bounded_job_metadata(conn, job_id)?
    };
    if context_digest(&job) != record.context_digest
        || job.plan_id != record.plan_id
        || job.plan_hash != record.plan_hash
    {
        return Err(stale(
            "accepted job provenance scope differs from the job context",
        ));
    }
    let mut associations = Vec::new();
    for reference in record.references {
        let proof = if reference.version == 2 {
            ProofRecord::Association(Box::new(old_owner::scoped_job_candidate(
                conn,
                job_id,
                &reference.import_id,
            )?))
        } else {
            store::get(conn, &reference.import_id)?.ok_or_else(refusal)?
        };
        if !proof.selects_job(job_id) || proof.digest()? != reference.digest {
            return Err(refusal());
        }
        match (reference.version, proof) {
            (1, ProofRecord::Legacy(_)) => {}
            (2, ProofRecord::Association(proof)) => associations.push(*proof),
            _ => return Err(refusal()),
        }
    }
    // Every selected proof already verified its exact accepted key/digest.
    // This job-local count is only a completeness refusal, never a grant, and
    // cannot make an unrelated damaged job's proof body affect this entry.
    let witnesses: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM meta WHERE key COLLATE BINARY>=?1
             AND key COLLATE BINARY<?2",
            params![
                format!("registration_association_accepted:{job_id}:"),
                format!("registration_association_accepted:{job_id};")
            ],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if witnesses != associations.len() as u64 {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "accepted job witness set differs from its verified proofs",
        ));
    }
    Ok(Some(associations))
}

fn retained_associations(
    conn: &Connection,
    record: &Record,
) -> Result<BTreeSet<String>, ImportError> {
    let (stored, raw) = catalog::load_plan_json(conn, &record.plan_id)?;
    if stored != record.plan_hash || hash::sha256_bytes(raw.as_bytes()) != record.plan_json_digest {
        return Err(stale("accepted job provenance scope retained plan changed"));
    }
    let plan = engine::job_entry_plan(conn, &record.plan_id, &record.plan_hash)?;
    Ok(plan
        .identity_associations
        .into_iter()
        .map(|confirmation| confirmation.import_id)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_utf8_refusal_does_not_relabel_shared_blob_io() {
        let invalid = metadata_read_error(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "synthetic invalid UTF8",
        ));
        assert_eq!(invalid.code, ErrorCode::HistoryAccessUnavailable);
        assert!(invalid.is_job_local());
        let io = metadata_read_error(std::io::Error::other("synthetic BLOB read I/O"));
        assert_eq!(io.code, ErrorCode::SnapshotUnavailable);
        assert!(!io.is_job_local());
    }
}
