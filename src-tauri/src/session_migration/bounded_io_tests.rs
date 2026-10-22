use super::*;
use crate::session_migration::catalog::{
    list_revisions_page, open_catalog, open_catalog_readonly, upsert_entry, CatalogEntry,
};

fn home_lock() -> (
    crate::session_migration::test_home::IsolatedAppHome,
    std::path::PathBuf,
) {
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("bound");
    let path = home.path.clone();
    (home, path)
}

#[test]
fn catalog_pages_past_fifty_with_versioned_cursor() {
    let (_lock, _tmp) = home_lock();
    let conn = open_catalog().unwrap();
    for i in 0..60 {
        upsert_entry(
            &conn,
            &CatalogEntry {
                import_id: format!("imp:{}", uuid::Uuid::from_u128(0x1000 + i as u128)),
                installation_id: format!("inst-{i}"),
                provider: "generic-file".into(),
                native_session_id: format!("n{i}"),
                title: format!("t{i}"),
                alias: None,
                hidden: false,
                current_revision: None,
                created_at: format!("2026-01-01T00:{:02}:{:02}Z", i / 60, i % 60),
            },
        )
        .unwrap();
    }
    let (page, next, total) = list_imported_page(None, Some(50)).unwrap();
    assert_eq!(page.len(), 50);
    assert_eq!(total, 60);
    let next = next.expect("catalog must expose a cursor past the first 50");
    assert!(
        next.parse::<u32>().is_err(),
        "catalog cursor must not be a bare number"
    );
    let (page2, next2, _) = list_imported_page(Some(&next), Some(50)).unwrap();
    assert_eq!(page2.len(), 10);
    assert!(next2.is_none());
    assert!(list_imported_page(Some("50"), Some(50)).is_err());
    let other = crate::session_migration::cursor::encode_bound_cursor(
        crate::session_migration::cursor::CursorScope {
            owner: "other",
            snapshot: "entries",
            leaf: "",
            filter: "entries",
        },
        0,
        None,
    )
    .unwrap();
    assert!(list_imported_page(Some(&other), Some(50)).is_err());
}

#[test]
fn revisions_page_and_readonly_catalog_do_not_init() {
    let (_lock, tmp) = home_lock();
    let conn = open_catalog().unwrap();
    let import_id = "imp:00000000-0000-4000-8000-000000000001";
    upsert_entry(
        &conn,
        &CatalogEntry {
            import_id: import_id.into(),
            installation_id: "inst".into(),
            provider: "generic-file".into(),
            native_session_id: "n".into(),
            title: "t".into(),
            alias: None,
            hidden: false,
            current_revision: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        },
    )
    .unwrap();
    for i in 0..55 {
        conn.execute(
            "INSERT INTO revisions(revision_id, import_id, source_snapshot_id, normalizer_version, options_hash, outcome, inventory_json, dir_path, created_at)
             VALUES (?1, ?2, ?3, '1', 'h', 'complete', '{}', 'x', ?4)",
            rusqlite::params![
                uuid::Uuid::from_u128(i as u128 + 1).to_string(),
                import_id,
                format!("snap-{i}"),
                format!("2026-01-01T00:00:{:02}Z", i)
            ],
        )
        .unwrap();
    }
    let (page, next, total) = list_revisions_page(&conn, import_id, None, Some(50)).unwrap();
    assert_eq!(page.len(), 50);
    assert_eq!(total, 55);
    let next = next.expect("revisions must page past 50");
    let (rest, end, _) = list_revisions_page(&conn, import_id, Some(&next), Some(50)).unwrap();
    assert_eq!(rest.len(), 5);
    assert!(end.is_none());
    drop(conn);
    let ro = open_catalog_readonly().unwrap();
    assert!(crate::session_migration::catalog::verify_catalog_schema(&ro).is_ok());
    drop(ro);
    let empty = tmp.join("empty-home");
    std::fs::create_dir_all(empty.join("imports")).unwrap();
    std::env::set_var("GROK_APP_HOME", &empty);
    let _ = crate::paths::ensure_app_dirs();
    let missing = open_catalog_readonly();
    assert!(missing.is_err());
}

#[test]
fn corrupt_index_and_manifest_fail_closed() {
    let (_lock, tmp) = home_lock();
    let dir = tmp.join("files");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("chat.json"),
        r#"[{"role":"user","content":"one"},{"role":"assistant","content":"two"}]"#,
    )
    .unwrap();
    let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
    let cand = &scan.candidates[0];
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    let maps = propose_mappings(std::slice::from_ref(&pid), None).unwrap();
    let plan = plan(vec![pid], maps).unwrap();
    let job = commit(&plan.plan_id, &plan.plan_hash, "req-corrupt").unwrap();
    super::super::job_control::run_pending().unwrap();
    let job = job_get(&job.job_id).unwrap();
    assert!(
        job.items[0].import_id.is_some(),
        "commit did not publish: {job:?}"
    );
    let import_id = job.items[0].import_id.clone().unwrap();
    let rev = job.items[0].revision_id.clone().unwrap();
    let rev_dir = crate::session_migration::catalog::revision_dir(&import_id, &rev).unwrap();
    std::fs::write(rev_dir.join("event-index.sqlite"), b"not-a-database").unwrap();
    assert!(events_page(&import_id, None, None, Some(10), None).is_err());
    std::fs::write(rev_dir.join("manifest.json"), b"{not-json").unwrap();
    assert!(events_page(&import_id, None, None, Some(10), Some("leaf")).is_err());
    let side = crate::session_migration::continuation::sidecar_path("missing-sid");
    if let Some(parent) = side.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&side, b"{bad").unwrap();
    assert!(crate::session_migration::continuation::read_sidecar_strict("missing-sid").is_err());
    std::fs::write(rev_dir.join("lease.json"), b"{bad-lease").unwrap();
    assert!(crate::session_migration::jobs::read_preview_lease(&rev_dir).is_err());
    conn_insert_corrupt_receipt();
}

fn conn_insert_corrupt_receipt() {
    let conn = open_catalog().unwrap();
    conn.execute(
        "INSERT INTO receipts(receipt_id, job_id, candidate_id, phase, import_id, revision_id, json, created_at)
         VALUES ('r1', 'job-bad', 'c1', 'publish', 'imp:x', 'rev', '{not-json', datetime('now'))",
        [],
    )
    .unwrap();
    let err = crate::session_migration::jobs::latest_receipt(&conn, "job-bad", "c1").unwrap_err();
    assert_eq!(
        err.code,
        crate::session_migration::errors::ErrorCode::SchemaUnsupported
    );
}

#[test]
fn job_items_page_past_fifty_and_events_page_skips_full_archive() {
    let (_lock, tmp) = home_lock();
    let conn = open_catalog().unwrap();
    let job = crate::session_migration::jobs::JobRecord {
        job_id: uuid::Uuid::new_v4().to_string(),
        request_id: uuid::Uuid::new_v4().to_string(),
        plan_hash: "h".into(),
        plan_id: "p".into(),
        state: crate::session_migration::jobs::JobState::Queued,
        state_version: 1,
        items: (0..60)
            .map(|i| crate::session_migration::jobs::JobItem {
                candidate_id: format!("c{i}"),
                title: None,
                phase: "queued".into(),
                import_id: None,
                revision_id: None,
                outcome: None,
                error: None,
                mapping_project_id: None,
            })
            .collect(),
        created_at: chrono::Utc::now().to_rfc3339(),
        item_count: 0,
        next_item_cursor: None,
        progress: None,
    };
    crate::session_migration::jobs::save_job(&conn, &job).unwrap();
    let first = job_items_page(&job.job_id, None, Some(50)).unwrap();
    assert_eq!(first.items.len(), 50);
    assert_eq!(first.item_count, 60);
    let next = first
        .next_item_cursor
        .clone()
        .expect("job items must page past 50");
    assert!(next.parse::<u32>().is_err());
    let second = job_items_page(&job.job_id, Some(&next), Some(50)).unwrap();
    assert_eq!(second.items.len(), 10);
    assert!(second.next_item_cursor.is_none());

    let dir = tmp.join("files");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("chat.json"),
        r#"[{"role":"user","content":"one"},{"role":"assistant","content":"two"},{"role":"user","content":"three"}]"#,
    )
    .unwrap();
    let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
    let cand = &scan.candidates[0];
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    let maps = propose_mappings(std::slice::from_ref(&pid), None).unwrap();
    let plan = plan(vec![pid], maps).unwrap();
    let committed = commit(&plan.plan_id, &plan.plan_hash, "req-no-archive").unwrap();
    super::super::job_control::run_pending().unwrap();
    let done = job_get(&committed.job_id).unwrap();
    let import_id = done.items[0].import_id.clone().unwrap();
    crate::session_migration::continuation::take_load_events_calls();
    let (page, next, total) = events_page(&import_id, None, None, Some(2), None).unwrap();
    assert_eq!(page.len(), 2);
    assert!(total >= 2);
    assert_eq!(
        crate::session_migration::continuation::take_load_events_calls(),
        0,
        "events_page must not load the full archive"
    );
    let next = next.expect("more events");
    let _ = events_page(&import_id, None, Some(&next), Some(2), None).unwrap();
    assert_eq!(
        crate::session_migration::continuation::take_load_events_calls(),
        0
    );
}

#[test]
fn fixture_restores_inherited_home_child() {
    crate::session_migration::test_home::run_fixture_home_contract(|| {
        catalog_pages_past_fifty_with_versioned_cursor();
    });
    crate::session_migration::test_home::run_fixture_home_contract(|| {
        revisions_page_and_readonly_catalog_do_not_init();
    });
    crate::session_migration::test_home::run_fixture_home_contract(|| {
        corrupt_index_and_manifest_fail_closed();
    });
    crate::session_migration::test_home::run_fixture_home_contract(|| {
        job_items_page_past_fifty_and_events_page_skips_full_archive();
    });
}
#[test]
fn fixture_restores_inherited_home_in_fresh_process() {
    crate::session_migration::test_home::assert_fixture_home_contract(
        "session_migration::engine::bounded_io_tests::fixture_restores_inherited_home_child",
    );
}
