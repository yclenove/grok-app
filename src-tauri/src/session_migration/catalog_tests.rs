
    #[test]
    fn readonly_open_during_initial_schema_commit_uses_a_consistent_snapshot() {
        let _home = super::super::test_home::IsolatedAppHome::acquire("catalog-initialize-read");
        std::fs::create_dir_all(imports_dir()).unwrap();
        let mut conn = Connection::open(catalog_path()).unwrap();
        let tx = conn.transaction().unwrap();
        tx.execute_batch(SCHEMA_SQL).unwrap();
        tx.execute(
            "INSERT INTO meta(key,value) VALUES('schema_version',?1)",
            [CATALOG_SCHEMA_VERSION.to_string()],
        )
        .unwrap();
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let reader = scope.spawn(|| {
                barrier.wait();
                open_catalog_readonly().map(|db| read_schema_version(&db).unwrap())
            });
            barrier.wait();
            std::thread::sleep(std::time::Duration::from_millis(50));
            tx.commit().unwrap();
            assert_eq!(
                reader.join().unwrap().unwrap(),
                Some(CATALOG_SCHEMA_VERSION)
            );
        });
    }

    #[test]
    fn newer_schema_is_rejected() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("cat-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let conn = open_catalog().unwrap();
        conn.execute(
            "UPDATE meta SET value = '99' WHERE key = 'schema_version'",
            [],
        )
        .unwrap();
        drop(conn);
        let before = std::fs::read(catalog_path()).unwrap();
        let err = open_catalog().unwrap_err();
        assert_eq!(err.code, ErrorCode::ImportSchemaTooNew);
        assert_eq!(before, std::fs::read(catalog_path()).unwrap());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn corrupt_or_newer_catalog_never_creates_missing_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
            INSERT INTO meta VALUES('schema_version', '99');",
        )
        .unwrap();
        assert_eq!(
            init_schema(&conn).unwrap_err().code,
            ErrorCode::ImportSchemaTooNew
        );
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        conn.execute("UPDATE meta SET value='invalid'", []).unwrap();
        assert_eq!(
            init_schema(&conn).unwrap_err().code,
            ErrorCode::SnapshotUnavailable
        );
    }

    #[test]
    fn revision_paths_reject_traversal() {
        let id = super::super::ids::new_import_id();
        assert!(revision_dir(&id, "../../outside").is_err());
        assert!(revision_dir("imp:../../outside", &uuid::Uuid::new_v4().to_string()).is_err());
    }

    #[test]
    fn reopen_does_not_rewrite_existing_sources_or_schema() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("cat-reopen-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let conn = open_catalog().unwrap();
        upsert_source(
            &conn,
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "codex",
            r"C:\src",
        )
        .unwrap();
        drop(conn);
        let before = std::fs::read(catalog_path()).unwrap();
        let conn = open_catalog().unwrap();
        init_schema(&conn).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let ver: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ver, CATALOG_SCHEMA_VERSION.to_string());
        let log: i64 = conn
            .query_row("SELECT COUNT(*) FROM migration_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(log, 0);
        drop(conn);
        assert_eq!(before, std::fs::read(catalog_path()).unwrap());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unversioned_catalog_is_rejected_without_rewriting_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sources (
                installation_id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                profile TEXT,
                host TEXT,
                root_path TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            INSERT INTO sources VALUES('keep-me','codex',NULL,'local','/old','t');",
        )
        .unwrap();
        assert_eq!(
            init_schema(&conn).unwrap_err().code,
            ErrorCode::SnapshotUnavailable
        );
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let has_meta: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!has_meta);
    }

    #[test]
    fn legacy_body_search_catalog_is_rejected_without_mutation() {
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("cat-legacy-body");
        let conn = open_catalog().unwrap();
        conn.execute_batch(
            "CREATE TABLE body_search(import_id TEXT PRIMARY KEY, text TEXT NOT NULL);
             INSERT INTO body_search VALUES('imp:legacy-marker', 'keep me');",
        )
        .unwrap();
        drop(conn);

        let before = std::fs::read(catalog_path()).unwrap();
        let err = open_catalog().unwrap_err();
        assert_eq!(err.code, ErrorCode::SchemaUnsupported);
        assert_eq!(before, std::fs::read(catalog_path()).unwrap());

        let conn = Connection::open(catalog_path()).unwrap();
        let marker: String = conn
            .query_row("SELECT text FROM body_search", [], |row| row.get(0))
            .unwrap();
        assert_eq!(marker, "keep me");
        let _ = home;
    }

    #[test]
    fn missing_body_search_chunks_is_rejected_without_creation() {
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("cat-missing-body");
        let conn = open_catalog().unwrap();
        conn.execute_batch("DROP TABLE body_search_chunks;").unwrap();
        drop(conn);

        let before = std::fs::read(catalog_path()).unwrap();
        let err = open_catalog().unwrap_err();
        assert_eq!(err.code, ErrorCode::SchemaUnsupported);
        assert_eq!(before, std::fs::read(catalog_path()).unwrap());

        let conn = Connection::open(catalog_path()).unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='body_search_chunks')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!exists, "opening an incompatible catalog must not create tables");
        let _ = home;
    }

    #[test]
    fn incompatible_body_search_chunks_is_rejected_without_rewrite() {
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("cat-bad-body");
        let conn = open_catalog().unwrap();
        conn.execute_batch(
            "DROP TABLE body_search_chunks;
             CREATE TABLE body_search_chunks(
               import_id TEXT,
               revision_id TEXT,
               chunk_ordinal INTEGER,
               text TEXT
             );
             INSERT INTO body_search_chunks VALUES('imp:marker', 'rev', 0, 'keep me');",
        )
        .unwrap();
        drop(conn);

        let before = std::fs::read(catalog_path()).unwrap();
        let err = open_catalog().unwrap_err();
        assert_eq!(err.code, ErrorCode::SchemaUnsupported);
        assert_eq!(before, std::fs::read(catalog_path()).unwrap());

        let conn = Connection::open(catalog_path()).unwrap();
        let marker: String = conn
            .query_row("SELECT text FROM body_search_chunks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(marker, "keep me");
        let _ = home;
    }

    fn seed_committed_catalog_revision(
        home: &crate::session_migration::test_home::IsolatedAppHome,
        needle: &str,
    ) -> (String, String) {
        use crate::session_migration::engine::{commit, job_get, plan, preview, propose_mappings};
        use crate::session_migration::types::Provider;

        let source = home.path.join(format!("catalog-rebuild-source-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&source).unwrap();
        let path = source.join("history.json");
        let payload = serde_json::json!([
            {"role": "user", "content": "catalog rebuild fixture"},
            {"role": "assistant", "content": needle}
        ]);
        std::fs::write(&path, serde_json::to_vec(&payload).unwrap()).unwrap();

        let scan = crate::session_migration::scan::scan_start(vec![(
            Provider::GenericFile,
            source.to_string_lossy().into_owned(),
        )])
        .unwrap();
        let candidate = scan
            .candidates
            .iter()
            .find(|candidate| candidate.locator.ends_with("history.json"))
            .unwrap();
        let (preview_id, _) =
            preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
        let mappings = propose_mappings(std::slice::from_ref(&preview_id), None).unwrap();
        let plan = plan(vec![preview_id], mappings).unwrap();
        let job = commit(
            &plan.plan_id,
            &plan.plan_hash,
            &format!("catalog-rebuild-{}", uuid::Uuid::new_v4()),
        )
        .unwrap();
        crate::session_migration::job_control::run_pending().unwrap();
        let completed = job_get(&job.job_id).unwrap();
        let item = &completed.items[0];
        (
            item.import_id.clone().unwrap(),
            item.revision_id.clone().unwrap(),
        )
    }

    fn downgrade_catalog_to_v1_with_legacy_projection() {
        let conn = Connection::open(catalog_path()).unwrap();
        conn.execute_batch(
            "UPDATE meta SET value='1' WHERE key='schema_version';
             DROP TABLE body_search_chunks;
             CREATE TABLE body_search(import_id TEXT PRIMARY KEY, text TEXT NOT NULL);
             INSERT INTO body_search VALUES('imp:legacy-marker', 'preserve this marker');",
        )
        .unwrap();
    }

    #[test]
    fn v1_catalog_rebuilds_verified_revisions_and_switches_to_v2() {
        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("cat-rebuild-v1");
        let needle = "catalog-v2-search-needle";
        let (import_id, _) = seed_committed_catalog_revision(&home, needle);
        downgrade_catalog_to_v1_with_legacy_projection();

        assert_eq!(open_catalog().unwrap_err().code, ErrorCode::SchemaUnsupported);
        let mut phases = Vec::new();
        let report = rebuild_catalog_index_with_hooks(&|| false, &mut |phase, done, total| {
            phases.push((phase, done, total));
        })
        .unwrap();

        assert_eq!(report.from_version, 1);
        assert_eq!(report.to_version, 2);
        assert_eq!(report.revisions_scanned, 1);
        assert_eq!(report.revisions_rebuilt, 1);
        assert!(report.chunks_written >= 1);
        assert_eq!(report.projection_sha256.len(), 64);
        assert!(phases
            .iter()
            .any(|(phase, _, _)| *phase == CatalogRebuildPhase::Committing));

        let conn = open_catalog_readonly().unwrap();
        assert_eq!(read_schema_version(&conn).unwrap(), Some(2));
        assert!(search_imported_bodies(&conn, needle, 10)
            .unwrap()
            .iter()
            .any(|(entry, _)| entry.import_id == import_id));
        let legacy_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='body_search')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!legacy_exists);
        let migration: (i64, i64) = conn
            .query_row(
                "SELECT from_version, to_version FROM migration_log",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(migration, (1, 2));
        assert!(!imports_dir().join("catalog.sqlite.rebuild-backup").exists());
        let _ = home;
    }

    #[test]
    fn failed_catalog_rebuild_rolls_back_and_preserves_legacy_rows() {
        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("cat-rebuild-rollback");
        let (_, revision_id) = seed_committed_catalog_revision(&home, "rollback-needle");
        downgrade_catalog_to_v1_with_legacy_projection();
        let conn = Connection::open(catalog_path()).unwrap();
        let import_id: String = conn
            .query_row(
                "SELECT import_id FROM entries WHERE current_revision=?1",
                params![revision_id],
                |row| row.get(0),
            )
            .unwrap();
        drop(conn);
        std::fs::write(
            revision_dir(&import_id, &revision_id)
                .unwrap()
                .join("events.jsonl"),
            b"corrupt after receipt",
        )
        .unwrap();

        let error = rebuild_catalog_index_with_hooks(&|| false, &mut |_, _, _| {}).unwrap_err();
        assert!(matches!(
            error.code,
            ErrorCode::SourceChanged
                | ErrorCode::HistoryAccessUnavailable
                | ErrorCode::SnapshotUnavailable
        ));
        let conn = Connection::open(catalog_path()).unwrap();
        assert_eq!(read_schema_version(&conn).unwrap(), Some(1));
        let marker: String = conn
            .query_row("SELECT text FROM body_search", [], |row| row.get(0))
            .unwrap();
        assert_eq!(marker, "preserve this marker");
        let staging_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='body_search_chunks_v2_staging')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!staging_exists);
        assert!(imports_dir().join("catalog.sqlite.rebuild-backup").is_file());
        let _ = home;
    }

    #[test]
    fn missing_revision_directory_cannot_pass_path_validation() {
        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("cat-rebuild-missing");
        let (import_id, revision_id) =
            seed_committed_catalog_revision(&home, "missing-revision-needle");
        downgrade_catalog_to_v1_with_legacy_projection();
        let revision = revision_dir(&import_id, &revision_id).unwrap();
        std::fs::rename(&revision, revision.with_extension("missing")).unwrap();

        let error = rebuild_catalog_index_with_hooks(&|| false, &mut |_, _, _| {}).unwrap_err();
        assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
        let conn = Connection::open(catalog_path()).unwrap();
        assert_eq!(read_schema_version(&conn).unwrap(), Some(1));
        let marker: String = conn
            .query_row("SELECT text FROM body_search", [], |row| row.get(0))
            .unwrap();
        assert_eq!(marker, "preserve this marker");
        let _ = home;
    }

    #[test]
    fn cancelled_catalog_rebuild_rolls_back_after_backup() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("cat-rebuild-cancel");
        seed_committed_catalog_revision(&home, "cancel-needle");
        downgrade_catalog_to_v1_with_legacy_projection();
        let checks = AtomicUsize::new(0);
        let error = rebuild_catalog_index_with_hooks(
            &|| checks.fetch_add(1, Ordering::SeqCst) >= 2,
            &mut |_, _, _| {},
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::PlanStale);
        let conn = Connection::open(catalog_path()).unwrap();
        assert_eq!(read_schema_version(&conn).unwrap(), Some(1));
        let marker: String = conn
            .query_row("SELECT text FROM body_search", [], |row| row.get(0))
            .unwrap();
        assert_eq!(marker, "preserve this marker");
        assert!(imports_dir().join("catalog.sqlite.rebuild-backup").is_file());
        let _ = home;
    }

    #[test]
    fn v2_catalog_rebuild_is_idempotent() {
        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("cat-rebuild-idempotent");
        let needle = "idempotent-search-needle";
        seed_committed_catalog_revision(&home, needle);

        let first = rebuild_catalog_index_with_hooks(&|| false, &mut |_, _, _| {}).unwrap();
        let second = rebuild_catalog_index_with_hooks(&|| false, &mut |_, _, _| {}).unwrap();
        assert_eq!(first.from_version, 2);
        assert_eq!(second.from_version, 2);
        assert_eq!(first.chunks_written, second.chunks_written);
        assert_eq!(first.searchable_bytes, second.searchable_bytes);
        assert_eq!(first.projection_sha256, second.projection_sha256);
        let conn = open_catalog_readonly().unwrap();
        let migration_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM migration_log", [], |row| row.get(0))
            .unwrap();
        assert_eq!(migration_count, 0);
        assert_eq!(search_imported_bodies(&conn, needle, 10).unwrap().len(), 1);
        let _ = home;
    }

    #[test]
    fn explicit_rebuild_repairs_an_incompatible_v2_projection_table() {
        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("cat-rebuild-v2-bad");
        let needle = "repair-v2-projection-needle";
        seed_committed_catalog_revision(&home, needle);
        let conn = Connection::open(catalog_path()).unwrap();
        conn.execute_batch(
            "DROP TABLE body_search_chunks;
             CREATE TABLE body_search_chunks(
               import_id TEXT,
               revision_id TEXT,
               chunk_ordinal INTEGER,
               text TEXT
             );",
        )
        .unwrap();
        drop(conn);
        assert_eq!(open_catalog().unwrap_err().code, ErrorCode::SchemaUnsupported);

        let report = rebuild_catalog_index_with_hooks(&|| false, &mut |_, _, _| {}).unwrap();
        assert_eq!(report.from_version, 2);
        let conn = open_catalog_readonly().unwrap();
        assert_eq!(search_imported_bodies(&conn, needle, 10).unwrap().len(), 1);
        let _ = home;
    }

    #[test]
    fn interrupted_backup_is_restored_when_live_catalog_is_missing() {
        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("cat-rebuild-restart");
        seed_committed_catalog_revision(&home, "restart-needle");
        let catalog = catalog_path();
        let backup = imports_dir().join("catalog.sqlite.rebuild-backup");
        std::fs::copy(&catalog, &backup).unwrap();
        std::fs::remove_file(&catalog).unwrap();

        let report = rebuild_catalog_index_with_hooks(&|| false, &mut |_, _, _| {}).unwrap();
        assert_eq!(report.from_version, 2);
        assert!(catalog.is_file());
        assert!(!backup.exists());
        assert_eq!(read_schema_version(&open_catalog_readonly().unwrap()).unwrap(), Some(2));
        let _ = home;
    }

    #[test]
    fn rebuild_lock_conflict_is_reported_without_catalog_mutation() {
        use fs2::FileExt;

        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("cat-rebuild-lock");
        open_catalog().unwrap();
        let before = std::fs::read(catalog_path()).unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(imports_dir().join("catalog-rebuild.lock"))
            .unwrap();
        lock.try_lock_exclusive().unwrap();

        let error = rebuild_catalog_index_with_hooks(&|| false, &mut |_, _, _| {}).unwrap_err();
        assert_eq!(error.code, ErrorCode::SourceBusy);
        assert_eq!(before, std::fs::read(catalog_path()).unwrap());
        FileExt::unlock(&lock).unwrap();
        let _ = home;
    }

    #[test]
    fn sqlite_exclusive_lock_conflict_is_reported_without_catalog_mutation() {
        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("cat-rebuild-db-lock");
        open_catalog().unwrap();
        let blocker = Connection::open(catalog_path()).unwrap();
        blocker.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        let before = std::fs::read(catalog_path()).unwrap();

        let error = rebuild_catalog_index_with_hooks(&|| false, &mut |_, _, _| {}).unwrap_err();
        assert_eq!(error.code, ErrorCode::SourceBusy);
        assert_eq!(before, std::fs::read(catalog_path()).unwrap());
        blocker.execute_batch("ROLLBACK;").unwrap();
        let _ = home;
    }

    #[test]
    fn rebuild_disk_budget_fails_closed() {
        let error = validate_rebuild_disk_space(0, 4096).unwrap_err();
        assert_eq!(error.code, ErrorCode::DiskSpaceLow);
    }

    crate::session_migration::test_home::fixture_home_contract_tests!(
        newer_schema_is_rejected,
        reopen_does_not_rewrite_existing_sources_or_schema,
    );
