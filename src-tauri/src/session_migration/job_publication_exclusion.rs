//! Exact negative owner facts from the existing registered acceptance writer.
//! Not caller permissions; later genuine adoption rotates the original fence.
use super::*;
use crate::session_migration::jobs::CommitItem;
use sha2::{Digest, Sha256};

use crate::session_migration::publication_write_fence::{row_stamp, row_unchanged, RowStamp};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Item {
    candidate_id: String,
    tuple_digest: String,
    input_digest: String,
    fence: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Exclusion {
    version: u32,
    job_id: String,
    context_digest: String,
    items: Vec<Item>,
}

pub(super) struct PreparedExclusion {
    record: Exclusion,
    encoded: String,
    closure_digest: String,
    fences: BTreeMap<String, (Option<String>, String)>,
}

fn tuple(installation: &str, provider: &str, native: &str) -> String {
    hash::options_hash(&serde_json::json!([installation, provider, native]))
}

fn record_key(job: &str) -> String {
    format!("job_publication_exclusion:{job}")
}

fn fence_key(tuple: &str) -> String {
    format!("publication_tuple_fence:{tuple}")
}

fn fence(conn: &Connection, tuple: &str) -> Result<Option<String>, ImportError> {
    let value = catalog::retention_metadata_text(conn, &fence_key(tuple), 36)?;
    if value
        .as_deref()
        .is_some_and(|value| !uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value))
    {
        return Err(refusal());
    }
    Ok(value)
}

pub(in crate::session_migration) fn advance_fence(
    conn: &Connection,
    tuple: &str,
) -> Result<(), ImportError> {
    if conn.is_autocommit() || !digest_shape(tuple) {
        return Err(refusal());
    }
    // Do not overwrite an incompatible present fact as a side-effect of read or
    // adoption. All producers, including those without tickets, invalidate old
    // tickets in their existing atomic proof2 acceptance transaction.
    fence(conn, tuple)?;
    conn.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![fence_key(tuple), uuid::Uuid::new_v4().to_string()],
    ).map_err(sqlite_storage_error)?;
    Ok(())
}

pub(super) fn capture(
    conn: &Connection,
    job: &JobRecord,
    prepared: &[(DurableInput, Arc<PreparedRevision>)],
    identities: &[Identity],
) -> Result<PreparedExclusion, ImportError> {
    if conn.is_autocommit() || prepared.len() != identities.len() {
        return Err(refusal());
    }
    let proofs = old_owner::migration_records(conn)?;
    let closure_digest = compatibility::closure_digest(&proofs)?;
    let mut owned = BTreeSet::new();
    for proof in &proofs {
        if let ProofRecord::Association(proof) = proof {
            owned.insert(proof.original_tuple_digest()?);
        }
    }
    for identity in identities {
        if let Identity::Association(_, proof, _) = identity {
            owned.insert(proof.original_tuple_digest()?);
        }
    }
    let mut fences = BTreeMap::new();
    let mut items = Vec::new();
    for (input, _) in prepared {
        let key = tuple(
            &input.source_id,
            &input.provider,
            &input.adapter.graph.native_session_id,
        );
        if owned.contains(&key) {
            // This item has no negative grant. The exact old-owner guard still
            // refuses it, even when the current enqueue itself adopts it.
            continue;
        }
        let (_, nonce) = match fences.get(&key) {
            Some(pair) => pair,
            None => {
                let previous = fence(conn, &key)?;
                let nonce = previous
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                fences.entry(key.clone()).or_insert((previous, nonce))
            }
        };
        let encoded = serde_json::to_string(input).map_err(|_| refusal())?;
        items.push(Item {
            candidate_id: input.candidate_id.clone(),
            tuple_digest: key,
            input_digest: hash::sha256_bytes(encoded.as_bytes()),
            fence: nonce.clone(),
        });
    }
    items.sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
    let record = Exclusion {
        version: 1,
        job_id: job.job_id.clone(),
        context_digest: context_digest(job),
        items,
    };
    record.validate(&job.job_id)?;
    let encoded = serde_json::to_string(&record).map_err(|_| refusal())?;
    if encoded.len() > SCOPE_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "publication exclusion exceeds metadata budget",
        ));
    }
    Ok(PreparedExclusion {
        record,
        encoded,
        closure_digest,
        fences,
    })
}

impl Exclusion {
    fn validate(&self, job: &str) -> Result<(), ImportError> {
        if self.version != 1 || self.job_id != job || !digest_shape(&self.context_digest) {
            return Err(refusal());
        }
        let mut previous: Option<&str> = None;
        for item in &self.items {
            if item.candidate_id.is_empty()
                || previous.is_some_and(|prior| prior >= item.candidate_id.as_str())
                || !digest_shape(&item.tuple_digest)
                || !digest_shape(&item.input_digest)
                || !uuid::Uuid::parse_str(&item.fence).is_ok_and(|id| id.to_string() == item.fence)
            {
                return Err(refusal());
            }
            previous = Some(&item.candidate_id);
        }
        Ok(())
    }
}

impl PreparedExclusion {
    pub(super) fn digest(&self) -> String {
        hash::sha256_bytes(self.encoded.as_bytes())
    }

    pub(super) fn verify_before_acceptance(&self, conn: &Connection) -> Result<(), ImportError> {
        if conn.is_autocommit()
            || compatibility::closure_digest(&old_owner::migration_records(conn)?)?
                != self.closure_digest
        {
            return Err(stale("publication owner closure changed after preflight"));
        }
        for (tuple, (previous, nonce)) in &self.fences {
            if fence(conn, tuple)? != *previous {
                return Err(stale("publication tuple fence changed after preflight"));
            }
            if previous.is_none() {
                conn.execute(
                    "INSERT INTO meta(key,value) VALUES(?1,?2)",
                    params![fence_key(tuple), nonce],
                )
                .map_err(sqlite_storage_error)?;
            }
        }
        Ok(())
    }

    pub(super) fn record(&self, conn: &Connection) -> Result<(), ImportError> {
        if conn.is_autocommit() {
            return Err(refusal());
        }
        for (tuple, (_, nonce)) in &self.fences {
            if fence(conn, tuple)?.as_deref() != Some(nonce.as_str()) {
                return Err(stale("publication tuple was adopted during acceptance"));
            }
        }
        conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)",
            params![record_key(&self.record.job_id), self.encoded],
        )
        .map_err(sqlite_storage_error)?;
        Ok(())
    }
}

fn input_digest(conn: &Connection, job: &str, candidate: &str) -> Result<String, ImportError> {
    let (row, kind): (i64, String) = conn
        .query_row(
            "SELECT rowid,typeof(result_json) FROM job_items WHERE job_id=?1 AND candidate_id=?2",
            params![job, candidate],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sqlite_storage_error)?;
    if kind != "text" {
        return Err(refusal());
    }
    // A hash-only point read must not inherit the association decoder's budget
    // or allocate a large ordinary input. Keep the same read snapshot pinned.
    let mut blob = conn
        .blob_open(
            rusqlite::DatabaseName::Main,
            "job_items",
            "result_json",
            row,
            true,
        )
        .map_err(sqlite_storage_error)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let length = blob
            .read(&mut buffer)
            .map_err(crate::session_migration::errors::io_storage_error)?;
        if length == 0 {
            break;
        }
        digest.update(&buffer[..length]);
    }
    Ok(hex::encode(digest.finalize()))
}

struct VerifiedRecord {
    job: String,
    rowid: i64,
    stamp: RowStamp,
    digest: String,
    record: std::rc::Rc<Exclusion>,
}

thread_local! {
    static VERIFIED: std::cell::RefCell<std::collections::VecDeque<VerifiedRecord>> =
        const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}

/// The accepted exclusion record names every item of the job and is immutable
/// after acceptance, yet each publication step re-read, hashed and decoded it
/// (16 times per item), which made publication quadratic in the batch size. On
/// the fenced worker connection a verified record is reused only while that
/// exact row is unchanged on this connection and no other connection has
/// committed; otherwise the bytes are read and verified again as before.
fn verified_record(
    conn: &Connection,
    job: &str,
    accepted_digest: &str,
) -> Result<std::rc::Rc<Exclusion>, ImportError> {
    let stamp = row_stamp(conn)?;
    let rowid: Option<i64> = conn
        .query_row(
            "SELECT rowid FROM meta WHERE key=?1",
            [record_key(job)],
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
                entry.job == job && entry.rowid == rowid && entry.digest == accepted_digest
            })
            .map(|entry| (entry.stamp, entry.record.clone()))
    });
    if let Some((verified, record)) = hit {
        if row_unchanged(conn, &verified, "meta", rowid)? {
            return Ok(record);
        }
    }
    #[cfg(test)]
    let read = crate::session_migration::codec_profile::Span::start("exclusion-read", 0);
    let encoded = bounded_record(conn, job)?;
    #[cfg(test)]
    drop(read);
    #[cfg(test)]
    let bytes_hash =
        crate::session_migration::codec_profile::Span::start("exclusion-bytes-hash", encoded.len());
    let digest = hash::sha256_bytes(encoded.as_bytes());
    #[cfg(test)]
    drop(bytes_hash);
    if digest != accepted_digest {
        return Err(refusal());
    }
    #[cfg(test)]
    let codec = crate::session_migration::codec_profile::Span::start(
        "exclusion-decode-shape",
        encoded.len(),
    );
    let record: Exclusion = serde_json::from_str(&encoded).map_err(|_| refusal())?;
    record.validate(job)?;
    #[cfg(test)]
    drop(codec);
    let record = std::rc::Rc::new(record);
    if let Some(stamp) = stamp {
        VERIFIED.with(|cache| {
            let mut cache = cache.borrow_mut();
            cache.retain(|entry| entry.job != job);
            if cache.len() == 16 {
                cache.pop_front();
            }
            cache.push_back(VerifiedRecord {
                job: job.into(),
                rowid,
                stamp,
                digest: accepted_digest.into(),
                record: record.clone(),
            });
        });
    }
    Ok(record)
}

fn bounded_record(conn: &Connection, job: &str) -> Result<String, ImportError> {
    // retention_metadata_text checks EXACT fixed-size facts (nonce/digest),
    // not variable-sized JSON. Check this record's type/upper bound before fetch.
    let (row, kind): (i64, String) = conn
        .query_row(
            "SELECT rowid,typeof(value) FROM meta WHERE key=?1",
            [record_key(job)],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?
        .ok_or_else(refusal)?;
    if kind != "text" {
        return Err(refusal());
    }
    let mut blob = conn
        .blob_open(rusqlite::DatabaseName::Main, "meta", "value", row, true)
        .map_err(sqlite_storage_error)?;
    if blob.len() > SCOPE_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "publication exclusion exceeds metadata budget",
        ));
    }
    let mut encoded = String::with_capacity(blob.len());
    blob.read_to_string(&mut encoded)
        .map_err(metadata_read_error)?;
    Ok(encoded)
}

/// Private canonical metadata witness, captured before the final catalog
/// writer. Verification never reopens/hashes prepared files or provider data.
pub(in crate::session_migration) struct PublicationOwner<'crypto> {
    job: String,
    candidate: String,
    installation: String,
    provider: String,
    native: String,
    publication_digest: Option<String>,
    input_digest: Option<String>,
    crypto: &'crypto crate::session_migration::publication_crypto::JobCrypto,
}

pub(in crate::session_migration) fn for_item<'crypto>(
    conn: &Connection,
    job: &str,
    item: &CommitItem,
    crypto: &'crypto crate::session_migration::publication_crypto::JobCrypto,
) -> Result<PublicationOwner<'crypto>, ImportError> {
    let publication_digest = if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        load(&tx, job)?.and_then(|scope| scope.publication_digest)
    } else {
        load(conn, job)?.and_then(|scope| scope.publication_digest)
    };
    let input_digest = if publication_digest.is_some() {
        let (input, _) = DurableInput::from_item(item)?;
        Some(hash::sha256_bytes(
            serde_json::to_string(&input)
                .map_err(|_| refusal())?
                .as_bytes(),
        ))
    } else {
        None
    };
    let owner = PublicationOwner {
        job: job.into(),
        candidate: item.candidate_id.clone(),
        installation: item.installation_id.clone(),
        provider: item.provider.clone(),
        native: item.adapter.graph.native_session_id.clone(),
        publication_digest,
        input_digest,
        crypto,
    };
    owner.verify(conn)?;
    Ok(owner)
}

/// Recovery borrows the exact durable metadata. No key loading, source read,
/// prepared-file hashing or preview reopen may occur in this constructor.
pub(in crate::session_migration::job_control) fn for_input<'crypto>(
    conn: &Connection,
    job: &str,
    input: &DurableInput,
    crypto: &'crypto crate::session_migration::publication_crypto::JobCrypto,
) -> Result<PublicationOwner<'crypto>, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return for_input(&tx, job, input, crypto);
    }
    input.validate_shape()?;
    let publication_digest = load(conn, job)?.and_then(|scope| scope.publication_digest);
    let owner = PublicationOwner {
        job: job.into(),
        candidate: input.candidate_id.clone(),
        installation: input.installation_id.clone(),
        provider: input.provider.clone(),
        native: input.adapter.graph.native_session_id.clone(),
        input_digest: publication_digest
            .as_ref()
            .map(|_| input.input_identity().map(|id| id.digest))
            .transpose()?,
        publication_digest,
        crypto,
    };
    owner.verify(conn)?;
    Ok(owner)
}

impl PublicationOwner<'_> {
    pub(in crate::session_migration) fn verify(
        &self,
        conn: &Connection,
    ) -> Result<(), ImportError> {
        if conn.is_autocommit() {
            let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
            return self.verify(&tx);
        }
        self.verify_with(conn, || {
            crate::session_migration::jobs::entry::authenticate_candidate_with_crypto(
                conn,
                &self.job,
                &self.candidate,
                self.crypto,
            )
        })
        .map(|_| ())
    }

    /// Return only the complete authentication already performed by this owner
    /// check. The legacy fallback alone does not authenticate a job projection.
    pub(in crate::session_migration) fn verify_for_progress<'snapshot, 'connection>(
        &'snapshot self,
        tx: &'snapshot rusqlite::Transaction<'connection>,
    ) -> Result<
        Option<
            crate::session_migration::jobs::entry::AuthenticatedWriterEntry<'snapshot, 'connection>,
        >,
        ImportError,
    > {
        self.verify_with(tx, || {
            crate::session_migration::jobs::entry::authenticate_writer(tx, &self.job, self.crypto)
        })
    }

    fn verify_with<T>(
        &self,
        conn: &Connection,
        authenticate: impl FnOnce() -> Result<T, ImportError>,
    ) -> Result<Option<T>, ImportError> {
        if conn.is_autocommit() {
            return Err(refusal());
        }
        let job = self.job.as_str();
        let mut entry = None;
        let scope = load(conn, job)?;
        if scope
            .as_ref()
            .and_then(|scope| scope.publication_digest.as_ref())
            != self.publication_digest.as_ref()
        {
            return Err(refusal());
        }
        if let Some(scope) = scope {
            if let Some(accepted_digest) = scope.publication_digest {
                let record = verified_record(conn, job, &accepted_digest)?;
                if record.context_digest != scope.context_digest {
                    return Err(refusal());
                }
                // Scope/plan routing is not proof/member authority. Authenticate
                // that chain in this very snapshot before using the negative fact.
                entry = Some(authenticate()?);
                if let Some(selected) = record
                    .items
                    .iter()
                    .find(|entry| entry.candidate_id == self.candidate)
                {
                    if selected.tuple_digest
                        != tuple(&self.installation, &self.provider, &self.native)
                        || self.input_digest.as_deref() != Some(selected.input_digest.as_str())
                        || selected.input_digest != input_digest(conn, job, &self.candidate)?
                        || fence(conn, &selected.tuple_digest)?.as_deref()
                            != Some(selected.fence.as_str())
                    {
                        return Err(refusal());
                    }
                    return Ok(entry);
                }
            }
        }
        crate::session_migration::sources::verify_publication_owner(
            conn,
            &self.installation,
            &self.provider,
            &self.native,
        )?;
        Ok(entry)
    }
}

#[cfg(test)]
#[path = "job_publication_exclusion_cache_tests.rs"]
mod cache_tests;
