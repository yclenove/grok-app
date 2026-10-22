use super::*;
use crate::session_migration::{
    sources, test_home::IsolatedAppHome, types::Provider, workspace, workspace_identity,
};

fn register(home: &IsolatedAppHome, name: &str) -> String {
    let path = home.path.join(name);
    std::fs::create_dir_all(&path).unwrap();
    sources::register(
        &catalog::open_catalog().unwrap(),
        Provider::Codex,
        &path,
        None,
        name,
    )
    .unwrap()
    .source_id
}

fn history(source: &str) -> String {
    let id = format!("imp:{}", uuid::Uuid::new_v4());
    catalog::upsert_entry(
        &catalog::open_catalog().unwrap(),
        &CatalogEntry {
            import_id: id.clone(),
            installation_id: source.into(),
            provider: "codex".into(),
            native_session_id: uuid::Uuid::new_v4().to_string(),
            title: "Synthetic history".into(),
            alias: None,
            hidden: false,
            current_revision: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        },
    )
    .unwrap();
    id
}

fn saved(source: &str, folder: &str) -> WorkspaceChoice {
    let conn = catalog::open_catalog().unwrap();
    let mut ws = workspace::source_workspace_from_folder(folder);
    ws.display_name = "Same name".into();
    workspace_identity::scope(&conn, &mut ws, source).unwrap();
    catalog::upsert_workspace(&conn, &ws).unwrap();
    super::workspace(&conn, &ws.source_workspace_id)
        .unwrap()
        .unwrap()
        .0
}

fn request(id: &str, version: u64, choice: Option<WorkspaceChoice>) -> UpdateRequest {
    UpdateRequest {
        import_id: id.into(),
        expected_version: version,
        request_id: uuid::Uuid::new_v4().to_string(),
        choice,
    }
}

fn raw_record(id: &str) -> String {
    catalog::open_catalog_readonly()
        .unwrap()
        .query_row(
            "SELECT json FROM entry_workspaces WHERE import_id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn legacy_reads_do_not_create_optional_schema_or_resolve_path_identity() {
    let _home = IsolatedAppHome::acquire("association-legacy");
    let id = history("local:synthetic-locator-hash");
    let result = get(&id).unwrap();
    assert!(!result.source_ready);
    assert_eq!(result.version, 0);
    assert!(eligible(&id, None, None).unwrap().items.is_empty());
    let conn = catalog::open_catalog_readonly().unwrap();
    let count: u32 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='entry_workspaces'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        catalog::get_entry(&conn, &id)
            .unwrap()
            .unwrap()
            .installation_id,
        "local:synthetic-locator-hash"
    );
}

#[test]
fn membership_is_explicit_same_source_only_and_cursors_cannot_cross_histories() {
    let home = IsolatedAppHome::acquire("association-options");
    let source = register(&home, "first");
    let other = register(&home, "second");
    let id = history(&source);
    let second_id = history(&source);
    let first = saved(&source, "/offline/one");
    let second = saved(&source, "/offline/two");
    let foreign = saved(&other, "/offline/one");
    let page = eligible(&id, None, Some(1)).unwrap();
    assert_eq!(page.items.len(), 1);
    let tail = eligible(&id, page.next_cursor.as_deref(), Some(1)).unwrap();
    assert_eq!(tail.items.len(), 1);
    assert!(tail.next_cursor.is_none());
    assert_ne!(page.items[0], tail.items[0]);
    for choice in page.items.iter().chain(&tail.items) {
        assert!(*choice == first || *choice == second);
    }
    assert!(eligible(&second_id, page.next_cursor.as_deref(), Some(1)).is_err());
    assert_eq!(
        update(request(&id, 0, Some(foreign))).unwrap_err().code,
        ErrorCode::MappingConflict
    );
    assert!(get(&id).unwrap().choice.is_none());
    let conn = catalog::open_catalog().unwrap();
    conn.execute(
        "UPDATE entries SET installation_id=?1 WHERE import_id=?2",
        params![other, id],
    )
    .unwrap();
    assert!(eligible(&id, page.next_cursor.as_deref(), Some(1)).is_err());
}

#[test]
fn reopen_clear_and_lost_response_replay_preserve_versions() {
    let home = IsolatedAppHome::acquire("association-retry");
    let source = register(&home, "source");
    let id = history(&source);
    let choice = saved(&source, "/offline/root");
    let first = request(&id, 0, Some(choice.clone()));
    assert_eq!(update(first.clone()).unwrap().version, 1);
    let bytes = raw_record(&id);
    assert_eq!(update(first.clone()).unwrap().version, 1);
    assert_eq!(raw_record(&id), bytes);
    let result = get(&id).unwrap();
    assert_eq!(result.choice, Some(choice.clone()));
    assert_eq!(result.status, AssociationStatus::Assigned);
    let mut changed = first.clone();
    changed.choice = None;
    assert_eq!(update(changed).unwrap_err().code, ErrorCode::PlanStale);
    assert_eq!(update(request(&id, 1, None)).unwrap().version, 2);
    assert_eq!(update(first).unwrap_err().code, ErrorCode::PlanStale);
    assert_eq!(get(&id).unwrap().status, AssociationStatus::Unassigned);
    assert_eq!(update(request(&id, 2, Some(choice))).unwrap().version, 3);
}

#[test]
fn concurrent_saves_have_one_winner() {
    let home = IsolatedAppHome::acquire("association-concurrent");
    let source = register(&home, "source");
    let id = history(&source);
    let one = request(&id, 0, Some(saved(&source, "/offline/one")));
    let two = request(&id, 0, Some(saved(&source, "/offline/two")));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = [one, two]
            .into_iter()
            .map(|input| {
                let barrier = barrier.clone();
                scope.spawn(move || {
                    barrier.wait();
                    update(input)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.into_iter().find_map(Result::err).unwrap().code,
        ErrorCode::PlanStale
    );
    assert_eq!(get(&id).unwrap().version, 1);
}

#[test]
fn changed_or_missing_workspace_requires_confirmation_and_offline_sources_work() {
    let home = IsolatedAppHome::acquire("association-revision");
    let source = register(&home, "source");
    let id = history(&source);
    let choice = saved(&source, "/offline/root");
    update(request(&id, 0, Some(choice.clone()))).unwrap();
    std::fs::remove_dir(home.path.join("source")).unwrap();
    assert_eq!(get(&id).unwrap().status, AssociationStatus::Assigned);
    let conn = catalog::open_catalog().unwrap();
    let (_, mut ws) = super::workspace(&conn, &choice.source_workspace_id)
        .unwrap()
        .unwrap();
    ws.display_name = "Changed".into();
    catalog::upsert_workspace(&conn, &ws).unwrap();
    assert_eq!(get(&id).unwrap().status, AssociationStatus::Stale);
    assert_eq!(
        update(request(&id, 1, Some(choice))).unwrap_err().code,
        ErrorCode::PlanStale
    );
    let refreshed = eligible(&id, None, None).unwrap().items.remove(0);
    assert_eq!(
        update(request(&id, 1, Some(refreshed.clone())))
            .unwrap()
            .status,
        AssociationStatus::Assigned
    );
    conn.execute(
        "DELETE FROM workspaces WHERE source_workspace_id=?1",
        [&refreshed.source_workspace_id],
    )
    .unwrap();
    assert_eq!(get(&id).unwrap().status, AssociationStatus::Missing);
    assert_eq!(
        update(request(&id, 2, None)).unwrap().status,
        AssociationStatus::Unassigned
    );
    assert!(!crate::paths::workspaces_file().exists());
    assert_eq!(
        conn.query_row("SELECT count(*) FROM continuation_refs", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
}

#[test]
fn missing_registration_is_unresolved_but_corrupt_registration_is_an_error() {
    let home = IsolatedAppHome::acquire("association-registration");
    let source = register(&home, "source");
    let id = history(&source);
    let choice = saved(&source, "/offline/root");
    let conn = catalog::open_catalog().unwrap();
    conn.execute(
        "UPDATE sources SET profile='broken' WHERE installation_id=?1",
        [&source],
    )
    .unwrap();
    assert_eq!(get(&id).unwrap_err().code, ErrorCode::SnapshotUnavailable);
    conn.execute("DELETE FROM sources WHERE installation_id=?1", [&source])
        .unwrap();
    assert!(!get(&id).unwrap().source_ready);
    assert_eq!(
        update(request(&id, 0, Some(choice))).unwrap_err().code,
        ErrorCode::MappingConflict
    );
}

#[test]
fn corrupted_schema_or_record_is_preserved_instead_of_reset() {
    let home = IsolatedAppHome::acquire("association-corrupt");
    let source = register(&home, "source");
    let id = history(&source);
    update(request(&id, 0, Some(saved(&source, "/offline/root")))).unwrap();
    let conn = catalog::open_catalog().unwrap();
    conn.execute(
        "UPDATE meta SET value='2' WHERE key='entry_workspaces_version'",
        [],
    )
    .unwrap();
    assert_eq!(get(&id).unwrap_err().code, ErrorCode::SnapshotUnavailable);
    assert!(update(request(&id, 1, None)).is_err());
    conn.execute(
        "UPDATE meta SET value='1' WHERE key='entry_workspaces_version'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE entry_workspaces SET json='broken' WHERE import_id=?1",
        [&id],
    )
    .unwrap();
    assert_eq!(get(&id).unwrap_err().code, ErrorCode::SnapshotUnavailable);
    assert!(update(request(&id, 1, None)).is_err());
    assert_eq!(raw_record(&id), "broken");
}

#[test]
fn hide_revokes_even_unsaved_drafts_and_rolls_back_with_its_caller() {
    let home = IsolatedAppHome::acquire("association-hide");
    let source = register(&home, "source");
    let id = history(&source);
    let input = request(&id, 0, Some(saved(&source, "/offline/root")));
    let mut conn = catalog::open_catalog().unwrap();
    {
        let tx = conn.transaction().unwrap();
        catalog::hide_entry(&tx, &id).unwrap();
    }
    assert_eq!(get(&id).unwrap().version, 0);
    catalog::hide_entry(&conn, &id).unwrap();
    assert_eq!(
        get(&id).unwrap_err().code,
        ErrorCode::HistoryAccessUnavailable
    );
    conn.execute("UPDATE entries SET hidden=0 WHERE import_id=?1", [&id])
        .unwrap();
    assert_eq!(get(&id).unwrap().version, 1);
    assert_eq!(update(input).unwrap_err().code, ErrorCode::PlanStale);
}

#[test]
fn failed_hide_does_not_partially_revoke_and_delete_tombstone_failure_is_atomic() {
    let home = IsolatedAppHome::acquire("association-delete-rollback");
    let source = register(&home, "source");
    let id = history(&source);
    update(request(&id, 0, Some(saved(&source, "/offline/root")))).unwrap();
    let before = raw_record(&id);
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch("CREATE TRIGGER fail_hide BEFORE UPDATE OF hidden ON entries BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
    assert!(catalog::hide_entry(&conn, &id).is_err());
    assert_eq!(raw_record(&id), before);
    conn.execute_batch("DROP TRIGGER fail_hide; CREATE TRIGGER fail_tombstone BEFORE INSERT ON tombstones BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
    assert!(crate::session_migration::jobs::delete_imported(&conn, &id).is_err());
    assert_eq!(raw_record(&id), before);
    assert_eq!(get(&id).unwrap().status, AssociationStatus::Assigned);
}

#[test]
fn delete_and_direct_tombstone_do_not_resurrect_links_after_reimport() {
    let home = IsolatedAppHome::acquire("association-delete");
    let source = register(&home, "source");
    let conn = catalog::open_catalog().unwrap();
    for direct in [false, true] {
        let id = history(&source);
        let choice = saved(&source, "/offline/root");
        update(request(&id, 0, Some(choice.clone()))).unwrap();
        let entry = catalog::get_entry(&conn, &id).unwrap().unwrap();
        let late = request(&id, 1, Some(choice));
        if direct {
            catalog::insert_tombstone(&conn, &source, "codex", &entry.native_session_id).unwrap();
        } else {
            crate::session_migration::jobs::delete_imported(&conn, &id).unwrap();
        }
        assert_eq!(
            get(&id).unwrap_err().code,
            ErrorCode::HistoryAccessUnavailable
        );
        catalog::clear_tombstone(&conn, &source, "codex", &entry.native_session_id).unwrap();
        conn.execute("UPDATE entries SET hidden=0 WHERE import_id=?1", [&id])
            .unwrap();
        assert_eq!(get(&id).unwrap().status, AssociationStatus::Unassigned);
        assert_eq!(get(&id).unwrap().version, 2);
        assert_eq!(update(late).unwrap_err().code, ErrorCode::PlanStale);
    }
}

#[test]
fn catalog_rebuild_keeps_workspace_membership() {
    let home = IsolatedAppHome::acquire("association-rebuild");
    let source = register(&home, "source");
    let id = history(&source);
    update(request(&id, 0, Some(saved(&source, "/offline/root")))).unwrap();
    let before = raw_record(&id);
    catalog::rebuild_catalog_index_with_hooks(&|| false, &mut |_, _, _| {}).unwrap();
    assert_eq!(raw_record(&id), before);
    assert_eq!(get(&id).unwrap().status, AssociationStatus::Assigned);
}

#[test]
fn escaped_metadata_cannot_publish_a_record_that_exceeds_the_read_limit() {
    let home = IsolatedAppHome::acquire("association-size");
    let source = register(&home, "source");
    let id = history(&source);
    let conn = catalog::open_catalog().unwrap();
    let mut ws = workspace::source_workspace_from_folder(&format!("/{}", "\u{1}".repeat(6000)));
    ws.display_name = "\u{1}".repeat(8192);
    workspace_identity::scope(&conn, &mut ws, &source).unwrap();
    catalog::upsert_workspace(&conn, &ws).unwrap();
    let choice = super::workspace(&conn, &ws.source_workspace_id)
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(
        update(request(&id, 0, Some(choice))).unwrap_err().code,
        ErrorCode::SnapshotUnavailable
    );
    assert_eq!(get(&id).unwrap().version, 0);
    assert!(storage::load(&conn, &id).unwrap().is_none());
}
