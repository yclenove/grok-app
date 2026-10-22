//! Original routing intent captured by the existing acceptance writer.
//! Not an ordinary allow flag, association grant, or mutable draft-plan cache.
use super::*;

#[path = "job_original_plan_codec.rs"]
mod original_codec;

use crate::session_migration::publication_protocol::PublicationProtocol;
use crate::session_migration::publication_write_fence::{row_stamp, row_unchanged, RowStamp};

struct Verified {
    job: String,
    rowid: i64,
    stamp: RowStamp,
    plan_id: String,
    plan_hash: String,
    json_digest: String,
    parsed: (PublicationProtocol, bool),
}

thread_local! {
    static VERIFIED: std::cell::RefCell<std::collections::VecDeque<Verified>> =
        const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}

/// The original plan snapshot row is immutable after acceptance, yet every
/// publication step re-read and re-hashed it (about 43 times per item). On the
/// fenced worker connection a verified result is reused only while that exact
/// row is unchanged on this connection and no other connection has committed;
/// otherwise the bytes are read and verified again exactly as before.
fn verified_plan(
    conn: &Connection,
    job: &str,
    plan_id: &str,
    plan_hash: &str,
    json_digest: &str,
) -> Result<(PublicationProtocol, bool), ImportError> {
    let stamp = row_stamp(conn)?;
    let rowid: Option<i64> = conn
        .query_row(
            "SELECT rowid FROM meta WHERE key=?1",
            [snapshot_key(job)],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    let rowid = rowid.ok_or_else(refusal)?;
    let hit = VERIFIED.with(|cache| {
        cache
            .borrow()
            .iter()
            .find(|entry| {
                entry.job == job
                    && entry.rowid == rowid
                    && entry.plan_id == plan_id
                    && entry.plan_hash == plan_hash
                    && entry.json_digest == json_digest
            })
            .map(|entry| (entry.stamp, entry.parsed))
    });
    if let Some((verified, parsed)) = hit {
        if row_unchanged(conn, &verified, "meta", rowid)? {
            return Ok(parsed);
        }
    }
    let original =
        metadata_record_with_budget(conn, &snapshot_key(job), None)?.ok_or_else(refusal)?;
    let parsed = original_codec::plan_identity(plan_id, plan_hash, json_digest, &original)?;
    if let Some(stamp) = stamp {
        VERIFIED.with(|cache| {
            let mut cache = cache.borrow_mut();
            cache.retain(|entry| entry.job != job);
            if cache.len() == 16 {
                cache.pop_front();
            }
            cache.push_back(Verified {
                job: job.into(),
                rowid,
                stamp,
                plan_id: plan_id.into(),
                plan_hash: plan_hash.into(),
                json_digest: json_digest.into(),
                parsed,
            });
        });
    }
    Ok(parsed)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Intent {
    version: u32,
    job_id: String,
    context_digest: String,
    plan_id: String,
    plan_hash: String,
    associations: Vec<String>,
}

fn key(job: &str) -> String {
    format!("job_entry_intent:{job}")
}

fn snapshot_key(job: &str) -> String {
    format!("job_entry_plan_snapshot:{job}")
}

/// Current original bytes plus a PURE codec result, not accepted SQL authority.
/// In particular never repair this snapshot from the mutable draft plan.
pub(in crate::session_migration::job_control) fn publication_plan_protocol(
    conn: &Connection,
    job: &str,
    plan_id: &str,
    plan_hash: &str,
    json_digest: &str,
) -> Result<crate::session_migration::publication_protocol::PublicationProtocol, ImportError> {
    if conn.is_autocommit() {
        return Err(refusal());
    }
    verified_plan(conn, job, plan_id, plan_hash, json_digest).map(|(protocol, _)| protocol)
}

pub(super) fn present(conn: &Connection, job: &str) -> Result<bool, ImportError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM meta WHERE key=?1 OR key=?2)",
        params![key(job), snapshot_key(job)],
        |row| row.get(0),
    )
    .map_err(sqlite_storage_error)
}

pub(super) struct PreparedIntent {
    job_id: String,
    encoded: String,
    original_plan: String,
}

impl PreparedIntent {
    pub(super) fn capture(
        job: &JobRecord,
        plan: &engine::PlanState,
        original_plan: &str,
    ) -> Result<Self, ImportError> {
        if job.plan_id != plan.plan_id || job.plan_hash != plan.plan_hash {
            return Err(refusal());
        }
        // The caller already verified complete original plan/input membership
        // and registered identities. Preserve only its routing projection.
        let mut associations: Vec<_> = plan
            .identity_associations
            .iter()
            .map(|confirmation| confirmation.import_id.clone())
            .collect();
        associations.sort();
        let record = Intent {
            version: 1,
            job_id: job.job_id.clone(),
            context_digest: context_digest(job),
            plan_id: job.plan_id.clone(),
            plan_hash: job.plan_hash.clone(),
            associations,
        };
        record.validate()?;
        let encoded = serde_json::to_string(&record).map_err(|_| refusal())?;
        if encoded.len() > SCOPE_BYTES {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "accepted intent exceeds metadata budget",
            ));
        }
        Ok(Self {
            job_id: job.job_id.clone(),
            encoded,
            original_plan: original_plan.into(),
        })
    }

    pub(super) fn digest(&self) -> String {
        hash::sha256_bytes(self.encoded.as_bytes())
    }

    pub(super) fn record(&self, conn: &Connection) -> Result<(), ImportError> {
        if conn.is_autocommit() {
            return Err(refusal());
        }
        conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)",
            params![key(&self.job_id), self.encoded],
        )
        .map_err(sqlite_storage_error)?;
        // Preserve the already-validated intent in this SAME acceptance
        // transaction. Do not impose routing-metadata's 16MiB limit on an
        // ordinary full plan, or initialize snapshots from a read path.
        conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)",
            params![snapshot_key(&self.job_id), self.original_plan],
        )
        .map_err(sqlite_storage_error)?;
        Ok(())
    }
}

impl Intent {
    fn validate(&self) -> Result<(), ImportError> {
        if self.version != 1
            || !digest_shape(&self.context_digest)
            || !digest_shape(&self.plan_hash)
            || !uuid::Uuid::parse_str(&self.job_id).is_ok_and(|id| id.to_string() == self.job_id)
            || !uuid::Uuid::parse_str(&self.plan_id).is_ok_and(|id| id.to_string() == self.plan_id)
        {
            return Err(refusal());
        }
        let mut previous: Option<&str> = None;
        for import in &self.associations {
            if !crate::session_migration::ids::parse_import_uuid(import)
                .is_ok_and(|id| format!("imp:{id}") == *import)
                || previous.is_some_and(|prior| prior >= import.as_str())
            {
                return Err(refusal());
            }
            previous = Some(import);
        }
        Ok(())
    }
}

pub(super) fn associations(
    conn: &Connection,
    scope: &Record,
    digest: &str,
) -> Result<BTreeSet<String>, ImportError> {
    let encoded = metadata_record(conn, &key(&scope.job_id))?.ok_or_else(refusal)?;
    if hash::sha256_bytes(encoded.as_bytes()) != digest {
        return Err(refusal());
    }
    let record: Intent = serde_json::from_str(&encoded).map_err(|_| refusal())?;
    record.validate()?;
    if record.job_id != scope.job_id
        || record.context_digest != scope.context_digest
        || record.plan_id != scope.plan_id
        || record.plan_hash != scope.plan_hash
    {
        return Err(stale("accepted intent differs from the job scope context"));
    }
    let associations: BTreeSet<_> = record.associations.into_iter().collect();
    if associations.is_empty() {
        // An empty label is NOT absence authority. The original pure codec
        // result must bind EXACT current bytes and immutable expected plan hash;
        // never cache current SQL authorization or association member proofs.
        #[cfg(test)]
        let read = crate::session_migration::codec_profile::Span::start("original-plan-read", 0);
        let (_, has_associations) = verified_plan(
            conn,
            &scope.job_id,
            &scope.plan_id,
            &scope.plan_hash,
            &scope.plan_json_digest,
        )?;
        #[cfg(test)]
        drop(read);
        if has_associations {
            return Err(refusal());
        }
    }
    Ok(associations)
}
