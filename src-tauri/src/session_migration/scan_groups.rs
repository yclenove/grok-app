//! Read-only, bounded metadata views over immutable scan candidates.
use super::cursor::{decode_bound_cursor, encode_bound_cursor, CursorScope};
use super::errors::{sqlite_storage_error, ErrorCode, ImportError};
use super::scan::CandidateRef;
use super::scan_group_store as store;
use super::types::{Provider, SourceCandidate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceGroupFilter {
    pub provider: Option<Provider>,
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceGroup {
    pub group_id: String,
    pub source_id: String,
    pub provider: Provider,
    pub host: String,
    pub native_session_id: String,
    /// Identity evidence only, NEVER a body-equivalence/completeness result.
    pub proof: String,
    /// First discovery metadata, not a recommendation of the best body.
    pub first_member: SourceCandidate,
    pub member_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceGroupMember {
    pub group_id: String,
    pub format: String,
    pub candidate: SourceCandidate,
    pub source_id: String,
    pub root_path: String,
    pub legacy_id: String,
    pub location: Option<super::source_locations::SourceLocation>,
}

impl SourceGroupMember {
    fn new(group_id: &str, reference: CandidateRef) -> Self {
        let format = match reference.candidate.provider {
            Provider::Cursor => match reference.candidate.completeness.as_str() {
                "composer" => "cursor-composer",
                "legacy-chat" => "cursor-legacy-chat",
                "store" => "cursor-store",
                "metadata-preview" if reference.legacy_id.starts_with("cursor-transcript:") => {
                    "cursor-transcript"
                }
                _ => "artifact",
            },
            _ => "artifact",
        };
        Self {
            group_id: group_id.into(),
            format: format.into(),
            candidate: reference.candidate,
            source_id: reference.source_id,
            root_path: reference.root_path,
            legacy_id: reference.legacy_id,
            location: reference.location,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceGroupPage {
    pub version: u32,
    pub groups: Vec<SourceGroup>,
    pub next_cursor: Option<String>,
    pub total_count: u32,
    pub complete: bool,
    pub state_version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceGroupMemberPage {
    pub version: u32,
    pub group_id: String,
    pub members: Vec<SourceGroupMember>,
    pub next_cursor: Option<String>,
    pub member_count: u32,
    pub complete: bool,
    pub state_version: u64,
}

fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}

fn parse_reference(raw: Option<String>) -> Result<CandidateRef, ImportError> {
    let raw = raw.ok_or_else(store::incompatible)?;
    if raw.len() > super::budget::READ_PAGE_TEXT_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "source member metadata exceeds page byte budget",
        ));
    }
    serde_json::from_str(&raw).map_err(|_| store::incompatible())
}

fn check_page_bytes(page: &impl Serialize) -> Result<(), ImportError> {
    let bytes = serde_json::to_vec(page).map_err(|_| store::incompatible())?;
    if bytes.len() > super::budget::READ_PAGE_TEXT_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "source group page exceeds byte budget; request a smaller page",
        ));
    }
    Ok(())
}

/// Both limits are ceilings, not promised page lengths. Keep room for the
/// bounded IDs/counters/authenticated cursor envelope, and resume BEFORE the
/// first item that would overflow. No item is discarded or rewritten.
fn fits_page(item: &impl Serialize, used: &mut usize) -> Result<bool, ImportError> {
    let bytes = serde_json::to_vec(item)
        .map_err(|_| store::incompatible())?
        .len()
        + 1;
    let capacity = super::budget::READ_PAGE_TEXT_BYTES - 4096;
    if used.saturating_add(bytes) > capacity {
        if *used == 0 {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "source member cannot fit the bounded metadata page",
            ));
        }
        return Ok(false);
    }
    *used += bytes;
    Ok(true)
}

fn progress(conn: &Connection) -> Result<(bool, u64), ImportError> {
    let (state, version): (String, u64) = conn
        .query_row("SELECT state,version FROM meta WHERE id=1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(sqlite_storage_error)?;
    Ok((
        matches!(
            state.as_str(),
            "completed" | "failed" | "cancelled" | "expired"
        ),
        version,
    ))
}

fn decode_after(cursor: Option<&str>, scope: CursorScope<'_>) -> Result<u32, ImportError> {
    cursor
        .filter(|s| !s.is_empty())
        .map(|s| decode_bound_cursor(s, scope).map(|c| c.offset))
        .transpose()
        .map(|offset| offset.unwrap_or(0))
}

fn next_cursor(
    scope: CursorScope<'_>,
    after: u32,
    more: bool,
    complete: bool,
) -> Result<Option<String>, ImportError> {
    if more || !complete {
        Ok(Some(encode_bound_cursor(scope, after, None)?))
    } else {
        Ok(None)
    }
}

pub fn page(
    scan_id: &str,
    epoch: u64,
    cursor: Option<&str>,
    limit: Option<usize>,
    filter: &SourceGroupFilter,
) -> Result<SourceGroupPage, ImportError> {
    if filter
        .source_id
        .as_ref()
        .is_some_and(|s| s.is_empty() || s.len() > 4096)
    {
        return Err(stale("invalid source group filter"));
    }
    let conn = super::scan::open_scan_readonly(scan_id, epoch)?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    store::verify(&tx)?;
    let snapshot = format!("{epoch}:source-groups:{}", store::SCHEMA_VERSION);
    let filter_key = super::hash::options_hash(&serde_json::json!(["groups", filter]));
    let scope = CursorScope {
        owner: scan_id,
        snapshot: &snapshot,
        leaf: "groups",
        filter: &filter_key,
    };
    let after = decode_after(cursor, scope)?;
    let cap = super::budget::page_limit(limit);
    let provider = filter.provider.map(|p| p.as_str());
    let total: u32 = tx.query_row("SELECT count(*) FROM source_groups WHERE (?1 IS NULL OR provider=?1) AND (?2 IS NULL OR source_id=?2)", params![provider, filter.source_id], |row| row.get(0)).map_err(sqlite_storage_error)?;
    let mut query = tx.prepare("SELECT g.seq,g.group_id,g.source_id,g.provider,g.host,g.identity,g.proof,c.json,
        (SELECT count(*) FROM source_group_members m WHERE m.group_id=g.group_id)
        FROM source_groups g LEFT JOIN source_group_members first ON first.seq=g.seq AND first.group_id=g.group_id
        LEFT JOIN candidates c ON c.seq=first.seq AND c.candidate_id=first.candidate_id
        WHERE g.seq>?1 AND (?2 IS NULL OR g.provider=?2) AND (?3 IS NULL OR g.source_id=?3)
        ORDER BY g.seq LIMIT ?4").map_err(sqlite_storage_error)?;
    let rows = query
        .query_map(params![after, provider, filter.source_id, cap + 1], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, u32>(8)?,
            ))
        })
        .map_err(sqlite_storage_error)?;
    let mut groups = Vec::new();
    let mut bytes = 0;
    let mut last = after;
    let mut more = false;
    for row in rows {
        let (seq, id, source_id, provider, host, identity, proof, raw, count) =
            row.map_err(sqlite_storage_error)?;
        if groups.len() == cap {
            more = true;
            break;
        }
        let reference = parse_reference(raw)?;
        super::source_member::validate_scan_member(scan_id, epoch, &id, &reference)?;
        if count == 0
            || count as usize > super::budget::DISCOVER_BUDGET
            || store::group_id(scan_id, &reference) != id
            || source_id != reference.source_id
            || provider != reference.candidate.provider.as_str()
            || host
                != reference
                    .location
                    .as_ref()
                    .map(|l| l.host.as_str())
                    .unwrap_or("local")
            || identity != reference.candidate.native_session_id
            || proof != store::proof(&reference)
        {
            return Err(store::incompatible());
        }
        let group = SourceGroup {
            group_id: id,
            source_id,
            provider: reference.candidate.provider,
            host,
            native_session_id: identity,
            proof,
            first_member: reference.candidate,
            member_count: count,
        };
        if !fits_page(&group, &mut bytes)? {
            more = true;
            break;
        }
        last = seq;
        groups.push(group);
    }
    let (complete, state_version) = progress(&tx)?;
    let page = SourceGroupPage {
        version: store::SCHEMA_VERSION,
        groups,
        next_cursor: next_cursor(scope, last, more, complete)?,
        total_count: total,
        complete,
        state_version,
    };
    check_page_bytes(&page)?;
    Ok(page)
}

fn check_group_id(scan_id: &str, group_id: &str) -> Result<(), ImportError> {
    let prefix = format!("group:{scan_id}:");
    if !group_id
        .strip_prefix(&prefix)
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(stale("source group belongs to another scan or is invalid"));
    }
    Ok(())
}

/// One indexed, immutable metadata lookup. This never reads a source file,
/// enumerates member pages, initializes a selector or chooses a fallback.
pub fn member(
    scan_id: &str,
    epoch: u64,
    group_id: &str,
    candidate_id: &str,
) -> Result<SourceGroupMember, ImportError> {
    check_group_id(scan_id, group_id)?;
    if !candidate_id
        .strip_prefix(&format!("candidate:{scan_id}:"))
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(stale("source member belongs to another scan or is invalid"));
    }
    let conn = super::scan::open_scan_readonly(scan_id, epoch)?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    store::verify(&tx)?;
    let row = tx
        .query_row(
            "SELECT CASE WHEN length(CAST(c.json AS BLOB))<=?3 THEN c.json END,
            length(CAST(c.json AS BLOB)),g.source_id,g.provider,g.host,g.identity,g.proof
            FROM source_group_members m JOIN source_groups g ON g.group_id=m.group_id
            LEFT JOIN candidates c ON c.seq=m.seq AND c.candidate_id=m.candidate_id
            WHERE m.group_id=?1 AND m.candidate_id=?2",
            params![group_id, candidate_id, super::budget::READ_PAGE_TEXT_BYTES],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<usize>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    let (raw, bytes, source, provider, host, identity, proof) = row.ok_or_else(|| {
        ImportError::new(
            ErrorCode::SourceNotFound,
            "source member is not in this scan group",
        )
    })?;
    if bytes.is_some_and(|n| n > super::budget::READ_PAGE_TEXT_BYTES) {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "source member metadata exceeds byte budget",
        ));
    }
    let reference = parse_reference(raw)?;
    super::source_member::validate_scan_member(scan_id, epoch, group_id, &reference)?;
    if reference.candidate.provider == Provider::Cursor
        && reference.location.is_some()
        && reference.source_member.is_none()
    {
        return Err(stale("new Cursor group member lost its fixed source pin"));
    }
    if reference.candidate.candidate_id != candidate_id
        || store::group_id(scan_id, &reference) != group_id
        || reference.source_id != source
        || reference.candidate.provider.as_str() != provider
        || reference.candidate.native_session_id != identity
        || reference
            .location
            .as_ref()
            .map(|l| l.host.as_str())
            .unwrap_or("local")
            != host
        || store::proof(&reference) != proof
    {
        return Err(store::incompatible());
    }
    let member = SourceGroupMember::new(group_id, reference);
    check_page_bytes(&member)?;
    Ok(member)
}

pub fn members(
    scan_id: &str,
    epoch: u64,
    group_id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<SourceGroupMemberPage, ImportError> {
    check_group_id(scan_id, group_id)?;
    let conn = super::scan::open_scan_readonly(scan_id, epoch)?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    store::verify(&tx)?;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM source_groups WHERE group_id=?1)",
            [group_id],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if !exists {
        return Err(ImportError::new(
            ErrorCode::SourceNotFound,
            "source group is not in scan",
        ));
    }
    let snapshot = format!("{epoch}:source-groups:{}", store::SCHEMA_VERSION);
    let scope = CursorScope {
        owner: scan_id,
        snapshot: &snapshot,
        leaf: group_id,
        filter: "members",
    };
    let after = decode_after(cursor, scope)?;
    let cap = super::budget::page_limit(limit);
    let count: u32 = tx
        .query_row(
            "SELECT count(*) FROM source_group_members WHERE group_id=?1",
            [group_id],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if count == 0 || count as usize > super::budget::DISCOVER_BUDGET {
        return Err(store::incompatible());
    }
    let mut query = tx
        .prepare(
            "SELECT m.seq,m.candidate_id,c.json FROM source_group_members m
        LEFT JOIN candidates c ON c.seq=m.seq AND c.candidate_id=m.candidate_id
        WHERE m.group_id=?1 AND m.seq>?2 ORDER BY m.seq LIMIT ?3",
        )
        .map_err(sqlite_storage_error)?;
    let rows = query
        .query_map(params![group_id, after, cap + 1], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(sqlite_storage_error)?;
    let mut members = Vec::new();
    let mut bytes = 0;
    let mut last = after;
    let mut more = false;
    for row in rows {
        let (seq, id, raw) = row.map_err(sqlite_storage_error)?;
        if members.len() == cap {
            more = true;
            break;
        }
        let reference = parse_reference(raw)?;
        super::source_member::validate_scan_member(scan_id, epoch, group_id, &reference)?;
        if reference.candidate.candidate_id != id
            || store::group_id(scan_id, &reference) != group_id
        {
            return Err(store::incompatible());
        }
        let member = SourceGroupMember::new(group_id, reference);
        if !fits_page(&member, &mut bytes)? {
            more = true;
            break;
        }
        last = seq;
        members.push(member);
    }
    let (complete, state_version) = progress(&tx)?;
    let page = SourceGroupMemberPage {
        version: store::SCHEMA_VERSION,
        group_id: group_id.into(),
        members,
        next_cursor: next_cursor(scope, last, more, complete)?,
        member_count: count,
        complete,
        state_version,
    };
    check_page_bytes(&page)?;
    Ok(page)
}
