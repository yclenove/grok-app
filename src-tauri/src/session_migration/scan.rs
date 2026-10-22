//! Background discovery with a disk-backed candidate list and frozen selections.

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "source_location_scan_tests.rs"]
mod source_location_source_fidelity_tests;

#[cfg(test)]
#[path = "source_location_association_tests.rs"]
mod source_location_association_source_fidelity_tests;

#[cfg(test)]
#[path = "scan_group_tests.rs"]
mod scan_group_source_fidelity_tests;

#[cfg(test)]
#[path = "group_selection_tests.rs"]
mod group_selection_source_fidelity_tests;

use super::cursor::{decode_bound_cursor, encode_bound_cursor, CursorScope};
use super::errors::{ErrorCode, ImportError};
use super::types::{Provider, SourceCandidate};
use crate::session_migration::sqlite_open;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSourcePath {
    pub provider: String,
    pub path: String,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default)]
    pub profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanIssue {
    pub provider: String,
    pub path: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanState {
    pub scan_id: String,
    pub epoch: u64,
    pub candidates: Vec<SourceCandidate>,
    pub extra_candidates: Vec<SourceCandidate>,
    pub issues: Vec<ScanIssue>,
    pub source_paths: Vec<ScanSourcePath>,
    pub complete: bool,
    pub discovered_count: u32,
    pub state: String,
    pub state_version: u64,
    pub issue_count: u32,
    pub extra_count: u32,
    pub selection: super::scan_selection::CandidateSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateRef {
    pub candidate: SourceCandidate,
    pub source_id: String,
    pub root_path: String,
    pub legacy_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<super::source_locations::SourceLocation>,
    #[serde(
        default,
        rename = "sourceMember",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_member: Option<super::source_member::SourceMemberPin>,
}

struct Control {
    path: PathBuf,
    cancel: AtomicBool,
    running: AtomicBool,
    worker: Mutex<Option<Worker>>,
}

struct Worker {
    sources: std::vec::IntoIter<ScanSourcePath>,
    active: Option<(super::sources::SourceRegistration, super::discovery::Walker)>,
    composer: Option<super::composer_discovery::ComposerDiscovery>,
}

struct Running<'a>(&'a AtomicBool);
impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

static SCANS: OnceLock<Mutex<HashMap<String, Arc<Control>>>> = OnceLock::new();

fn failure(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
}

fn controls() -> &'static Mutex<HashMap<String, Arc<Control>>> {
    SCANS.get_or_init(Default::default)
}

fn control(id: &str) -> Result<Arc<Control>, ImportError> {
    let control = controls()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(id)
        .cloned()
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "scan expired; scan again"))?;
    if !control.path.starts_with(super::catalog::imports_dir()) {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "scan belongs to another app data root",
        ));
    }
    Ok(control)
}

fn open(control: &Control) -> Result<Connection, ImportError> {
    let conn = sqlite_open::open(&control.path).map_err(failure)?;
    conn.busy_timeout(std::time::Duration::from_millis(1000))
        .map_err(failure)?;
    Ok(conn)
}

fn validate_epoch(epoch: u64) -> Result<(), ImportError> {
    if epoch != 1 {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "scan epoch mismatch",
        ));
    }
    Ok(())
}

pub fn begin(sources: Vec<ScanSourcePath>) -> Result<ScanState, ImportError> {
    // Create/validate the catalog root before resolving a new scan child on
    // Windows, whose canonical paths include the extended-length prefix.
    drop(super::catalog::open_catalog()?);
    let sources = if sources.is_empty() {
        super::detect::default_roots()
            .into_iter()
            .filter(|r| r.exists)
            .map(|r| ScanSourcePath {
                provider: r.provider.as_str().into(),
                path: r.path,
                source_id: None,
                location_id: None,
                host: None,
                profile: "default-local".into(),
            })
            .collect()
    } else {
        sources
    };
    for source in &sources {
        if Provider::parse(&source.provider).is_none() {
            return Err(ImportError::new(
                ErrorCode::FormatUnsupported,
                "unknown source provider",
            ));
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let root = super::catalog::imports_dir().join("scans");
    super::catalog::ensure_inside_imports(&root)?;
    std::fs::create_dir_all(&root).map_err(failure)?;
    let control = Arc::new(Control {
        path: root.join(format!("{id}.sqlite")),
        cancel: AtomicBool::new(false),
        running: AtomicBool::new(false),
        worker: Mutex::new(Some(Worker {
            sources: sources.clone().into_iter(),
            active: None,
            composer: None,
        })),
    });
    let conn = open(&control)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
        CREATE TABLE meta(id INTEGER PRIMARY KEY, sources TEXT NOT NULL, state TEXT NOT NULL, version INTEGER NOT NULL, frozen INTEGER, issue_count INTEGER NOT NULL DEFAULT 0, group_schema_version INTEGER NOT NULL DEFAULT 1, group_selection_schema_version INTEGER NOT NULL DEFAULT 1);
        CREATE TABLE candidates(seq INTEGER PRIMARY KEY, candidate_id TEXT NOT NULL UNIQUE, source_key TEXT NOT NULL UNIQUE, json TEXT NOT NULL);
        CREATE TABLE issues(seq INTEGER PRIMARY KEY, json TEXT NOT NULL);").map_err(failure)?;
    super::scan_selection::initialize(&conn, &id, 1)?;
    conn.execute(
        "INSERT INTO meta(id,sources,state,version) VALUES(1,?1,'queued',1)",
        params![serde_json::to_string(&sources).map_err(failure)?],
    )
    .map_err(failure)?;
    super::scan_group_store::initialize(&conn)?;
    super::group_selection::initialize(&conn, &id, 1)?;
    controls()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(id.clone(), control);
    state(&id, 1)
}

pub fn start_background(sources: Vec<ScanSourcePath>) -> Result<ScanState, ImportError> {
    let result = begin(sources)?;
    let id = result.scan_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = run(&id);
    });
    Ok(result)
}

fn set_state(conn: &Connection, state: &str) -> Result<(), ImportError> {
    conn.execute(
        "UPDATE meta SET state=?1,version=version+1 WHERE id=1",
        params![state],
    )
    .map_err(failure)?;
    Ok(())
}

fn issue(
    conn: &Connection,
    provider: &str,
    path: &str,
    error: &ImportError,
) -> Result<(), ImportError> {
    let issue = ScanIssue {
        provider: provider.into(),
        path: path.into(),
        code: error.code.as_str().into(),
        message: error.message.clone(),
    };
    // Bound diagnostic storage as well as each IPC page. Counts remain in DB.
    let tx = conn.unchecked_transaction().map_err(failure)?;
    tx.execute(
        "INSERT INTO issues(json) SELECT ?1 WHERE (SELECT COUNT(*) FROM issues)<100",
        params![serde_json::to_string(&issue).map_err(failure)?],
    )
    .map_err(failure)?;
    tx.execute(
        "UPDATE meta SET issue_count=issue_count+1,version=version+1 WHERE id=1",
        [],
    )
    .map_err(failure)?;
    tx.commit().map_err(failure)?;
    Ok(())
}

fn add(
    conn: &Connection,
    scan_id: &str,
    source: &super::sources::SourceRegistration,
    mut candidate: SourceCandidate,
) -> Result<(), ImportError> {
    let legacy_id = candidate.candidate_id.clone();
    let key = super::hash::options_hash(&serde_json::json!([source.source_id, candidate.locator]));
    candidate.candidate_id = format!("candidate:{scan_id}:{key}");
    let mut reference = CandidateRef {
        candidate,
        source_id: source.source_id.clone(),
        root_path: source.root_path.clone(),
        legacy_id,
        location: source.location.clone(),
        source_member: None,
    };
    reference.source_member = super::source_member::capture(scan_id, &reference)?;
    let tx = conn.unchecked_transaction().map_err(failure)?;
    let inserted = tx
        .execute(
            "INSERT OR IGNORE INTO candidates(candidate_id,source_key,json) VALUES(?1,?2,?3)",
            params![
                reference.candidate.candidate_id,
                key,
                serde_json::to_string(&reference).map_err(failure)?
            ],
        )
        .map_err(failure)?;
    if inserted != 0 {
        super::scan_group_store::insert_member(&tx, scan_id, tx.last_insert_rowid(), &reference)?;
    }
    tx.execute("UPDATE meta SET version=version+1 WHERE id=1", [])
        .map_err(failure)?;
    tx.commit().map_err(failure)?;
    Ok(())
}

pub fn run(id: &str) -> Result<(), ImportError> {
    run_slice(id, super::budget::DISCOVER_BUDGET)
}

fn run_slice(id: &str, budget: usize) -> Result<(), ImportError> {
    let control = control(id)?;
    if control.running.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let _running = Running(&control.running);
    let conn = open(&control)?;
    let mut worker = control.worker.lock().unwrap_or_else(|p| p.into_inner());
    let Some(progress) = worker.as_mut() else {
        return Ok(());
    };
    if control.cancel.load(Ordering::Acquire) {
        *worker = None;
        return set_state(&conn, "cancelled");
    }
    set_state(&conn, "scanning")?;
    #[cfg(test)]
    super::preview_lifecycle_tests::after_publish_point("scan_started");
    let result = discover(&conn, id, &control, progress, budget);
    if let Err(error) = &result {
        let _ = issue(&conn, "", "", error);
    }
    let status = if control.cancel.load(Ordering::Acquire) {
        "cancelled"
    } else {
        match &result {
            Ok(true) => "completed",
            Ok(false) => "paused",
            Err(_) => "failed",
        }
    };
    if status != "paused" {
        *worker = None;
    }
    set_state(&conn, status)?;
    #[cfg(test)]
    super::preview_lifecycle_tests::after_publish_point("scan_finished");
    result.map(|_| ())
}

/// Retain only traversal handles and one DB snapshot between explicit slices.
/// Every entry, including a nonmatching file, consumes traversal budget.
fn discover(
    conn: &Connection,
    id: &str,
    control: &Control,
    worker: &mut Worker,
    budget: usize,
) -> Result<bool, ImportError> {
    let catalog = super::catalog::open_catalog()?;
    let mut found = 0usize;
    let mut visited = 0usize;
    while found < budget && visited < budget.saturating_mul(2) {
        if control.cancel.load(Ordering::Acquire) {
            return Ok(true);
        }
        if worker.active.is_none() {
            let Some(request) = worker.sources.next() else {
                return Ok(true);
            };
            visited += 1;
            let provider =
                Provider::parse(&request.provider).ok_or_else(|| failure("invalid provider"))?;
            let registration = super::source_locations::resolve_scan(&catalog, &request, provider);
            let prepared = registration.and_then(|source| {
                super::discovery::Walker::new(provider, Path::new(&source.root_path))
                    .map(|walker| (source, walker))
            });
            match prepared {
                Ok(active) => worker.active = Some(active),
                Err(error) => {
                    issue(conn, &request.provider, &request.path, &error)?;
                    continue;
                }
            }
        }
        let (source, walker) = worker
            .active
            .as_mut()
            .ok_or_else(|| failure("scan source missing"))?;
        let provider = source.provider;
        visited += 1;
        if let Some(composer) = worker.composer.as_mut() {
            match composer.next_candidate() {
                Ok(Some(candidate)) => {
                    add(conn, id, source, candidate)?;
                    found += 1;
                }
                Ok(None) => worker.composer = None,
                Err(error) => {
                    issue(conn, provider.as_str(), &source.root_path, &error)?;
                    worker.composer = None;
                }
            }
            continue;
        }
        let artifact = match walker.step() {
            None => {
                worker.active = None;
                continue;
            }
            Some(Ok(None)) => continue,
            Some(Ok(Some(path))) => path,
            Some(Err(error)) => {
                issue(conn, provider.as_str(), &source.root_path, &error)?;
                continue;
            }
        };
        let filename = artifact.file_name().and_then(|s| s.to_str());
        if provider == Provider::Cursor && filename == Some("state.vscdb") {
            match super::composer_discovery::ComposerDiscovery::open(&artifact) {
                Ok(composer) => worker.composer = Some(composer),
                Err(error) => issue(conn, provider.as_str(), &artifact.to_string_lossy(), &error)?,
            }
            continue;
        }
        let candidate = if provider == Provider::Cursor && filename == Some("store.db") {
            super::adapters::cursor::store_identity_scoped(
                &artifact,
                Some(Path::new(&source.root_path)),
                source.location.as_ref().and_then(|l| l.identity_scope()),
            )
            .and_then(|identity| {
                super::sqlite_ro::open_readonly(&artifact).map(|snapshot| SourceCandidate {
                    candidate_id: format!(
                        "cursor-store:{}",
                        artifact
                            .parent()
                            .unwrap_or(Path::new(&source.root_path))
                            .display()
                    ),
                    provider,
                    native_session_id: identity,
                    title: artifact
                        .parent()
                        .and_then(|p| p.file_name())
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    cwd: None,
                    started_at: None,
                    updated_at: None,
                    locator: artifact.to_string_lossy().into_owned(),
                    source_revision: snapshot.source_revision(),
                    archived: None,
                    completeness: "store".into(),
                })
            })
        } else {
            super::discovery::file_candidate_in_location(
                provider,
                &artifact,
                Path::new(&source.root_path),
                source.location.as_ref().and_then(|l| l.identity_scope()),
            )
        };
        if control.cancel.load(Ordering::Acquire) {
            return Ok(true);
        }
        match candidate {
            Ok(candidate) => {
                add(conn, id, source, candidate)?;
                #[cfg(test)]
                super::preview_lifecycle_tests::after_publish_point("scan_candidate");
                found += 1;
            }
            Err(error) => issue(conn, provider.as_str(), &artifact.to_string_lossy(), &error)?,
        }
    }
    Ok(false)
}

pub fn state(id: &str, epoch: u64) -> Result<ScanState, ImportError> {
    validate_epoch(epoch)?;
    let control = control(id)?;
    let conn = open(&control)?;
    let (raw, status, version, frozen, issue_count): (String, String, u64, Option<u32>, u32) = conn
        .query_row(
            "SELECT sources,state,version,frozen,issue_count FROM meta WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .map_err(failure)?;
    let count: u32 = conn
        .query_row("SELECT COUNT(*) FROM candidates", [], |r| r.get(0))
        .map_err(failure)?;
    let mut issues_query = conn
        .prepare("SELECT json FROM issues ORDER BY seq LIMIT 100")
        .map_err(failure)?;
    let issues = issues_query
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(failure)?
        .map(|row| serde_json::from_str(&row.map_err(failure)?).map_err(failure))
        .collect::<Result<_, _>>()?;
    Ok(ScanState {
        scan_id: id.into(),
        epoch,
        candidates: rows(&conn, 0, super::budget::PAGE_SIZE, None)?,
        extra_candidates: match frozen {
            Some(bound) => rows(&conn, bound, super::budget::PAGE_SIZE, None)?,
            None => vec![],
        },
        source_paths: serde_json::from_str(&raw).map_err(failure)?,
        issues,
        discovered_count: count,
        complete: matches!(
            status.as_str(),
            "completed" | "failed" | "cancelled" | "expired"
        ),
        state: status,
        state_version: version,
        issue_count,
        extra_count: if let Some(bound) = frozen {
            conn.query_row(
                "SELECT COUNT(*) FROM candidates WHERE seq>?1",
                params![bound],
                |r| r.get(0),
            )
            .map_err(failure)?
        } else {
            0
        },
        selection: super::scan_selection::descriptor_from(&conn, id, epoch, None)?,
    })
}

fn rows(
    conn: &Connection,
    after: u32,
    limit: usize,
    through: Option<u32>,
) -> Result<Vec<SourceCandidate>, ImportError> {
    let mut statement = conn.prepare("SELECT json FROM candidates WHERE seq>?1 AND (?3 IS NULL OR seq<=?3) ORDER BY seq LIMIT ?2").map_err(failure)?;
    let result = statement
        .query_map(params![after, limit, through], |r| r.get::<_, String>(0))
        .map_err(failure)?
        .map(|row| {
            serde_json::from_str::<CandidateRef>(&row.map_err(failure)?)
                .map(|r| r.candidate)
                .map_err(failure)
        })
        .collect();
    result
}

pub fn page(
    id: &str,
    epoch: u64,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<(Vec<SourceCandidate>, Option<String>, u32, bool), ImportError> {
    validate_epoch(epoch)?;
    let control = control(id)?;
    let conn = open(&control)?;
    let epoch_key = epoch.to_string();
    let scope = CursorScope {
        owner: id,
        snapshot: &epoch_key,
        leaf: "",
        filter: "candidates",
    };
    let after = match cursor.filter(|raw| !raw.is_empty()) {
        None => 0,
        Some(raw) => decode_bound_cursor(raw, scope)?.offset,
    };
    let limit = super::budget::page_limit(limit);
    let mut found = rows(&conn, after, limit + 1, None)?;
    let more = found.len() > limit;
    found.truncate(limit);
    let last: u32 = if let Some(candidate) = found.last() {
        conn.query_row(
            "SELECT seq FROM candidates WHERE candidate_id=?1",
            params![candidate.candidate_id],
            |r| r.get(0),
        )
        .map_err(failure)?
    } else {
        after
    };
    let status: String = conn
        .query_row("SELECT state FROM meta WHERE id=1", [], |r| r.get(0))
        .map_err(failure)?;
    let complete = matches!(
        status.as_str(),
        "completed" | "failed" | "cancelled" | "expired"
    );
    let count: u32 = conn
        .query_row("SELECT COUNT(*) FROM candidates", [], |r| r.get(0))
        .map_err(failure)?;
    let next = if more || !complete {
        Some(encode_bound_cursor(scope, last, None)?)
    } else {
        None
    };
    Ok((found, next, count, complete))
}

pub(crate) fn open_scan(id: &str, epoch: u64) -> Result<Connection, ImportError> {
    validate_epoch(epoch)?;
    let control = control(id)?;
    let conn = open(&control)?;
    super::scan_selection::initialize(&conn, id, epoch)?;
    Ok(conn)
}

/// Group queries must never initialize selections or migrate an old scan.
pub(crate) fn open_scan_group_writer(id: &str, epoch: u64) -> Result<Connection, ImportError> {
    validate_epoch(epoch)?;
    let control = control(id)?;
    open(&control)
}

/// Group queries must never initialize selections or migrate an old scan.
pub(crate) fn open_scan_readonly(id: &str, epoch: u64) -> Result<Connection, ImportError> {
    validate_epoch(epoch)?;
    let control = control(id)?;
    let conn =
        sqlite_open::open_with_flags(&control.path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(failure)?;
    conn.busy_timeout(std::time::Duration::from_millis(super::budget::DB_BUSY_MS))
        .map_err(failure)?;
    Ok(conn)
}

pub fn select_current(id: &str, epoch: u64) -> Result<Vec<String>, ImportError> {
    validate_epoch(epoch)?;
    let control = control(id)?;
    let conn = open(&control)?;
    conn.execute("UPDATE meta SET frozen=COALESCE(frozen,(SELECT COALESCE(MAX(seq),0) FROM candidates)) WHERE id=1",[]).map_err(failure)?;
    let mut statement = conn.prepare("SELECT candidate_id FROM candidates WHERE seq<=(SELECT frozen FROM meta WHERE id=1) ORDER BY seq").map_err(failure)?;
    let result = statement
        .query_map([], |r| r.get(0))
        .map_err(failure)?
        .collect::<Result<_, _>>()
        .map_err(failure);
    result
}

pub fn candidate(id: &str) -> Result<CandidateRef, ImportError> {
    let mut parts = id.split(':');
    if parts.next() != Some("candidate") {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "invalid candidate ID",
        ));
    }
    let owner = parts
        .next()
        .ok_or_else(|| ImportError::new(ErrorCode::PlanStale, "missing scan ID"))?;
    let control = control(owner)?;
    let conn = open(&control)?;
    let raw: Option<String> = conn
        .query_row(
            "SELECT json FROM candidates WHERE candidate_id=?1",
            params![id],
            |r| r.get(0),
        )
        .optional()
        .map_err(failure)?;
    serde_json::from_str(
        &raw.ok_or_else(|| ImportError::new(ErrorCode::SourceNotFound, "candidate not in scan"))?,
    )
    .map_err(failure)
}

pub fn cancel(id: &str, epoch: u64) -> Result<ScanState, ImportError> {
    validate_epoch(epoch)?;
    let control = control(id)?;
    let conn = open(&control)?;
    control.cancel.store(true, Ordering::Release);
    let status = if control.running.load(Ordering::Acquire) {
        "cancelling"
    } else {
        "cancelled"
    };
    let changed = conn.execute("UPDATE meta SET state=?1,version=version+1 WHERE id=1 AND state IN ('queued','scanning','paused')",params![status]).map_err(failure)?;
    if changed > 0 {
        if let Ok(mut worker) = control.worker.try_lock() {
            *worker = None;
        }
    }
    state(id, epoch)
}

pub fn continue_background(id: &str, epoch: u64) -> Result<ScanState, ImportError> {
    validate_epoch(epoch)?;
    let control = control(id)?;
    let conn = open(&control)?;
    if conn
        .execute(
            "UPDATE meta SET state='queued',version=version+1 WHERE id=1 AND state='paused'",
            [],
        )
        .map_err(failure)?
        > 0
    {
        let id = id.to_string();
        tauri::async_runtime::spawn_blocking(move || {
            let _ = run(&id);
        });
    }
    state(id, epoch)
}

pub fn refresh_background(id: &str, epoch: u64) -> Result<ScanState, ImportError> {
    let sources = state(id, epoch)?.source_paths;
    cancel(id, epoch)?;
    start_background(sources)
}

fn scans_dir() -> PathBuf {
    super::catalog::imports_dir().join("scans")
}

fn scan_db_path(id: &str) -> PathBuf {
    scans_dir().join(format!("{id}.sqlite"))
}

fn scan_sidecar(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.display()))
}

fn is_reparse(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| {
            if m.file_type().is_symlink() {
                return true;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if m.file_attributes() & 0x400 != 0 {
                    return true;
                }
            }
            false
        })
        .unwrap_or(false)
}

fn owned_scan_db(id: &str, path: &Path) -> bool {
    if uuid::Uuid::parse_str(id).is_err() {
        return false;
    }
    if is_reparse(path) {
        return false;
    }
    super::catalog::ensure_inside_imports(path).is_ok()
        && path.starts_with(scans_dir())
        && path.file_name().and_then(|n| n.to_str()) == Some(&format!("{id}.sqlite"))
}

fn remove_scan_files(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(scan_sidecar(path, "-wal"));
    let _ = std::fs::remove_file(scan_sidecar(path, "-shm"));
}

/// Drop in-memory ownership so leftover files look like a process restart.
#[cfg(test)]
pub fn forget_for_restart(id: &str) {
    controls()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(id);
}

/// Reopen scan DBs that this process no longer owns. Incomplete states become
/// `expired` because the walker cannot be restored.
pub fn adopt_orphaned_scans() -> Result<u32, ImportError> {
    let root = scans_dir();
    if !root.is_dir() {
        return Ok(0);
    }
    super::catalog::ensure_inside_imports(&root)?;
    let mut adopted = 0u32;
    for entry in std::fs::read_dir(&root).map_err(failure)? {
        let entry = entry.map_err(failure)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(id) = name.strip_suffix(".sqlite") else {
            continue;
        };
        if name.ends_with("-wal") || name.ends_with("-shm") {
            continue;
        }
        let path = scan_db_path(id);
        if !owned_scan_db(id, &path) {
            continue;
        }
        {
            let map = controls().lock().unwrap_or_else(|p| p.into_inner());
            if map.contains_key(id) {
                continue;
            }
        }
        let control = Arc::new(Control {
            path: path.clone(),
            cancel: AtomicBool::new(true),
            running: AtomicBool::new(false),
            worker: Mutex::new(None),
        });
        let Ok(conn) = open(&control) else {
            continue;
        };
        let Ok(status) = conn.query_row("SELECT state FROM meta WHERE id=1", [], |r| {
            r.get::<_, String>(0)
        }) else {
            continue;
        };
        if matches!(
            status.as_str(),
            "queued" | "scanning" | "paused" | "cancelling"
        ) {
            set_state(&conn, "expired")?;
        }
        controls()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.to_string(), control);
        adopted += 1;
    }
    Ok(adopted)
}

/// Delete expired scan DB/WAL/SHM owned by this app. Live walkers stay.
pub fn cleanup_expired_scan_files() -> Result<u32, ImportError> {
    let ids: Vec<String> = controls()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .keys()
        .cloned()
        .collect();
    let mut removed = 0u32;
    for id in ids {
        let control = match control(&id) {
            Ok(control) => control,
            Err(_) => continue,
        };
        if control.running.load(Ordering::Acquire) {
            continue;
        }
        if control
            .worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
        {
            continue;
        }
        if !owned_scan_db(&id, &control.path) {
            continue;
        }
        let status: String = {
            let conn = open(&control)?;
            conn.query_row("SELECT state FROM meta WHERE id=1", [], |r| r.get(0))
                .map_err(failure)?
        };
        if status != "expired" {
            continue;
        }
        remove_scan_files(&control.path);
        controls()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
        removed += 1;
    }
    Ok(removed)
}

pub fn recover_scans_on_startup() -> Result<u32, ImportError> {
    let adopted = adopt_orphaned_scans()?;
    let cleaned = cleanup_expired_scan_files()?;
    Ok(adopted.saturating_add(cleaned))
}

/// Synchronous harness exercises the exact worker and storage used in the app.
#[cfg(test)]
pub fn scan_start(paths: Vec<(Provider, String)>) -> Result<ScanState, ImportError> {
    let sources = paths
        .into_iter()
        .map(|(provider, path)| ScanSourcePath {
            provider: provider.as_str().into(),
            path,
            source_id: None,
            location_id: None,
            host: None,
            profile: String::new(),
        })
        .collect();
    let scan = begin(sources)?;
    let _ = run(&scan.scan_id);
    state(&scan.scan_id, scan.epoch)
}

#[cfg(test)]
pub fn scan_refresh(id: &str, epoch: u64) -> Result<ScanState, ImportError> {
    let sources = state(id, epoch)?.source_paths;
    cancel(id, epoch)?;
    let scan = begin(sources)?;
    run(&scan.scan_id)?;
    state(&scan.scan_id, scan.epoch)
}

#[cfg(test)]
pub fn scan_append_discoveries(id: &str, extra: Vec<SourceCandidate>) -> Result<(), ImportError> {
    let control = control(id)?;
    let conn = open(&control)?;
    for candidate in extra {
        let source = super::sources::SourceRegistration {
            source_id: "synthetic-extra".into(),
            provider: candidate.provider,
            root_path: String::new(),
            profile: String::new(),
            location: None,
        };
        add(&conn, id, &source, candidate)?;
    }
    Ok(())
}
