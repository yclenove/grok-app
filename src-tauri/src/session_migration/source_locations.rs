//! Source location references, separate from the explicit relocation operation.
//! Primary IDs derive from persisted installation IDs, never current paths.
use super::errors::{sqlite_storage_error, ErrorCode, ImportError};
use super::source_location_store as store;
use super::sources::{self, SourceRegistration};
use super::types::Provider;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const LOCATION_REFERENCE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceAssociationRequest {
    pub source_id: String,
    pub provider: Provider,
    pub host: String,
    pub path: String,
    pub expected_primary_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceLocation {
    pub version: u32,
    pub location_id: String,
    pub source_id: String,
    pub provider: Provider,
    pub host: String,
    pub root_path: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLocationPage {
    pub locations: Vec<SourceLocation>,
    pub next_cursor: Option<String>,
    pub total_count: u32,
}

fn local_registration(
    conn: &Connection,
    source_id: &str,
    provider: Provider,
    host: &str,
) -> Result<SourceRegistration, ImportError> {
    if host != "local" {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "source locations currently require a local filesystem host",
        ));
    }
    let source = sources::get(conn, source_id)?;
    let stored_host: Option<String> = conn
        .query_row(
            "SELECT host FROM sources WHERE installation_id=?1",
            [source_id],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if source.provider != provider || stored_host.as_deref() != Some(host) {
        return Err(ImportError::new(
            ErrorCode::SourceNotFound,
            "source installation/provider/host does not match",
        ));
    }
    store::verify_owners(conn)?;
    Ok(source)
}

fn primary(source: &SourceRegistration) -> SourceLocation {
    SourceLocation {
        version: LOCATION_REFERENCE_VERSION,
        location_id: format!("primary:{}", source.source_id),
        source_id: source.source_id.clone(),
        provider: source.provider,
        host: "local".into(),
        root_path: source.root_path.clone(),
        kind: "primary".into(),
    }
}

fn associated_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceLocation> {
    Ok(SourceLocation {
        version: LOCATION_REFERENCE_VERSION,
        location_id: row.get(0)?,
        source_id: row.get(1)?,
        provider: Provider::Cursor,
        host: row.get(2)?,
        root_path: row.get(3)?,
        kind: "associated".into(),
    })
}

fn validate_associated(conn: &Connection, location: &SourceLocation) -> Result<(), ImportError> {
    if uuid::Uuid::parse_str(&location.location_id).is_err()
        || location.host != "local"
        || location.root_path.is_empty()
        || location.root_path.len() > 8192
    {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "associated source location is invalid",
        ));
    }
    let key: String = conn
        .query_row(
            "SELECT path_key FROM source_locations WHERE location_id=?1",
            [&location.location_id],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if key != sources::identity_key(&location.root_path) {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "associated source path key has changed",
        ));
    }
    store::guard_associated_path(
        conn,
        location.provider,
        &location.root_path,
        Some(&location.location_id),
    )
}

fn find_associated(
    conn: &Connection,
    source_id: &str,
    location_id: &str,
) -> Result<Option<SourceLocation>, ImportError> {
    if !store::verify_schema(conn)? {
        return Ok(None);
    }
    conn.query_row(
        "SELECT location_id,source_id,host,root_path FROM source_locations
        WHERE source_id=?1 AND location_id=?2 AND provider='cursor' AND host='local'",
        params![source_id, location_id],
        associated_row,
    )
    .optional()
    .map_err(sqlite_storage_error)
}

/// Explicit association is not relocation. Sources/profile/aliases stay intact.
/// Cursor is the first supported multi-root adapter; other providers retain
/// their existing single-root identity contract until separately implemented.
pub fn associate(
    conn: &Connection,
    request: &SourceAssociationRequest,
) -> Result<SourceLocation, ImportError> {
    if request.provider != Provider::Cursor {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "associated roots currently support Cursor only",
        ));
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(sqlite_storage_error)?;
    let source = local_registration(&tx, &request.source_id, request.provider, &request.host)?;
    if !sources::same_location(&source.root_path, &request.expected_primary_root) {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "primary source changed before explicit association",
        ));
    }
    let root = sources::canonical_root(Path::new(&request.path))?;
    if !root.is_dir() {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "associated source must be a directory",
        ));
    }
    let root_path = root
        .to_str()
        .filter(|p| p.len() <= 8192)
        .ok_or_else(|| {
            ImportError::new(
                ErrorCode::ResourceLimit,
                "associated source path exceeds metadata budget",
            )
        })?
        .to_string();
    let path_key = sources::identity_key(&root_path);
    if store::verify_schema(&tx)? {
        let existing: Option<SourceLocation> = tx.query_row(
            "SELECT location_id,source_id,host,root_path FROM source_locations WHERE provider='cursor' AND host='local' AND path_key=?1",
            [&path_key], associated_row,
        ).optional().map_err(sqlite_storage_error)?;
        if let Some(existing) = existing {
            if existing.source_id != source.source_id {
                return Err(ImportError::new(
                    ErrorCode::MappingConflict,
                    "path belongs to another source installation",
                ));
            }
            validate_associated(&tx, &existing)?;
            return Ok(existing);
        }
    }
    store::guard_associated_path(&tx, request.provider, &root_path, None)?;
    store::initialize(&tx)?;
    store::pin_owner(&tx, &source.source_id)?;
    let location_id = uuid::Uuid::new_v4().to_string();
    tx.execute("INSERT INTO source_locations(location_id,source_id,provider,host,root_path,path_key,evidence)
        VALUES(?1,?2,'cursor','local',?3,?4,'explicit-association-v1')",
        params![location_id, source.source_id, root_path, path_key]).map_err(sqlite_storage_error)?;
    let result = find_associated(&tx, &source.source_id, &location_id)?.ok_or_else(|| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "associated location was not stored",
        )
    })?;
    tx.commit().map_err(sqlite_storage_error)?;
    Ok(result)
}

/// Signed keyset pagination, primary first. Changes invalidate the old page
/// generation; reads never create extension tables or alter aliases.
pub fn page(
    conn: &Connection,
    source_id: &str,
    provider: Provider,
    host: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<SourceLocationPage, ImportError> {
    let cap = super::budget::page_limit(limit);
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    let source = local_registration(&tx, source_id, provider, host)?;
    let extended = store::verify_schema(&tx)?;
    let (count, last): (i64, i64) = if extended {
        tx.query_row(
            "SELECT count(*),coalesce(max(seq),0) FROM source_locations WHERE source_id=?1",
            [source_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sqlite_storage_error)?
    } else {
        (0, 0)
    };
    let total_count = count
        .checked_add(1)
        .and_then(|count| u32::try_from(count).ok())
        .ok_or_else(|| {
            ImportError::new(
                ErrorCode::ResourceLimit,
                "source location count exceeds page budget",
            )
        })?;
    let snapshot = super::hash::options_hash(&serde_json::json!([
        "source-locations-v1",
        extended,
        last,
        count,
        source.root_path
    ]));
    let filter =
        serde_json::json!([provider.as_str(), host, LOCATION_REFERENCE_VERSION]).to_string();
    let scope = super::cursor::CursorScope {
        owner: source_id,
        snapshot: &snapshot,
        leaf: "source-locations",
        filter: &filter,
    };
    let decoded = cursor
        .filter(|s| !s.is_empty())
        .map(|s| super::cursor::decode_bound_cursor(s, scope))
        .transpose()?;
    let after = match &decoded {
        None => None,
        Some(c) => Some(
            c.mark
                .as_deref()
                .and_then(|s| s.parse::<i64>().ok())
                .filter(|v| *v >= 0)
                .ok_or_else(|| {
                    ImportError::new(ErrorCode::PlanStale, "invalid location page mark")
                })?,
        ),
    };
    let offset = decoded.as_ref().map(|c| c.offset).unwrap_or(0);
    if offset >= total_count {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "source location page is outside its generation",
        ));
    }
    if let Some(after) = after {
        let prior: u32 = if extended {
            tx.query_row(
                "SELECT count(*) FROM source_locations WHERE source_id=?1 AND seq<=?2",
                params![source_id, after],
                |row| row.get(0),
            )
            .map_err(sqlite_storage_error)?
        } else {
            0
        };
        if after > last || prior.checked_add(1) != Some(offset) {
            return Err(ImportError::new(
                ErrorCode::PlanStale,
                "source location page mark is inconsistent",
            ));
        }
    }
    let mut locations = Vec::with_capacity(cap);
    let mut mark = after.unwrap_or(0);
    if decoded.is_none() {
        locations.push(primary(&source));
    }
    if extended && locations.len() < cap {
        let mut stmt = tx.prepare("SELECT seq,location_id,source_id,host,root_path FROM source_locations WHERE source_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3").map_err(sqlite_storage_error)?;
        let mut rows = stmt
            .query(params![source_id, mark, (cap - locations.len()) as i64])
            .map_err(sqlite_storage_error)?;
        while let Some(row) = rows.next().map_err(sqlite_storage_error)? {
            mark = row.get(0).map_err(sqlite_storage_error)?;
            let location = SourceLocation {
                version: LOCATION_REFERENCE_VERSION,
                location_id: row.get(1).map_err(sqlite_storage_error)?,
                source_id: row.get(2).map_err(sqlite_storage_error)?,
                provider: Provider::Cursor,
                host: row.get(3).map_err(sqlite_storage_error)?,
                root_path: row.get(4).map_err(sqlite_storage_error)?,
                kind: "associated".into(),
            };
            validate_associated(&tx, &location)?;
            locations.push(location);
        }
    }
    let next_offset = offset + locations.len() as u32;
    if locations.is_empty() || next_offset > total_count {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "source location page generation is inconsistent",
        ));
    }
    let next_cursor = if next_offset < total_count {
        Some(super::cursor::encode_bound_cursor(
            scope,
            next_offset,
            Some(mark.to_string()),
        )?)
    } else {
        None
    };
    Ok(SourceLocationPage {
        locations,
        next_cursor,
        total_count,
    })
}

pub fn resolve(
    conn: &Connection,
    source_id: &str,
    provider: Provider,
    host: &str,
    location_id: &str,
    path: &Path,
) -> Result<SourceRegistration, ImportError> {
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    let mut source = local_registration(&tx, source_id, provider, host)?;
    let mut location = primary(&source);
    let associated = location.location_id != location_id;
    if associated {
        location = find_associated(&tx, source_id, location_id)?
            .filter(|l| l.provider == provider && l.host == host)
            .ok_or_else(|| {
                ImportError::new(
                    ErrorCode::SourceNotFound,
                    "source location is not registered to this installation",
                )
            })?;
        validate_associated(&tx, &location)?;
    }
    let canonical = sources::canonical_root(path)?
        .to_string_lossy()
        .into_owned();
    let paths = if associated {
        vec![location.root_path.clone()]
    } else {
        sources::registered_paths(&tx, &source)?
    };
    if !paths
        .iter()
        .any(|saved| sources::same_location(saved, &canonical))
    {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "source location path does not match its registered root or aliases",
        ));
    }
    // A saved alias may still be readable. Pin the exact selected directory;
    // never overwrite the registration's current primary root while scanning.
    source.root_path = canonical.clone();
    location.root_path = canonical;
    source.location = Some(location);
    Ok(source)
}

pub(crate) fn resolve_scan(
    conn: &Connection,
    request: &super::scan::ScanSourcePath,
    provider: Provider,
) -> Result<SourceRegistration, ImportError> {
    let host = request.host.as_deref().unwrap_or("local");
    if host != "local" {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "source scan requires a local filesystem host",
        ));
    }
    match request.location_id.as_deref() {
        Some(location_id) => {
            let source_id = request.source_id.as_deref().ok_or_else(|| {
                ImportError::new(
                    ErrorCode::MappingConflict,
                    "source location requires an installation ID",
                )
            })?;
            resolve(
                conn,
                source_id,
                provider,
                host,
                location_id,
                Path::new(&request.path),
            )
        }
        None => {
            let mut source = sources::register(
                conn,
                provider,
                Path::new(&request.path),
                request.source_id.as_deref(),
                &request.profile,
            )?;
            // New scans pin ordinary primary roots too. This does not change
            // registration/relocation or the primary native identity scope.
            source.location = Some(primary(&source));
            Ok(source)
        }
    }
}

pub(crate) fn validate_relocation_request(
    request: &super::scan::ScanSourcePath,
) -> Result<(), ImportError> {
    if request.host.as_deref().unwrap_or("local") != "local" {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "source relocation requires a local host",
        ));
    }
    if request.location_id.is_some() {
        return Err(ImportError::new(
            ErrorCode::MappingConflict,
            "a pinned source location is not a relocation request",
        ));
    }
    Ok(())
}

pub(crate) fn validate_reference(reference: &super::scan::CandidateRef) -> Result<(), ImportError> {
    let Some(location) = &reference.location else {
        return Ok(());
    };
    if location.version != LOCATION_REFERENCE_VERSION
        || location.source_id != reference.source_id
        || location.provider != reference.candidate.provider
        || location.root_path != reference.root_path
        || !matches!(location.kind.as_str(), "primary" | "associated")
    {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "fixed source location reference is inconsistent",
        ));
    }
    let conn = super::catalog::open_catalog_readonly()?;
    let resolved = resolve(
        &conn,
        &location.source_id,
        location.provider,
        &location.host,
        &location.location_id,
        Path::new(&location.root_path),
    )?;
    if resolved.location.as_ref().map(|l| l.kind.as_str()) != Some(location.kind.as_str()) {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "fixed location kind differs from its registration",
        ));
    }
    Ok(())
}

/// Validate only persisted registration metadata. Prepared recovery must not
/// canonicalize or read an offline source directory.
pub(crate) fn validate_registered(location: &SourceLocation) -> Result<(), ImportError> {
    if location.version != LOCATION_REFERENCE_VERSION {
        return Err(ImportError::new(
            ErrorCode::SchemaUnsupported,
            "unsupported fixed location version",
        ));
    }
    let conn = super::catalog::open_catalog_readonly()?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    validate_registered_with(&tx, location)
}

/// Reuse the caller's catalog snapshot when location facts authorize other
/// catalog facts. Never validate those two halves in independent snapshots.
pub(crate) fn validate_registered_with(
    conn: &Connection,
    location: &SourceLocation,
) -> Result<(), ImportError> {
    if conn.is_autocommit() {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "source location validation requires a catalog snapshot",
        ));
    }
    if location.version != LOCATION_REFERENCE_VERSION {
        return Err(ImportError::new(
            ErrorCode::SchemaUnsupported,
            "unsupported fixed location version",
        ));
    }
    let source = local_registration(conn, &location.source_id, location.provider, &location.host)?;
    match location.kind.as_str() {
        "primary" if location.location_id == primary(&source).location_id => {
            if !sources::registered_paths(conn, &source)?
                .iter()
                .any(|root| sources::same_location(root, &location.root_path))
            {
                return Err(ImportError::new(
                    ErrorCode::SourceChanged,
                    "fixed primary is not a registered root or alias",
                ));
            }
        }
        "associated" => {
            let stored = find_associated(conn, &location.source_id, &location.location_id)?
                .ok_or_else(|| {
                    ImportError::new(
                        ErrorCode::SourceNotFound,
                        "fixed associated location missing",
                    )
                })?;
            if stored != *location {
                return Err(ImportError::new(
                    ErrorCode::SourceChanged,
                    "fixed associated registration changed",
                ));
            }
            validate_associated(conn, &stored)?;
        }
        _ => {
            return Err(ImportError::new(
                ErrorCode::SourceChanged,
                "fixed location kind or identity changed",
            ))
        }
    }
    Ok(())
}

impl SourceLocation {
    /// Primary preserves all 1.0.5 native identities. Only newly associated
    /// fallback identities acquire an independent stable location scope.
    pub(crate) fn identity_scope(&self) -> Option<&str> {
        (self.kind == "associated").then_some(self.location_id.as_str())
    }
}
