//! Production association/scan/preview/publication contracts, synthetic only.
use crate::session_migration::{
    adapters::cursor,
    catalog, engine,
    errors::ErrorCode,
    scan,
    source_locations::{self, SourceAssociationRequest, SourceLocation},
    sources,
    test_home::IsolatedAppHome,
    types::Provider,
};
use rusqlite::{params, Connection};
use serde_json::json;
use std::path::{Path, PathBuf};

struct Fixture {
    home: IsolatedAppHome,
    primary: PathBuf,
    associated: PathBuf,
    source: sources::SourceRegistration,
}

impl Fixture {
    fn new() -> Self {
        let home = IsolatedAppHome::acquire("source-location-association");
        engine::reset_runtime();
        let primary = home.path.join("primary cursor");
        let associated = home.path.join("associated cursor");
        for root in [&primary, &associated] {
            std::fs::create_dir_all(root).unwrap();
        }
        let source = sources::register(
            &catalog::open_catalog().unwrap(),
            Provider::Cursor,
            &primary,
            None,
            "synthetic",
        )
        .unwrap();
        Self {
            home,
            primary,
            associated,
            source,
        }
    }

    fn request(&self, path: &Path) -> SourceAssociationRequest {
        SourceAssociationRequest {
            source_id: self.source.source_id.clone(),
            provider: Provider::Cursor,
            host: "local".into(),
            path: path.to_string_lossy().into_owned(),
            expected_primary_root: sources::get(
                &catalog::open_catalog().unwrap(),
                &self.source.source_id,
            )
            .unwrap()
            .root_path,
        }
    }

    fn associate(&self, path: &Path) -> SourceLocation {
        tauri::async_runtime::block_on(crate::commands::session_import_source_associate(
            self.request(path),
        ))
        .unwrap()
    }

    fn page(&self, cursor: Option<String>, limit: usize) -> source_locations::SourceLocationPage {
        tauri::async_runtime::block_on(crate::commands::session_import_source_locations(
            self.source.source_id.clone(),
            "cursor".into(),
            Some("local".into()),
            cursor,
            Some(limit),
        ))
        .unwrap()
    }

    fn row(&self) -> (String, Option<String>) {
        catalog::open_catalog_readonly()
            .unwrap()
            .query_row(
                "SELECT root_path,profile FROM sources WHERE installation_id=?1",
                [&self.source.source_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }
}

fn schema(conn: &Connection) -> Vec<(String, String, Option<String>)> {
    conn.prepare("SELECT name,type,sql FROM sqlite_master ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

#[test]
fn association_command_preserves_primary_and_is_idempotent_after_reopen() {
    let f = Fixture::new();
    let before = f.row();
    let primary = f.page(None, 1).locations.remove(0);
    let location = f.associate(&f.associated);
    assert_eq!(location.kind, "associated");
    assert_eq!(location.source_id, f.source.source_id);
    assert_ne!(location.location_id, primary.location_id);
    assert_eq!(f.row(), before);
    assert_eq!(f.associate(&f.associated), location);
    engine::reset_runtime();
    let first = f.page(None, 1);
    assert_eq!(first.total_count, 2);
    assert_eq!(first.locations, vec![primary]);
    let before = schema(&catalog::open_catalog_readonly().unwrap());
    let second = f.page(first.next_cursor, 1);
    assert_eq!(second.locations, vec![location]);
    assert!(second.next_cursor.is_none());
    assert_eq!(schema(&catalog::open_catalog_readonly().unwrap()), before);
    assert_eq!(
        sources::list(&catalog::open_catalog_readonly().unwrap())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn first_association_rolls_back_extension_ddl_when_marker_write_fails() {
    let f = Fixture::new();
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch(
        "CREATE TRIGGER deny_location_extension BEFORE INSERT ON meta
        WHEN NEW.key='source_location_schema_version'
        BEGIN SELECT RAISE(ABORT,'synthetic marker write failure'); END;",
    )
    .unwrap();
    let before = schema(&conn);
    let row = f.row();
    let error = tauri::async_runtime::block_on(crate::commands::session_import_source_associate(
        f.request(&f.associated),
    ))
    .unwrap_err();
    assert!(error.contains("synthetic marker write failure"), "{error}");
    assert_eq!(schema(&conn), before);
    assert_eq!(f.row(), row);
    assert_eq!(f.page(None, 50).total_count, 1);
    conn.execute_batch("DROP TRIGGER deny_location_extension")
        .unwrap();
    assert_eq!(f.associate(&f.associated).source_id, f.source.source_id);
}

#[test]
fn concurrent_association_requests_return_one_durable_location() {
    let f = Fixture::new();
    let request = f.request(&f.associated);
    let (a, b) = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            tauri::async_runtime::block_on(crate::commands::session_import_source_associate(
                request.clone(),
            ))
            .unwrap()
        });
        let b = scope.spawn(|| {
            tauri::async_runtime::block_on(crate::commands::session_import_source_associate(
                request.clone(),
            ))
            .unwrap()
        });
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(a, b);
    assert_eq!(f.page(None, 50).total_count, 2);
}

#[test]
fn location_page_cursor_rejects_tamper_other_owner_and_changed_generation() {
    let f = Fixture::new();
    f.associate(&f.associated);
    let raw = f.page(None, 1).next_cursor.unwrap();
    let conn = catalog::open_catalog_readonly().unwrap();
    for cursor in [
        "1".into(),
        crate::session_migration::cursor::tamper_cursor_signature(&raw),
    ] {
        assert_eq!(
            source_locations::page(
                &conn,
                &f.source.source_id,
                Provider::Cursor,
                "local",
                Some(&cursor),
                Some(1)
            )
            .unwrap_err()
            .code,
            ErrorCode::PlanStale
        );
    }
    drop(conn);
    let other_root = f.home.path.join("other installation");
    std::fs::create_dir_all(&other_root).unwrap();
    let conn = catalog::open_catalog().unwrap();
    let other = sources::register(&conn, Provider::Cursor, &other_root, None, "synthetic").unwrap();
    assert_eq!(
        source_locations::page(
            &conn,
            &other.source_id,
            Provider::Cursor,
            "local",
            Some(&raw),
            Some(1)
        )
        .unwrap_err()
        .code,
        ErrorCode::PlanStale
    );
    drop(conn);
    let third = f.home.path.join("third cursor");
    std::fs::create_dir_all(&third).unwrap();
    f.associate(&third);
    let conn = catalog::open_catalog_readonly().unwrap();
    assert_eq!(
        source_locations::page(
            &conn,
            &f.source.source_id,
            Provider::Cursor,
            "local",
            Some(&raw),
            Some(1)
        )
        .unwrap_err()
        .code,
        ErrorCode::PlanStale
    );
    assert_eq!(f.page(None, 200).total_count, 3);
}

#[test]
fn associated_location_pages_apply_the_existing_cap_without_losing_roots() {
    let f = Fixture::new();
    let mut expected = std::collections::HashSet::new();
    for index in 0..201 {
        let root = f.home.path.join(format!("associated root {index}"));
        std::fs::create_dir_all(&root).unwrap();
        expected.insert(f.associate(&root).location_id);
    }
    let first = f.page(None, usize::MAX);
    assert_eq!(first.total_count, 202);
    assert_eq!(
        first.locations.len(),
        crate::session_migration::budget::PAGE_SIZE_MAX
    );
    assert_eq!(first.locations[0].kind, "primary");
    let next = f.page(first.next_cursor, usize::MAX);
    assert_eq!(next.locations.len(), 2);
    assert!(next.next_cursor.is_none());
    let actual: std::collections::HashSet<_> = first
        .locations
        .into_iter()
        .chain(next.locations)
        .filter(|l| l.kind == "associated")
        .map(|l| l.location_id)
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn association_rejects_other_installation_primary_alias_and_stale_claims_without_ddl() {
    let f = Fixture::new();
    let conn = catalog::open_catalog().unwrap();
    let other = sources::register(&conn, Provider::Cursor, &f.associated, None, "other").unwrap();
    let before_schema = schema(&conn);
    let before_row = f.row();
    let requests = [
        (f.request(&f.associated), ErrorCode::MappingConflict),
        (f.request(&f.primary), ErrorCode::MappingConflict),
        (
            SourceAssociationRequest {
                host: "ssh:synthetic".into(),
                ..f.request(&f.associated)
            },
            ErrorCode::FormatUnsupported,
        ),
        (
            SourceAssociationRequest {
                provider: Provider::Codex,
                ..f.request(&f.associated)
            },
            ErrorCode::FormatUnsupported,
        ),
        (
            SourceAssociationRequest {
                expected_primary_root: "not-the-selected-primary".into(),
                ..f.request(&f.associated)
            },
            ErrorCode::SourceChanged,
        ),
    ];
    for (request, code) in requests {
        let error = tauri::async_runtime::block_on(
            crate::commands::session_import_source_associate(request),
        )
        .unwrap_err();
        assert!(error.starts_with(&format!("{}:", code.as_str())), "{error}");
        assert_eq!(schema(&conn), before_schema);
        assert_eq!(f.row(), before_row);
        assert_eq!(
            sources::get(&conn, &other.source_id).unwrap().root_path,
            other.root_path
        );
    }
    let moved = f.home.path.join("moved primary");
    std::fs::create_dir_all(&moved).unwrap();
    sources::register(
        &conn,
        Provider::Cursor,
        &moved,
        Some(&f.source.source_id),
        "synthetic",
    )
    .unwrap();
    assert_eq!(
        source_locations::associate(&conn, &f.request(&f.primary))
            .unwrap_err()
            .code,
        ErrorCode::MappingConflict
    );
    assert_eq!(schema(&conn), before_schema);
}

#[test]
fn registered_associated_root_cannot_be_used_as_implicit_or_explicit_relocation() {
    let f = Fixture::new();
    f.associate(&f.associated);
    let row = f.row();
    let conn = catalog::open_catalog().unwrap();
    for owner in [None, Some(f.source.source_id.as_str())] {
        assert_eq!(
            sources::register(&conn, Provider::Cursor, &f.associated, owner, "synthetic")
                .unwrap_err()
                .code,
            ErrorCode::MappingConflict
        );
        assert_eq!(f.row(), row);
    }
    let request: scan::ScanSourcePath =
        serde_json::from_value(json!({"provider":"cursor", "path":f.associated})).unwrap();
    let started = scan::begin(vec![request]).unwrap();
    scan::run(&started.scan_id).unwrap();
    let done = scan::state(&started.scan_id, started.epoch).unwrap();
    assert_eq!(done.discovered_count, 0);
    assert_eq!(done.issues[0].code, "MAPPING_CONFLICT");
    assert_eq!(f.row(), row);
}

#[test]
fn location_aware_primary_relocation_preserves_association_and_primary_id() {
    let f = Fixture::new();
    let primary = f.page(None, 1).locations.remove(0);
    let associated = f.associate(&f.associated);
    let moved = f.home.path.join("moved primary");
    std::fs::create_dir_all(&moved).unwrap();
    let conn = catalog::open_catalog().unwrap();
    sources::register(
        &conn,
        Provider::Cursor,
        &moved,
        Some(&f.source.source_id),
        "synthetic",
    )
    .unwrap();
    let page = f.page(None, 50);
    assert_eq!(page.locations[0].location_id, primary.location_id);
    assert_ne!(page.locations[0].root_path, primary.root_path);
    assert_eq!(page.locations[1], associated);
    let alias = source_locations::resolve(
        &conn,
        &f.source.source_id,
        Provider::Cursor,
        "local",
        &primary.location_id,
        &f.primary,
    )
    .unwrap();
    assert_eq!(alias.root_path, primary.root_path);
    assert_eq!(f.page(None, 50).locations, page.locations);
}

#[test]
fn old_binary_primary_drift_is_detected_without_repair_or_relocation() {
    let f = Fixture::new();
    f.associate(&f.associated);
    let conn = catalog::open_catalog().unwrap();
    conn.execute(
        "UPDATE sources SET profile=?1 WHERE installation_id=?2",
        params![
            r#"{"aliases":[],"profile":"older-binary"}"#,
            f.source.source_id
        ],
    )
    .unwrap();
    let before = std::fs::read(catalog::catalog_path()).unwrap();
    let request = f.request(&f.associated);
    assert_eq!(
        source_locations::associate(&conn, &request)
            .unwrap_err()
            .code,
        ErrorCode::SourceChanged
    );
    assert_eq!(
        source_locations::page(
            &conn,
            &f.source.source_id,
            Provider::Cursor,
            "local",
            None,
            None
        )
        .unwrap_err()
        .code,
        ErrorCode::SourceChanged
    );
    assert_eq!(
        sources::register(
            &conn,
            Provider::Cursor,
            &f.primary,
            Some(&f.source.source_id),
            "synthetic"
        )
        .unwrap_err()
        .code,
        ErrorCode::SourceChanged
    );
    assert_eq!(std::fs::read(catalog::catalog_path()).unwrap(), before);
}

#[test]
fn marked_missing_or_incompatible_location_schema_is_not_repaired_by_reopen() {
    for damage in [
        "DROP INDEX source_locations_path",
        "DROP TABLE source_locations",
        "UPDATE meta SET value='99' WHERE key='source_location_schema_version'",
    ] {
        let f = Fixture::new();
        f.associate(&f.associated);
        let conn = catalog::open_catalog().unwrap();
        conn.execute_batch(damage).unwrap();
        let before = schema(&conn);
        drop(conn);
        let error = catalog::open_catalog().unwrap_err();
        assert!(matches!(
            error.code,
            ErrorCode::SchemaUnsupported | ErrorCode::ImportSchemaTooNew
        ));
        let conn = Connection::open(catalog::catalog_path()).unwrap();
        assert_eq!(schema(&conn), before);
    }
}

fn write_transcript(root: &Path, name: &str, marker: &str, declared: bool) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut row = json!({"role":"user", "content":marker});
    if declared {
        row["composerId"] = json!("shared-composer-fact");
    }
    std::fs::write(&path, format!("{row}\n")).unwrap();
    path
}

fn write_store(root: &Path, marker: &str) -> Vec<PathBuf> {
    let directory = root.join("unknown/same");
    std::fs::create_dir_all(&directory).unwrap();
    let db = directory.join("store.db");
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch("CREATE TABLE blobs(id TEXT PRIMARY KEY, data BLOB)")
        .unwrap();
    conn.execute(
        "INSERT INTO blobs VALUES('root',?1)",
        [serde_json::to_vec(&json!({"role":"user","content":marker})).unwrap()],
    )
    .unwrap();
    drop(conn);
    let meta = directory.join("meta.json");
    std::fs::write(&meta, br#"{"rootBlobId":"root"}"#).unwrap();
    vec![db, meta]
}

fn write_wire_store(root: &Path, marker: &str) -> PathBuf {
    use crate::session_migration::hash::sha256_bytes;
    let directory = root.join("wire/same");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("store.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE blobs(id TEXT PRIMARY KEY, data BLOB); CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT)").unwrap();
    let message = serde_json::to_vec(&json!({"role":"user", "content":marker})).unwrap();
    let leaf = sha256_bytes(&message);
    conn.execute("INSERT INTO blobs VALUES(?1,?2)", params![leaf, message])
        .unwrap();
    let mut tree = vec![0x0a, 32];
    tree.extend(hex::decode(&leaf).unwrap());
    let selected = sha256_bytes(&tree);
    conn.execute("INSERT INTO blobs VALUES(?1,?2)", params![selected, tree])
        .unwrap();
    let meta = hex::encode(
        serde_json::to_vec(&json!({"latestRootBlobId":selected,"name":"Synthetic wire title"}))
            .unwrap(),
    );
    conn.execute("INSERT INTO meta VALUES('0',?1)", [meta])
        .unwrap();
    path
}

fn publish(candidate: &crate::session_migration::types::SourceCandidate) -> (String, String) {
    let (preview, details) =
        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    assert!(details.event_count > 0);
    let plan = engine::plan(vec![preview], vec![]).unwrap();
    let job = engine::commit(
        &plan.plan_id,
        &plan.plan_hash,
        &uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let job = engine::job_get(&job.job_id).unwrap();
    assert_eq!(
        job.state,
        crate::session_migration::jobs::JobState::Completed,
        "{job:?}"
    );
    (
        job.items[0].import_id.clone().unwrap(),
        job.items[0].revision_id.clone().unwrap(),
    )
}

#[test]
fn associated_same_relative_fallbacks_and_subagents_publish_distinct_pinned_histories() {
    let f = Fixture::new();
    let mut original = Vec::new();
    for (root, prefix) in [(&f.primary, "PRIMARY"), (&f.associated, "ASSOCIATED")] {
        for (relative, declared) in [
            ("same.jsonl", false),
            ("subagents/same.jsonl", true),
            ("declared.jsonl", true),
        ] {
            let marker = format!("{prefix}_{relative}_MARKER");
            let path = write_transcript(root, relative, &marker, declared);
            original.push((path.clone(), std::fs::read(&path).unwrap()));
        }
        for path in write_store(root, &format!("{prefix}_STORE_MARKER")) {
            original.push((path.clone(), std::fs::read(&path).unwrap()));
        }
        let path = write_wire_store(root, &format!("{prefix}_WIRE_MARKER"));
        original.push((path.clone(), std::fs::read(&path).unwrap()));
    }
    f.associate(&f.associated);
    let roots = f.page(None, 50).locations;
    let requests = roots.iter().map(|l| serde_json::from_value::<scan::ScanSourcePath>(json!({"provider":"cursor", "path":l.root_path, "sourceId":l.source_id, "locationId":l.location_id, "host":l.host})).unwrap()).collect();
    let started = scan::begin(requests).unwrap();
    scan::run(&started.scan_id).unwrap();
    let done = scan::state(&started.scan_id, started.epoch).unwrap();
    assert_eq!(done.issue_count, 0, "{done:?}");
    assert_eq!(done.discovered_count, 10);
    let frozen =
        crate::session_migration::scan_selection::freeze_current(&done.scan_id, done.epoch)
            .unwrap();
    assert_eq!(frozen.selected_count, 10);
    let (references, next, total) = crate::session_migration::scan_selection::page_refs(
        &done.scan_id,
        done.epoch,
        &frozen.selection_id,
        None,
        Some(50),
    )
    .unwrap();
    assert_eq!(total, 10);
    assert!(next.is_none());
    for reference in references {
        assert!(roots.iter().any(|l| Some(l) == reference.location.as_ref()));
    }
    let row = f.row();
    let mut published = Vec::new();
    for candidate in &done.candidates {
        let reference = scan::candidate(&candidate.candidate_id).unwrap();
        let location = reference.location.as_ref().unwrap();
        let relative = Path::new(&candidate.locator)
            .strip_prefix(&reference.root_path)
            .unwrap();
        if location.kind == "primary" && relative == Path::new("same.jsonl") {
            assert_eq!(
                candidate.native_session_id,
                cursor::TranscriptIdentity::new(
                    Path::new(&candidate.locator),
                    Some(Path::new(&reference.root_path))
                )
                .unwrap()
                .finish()
            );
        }
        published.push((candidate.clone(), publish(candidate)));
    }
    assert_eq!(f.row(), row);
    assert_eq!(
        catalog::list_entries(&catalog::open_catalog_readonly().unwrap())
            .unwrap()
            .len(),
        9
    );
    engine::reset_runtime();
    for (candidate, (import_id, revision)) in &published {
        let reference = scan::candidate(&candidate.candidate_id).unwrap();
        let prefix = if reference.location.as_ref().unwrap().kind == "primary" {
            "PRIMARY"
        } else {
            "ASSOCIATED"
        };
        let (events, _, _) =
            engine::events_page(import_id, Some(revision), None, Some(20), None).unwrap();
        let text = serde_json::to_string(&events).unwrap();
        assert!(text.contains(prefix), "{text}");
        assert!(!text.contains(if prefix == "PRIMARY" {
            "ASSOCIATED"
        } else {
            "PRIMARY"
        }));
    }
    for (candidate, result) in &published {
        assert_eq!(
            &publish(candidate),
            result,
            "same fixed source re-import must be idempotent"
        );
    }
    for (path, bytes) in original {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    engine::reset_runtime();
}

#[test]
fn associated_resolve_rejects_owner_host_path_and_kind_substitution() {
    let f = Fixture::new();
    write_transcript(&f.associated, "same.jsonl", "ASSOCIATED_MARKER", false);
    let location = f.associate(&f.associated);
    let conn = catalog::open_catalog_readonly().unwrap();
    assert_eq!(
        source_locations::resolve(
            &conn,
            &f.source.source_id,
            Provider::Cursor,
            "local",
            &location.location_id,
            &f.primary
        )
        .unwrap_err()
        .code,
        ErrorCode::SourceChanged
    );
    assert_eq!(
        source_locations::resolve(
            &conn,
            &f.source.source_id,
            Provider::Codex,
            "local",
            &location.location_id,
            &f.associated
        )
        .unwrap_err()
        .code,
        ErrorCode::SourceNotFound
    );
    assert_eq!(
        source_locations::resolve(
            &conn,
            &f.source.source_id,
            Provider::Cursor,
            "ssh:synthetic",
            &location.location_id,
            &f.associated
        )
        .unwrap_err()
        .code,
        ErrorCode::FormatUnsupported
    );
    let request = serde_json::from_value(json!({"provider":"cursor", "path":location.root_path, "sourceId":location.source_id, "locationId":location.location_id, "host":"local"})).unwrap();
    let started = scan::begin(vec![request]).unwrap();
    scan::run(&started.scan_id).unwrap();
    let done = scan::state(&started.scan_id, started.epoch).unwrap();
    let mut reference = scan::candidate(&done.candidates[0].candidate_id).unwrap();
    reference.location.as_mut().unwrap().kind = "primary".into();
    assert_eq!(
        engine::preview_reference(reference, None, None)
            .unwrap_err()
            .code,
        ErrorCode::SourceChanged
    );
}

#[test]
fn catalog_rebuild_preserves_location_extension_and_cancellation_rolls_back() {
    let f = Fixture::new();
    f.associate(&f.associated);
    let before = f.page(None, 50).locations;
    let before_row = f.row();
    catalog::rebuild_catalog_index_with_hooks(&|| false, &mut |_, _, _| {}).unwrap();
    assert_eq!(f.page(None, 50).locations, before);
    assert_eq!(f.row(), before_row);
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let result = catalog::rebuild_catalog_index_with_hooks(
        &|| cancelled.load(std::sync::atomic::Ordering::Relaxed),
        &mut |phase, _, _| {
            if phase == catalog::CatalogRebuildPhase::Verifying {
                cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        },
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::PlanStale);
    assert_eq!(f.page(None, 50).locations, before);
    assert_eq!(f.row(), before_row);
}
