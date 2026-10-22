//! New-scan group metadata. No source body is read or combined here.
use super::errors::{sqlite_storage_error, ErrorCode, ImportError};
use super::scan::CandidateRef;
use super::types::Provider;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

pub(super) const SCHEMA_VERSION: u32 = 1;
const GROUP_SQL: &str = "CREATE TABLE source_groups (
  seq INTEGER PRIMARY KEY CHECK(seq>0),
  group_id TEXT NOT NULL UNIQUE,
  source_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  host TEXT NOT NULL,
  identity TEXT NOT NULL,
  proof TEXT NOT NULL CHECK(proof IN ('typed-native','artifact'))
)";
const MEMBER_SQL: &str = "CREATE TABLE source_group_members (
  seq INTEGER PRIMARY KEY CHECK(seq>0),
  group_id TEXT NOT NULL,
  candidate_id TEXT NOT NULL UNIQUE,
  FOREIGN KEY(group_id) REFERENCES source_groups(group_id),
  FOREIGN KEY(candidate_id) REFERENCES candidates(candidate_id)
)";
const GROUP_PAGE_SQL: &str =
    "CREATE INDEX source_groups_page ON source_groups(provider,source_id,seq)";
const MEMBER_PAGE_SQL: &str =
    "CREATE INDEX source_group_members_page ON source_group_members(group_id,seq)";
const OBJECTS: [(&str, &str); 4] = [
    ("source_groups", GROUP_SQL),
    ("source_group_members", MEMBER_SQL),
    ("source_groups_page", GROUP_PAGE_SQL),
    ("source_group_members_page", MEMBER_PAGE_SQL),
];

pub(super) fn incompatible() -> ImportError {
    ImportError::new(
        ErrorCode::SchemaUnsupported,
        "scan group schema is incompatible; scan again",
    )
}

/// Only begin calls this, before publishing its control to readers/workers.
pub(super) fn initialize(conn: &Connection) -> Result<(), ImportError> {
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    for (_, sql) in OBJECTS {
        tx.execute_batch(sql).map_err(sqlite_storage_error)?;
    }
    verify(&tx)?;
    tx.commit().map_err(sqlite_storage_error)
}

/// Read in the caller's snapshot. Missing old marker means restart-required,
/// never an invitation to create tables. A marked scan with missing objects
/// is corruption, not legacy.
pub(super) fn verify(conn: &Connection) -> Result<(), ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return verify(&tx);
    }
    let marked: bool = conn
        .prepare("PRAGMA table_info(meta)")
        .map_err(sqlite_storage_error)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sqlite_storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_storage_error)?
        .iter()
        .any(|column| column == "group_schema_version");
    if !marked {
        for (name, _) in OBJECTS {
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1)",
                    [name],
                    |row| row.get(0),
                )
                .map_err(sqlite_storage_error)?;
            if exists {
                return Err(incompatible());
            }
        }
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "legacy scan has no source groups; scan again",
        ));
    }
    let version: Option<u32> = conn
        .query_row(
            "SELECT group_schema_version FROM meta WHERE id=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    if version != Some(SCHEMA_VERSION) {
        return Err(if version.is_some_and(|v| v > SCHEMA_VERSION) {
            ImportError::new(
                ErrorCode::ImportSchemaTooNew,
                "scan group schema is newer than this app",
            )
        } else {
            incompatible()
        });
    }
    for (name, expected) in OBJECTS {
        let sql: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name=?1",
                [name],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_storage_error)?;
        if sql.as_deref().map(str::trim) != Some(expected) {
            return Err(incompatible());
        }
    }
    let triggers: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('source_groups','source_group_members'))", [], |row| row.get(0)).map_err(sqlite_storage_error)?;
    if triggers {
        return Err(incompatible());
    }
    Ok(())
}

pub(super) fn proof(reference: &CandidateRef) -> &'static str {
    let candidate = &reference.candidate;
    if candidate.provider == Provider::Cursor {
        let family = super::adapters::cursor::declared_family(&candidate.native_session_id);
        let transcript = candidate.completeness == "metadata-preview"
            && reference.legacy_id.starts_with("cursor-transcript:");
        let database = matches!(
            (candidate.completeness.as_str(), family),
            ("composer", Some("composer")) | ("legacy-chat", Some("legacy-chat"))
        );
        if family.is_some() && (transcript || database) {
            return "typed-native";
        }
    }
    "artifact"
}

pub(super) fn group_id(scan_id: &str, reference: &CandidateRef) -> String {
    let kind = proof(reference);
    let host = reference
        .location
        .as_ref()
        .map(|l| l.host.as_str())
        .unwrap_or("local");
    let identity = if kind == "typed-native" {
        &reference.candidate.native_session_id
    } else {
        &reference.candidate.candidate_id
    };
    let key = super::hash::options_hash(&json!([
        "scan-source-group-v1",
        reference.source_id,
        host,
        reference.candidate.provider,
        kind,
        identity,
    ]));
    format!("group:{scan_id}:{key}")
}

/// Called only for an actually inserted candidate in that SAME transaction.
/// A duplicate locator leaves the original CandidateRef and membership alone.
pub(super) fn insert_member(
    conn: &Connection,
    scan_id: &str,
    seq: i64,
    reference: &CandidateRef,
) -> Result<(), ImportError> {
    if conn.is_autocommit() {
        return Err(incompatible());
    }
    u32::try_from(seq).map_err(|_| {
        ImportError::new(
            ErrorCode::ResourceLimit,
            "scan sequence exceeds cursor budget",
        )
    })?;
    let group_id = group_id(scan_id, reference);
    let host = reference
        .location
        .as_ref()
        .map(|l| l.host.as_str())
        .unwrap_or("local");
    conn.execute(
        "INSERT INTO source_groups(seq,group_id,source_id,provider,host,identity,proof)
        VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(group_id) DO NOTHING",
        params![
            seq,
            group_id,
            reference.source_id,
            reference.candidate.provider.as_str(),
            host,
            reference.candidate.native_session_id,
            proof(reference)
        ],
    )
    .map_err(sqlite_storage_error)?;
    let count: u32 = conn
        .query_row(
            "SELECT count(*) FROM source_group_members WHERE group_id=?1",
            [&group_id],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if count as usize >= super::budget::DISCOVER_BUDGET {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "source group exceeds member discovery budget",
        ));
    }
    conn.execute(
        "INSERT INTO source_group_members(seq,group_id,candidate_id) VALUES(?1,?2,?3)",
        params![seq, group_id, reference.candidate.candidate_id],
    )
    .map_err(sqlite_storage_error)?;
    Ok(())
}
