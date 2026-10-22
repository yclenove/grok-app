use super::*;
use crate::session_migration::{engine, jobs, test_home::IsolatedAppHome, types::Provider};

fn import(home: &IsolatedAppHome, name: &str) -> jobs::JobRecord {
    let source = home.path.join(name);
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("chat.json"),
        br#"[{"role":"user","content":"synthetic question"}]"#,
    )
    .unwrap();
    let scan = crate::session_migration::scan::scan_start(vec![(
        Provider::GenericFile,
        source.to_string_lossy().into(),
    )])
    .unwrap();
    let candidate = &scan.candidates[0];
    let (id, _) =
        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let mappings = engine::propose_mappings(std::slice::from_ref(&id), None).unwrap();
    let planned = engine::plan(vec![id], mappings).unwrap();
    let job = engine::commit(&planned.plan_id, &planned.plan_hash, name).unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    engine::job_get(&job.job_id).unwrap()
}

#[test]
fn completed_jobs_are_explicitly_pageable_after_restart_without_recovering_or_mutating() {
    let home = IsolatedAppHome::acquire("job-history-restart");
    let _clock = crate::session_migration::clock::Fixed::at(chrono::Utc::now());
    let first = import(&home, "first");
    let second = import(&home, "second");
    assert_eq!(first.created_at, second.created_at);
    assert_eq!(second.state, JobState::Completed);
    engine::reset_runtime();
    assert!(engine::recovery().unwrap().job.is_none());
    // Close/checkpoint the database before comparing its exact bytes.
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    drop(conn);
    let before = std::fs::read(catalog::catalog_path()).unwrap();
    jobs::take_job_item_deserializes();
    let one = page(None, Some(1)).unwrap();
    let two = page(one.next_cursor.as_deref(), Some(1)).unwrap();
    assert_eq!(one.items.len(), 1);
    assert_eq!(two.items.len(), 1);
    assert_eq!(one.items[0].job_id, second.job_id);
    assert_eq!(two.items[0].job_id, first.job_id);
    assert_eq!(one.items[0].item_count, 1);
    assert_eq!(one.items[0].state, Some(JobState::Completed));
    assert_eq!(
        one.items[0].created_at.as_deref(),
        Some(first.created_at.as_str())
    );
    assert!(two.next_cursor.is_none());
    assert_eq!(jobs::take_job_item_deserializes(), 0);
    assert_eq!(std::fs::read(catalog::catalog_path()).unwrap(), before);
    let viewed = engine::job_get(&two.items[0].job_id).unwrap();
    assert_eq!(viewed.items[0].import_id, first.items[0].import_id);
    assert_eq!(viewed.state_version, first.state_version);
    assert_eq!(engine::list_imported_page(None, None).unwrap().0.len(), 2);
    assert!(engine::recovery().unwrap().plan.is_none());
}

#[test]
fn browsing_uses_only_display_columns_and_never_grants_access_to_corrupt_jobs() {
    let home = IsolatedAppHome::acquire("job-history-corruption");
    let good = import(&home, "good");
    let bad = import(&home, "bad");
    let conn = catalog::open_catalog().unwrap();
    conn.execute("UPDATE jobs SET json='not-json',state='future-state',created_at=CAST('bad' AS BLOB) WHERE job_id=?1", [&bad.job_id]).unwrap();
    let before: String = conn
        .query_row("SELECT group_concat(json) FROM jobs", [], |r| r.get(0))
        .unwrap();
    jobs::take_job_item_deserializes();
    let list = page(None, Some(200)).unwrap();
    assert_eq!(list.items.len(), 2);
    let damaged = list
        .items
        .iter()
        .find(|item| item.job_id == bad.job_id)
        .unwrap();
    assert_eq!(damaged.state, None);
    assert_eq!(damaged.created_at, None);
    assert_eq!(jobs::take_job_item_deserializes(), 0);
    assert!(engine::job_get(&bad.job_id).is_err());
    assert_eq!(engine::job_get(&good.job_id).unwrap().job_id, good.job_id);
    assert_eq!(
        conn.query_row("SELECT group_concat(json) FROM jobs", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        before
    );
}

#[test]
fn empty_history_creates_no_catalog_and_cursors_cannot_cross_domains_or_be_forged() {
    let _home = IsolatedAppHome::acquire("job-history-cursors");
    assert!(page(None, None).unwrap().items.is_empty());
    assert!(!catalog::catalog_path().exists());
    let wrong = encode_bound_cursor(
        CursorScope {
            owner: "catalog",
            ..SCOPE
        },
        0,
        Some("[\"\",1]".into()),
    )
    .unwrap();
    assert_eq!(
        page(Some(&wrong), None).unwrap_err().code,
        ErrorCode::PlanStale
    );
    for mark in ["null", "[\"date\",0]", "[\"date\",-1]"] {
        let cursor = encode_bound_cursor(SCOPE, 0, Some(mark.into())).unwrap();
        assert_eq!(
            page(Some(&cursor), None).unwrap_err().code,
            ErrorCode::PlanStale
        );
    }
    assert!(page(Some("impcur1.forged.mac"), None).is_err());
    assert!(!catalog::catalog_path().exists());
}
