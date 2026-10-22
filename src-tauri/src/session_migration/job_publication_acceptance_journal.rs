//! SQL-only fixed metadata reads and uncapped streaming original-input hashing.
//! The caller loads crypto before its writer; no filesystem access or repair.
use std::{collections::BTreeSet, io::Read};

use super::*;
use crate::session_migration::{
    catalog, errors::sqlite_storage_error, job_input_association,
    publication_protocol::PublicationProtocol,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};

#[path = "job_publication_acceptance_members.rs"]
mod members;

fn header_key(job_id: &str) -> String {
    format!("publication_acceptance:v1:{job_id}:header")
}
fn member_key(job_id: &str, candidate: &str) -> String {
    format!("publication_acceptance:v1:{job_id}:member:{candidate}")
}

pub(super) fn header_present(conn: &Connection, job_id: &str) -> Result<bool, ImportError> {
    Ok(raw(conn, &header_key(job_id))?.is_some())
}

fn raw(tx: &Connection, key: &str) -> Result<Option<String>, ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("acceptance-meta-read", 0);
    let metadata = tx
        .query_row(
            "SELECT typeof(value),length(CAST(value AS BLOB)) FROM meta WHERE key=?1",
            [key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    let Some((kind, bytes)) = metadata else {
        return Ok(None);
    };
    if kind != "text" || bytes < 0 || bytes as u64 > RECORD_BYTES as u64 {
        return Err(invalid());
    }
    tx.query_row("SELECT value FROM meta WHERE key=?1", [key], |row| {
        row.get(0)
    })
    .map(Some)
    .map_err(|error| match error {
        rusqlite::Error::FromSqlConversionFailure(_, _, _)
        | rusqlite::Error::InvalidColumnType(_, _, _) => invalid(),
        error => sqlite_storage_error(error),
    })
}

fn blob_digest(
    conn: &Connection,
    table: &str,
    column: &str,
    row: i64,
) -> Result<String, ImportError> {
    #[cfg(test)]
    let open_span = crate::session_migration::codec_profile::Span::start("acceptance-blob-open", 0);
    let mut blob = conn
        .blob_open(rusqlite::DatabaseName::Main, table, column, row, true)
        .map_err(sqlite_storage_error)?;
    #[cfg(test)]
    drop(open_span);
    #[cfg(test)]
    let _span =
        crate::session_migration::codec_profile::Span::start("acceptance-blob-hash", blob.len());
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = blob
            .read(&mut buffer)
            .map_err(crate::session_migration::errors::io_storage_error)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

pub(super) fn input_digest(
    tx: &Connection,
    job_id: &str,
    candidate: &str,
) -> Result<String, ImportError> {
    #[cfg(test)]
    let _input = crate::session_migration::codec_profile::Span::start("acceptance-input-digest", 0);
    #[cfg(test)]
    let lookup = crate::session_migration::codec_profile::Span::start("acceptance-input-lookup", 0);
    let (row, kind): (i64, String) = tx
        .query_row(
            "SELECT rowid,typeof(result_json) FROM job_items WHERE job_id=?1 AND candidate_id=?2",
            params![job_id, candidate],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?
        .ok_or_else(invalid)?;
    #[cfg(test)]
    drop(lookup);
    if kind != "text" {
        return Err(invalid());
    }
    blob_digest(tx, "job_items", "result_json", row)
}

fn candidate_ids(tx: &Connection, job_id: &str) -> Result<BTreeSet<String>, ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("acceptance-candidate-set", 0);
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Item {
        candidate_id: String,
    }
    #[derive(Deserialize)]
    struct Membership {
        items: Vec<Item>,
    }
    let (row, kind): (i64, String) = tx
        .query_row(
            "SELECT rowid,typeof(json) FROM jobs WHERE job_id=?1",
            [job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?
        .ok_or_else(invalid)?;
    if kind != "text" {
        return Err(invalid());
    }
    let blob = tx
        .blob_open(rusqlite::DatabaseName::Main, "jobs", "json", row, true)
        .map_err(sqlite_storage_error)?;
    let body: Membership = serde_json::from_reader(
        crate::session_migration::job_control::input_reader::HeaderReader::new(
            std::io::BufReader::with_capacity(64 * 1024, blob),
        ),
    )
    .map_err(|_| invalid())?;
    let mut from_json = BTreeSet::new();
    for item in body.items {
        if item.candidate_id.is_empty() || !from_json.insert(item.candidate_id) {
            return Err(invalid());
        }
    }
    let mut statement = tx
        .prepare("SELECT candidate_id FROM job_items WHERE job_id=?1")
        .map_err(sqlite_storage_error)?;
    let rows = statement
        .query_map([job_id], |row| row.get::<_, String>(0))
        .map_err(sqlite_storage_error)?;
    let from_rows = rows
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(sqlite_storage_error)?;
    if from_rows != from_json {
        return Err(invalid());
    }
    Ok(from_json)
}

pub(super) fn require_header_facts(
    tx: &Connection,
    header: &Header,
) -> Result<crate::session_migration::jobs::JobRecord, ImportError> {
    Ok(header_facts_and_candidates(tx, header)?.0)
}

fn header_facts_and_candidates(
    tx: &Connection,
    header: &Header,
) -> Result<(crate::session_migration::jobs::JobRecord, BTreeSet<String>), ImportError> {
    let job = header_facts(tx, header)?;
    let candidates = candidate_ids(tx, &header.job_id)?;
    if candidates.len() != header.member_count as usize {
        return Err(invalid());
    }
    Ok((job, candidates))
}

/// Point reads only: the signed header's job, plan, protocol and scope facts.
fn header_facts(
    tx: &Connection,
    header: &Header,
) -> Result<crate::session_migration::jobs::JobRecord, ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("acceptance-header-facts", 0);
    header.validate()?;
    let job = job_input_association::ordinary_job_metadata(tx, &header.job_id)?;
    if record::context_digest(&job) != header.context_digest
        || job.plan_id != header.plan_id
        || job.plan_hash != header.plan_hash
        || job.item_count != header.member_count
    {
        return Err(invalid());
    }
    let protocol =
        crate::session_migration::job_control::provenance_scope::publication_plan_protocol(
            tx,
            &header.job_id,
            &header.plan_id,
            &header.plan_hash,
            &header.plan_digest,
        )?;
    if protocol != PublicationProtocol::SignedOutcomeV1 {
        return Err(invalid());
    }
    protocol.require_job_id(&header.job_id)?;
    let (row, kind): (i64, String) = tx
        .query_row(
            "SELECT rowid,typeof(value) FROM meta WHERE key=?1",
            [format!("job_entry_scope:{}", header.job_id)],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sqlite_storage_error)?
        .ok_or_else(invalid)?;
    if kind != "text" || blob_digest(tx, "meta", "value", row)? != header.scope_digest {
        return Err(invalid());
    }
    let accepted = catalog::retention_metadata_text(
        tx,
        &format!("job_entry_scope_accepted:{}", header.job_id),
        64,
    )?;
    if accepted.as_deref() != Some(header.scope_digest.as_str()) {
        return Err(invalid());
    }
    Ok(job)
}

fn insert_once(tx: &Transaction<'_>, key: &str, encoded: &str) -> Result<(), ImportError> {
    if let Some(existing) = raw(tx, key)? {
        if existing == encoded {
            return Ok(());
        }
        return Err(invalid());
    }
    tx.execute("INSERT INTO meta(key,value) VALUES(?1,?2)", [key, encoded])
        .map_err(sqlite_storage_error)?;
    Ok(())
}

pub(super) fn insert_header(
    tx: &Transaction<'_>,
    auth: &InstallationAuthenticator,
    header: &SealedHeader,
) -> Result<(), ImportError> {
    verify_header(auth, header.encoded.as_bytes(), &header.body.job_id)?;
    insert_once(tx, &header_key(&header.body.job_id), &header.encoded)
}
pub(super) fn insert_member(
    tx: &Transaction<'_>,
    auth: &InstallationAuthenticator,
    header: &VerifiedHeader,
    member: &SealedMember,
) -> Result<(), ImportError> {
    verify_member(
        auth,
        member.encoded.as_bytes(),
        header,
        &member.body.candidate_digest,
    )?;
    // A member cannot be written under a missing/different durable header.
    let current = raw(tx, &header_key(&header.body.job_id))?.ok_or_else(invalid)?;
    if verify_header(auth, current.as_bytes(), &header.body.job_id)?.digest != header.digest {
        return Err(invalid());
    }
    insert_once(
        tx,
        &member_key(&member.body.job_id, &member.body.candidate_digest),
        &member.encoded,
    )
}

/// Same-snapshot acceptance evidence only. Existing source/proof/fence/target
/// authorization must still be checked before the first publication/Undo write.
pub(in crate::session_migration) struct AcceptedJob<'snapshot, 'connection> {
    header: VerifiedHeader,
    tx: &'snapshot Transaction<'connection>,
    auth: &'snapshot InstallationAuthenticator,
    changes: u64,
}

impl AcceptedJob<'_, '_> {
    pub(in crate::session_migration) fn require_current(
        &self,
        tx: &Transaction<'_>,
    ) -> Result<(), ImportError> {
        if tx.is_autocommit() || !std::ptr::eq(self.tx, tx) || self.changes != tx.total_changes() {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn candidate(
        &self,
        tx: &Transaction<'_>,
        candidate: &str,
    ) -> Result<VerifiedMember, ImportError> {
        self.require_current(tx)?;
        read_candidate(tx, self.auth, &self.header, candidate)
    }

    /// Derive result identity only from the current signed original acceptance,
    /// never mutable receipt/JobItem results. This detached codec value is NOT
    /// SQL authorization: the publisher must recheck acceptance and all other
    /// permissions in its own writer before performing a transition.
    pub(in crate::session_migration) fn outcome_binding(
        &self,
        tx: &Transaction<'_>,
        candidate: &str,
    ) -> Result<crate::session_migration::publication_outcome::Binding, ImportError> {
        let member = self.candidate(tx, candidate)?;
        self.binding(&member)
    }

    fn binding(
        &self,
        member: &VerifiedMember,
    ) -> Result<crate::session_migration::publication_outcome::Binding, ImportError> {
        let header = &self.header.body;
        let binding = crate::session_migration::publication_outcome::Binding {
            job_id: header.job_id.clone(),
            context_digest: header.context_digest.clone(),
            plan_id: header.plan_id.clone(),
            plan_hash: header.plan_hash.clone(),
            plan_digest: header.plan_digest.clone(),
            // Canonical signed member envelope; its authenticated headerDigest
            // transitively binds the full accepted scope and membership root.
            acceptance_digest: member.digest.clone(),
            candidate_digest: member.body.candidate_digest.clone(),
            input_digest: member.body.input_digest.clone(),
        };
        binding.validate()?;
        Ok(binding)
    }

    /// Required original allocation, not a permission grant and never a mint.
    pub(in crate::session_migration) fn allocated(
        &self,
        tx: &Transaction<'_>,
        candidate: &str,
    ) -> Result<crate::session_migration::publication_outcome::VerifiedRecord, ImportError> {
        let member = self.candidate(tx, candidate)?;
        let binding = self.binding(&member)?;
        crate::session_migration::publication_outcome::VerifiedRecord::load_allocation(
            tx,
            self.auth,
            &binding,
            &member.body.allocation_digest,
        )
    }
}

pub(in crate::session_migration) fn load<'snapshot, 'connection>(
    tx: &'snapshot Transaction<'connection>,
    auth: &'snapshot InstallationAuthenticator,
    job_id: &str,
) -> Result<AcceptedJob<'snapshot, 'connection>, ImportError> {
    let header = read_header(tx, auth, job_id)?.header;
    Ok(AcceptedJob {
        header,
        tx,
        auth,
        changes: tx.total_changes(),
    })
}

/// Per-candidate writer witness for repeated publication steps of one item:
/// the signed header with its job/plan/protocol/scope facts. Every candidate
/// read through it still verifies that member's signature, its binding to this
/// header (whose digest commits to the membership root) and its exact input
/// bytes. It does not re-prove the complete member set on every step; that is
/// `validate_snapshot`, run when work starts and before the job turns terminal.
/// Use `load` where the complete set is part of the decision.
pub(in crate::session_migration) fn load_candidate<'snapshot, 'connection>(
    tx: &'snapshot Transaction<'connection>,
    auth: &'snapshot InstallationAuthenticator,
    job_id: &str,
) -> Result<AcceptedJob<'snapshot, 'connection>, ImportError> {
    #[cfg(test)]
    let _span =
        crate::session_migration::codec_profile::Span::start("acceptance-candidate-header", 0);
    if tx.is_autocommit() {
        return Err(invalid());
    }
    PublicationProtocol::SignedOutcomeV1.require_job_id(job_id)?;
    let encoded = raw(tx, &header_key(job_id))?.ok_or_else(invalid)?;
    let header = verify_header(auth, encoded.as_bytes(), job_id)?;
    header_facts(tx, &header.body)?;
    Ok(AcceptedJob {
        header,
        tx,
        auth,
        changes: tx.total_changes(),
    })
}

/// Read-only validation returns no transferable writer capability. Connection
/// helpers below only read SQL; AcceptedJob still borrows a concrete Transaction.
pub(in crate::session_migration) fn validate_snapshot(
    conn: &Connection,
    auth: &InstallationAuthenticator,
    job_id: &str,
) -> Result<(), ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("acceptance-full-snapshot", 0);
    let mut snapshot = read_header(conn, auth, job_id)?;
    // The same SQL snapshot already fixes the exact member set. Enumerate only
    // input row identities/types once, then stream every original body; avoid
    // preparing a point lookup per member without retaining any SQL authority.
    #[cfg(test)]
    let _range = crate::session_migration::codec_profile::Span::start("acceptance-input-range", 0);
    let mut statement = conn
        .prepare("SELECT rowid,candidate_id,typeof(result_json) FROM job_items WHERE job_id=?1")
        .map_err(sqlite_storage_error)?;
    let mut rows = statement.query([job_id]).map_err(sqlite_storage_error)?;
    while let Some(row) = rows.next().map_err(sqlite_storage_error)? {
        let row_id: i64 = row.get(0).map_err(sqlite_storage_error)?;
        let candidate: String = row.get(1).map_err(sqlite_storage_error)?;
        let kind: String = row.get(2).map_err(sqlite_storage_error)?;
        let member = snapshot.members.remove(&candidate).ok_or_else(invalid)?;
        if kind != "text"
            || blob_digest(conn, "job_items", "result_json", row_id)? != member.body.input_digest
        {
            return Err(invalid());
        }
    }
    if !snapshot.members.is_empty() {
        return Err(invalid());
    }
    Ok(())
}

/// One candidate under the signed header: header signature and its job/plan/
/// protocol/scope facts, this member's signature and header binding (the
/// header digest commits to the membership root), and its exact input bytes.
/// Used only where `publication_write_fence` proves the open transaction did
/// not change another item's input or this job's acceptance rows.
pub(in crate::session_migration) fn validate_candidate(
    conn: &Connection,
    auth: &InstallationAuthenticator,
    job_id: &str,
    candidate: &str,
) -> Result<String, ImportError> {
    if conn.is_autocommit() {
        return Err(invalid());
    }
    PublicationProtocol::SignedOutcomeV1.require_job_id(job_id)?;
    let encoded = raw(conn, &header_key(job_id))?.ok_or_else(invalid)?;
    let header = verify_header(auth, encoded.as_bytes(), job_id)?;
    header_facts(conn, &header.body)?;
    Ok(read_candidate(conn, auth, &header, candidate)?
        .body
        .input_digest
        .clone())
}

/// Identity digest only, not a writer capability. Keeps full header/member-set
/// verification but hashes only the selected original input, never all bodies.
pub(in crate::session_migration) fn validate_input_snapshot(
    conn: &Connection,
    auth: &InstallationAuthenticator,
    job_id: &str,
    candidate: &str,
) -> Result<String, ImportError> {
    let snapshot = read_header(conn, auth, job_id)?;
    let member = snapshot.members.get(candidate).ok_or_else(invalid)?;
    if input_digest(conn, job_id, candidate)? != member.body.input_digest {
        return Err(invalid());
    }
    Ok(member.body.input_digest.clone())
}

fn read_candidate(
    conn: &Connection,
    auth: &InstallationAuthenticator,
    header: &VerifiedHeader,
    candidate: &str,
) -> Result<VerifiedMember, ImportError> {
    let digest = hash::sha256_bytes(candidate.as_bytes());
    let job_id = &header.body.job_id;
    let encoded = raw(conn, &member_key(job_id, &digest))?.ok_or_else(invalid)?;
    let member = verify_member(auth, encoded.as_bytes(), header, &digest)?;
    if input_digest(conn, job_id, candidate)? != member.body.input_digest {
        return Err(invalid());
    }
    Ok(member)
}

// Private to this synchronous read: no writes, callbacks, or snapshot reuse.
// AcceptedJob retains only its header and still re-reads each selected member
// under its concrete transaction/total_changes witness before a writer action.
struct Snapshot {
    header: VerifiedHeader,
    members: std::collections::BTreeMap<String, VerifiedMember>,
}

fn read_header(
    tx: &Connection,
    auth: &InstallationAuthenticator,
    job_id: &str,
) -> Result<Snapshot, ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("acceptance-header", 0);
    if tx.is_autocommit() {
        return Err(invalid());
    }
    PublicationProtocol::SignedOutcomeV1.require_job_id(job_id)?;
    let encoded = raw(tx, &header_key(job_id))?.ok_or_else(invalid)?;
    let header = verify_header(auth, encoded.as_bytes(), job_id)?;
    let (_, candidates) = header_facts_and_candidates(tx, &header.body)?;
    let members = members::read(tx, auth, &header, candidates)?;
    Ok(Snapshot { header, members })
}
