//! Real import-domain calls on paths beyond both MAX_PATH and default VFS limits.
use super::test_home::IsolatedAppHome;
use super::types::{ContentBlock, EventBase, ImportedEvent, MessageRole, Provider, TimeOrigin};
use super::{catalog, engine, prepare, scan, scan_selection, search, sqlite_ro};
use rusqlite::Connection;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Fixture {
    home: IsolatedAppHome,
    app: PathBuf,
}

impl Fixture {
    fn new(min_units: usize) -> Self {
        let home = IsolatedAppHome::acquire("sql-long");
        let mut app = home.path.join("deep");
        while app.as_os_str().encode_wide().count() < min_units {
            app.push("unicode-\u{4f1a}\u{8bdd}-component-0123456789-abcdefghijklmnopqrstuvwxyz");
        }
        std::fs::create_dir_all(&app).unwrap();
        assert!(app.as_os_str().encode_wide().count() >= min_units);
        std::env::set_var("GROK_APP_HOME", &app);
        crate::paths::ensure_app_dirs().unwrap();
        Self { home, app }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        prepare::reset_runtime();
        engine::reset_runtime();
    }
}

fn import_roundtrip(min_units: usize) {
    let f = Fixture::new(min_units);
    let source = f.app.join("source");
    std::fs::create_dir_all(&source).unwrap();
    let path = source.join("chat.json");
    let body =
        br#"[{"role":"user","content":"longpath needle"},{"role":"assistant","content":"answer"}]"#;
    std::fs::write(&path, body).unwrap();
    let scan = scan::scan_start(vec![(
        Provider::GenericFile,
        source.to_string_lossy().into(),
    )])
    .unwrap();
    assert_eq!(scan.candidates.len(), 1, "{scan:?}");
    let selection = scan_selection::freeze_current(&scan.scan_id, scan.epoch).unwrap();
    let batch = prepare::start(&scan.scan_id, scan.epoch, &selection.selection_id, vec![]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = prepare::status(&batch.batch_id).unwrap();
        if !prepare::running_for_test(&batch.batch_id)
            && !matches!(
                status.state,
                prepare::PrepareBatchState::Queued
                    | prepare::PrepareBatchState::Preparing
                    | prepare::PrepareBatchState::Cancelling
            )
        {
            assert_eq!(
                status.state,
                prepare::PrepareBatchState::Completed,
                "{status:?}"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "prepare worker did not finish: {status:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let (ids, _) = prepare::plan_inputs(&batch.batch_id).unwrap();
    assert_eq!(ids.len(), 1);
    let mappings = engine::propose_mappings(&ids, None).unwrap();
    let plan = engine::plan(ids, mappings).unwrap();
    let job = engine::commit(&plan.plan_id, &plan.plan_hash, "longpath-regression").unwrap();
    super::job_control::run_pending().unwrap();
    let job = engine::job_get(&job.job_id).unwrap();
    assert_eq!(job.items.len(), 1);
    let item = &job.items[0];
    let import_id = item
        .import_id
        .as_ref()
        .unwrap_or_else(|| panic!("not published: {job:?}"));
    let rev = item.revision_id.as_ref().unwrap();
    let dir = catalog::revision_dir(import_id, rev).unwrap();
    let (events, _, _) = engine::events_page(import_id, None, None, Some(10), None).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        search::search(&dir, "needle", None, Some(10), None)
            .unwrap()
            .0
            .len(),
        1
    );
    drop(catalog::open_catalog_readonly().unwrap());
    let rebuilt = catalog::run_catalog_rebuild().unwrap();
    assert_eq!(rebuilt.revisions_scanned, 1);
    assert_eq!(rebuilt.revisions_rebuilt, 1);
    // A leftover verified backup is recovered by the maintenance read-only open.
    std::fs::copy(
        catalog::catalog_path(),
        catalog::imports_dir().join("catalog.sqlite.rebuild-backup"),
    )
    .unwrap();
    assert_eq!(catalog::run_catalog_rebuild().unwrap().revisions_rebuilt, 1);
    assert!(!catalog::imports_dir()
        .join("catalog.sqlite.rebuild-backup")
        .exists());
    assert_eq!(std::fs::read(&path).unwrap(), body);
}

#[test]
fn long_path_import_scan_prepare_publish_and_read() {
    import_roundtrip(300);
}

#[test]
fn very_long_path_import_exceeds_default_vfs_buffer() {
    import_roundtrip(1200);
}

#[test]
fn long_path_event_index_rebuild_and_read() {
    let f = Fixture::new(1200);
    let dir = f.app.join("revision");
    let events = [ImportedEvent::Message {
        base: EventBase {
            id: "e1".into(),
            ordinal: 0,
            spans: vec![],
            timestamp: None,
            time_origin: TimeOrigin::Unknown,
        },
        role: MessageRole::User,
        blocks: vec![ContentBlock::text("deep needle")],
        complete: true,
    }];
    search::build_index(&dir, &events).unwrap();
    assert_eq!(
        search::search(&dir, "needle", None, Some(10), None)
            .unwrap()
            .0
            .len(),
        1
    );
    let archive = std::fs::read(search::archive_events_path(&dir)).unwrap();
    search::rebuild_index(&dir, &|| false).unwrap();
    assert_eq!(
        search::last_indexed_event_id(&dir).unwrap().as_deref(),
        Some("e1")
    );
    assert_eq!(
        std::fs::read(search::archive_events_path(&dir)).unwrap(),
        archive
    );
}

#[test]
fn long_path_catalog_readonly_and_writer_lock_remain_enforced() {
    let f = Fixture::new(1200);
    assert!(catalog::open_catalog_readonly().is_err());
    assert!(
        !catalog::catalog_path().exists(),
        "read-only must not create a missing DB"
    );
    let writer = catalog::open_catalog().unwrap();
    let ro = catalog::open_catalog_readonly().unwrap();
    assert!(ro
        .execute_batch("CREATE TABLE forbidden (id INTEGER)")
        .is_err());
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let other = catalog::open_catalog_under_store_lock().unwrap();
    let timeout: u32 = other
        .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
        .unwrap();
    assert_eq!(timeout, 0);
    let error = other.execute_batch("BEGIN IMMEDIATE").unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    writer.execute_batch("ROLLBACK").unwrap();
    other.execute_batch("BEGIN IMMEDIATE; ROLLBACK;").unwrap();
    assert!(catalog::catalog_path().starts_with(&f.app));
}

#[test]
fn long_path_wal_backup_keeps_source_readonly() {
    let f = Fixture::new(1200);
    let short = f.home.path.join("source.sqlite");
    let writer = Connection::open(&short).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE payload (value TEXT); INSERT INTO payload VALUES ('committed-wal');").unwrap();
    let source = f.app.join("source.sqlite");
    let wal = source.with_file_name("source.sqlite-wal");
    std::fs::copy(&short, &source).unwrap();
    std::fs::copy(short.with_file_name("source.sqlite-wal"), &wal).unwrap();
    let before = (
        std::fs::read(&source).unwrap(),
        std::fs::read(&wal).unwrap(),
    );
    let dest = f.app.join("imports/staging/copy.sqlite");
    let snapshot = sqlite_ro::backup_readonly_to(&source, &dest).unwrap();
    assert!(snapshot.source_unchanged);
    let copy = sqlite_ro::open_readonly(&dest).unwrap();
    let value: String = copy
        .query_row("SELECT value FROM payload", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, "committed-wal");
    assert!(copy
        .execute_batch("INSERT INTO payload VALUES ('forbidden')")
        .is_err());
    assert_eq!(
        (
            std::fs::read(&source).unwrap(),
            std::fs::read(&wal).unwrap()
        ),
        before
    );
    assert!(!source.with_file_name("source.sqlite-shm").exists());
    assert!(!source.with_file_name("source.sqlite-journal").exists());
    assert!(
        sqlite_ro::backup_readonly_to(&source, &dest).is_err(),
        "no overwrite of an existing destination"
    );
}

#[test]
fn long_path_corrupt_and_future_indexes_fail_closed() {
    let f = Fixture::new(1200);
    let dir = f.app.join("index-errors");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(search::archive_events_path(&dir), b"").unwrap();
    let path = search::index_path(&dir);
    let db = super::sqlite_open::open(&path).unwrap();
    db.execute_batch("PRAGMA user_version=99;").unwrap();
    drop(db);
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        search::ensure_index(&dir).unwrap_err().code,
        super::errors::ErrorCode::ImportSchemaTooNew
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::write(&path, b"not-a-sqlite-database").unwrap();
    assert!(search::ensure_index(&dir).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"not-a-sqlite-database");
}

#[test]
fn private_snapshot_long_temp_child() {
    let Some(source) = std::env::var_os("GROK_SQLITE_LONG_TEMP_SOURCE") else {
        return;
    };
    let temp = std::env::temp_dir();
    assert!(
        (230..260).contains(&temp.as_os_str().encode_wide().count()),
        "unexpected OS temp: {temp:?}"
    );
    let copy = sqlite_ro::open_readonly(std::path::Path::new(&source)).unwrap();
    let value: String = copy
        .query_row("SELECT value FROM payload", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, "long-temp-wal");
    let private_dir = std::fs::read_dir(&temp)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("grok-import-sqlite-")
        })
        .unwrap();
    assert!(private_dir.as_os_str().encode_wide().count() > 260);

    assert!(copy.execute_batch("DELETE FROM payload").is_err());
    drop(copy);
}

#[test]
fn private_snapshot_accepts_long_temp_in_fresh_process() {
    let f = Fixture::new(1200);
    let source = f.home.path.join("child-source.sqlite");
    let writer = Connection::open(&source).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE payload (value TEXT); INSERT INTO payload VALUES ('long-temp-wal');").unwrap();
    let paths = [
        &source,
        &source.with_file_name("child-source.sqlite-wal"),
        &source.with_file_name("child-source.sqlite-shm"),
    ];
    let before: Vec<_> = paths
        .iter()
        .map(std::fs::read)
        .collect::<Result<_, _>>()
        .unwrap();
    // GetTempPath may reject an oversized TEMP itself. Use a valid TEMP whose
    // owned SQLite child exceeds MAX_PATH; a separate test covers >1040 copies.
    let width = 240 - f.home.path.as_os_str().encode_wide().count() - 1;
    let temp = f.home.path.join("t".repeat(width));
    std::fs::create_dir(&temp).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "session_migration::sqlite_long_path_tests::private_snapshot_long_temp_child",
            "--test-threads=1",
        ])
        .env("GROK_SQLITE_LONG_TEMP_SOURCE", &source)
        .env("TEMP", &temp)
        .env("TMP", &temp)
        .env("TMPDIR", &temp)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
    assert_eq!(
        paths
            .iter()
            .map(std::fs::read)
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
        before
    );
    assert!(!std::fs::read_dir(&temp).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("grok-import-sqlite-")));
}
