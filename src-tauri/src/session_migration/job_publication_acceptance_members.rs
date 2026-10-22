//! Stream the complete current member range once, with a per-row byte budget.
//! Nothing here survives the caller's synchronous read or grants writer access.
use std::collections::{BTreeMap, BTreeSet};

use super::*;

pub(super) fn read(
    conn: &Connection,
    auth: &InstallationAuthenticator,
    header: &VerifiedHeader,
    candidates: BTreeSet<String>,
) -> Result<BTreeMap<String, VerifiedMember>, ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("acceptance-member-range", 0);
    let job_id = &header.body.job_id;
    let prefix = format!("publication_acceptance:v1:{job_id}:member:");
    let mut expected = BTreeMap::new();
    for candidate in candidates {
        let digest = hash::sha256_bytes(candidate.as_bytes());
        if expected.insert(digest, candidate).is_some() {
            return Err(invalid());
        }
    }
    let mut statement = conn
        .prepare(
            "SELECT CASE WHEN typeof(key)='text' AND length(CAST(key AS BLOB))=?4
                         THEN key END,
                    typeof(value),length(CAST(value AS BLOB)),
                    CASE WHEN typeof(value)='text' AND length(CAST(value AS BLOB))<=?3
                         THEN value END
             FROM meta WHERE key COLLATE BINARY>=?1 AND key COLLATE BINARY<?2",
        )
        .map_err(sqlite_storage_error)?;
    let mut rows = statement
        .query(params![
            prefix,
            format!("publication_acceptance:v1:{job_id}:member;"),
            RECORD_BYTES,
            prefix.len() + 64
        ])
        .map_err(sqlite_storage_error)?;
    let mut members = BTreeMap::new();
    let mut commitments = BTreeMap::new();
    while let Some(row) = rows.next().map_err(sqlite_storage_error)? {
        // Do not materialize an over-budget/BLOB value or an oversized key in
        // Rust. Legitimate keys are exactly prefix + a 64-byte SHA256 digest.
        // Preserve point-read TEXT/type/UTF-8/byte limits, including NULs.
        let kind: String = row.get(1).map_err(|_| invalid())?;
        let bytes: i64 = row.get(2).map_err(|_| invalid())?;
        if kind != "text" || bytes < 0 || bytes as u64 > RECORD_BYTES as u64 {
            return Err(invalid());
        }
        let key: String = row.get(0).map_err(|_| invalid())?;
        let digest = key.strip_prefix(&prefix).ok_or_else(invalid)?;
        let candidate = expected.remove(digest).ok_or_else(invalid)?;
        let encoded: String = row.get(3).map_err(|_| invalid())?;
        let member = verify_member(auth, encoded.as_bytes(), header, digest)?;
        commitments.insert(digest.to_owned(), member.commitment.clone());
        members.insert(candidate, member);
    }
    // Missing, extra, renamed, duplicate or foreign members still fail against
    // both current job representations and the full signed membership root.
    if !expected.is_empty()
        || members.len() != header.body.member_count as usize
        || record::members_digest(&commitments) != header.body.members_digest
    {
        return Err(invalid());
    }
    Ok(members)
}
