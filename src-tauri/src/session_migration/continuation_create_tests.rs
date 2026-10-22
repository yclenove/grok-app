use super::*;
use crate::session_migration::{engine, scan, test_home::IsolatedAppHome, types::Provider};
use crate::{store, workspace_store};
include!("continuation_view_tests.rs");
include!("continuation_prefix_tests.rs");
include!("continuation_notification_tests.rs");
include!("continuation_contention_tests.rs");
include!("continuation_cleanup_tests.rs");
include!("continuation_narrowing_tests.rs");
include!("continuation_order_tests.rs");

#[test]
fn friendly_title_uses_source_name_and_keeps_user_rename_on_retry() {
    let home = IsolatedAppHome::acquire("continue-friendly-title");
    let prepared = draft(&home);
    let conn = catalog::open_catalog().unwrap();
    conn.execute(
        "UPDATE entries SET title=?1, alias=NULL WHERE import_id=?2",
        rusqlite::params!["  设计评审\n第二阶段  ", prepared.import_id],
    )
    .unwrap();
    let sid = create(&prepared, "friendly-title").unwrap();
    let row = store::load_sessions_snapshot().unwrap().remove(0);
    assert_eq!(row.title, "设计评审 第二阶段");
    assert!(!row.title.contains(&prepared.import_id));
    store::rename_session(&sid, "My renamed continuation").unwrap();
    conn.execute("UPDATE entries SET alias='Changed later'", [])
        .unwrap();
    assert_eq!(create(&prepared, "friendly-title").unwrap(), sid);
    assert_eq!(
        store::load_sessions_snapshot().unwrap().remove(0).title,
        "My renamed continuation"
    );
    assert_eq!(
        std::fs::read_to_string(continuation::sidecar_path(&sid).with_file_name("messages.json"))
            .unwrap(),
        "[]"
    );
}

#[test]
fn friendly_title_is_fixed_in_intent_across_interrupted_creation() {
    let home = IsolatedAppHome::acquire("continue-friendly-title-recovery");
    let prepared = draft(&home);
    let conn = catalog::open_catalog().unwrap();
    conn.execute(
        "UPDATE entries SET title='Source name', alias=' Reviewed alias '",
        [],
    )
    .unwrap();
    FAIL_AT.with(|fail| *fail.borrow_mut() = Some("intent"));
    let first = create(&prepared, "friendly-title-recovery");
    FAIL_AT.with(|fail| *fail.borrow_mut() = None);
    assert!(first.is_err());
    let intent = intents::load(&conn, "friendly-title-recovery")
        .unwrap()
        .unwrap();
    assert_eq!(intent.meta.title, "Reviewed alias");
    conn.execute(
        "UPDATE entries SET alias='Different after interruption'",
        [],
    )
    .unwrap();
    let sid = create(&prepared, "friendly-title-recovery").unwrap();
    assert_eq!(sid, intent.meta.id);
    assert_eq!(
        store::load_sessions_snapshot().unwrap().remove(0).title,
        "Reviewed alias"
    );
}

#[test]
fn friendly_title_bounds_unicode_and_localizes_empty_names() {
    let home = IsolatedAppHome::acquire("continue-friendly-title-empty");
    let prepared = draft(&home);
    let conn = catalog::open_catalog().unwrap();
    let mut entry = catalog::get_entry(&conn, &prepared.import_id)
        .unwrap()
        .unwrap();
    entry.title = "你🚀".repeat(100);
    entry.alias = Some(" \n\t\0 ".into());
    assert_eq!(history_title(&entry), "你🚀".repeat(40));
    entry.alias = Some(" Named\0conversation\nnext ".into());
    assert_eq!(history_title(&entry), "Named conversation next");
    entry.alias = None;
    entry.title = " \n\0 ".into();
    assert_eq!(
        history_title(&entry),
        crate::tray_i18n::strings(crate::tray_i18n::app_locale()).untitled
    );
}

#[test]
fn short_catalog_reader_does_not_refuse_dispatch_after_the_writer_reservation_is_owned() {
    let home = IsolatedAppHome::acquire("continue-catalog-reader");
    let prepared = draft(&home);
    let sid = create(&prepared, "reader-dispatch").unwrap();
    let mut conn = catalog::open_catalog_readonly().unwrap();
    let tx = conn.transaction().unwrap();
    let _: i64 = tx
        .query_row("SELECT COUNT(*) FROM continuation_refs", [], |r| r.get(0))
        .unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let dispatch = scope.spawn(|| {
            barrier.wait();
            super::super::continuation_lifecycle::mark_dispatching(&sid, "synthetic", None)
        });
        barrier.wait();
        std::thread::sleep(std::time::Duration::from_millis(50));
        tx.commit().unwrap();
        assert_eq!(
            dispatch.join().unwrap().unwrap(),
            DispatchState::Dispatching
        );
    });
    assert_eq!(
        continuation::read_sidecar(&sid).unwrap().state,
        DispatchState::Dispatching
    );
}

#[test]
fn catalog_contention_refuses_creation_dispatch_and_removal_without_waiting_under_store_lock() {
    let home = IsolatedAppHome::acquire("continue-catalog-contention");
    let prepared = draft(&home);
    let mut conn = catalog::open_catalog().unwrap();
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let started = std::time::Instant::now();
    let failed = create(&prepared, "contended").unwrap_err();
    assert_eq!(failed.code, ErrorCode::SourceBusy);
    assert!(started.elapsed() < std::time::Duration::from_millis(super::super::budget::DB_BUSY_MS));
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
    assert!(intents::load(&tx, "contended").unwrap().is_none());
    tx.rollback().unwrap();
    let sid = create(&prepared, "contended").unwrap();
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let started = std::time::Instant::now();
    let failed = super::super::continuation_lifecycle::mark_dispatching(&sid, "synthetic", None)
        .unwrap_err();
    assert_eq!(failed.code, ErrorCode::SourceBusy);
    assert!(started.elapsed() < std::time::Duration::from_millis(super::super::budget::DB_BUSY_MS));
    assert_eq!(
        continuation::read_sidecar(&sid).unwrap().state,
        DispatchState::Prepared
    );
    tx.rollback().unwrap();
    assert_eq!(
        super::super::continuation_lifecycle::mark_dispatching(&sid, "synthetic", None).unwrap(),
        DispatchState::Dispatching
    );

    let second = draft(&home);
    FAIL_AT.with(|fail| *fail.borrow_mut() = Some("index"));
    assert!(create(&second, "delete-contended").is_err());
    FAIL_AT.with(|fail| *fail.borrow_mut() = None);
    let (second_id, _) = continuation::load_ref_by_request(&conn, "delete-contended")
        .unwrap()
        .unwrap();
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let started = std::time::Instant::now();
    assert!(store::delete_session(&second_id)
        .unwrap_err()
        .contains("database is locked"));
    assert!(started.elapsed() < std::time::Duration::from_millis(super::super::budget::DB_BUSY_MS));
    assert!(store::load_sessions_snapshot()
        .unwrap()
        .iter()
        .any(|row| row.id == second_id));
    assert!(continuation::sidecar_path(&second_id).exists());
    tx.rollback().unwrap();
    store::delete_session(&second_id).unwrap();
    assert!(create(&second, "delete-contended").is_err());
}

fn draft(home: &IsolatedAppHome) -> HandoffDraft {
    let files = home.path.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(
        files.join("history.json"),
        r#"[{"role":"user","content":"synthetic continuation"}]"#,
    )
    .unwrap();
    let scan = scan::scan_start(vec![(
        Provider::GenericFile,
        files.to_string_lossy().into(),
    )])
    .unwrap();
    let candidate = &scan.candidates[0];
    let (id, _) =
        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let maps = engine::propose_mappings(std::slice::from_ref(&id), None).unwrap();
    let plan = engine::plan(vec![id], maps).unwrap();
    let job = engine::commit(
        &plan.plan_id,
        &plan.plan_hash,
        &uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let job = engine::job_get(&job.job_id).unwrap();
    let import = job.items[0].import_id.as_ref().unwrap();
    engine::continue_prepare(import, None, None, None, None).unwrap()
}

fn workspace(home: &IsolatedAppHome) -> workspace_store::WorkspaceRecord {
    let folder = home.path.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&folder).unwrap();
    let project = store::add_project(folder.to_string_lossy().into(), false).unwrap();
    workspace_store::upsert_workspace(None, "Synthetic workspace".into(), project.id, vec![])
        .unwrap()
}

fn create(draft: &HandoffDraft, request: &str) -> Result<String, ImportError> {
    engine::continue_create(
        &draft.draft_id,
        &draft.draft_hash,
        request,
        Some("edited input".into()),
    )
}

#[test]
fn reviewed_workspace_is_hashed_and_published_without_starting_or_changing_trust() {
    let home = IsolatedAppHome::acquire("continue-workspace");
    let original = draft(&home);
    let workspace = workspace(&home);
    let prepared = engine::continue_workspace(
        &original.draft_id,
        &original.draft_hash,
        Some(&workspace.id),
    )
    .unwrap();
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
    assert_ne!(prepared.draft_hash, original.draft_hash);
    assert_eq!(prepared.workspace.as_ref(), Some(&workspace));
    let sid = create(&prepared, "create-one").unwrap();
    let meta = store::load_sessions_snapshot().unwrap().remove(0);
    assert_eq!(meta.id, sid);
    assert_eq!(
        meta.project_id.as_deref(),
        Some(workspace.primary_project_id.as_str())
    );
    assert_eq!(meta.workspace_id.as_deref(), Some(workspace.id.as_str()));
    assert_eq!(
        meta.workspace_root_snapshot,
        Some(workspace_store::root_snapshot(&workspace.roots))
    );
    assert!(!store::load_projects_snapshot().unwrap()[0].trusted);
    let side = continuation::read_sidecar(&sid).unwrap();
    assert_eq!(side.state, DispatchState::Prepared);
    assert!(side.agent_session_id.is_none());
    assert_eq!(create(&prepared, "create-one").unwrap(), sid);
    assert_eq!(store::load_sessions_snapshot().unwrap().len(), 1);
}

#[test]
fn each_creation_boundary_recovers_one_complete_workspace_session() {
    let home = IsolatedAppHome::acquire("continue-faults");
    let workspace = workspace(&home);
    for (n, point) in ["intent", "files", "sidecar", "index"].iter().enumerate() {
        let original = draft(&home);
        let prepared = engine::continue_workspace(
            &original.draft_id,
            &original.draft_hash,
            Some(&workspace.id),
        )
        .unwrap();
        let request = format!("failure-{point}");
        FAIL_AT.with(|fail| *fail.borrow_mut() = Some(point));
        let failed = create(&prepared, &request);
        FAIL_AT.with(|fail| *fail.borrow_mut() = None);
        assert!(failed.is_err());
        let conn = catalog::open_catalog().unwrap();
        let (sid, _) = continuation::load_ref_by_request(&conn, &request)
            .unwrap()
            .unwrap();
        assert_eq!(
            store::load_sessions_snapshot().unwrap().len(),
            n + usize::from(*point == "index")
        );
        assert_eq!(create(&prepared, &request).unwrap(), sid);
        let rows = store::load_sessions_snapshot().unwrap();
        assert_eq!(rows.len(), n + 1);
        assert!(rows
            .iter()
            .all(|row| row.workspace_id.as_deref() == Some(&workspace.id)));
        assert_eq!(
            std::fs::read_to_string(
                continuation::sidecar_path(&sid).with_file_name("handoff_draft.txt")
            )
            .unwrap(),
            "edited input"
        );
        assert!(intents::load(&conn, &request).unwrap().unwrap().published);
    }
}

#[test]
fn changed_workspace_and_missing_roots_refuse_before_claiming_a_request() {
    let home = IsolatedAppHome::acquire("continue-changed-workspace");
    let original = draft(&home);
    let workspace = workspace(&home);
    let prepared = engine::continue_workspace(
        &original.draft_id,
        &original.draft_hash,
        Some(&workspace.id),
    )
    .unwrap();
    workspace_store::upsert_workspace(
        Some(workspace.id.clone()),
        "Renamed".into(),
        workspace.primary_project_id.clone(),
        workspace.roots.clone(),
    )
    .unwrap();
    assert!(create(&prepared, "stale").is_err());
    assert!(
        continuation::load_ref_by_request(&catalog::open_catalog().unwrap(), "stale")
            .unwrap()
            .is_none()
    );
    let refreshed = engine::continue_workspace(
        &prepared.draft_id,
        &prepared.draft_hash,
        Some(&workspace.id),
    )
    .unwrap();
    std::fs::remove_dir(&workspace.roots[0].path).unwrap();
    assert!(create(&refreshed, "missing").is_err());
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
}

#[test]
fn concurrent_confirmations_and_lost_reply_preserve_the_same_id_and_input() {
    let home = IsolatedAppHome::acquire("continue-concurrent");
    let prepared = draft(&home);
    let results = std::thread::scope(|scope| {
        let tasks: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| create(&prepared, "shared-request")))
            .collect();
        tasks
            .into_iter()
            .map(|task| task.join().unwrap().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(results.iter().all(|sid| sid == &results[0]));
    assert_eq!(store::load_sessions_snapshot().unwrap().len(), 1);
    assert!(engine::continue_create(
        &prepared.draft_id,
        &prepared.draft_hash,
        "shared-request",
        Some("changed input".into())
    )
    .is_err());
    let sid = &results[0];
    let messages = continuation::sidecar_path(sid).with_file_name("messages.json");
    std::fs::write(&messages, "[existing messages]").unwrap();
    std::fs::remove_file(continuation::sidecar_path(sid)).unwrap();
    assert_eq!(create(&prepared, "shared-request").unwrap(), *sid);
    assert_eq!(
        std::fs::read_to_string(messages).unwrap(),
        "[existing messages]"
    );
}

#[test]
fn removed_completed_session_is_not_resurrected_and_corrupt_index_is_not_quarantined() {
    let home = IsolatedAppHome::acquire("continue-index");
    let prepared = draft(&home);
    let sid = create(&prepared, "done").unwrap();
    store::delete_session(&sid).unwrap();
    assert!(create(&prepared, "done").is_err());
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
    let prepared = draft(&home);
    std::fs::write(crate::paths::sessions_index_file(), "broken").unwrap();
    assert!(create(&prepared, "broken").is_err());
    assert_eq!(
        std::fs::read_to_string(crate::paths::sessions_index_file()).unwrap(),
        "broken"
    );
}

#[test]
fn legacy_hash_fields_stay_absent_and_new_draft_content_is_verified() {
    let home = IsolatedAppHome::acquire("continue-hash");
    let prepared = draft(&home);
    let mut legacy = prepared.clone();
    legacy.creation_version = None;
    legacy.workspace = None;
    let value = serde_json::to_value(&legacy).unwrap();
    assert!(value.get("creationVersion").is_none());
    assert!(value.get("workspace").is_none());
    assert!(check_draft(&legacy).is_ok());
    let mut changed = prepared;
    changed.project_id = Some("forged-project".into());
    assert!(check_draft(&changed).is_err());
}

#[test]
fn pending_creation_restores_frozen_input_after_expiry_and_can_be_cancelled() {
    let home = IsolatedAppHome::acquire("continue-restore");
    let prepared = draft(&home);
    FAIL_AT.with(|fail| *fail.borrow_mut() = Some("files"));
    assert!(create(&prepared, "frozen").is_err());
    FAIL_AT.with(|fail| *fail.borrow_mut() = None);
    let conn = catalog::open_catalog().unwrap();
    conn.execute("UPDATE drafts SET expires_at='2000-01-01T00:00:00Z'", [])
        .unwrap();
    let restored = engine::continue_restore(&prepared.draft_id, &prepared.draft_hash).unwrap();
    assert_eq!(restored.text, "edited input");
    assert_eq!(restored.creation_request_id.as_deref(), Some("frozen"));
    let recovered = engine::recovery().unwrap().draft.unwrap();
    assert_eq!(recovered.creation_request_id, restored.creation_request_id);
    // Hydration is a response projection, not a change to the hashed draft.
    let json = catalog::load_draft_json(&conn, &prepared.draft_id)
        .unwrap()
        .unwrap();
    check_draft(&serde_json::from_str(&json).unwrap()).unwrap();
    assert!(engine::continue_workspace(&prepared.draft_id, &prepared.draft_hash, None).is_err());
    let reopened = engine::continue_prepare(&prepared.import_id, None, None, None, None).unwrap();
    assert_eq!(reopened.creation_request_id.as_deref(), Some("frozen"));
    assert_eq!(reopened.text, "edited input");
    assert!(create(&prepared, "second-request").is_err());
    engine::continue_discard(&prepared.draft_id, &prepared.draft_hash).unwrap();
    engine::continue_discard(&prepared.draft_id, &prepared.draft_hash).unwrap();
    assert!(create(&prepared, "frozen").is_err());
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
    let intent = intents::load(&conn, "frozen").unwrap().unwrap();
    assert!(intent.cancelled);
    assert!(!continuation::sidecar_path(&intent.meta.id)
        .with_file_name("handoff_draft.txt")
        .exists());
    assert!(engine::continue_prepare(&prepared.import_id, None, None, None, None).is_ok());
}

#[test]
fn cancellation_never_deletes_a_published_session_even_if_reply_was_lost() {
    let home = IsolatedAppHome::acquire("continue-cancel-published");
    let prepared = draft(&home);
    FAIL_AT.with(|fail| *fail.borrow_mut() = Some("index"));
    assert!(create(&prepared, "visible").is_err());
    FAIL_AT.with(|fail| *fail.borrow_mut() = None);
    let before = store::load_sessions_snapshot().unwrap().remove(0);
    engine::continue_discard(&prepared.draft_id, &prepared.draft_hash).unwrap();
    assert_eq!(store::load_sessions_snapshot().unwrap().len(), 1);
    assert_eq!(create(&prepared, "visible").unwrap(), before.id);
    let next = draft(&home);
    engine::continue_discard(&next.draft_id, &next.draft_hash).unwrap();
    assert!(create(&next, "late").is_err());
    assert_eq!(store::load_sessions_snapshot().unwrap().len(), 1);
}

#[test]
fn changed_pending_files_and_corrupt_intents_are_not_published() {
    let home = IsolatedAppHome::acquire("continue-invalid-files");
    let prepared = draft(&home);
    FAIL_AT.with(|fail| *fail.borrow_mut() = Some("files"));
    assert!(create(&prepared, "pending").is_err());
    FAIL_AT.with(|fail| *fail.borrow_mut() = None);
    let conn = catalog::open_catalog().unwrap();
    let intent = intents::load(&conn, "pending").unwrap().unwrap();
    let path = continuation::sidecar_path(&intent.meta.id).with_file_name("messages.json");
    std::fs::write(&path, "[unexpected]").unwrap();
    assert!(create(&prepared, "pending").is_err());
    assert!(engine::continue_discard(&prepared.draft_id, &prepared.draft_hash).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[unexpected]");
    conn.execute("UPDATE continuation_intents SET digest='bad'", [])
        .unwrap();
    assert!(engine::continue_restore(&prepared.draft_id, &prepared.draft_hash).is_err());
    assert!(create(&prepared, "pending").is_err());
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
}

#[test]
fn missing_project_refuses_before_claim_and_legacy_missing_session_is_not_recreated() {
    let home = IsolatedAppHome::acquire("continue-legacy-target");
    let prepared = draft(&home);
    let invalid = engine::continue_prepare(
        &prepared.import_id,
        None,
        None,
        Some("missing-project".into()),
        None,
    )
    .unwrap();
    assert!(create(&invalid, "invalid-project").is_err());
    let conn = catalog::open_catalog().unwrap();
    assert!(intents::load(&conn, "invalid-project").unwrap().is_none());
    let prepared = engine::continue_prepare(&prepared.import_id, None, None, None, None).unwrap();
    let sid = create(&prepared, "legacy").unwrap();
    conn.execute("DELETE FROM continuation_intents", [])
        .unwrap();
    store::delete_session(&sid).unwrap();
    assert!(create(&prepared, "legacy").is_err());
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
}

#[test]
fn deleted_visible_session_after_interrupted_publication_is_not_resurrected() {
    let home = IsolatedAppHome::acquire("continue-delete-gap");
    let prepared = draft(&home);
    FAIL_AT.with(|fail| *fail.borrow_mut() = Some("index"));
    assert!(create(&prepared, "visible-gap").is_err());
    FAIL_AT.with(|fail| *fail.borrow_mut() = None);
    let row = store::load_sessions_snapshot().unwrap().remove(0);
    store::delete_session(&row.id).unwrap();
    assert!(create(&prepared, "visible-gap").is_err());
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
    engine::continue_discard(&prepared.draft_id, &prepared.draft_hash).unwrap();
    engine::continue_discard(&prepared.draft_id, &prepared.draft_hash).unwrap();
    assert!(engine::recovery().unwrap().draft.is_none());
}

#[test]
fn workspace_changes_preserve_the_original_explicit_project_constraint() {
    let home = IsolatedAppHome::acquire("continue-target-scope");
    let original = draft(&home);
    let first = workspace(&home);
    let second = workspace(&home);
    let selected =
        engine::continue_workspace(&original.draft_id, &original.draft_hash, Some(&first.id))
            .unwrap();
    let switched =
        engine::continue_workspace(&selected.draft_id, &selected.draft_hash, Some(&second.id))
            .unwrap();
    assert_eq!(
        switched.project_id.as_deref(),
        Some(second.primary_project_id.as_str())
    );
    let cleared =
        engine::continue_workspace(&switched.draft_id, &switched.draft_hash, None).unwrap();
    assert!(cleared.project_id.is_none());
    let scoped = engine::continue_prepare(
        &original.import_id,
        None,
        None,
        Some(first.primary_project_id.clone()),
        None,
    )
    .unwrap();
    assert!(
        engine::continue_workspace(&scoped.draft_id, &scoped.draft_hash, Some(&second.id)).is_err()
    );
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
}

#[path = "continuation_narrowing_assets_tests.rs"]
mod narrowing_assets;
