use super::*;
use crate::session_migration::{
    catalog::{self, CatalogEntry, CatalogSort, EntryPageFilter},
    sources,
    test_home::IsolatedAppHome,
    types::Provider,
    workspace, workspace_identity,
};

fn registration(home: &IsolatedAppHome, name: &str) -> String {
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

fn history(conn: &Connection, source: &str, title: &str) -> String {
    let id = format!("imp:{}", uuid::Uuid::new_v4());
    catalog::upsert_entry(
        conn,
        &CatalogEntry {
            import_id: id.clone(),
            installation_id: source.into(),
            provider: "codex".into(),
            native_session_id: uuid::Uuid::new_v4().to_string(),
            title: title.into(),
            alias: None,
            hidden: false,
            current_revision: None,
            created_at: "2026-09-20T00:00:00Z".into(),
        },
    )
    .unwrap();
    id
}

fn saved(source: &str, name: &str) -> association::WorkspaceChoice {
    let conn = catalog::open_catalog().unwrap();
    let mut ws = workspace::source_workspace_from_folder(name);
    ws.display_name = "Same name".into();
    workspace_identity::scope(&conn, &mut ws, source).unwrap();
    catalog::upsert_workspace(&conn, &ws).unwrap();
    association::workspace(&conn, &ws.source_workspace_id)
        .unwrap()
        .unwrap()
        .0
}

fn assign(id: &str, choice: &association::WorkspaceChoice) {
    association::update(association::UpdateRequest {
        import_id: id.into(),
        expected_version: 0,
        request_id: uuid::Uuid::new_v4().to_string(),
        choice: Some(choice.clone()),
    })
    .unwrap();
}

fn filter(choice: &association::WorkspaceChoice) -> WorkspaceFilter {
    WorkspaceFilter::Workspace {
        source_workspace_id: choice.source_workspace_id.clone(),
    }
}

fn list(
    conn: &Connection,
    workspace: Option<&WorkspaceFilter>,
    cursor: Option<&str>,
    size: usize,
) -> Result<(Vec<CatalogEntry>, Option<String>, u32), ImportError> {
    catalog::list_entries_page_filtered(
        conn,
        cursor,
        Some(size),
        EntryPageFilter {
            workspace,
            ..EntryPageFilter::default_newest()
        },
    )
}

#[test]
fn empty_and_legacy_catalog_browsing_is_read_only() {
    let _home = IsolatedAppHome::acquire("workspace-catalog-empty");
    assert!(groups(None, None, None).unwrap().items.is_empty());
    assert!(!catalog::catalog_path().exists());
    assert!(
        crate::session_migration::engine::list_imported_page(None, None)
            .unwrap()
            .0
            .is_empty()
    );
    assert!(!catalog::catalog_path().exists());
    let conn = catalog::open_catalog().unwrap();
    history(&conn, "local:old", "Legacy");
    assert_eq!(
        list(&conn, Some(&WorkspaceFilter::Unassigned), None, 50)
            .unwrap()
            .2,
        1
    );
    assert!(groups(None, None, None).unwrap().items.is_empty());
    assert!(!storage::available(&conn).unwrap());
}

#[test]
fn workspace_filter_is_paged_in_sql_and_keeps_distinct_source_groups() {
    let home = IsolatedAppHome::acquire("workspace-catalog-paging");
    let a = registration(&home, "machine-a");
    let b = registration(&home, "machine-b");
    let first = saved(&a, "/same/repo");
    let second = saved(&b, "/same/repo");
    let conn = catalog::open_catalog().unwrap();
    for n in 0..6 {
        let id = history(&conn, if n < 4 { &a } else { &b }, &format!("row {n}"));
        assign(&id, if n < 4 { &first } else { &second });
    }
    let plain = history(&conn, &a, "unassigned");
    let choice = filter(&first);
    let one = list(&conn, Some(&choice), None, 2).unwrap();
    assert_eq!(one.0.len(), 2);
    assert_eq!(one.2, 4);
    let two = list(&conn, Some(&choice), one.1.as_deref(), 2).unwrap();
    assert_eq!(two.0.len(), 2);
    assert!(two.1.is_none());
    assert!(two
        .0
        .iter()
        .all(|row| !one.0.iter().any(|old| old.import_id == row.import_id)));
    assert_eq!(list(&conn, Some(&filter(&second)), None, 50).unwrap().2, 2);
    assert_eq!(
        list(&conn, Some(&WorkspaceFilter::Unassigned), None, 50)
            .unwrap()
            .0[0]
            .import_id,
        plain
    );
    assert_eq!(list(&conn, None, None, 50).unwrap().2, 7);
    let groups = groups(None, Some(50), None).unwrap();
    assert_eq!(groups.items.len(), 2);
    assert_ne!(groups.items[0].source_id, groups.items[1].source_id);
}

#[test]
fn workspace_cursor_cannot_cross_filters_and_combines_query_provider_sort() {
    let home = IsolatedAppHome::acquire("workspace-catalog-cursors");
    let source = registration(&home, "source");
    let ws = saved(&source, "/offline/repo");
    let conn = catalog::open_catalog().unwrap();
    for name in ["match one", "match two", "other"] {
        assign(&history(&conn, &source, name), &ws);
    }
    let scoped = filter(&ws);
    let first = list(&conn, Some(&scoped), None, 1).unwrap();
    for workspace in [None, Some(&WorkspaceFilter::Unassigned)] {
        assert_eq!(
            list(&conn, workspace, first.1.as_deref(), 1)
                .unwrap_err()
                .code,
            ErrorCode::PlanStale
        );
    }
    let changed = EntryPageFilter {
        workspace: Some(&scoped),
        provider: Some("codex"),
        query: Some("MATCH"),
        sort: CatalogSort::Oldest,
    };
    assert!(
        catalog::list_entries_page_filtered(&conn, first.1.as_deref(), Some(1), changed).is_err()
    );
    let matches = catalog::list_entries_page_filtered(&conn, None, Some(50), changed).unwrap();
    assert_eq!(matches.2, 2);
    assert_eq!(matches.0.len(), 2);
    let foreign = WorkspaceFilter::Workspace {
        source_workspace_id: "missing".into(),
    };
    assert_eq!(list(&conn, Some(&foreign), None, 50).unwrap().2, 0);
    assert!(groups(first.1.as_deref(), None, None).is_err());
}

#[test]
fn clear_hide_and_tombstone_update_counts_without_reviving_membership() {
    let home = IsolatedAppHome::acquire("workspace-catalog-deleted");
    let source = registration(&home, "source");
    let ws = saved(&source, "/offline/repo");
    let conn = catalog::open_catalog().unwrap();
    let clear = history(&conn, &source, "clear");
    let hide = history(&conn, &source, "hide");
    let tombstone = history(&conn, &source, "tombstone");
    for id in [&clear, &hide, &tombstone] {
        assign(id, &ws);
    }
    association::update(association::UpdateRequest {
        import_id: clear.clone(),
        expected_version: 1,
        request_id: uuid::Uuid::new_v4().to_string(),
        choice: None,
    })
    .unwrap();
    catalog::hide_entry(&conn, &hide).unwrap();
    let row = catalog::get_entry(&conn, &tombstone).unwrap().unwrap();
    catalog::insert_tombstone(&conn, &source, "codex", &row.native_session_id).unwrap();
    assert_eq!(list(&conn, Some(&filter(&ws)), None, 50).unwrap().2, 0);
    assert_eq!(
        list(&conn, Some(&WorkspaceFilter::Unassigned), None, 50)
            .unwrap()
            .2,
        1
    );
    assert_eq!(list(&conn, None, None, 50).unwrap().0[0].import_id, clear);
    assert_eq!(list(&conn, None, None, 50).unwrap().2, 1);
    // Optional serde fields accept absence as None; SQL must agree with decoding.
    conn.execute(
        "UPDATE entry_workspaces SET json=json_remove(json,'$.choice') WHERE import_id=?1",
        [&clear],
    )
    .unwrap();
    assert_eq!(
        list(&conn, Some(&WorkspaceFilter::Unassigned), None, 50)
            .unwrap()
            .2,
        1
    );
}

#[test]
fn changed_missing_and_empty_workspaces_still_organize_history_offline() {
    let home = IsolatedAppHome::acquire("workspace-catalog-offline");
    let source = registration(&home, "source");
    let ws = saved(&source, "/offline/repo");
    let conn = catalog::open_catalog().unwrap();
    let id = history(&conn, &source, "history");
    assign(&id, &ws);
    let mut empty_workspace = workspace::source_workspace_from_folder("/offline/empty");
    empty_workspace.workspace_file = Some("/offline/empty.code-workspace".into());
    empty_workspace.roots.clear();
    empty_workspace.default_root_id = None;
    workspace_identity::scope(&conn, &mut empty_workspace, &source).unwrap();
    catalog::upsert_workspace(&conn, &empty_workspace).unwrap();
    let empty = association::workspace(&conn, &empty_workspace.source_workspace_id)
        .unwrap()
        .unwrap()
        .0;
    let (_, mut changed) = association::workspace(&conn, &ws.source_workspace_id)
        .unwrap()
        .unwrap();
    changed.display_name = "new name".into();
    catalog::upsert_workspace(&conn, &changed).unwrap();
    assert_eq!(
        association::get(&id).unwrap().status,
        association::AssociationStatus::Stale
    );
    assert_eq!(list(&conn, Some(&filter(&ws)), None, 50).unwrap().2, 1);
    conn.execute(
        "DELETE FROM workspaces WHERE source_workspace_id=?1",
        [&ws.source_workspace_id],
    )
    .unwrap();
    std::fs::remove_dir(home.path.join("source")).unwrap();
    let page = groups(None, Some(1), Some(&ws.source_workspace_id)).unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.selected.unwrap().missing);
    let rest = groups(page.next_cursor.as_deref(), Some(1), None).unwrap();
    assert_eq!(rest.items.len(), 1);
    assert!(rest.next_cursor.is_none());
    assert_eq!(list(&conn, Some(&filter(&empty)), None, 50).unwrap().2, 0);
    assert_eq!(
        list(&conn, Some(&filter(&ws)), None, 50).unwrap().0[0].import_id,
        id
    );
    assert_eq!(catalog::list_entries(&conn).unwrap().len(), 1);
    assert!(!crate::paths::workspaces_file().exists());
}

#[test]
fn malformed_membership_fails_filtering_instead_of_becoming_unassigned() {
    let home = IsolatedAppHome::acquire("workspace-catalog-corrupt");
    let source = registration(&home, "source");
    let ws = saved(&source, "/offline/repo");
    let conn = catalog::open_catalog().unwrap();
    let id = history(&conn, &source, "history");
    assign(&id, &ws);
    for bad in ["broken", "{}"] {
        conn.execute(
            "UPDATE entry_workspaces SET json=?1 WHERE import_id=?2",
            params![bad, id],
        )
        .unwrap();
        assert_eq!(
            list(&conn, Some(&WorkspaceFilter::Unassigned), None, 50)
                .unwrap_err()
                .code,
            ErrorCode::SnapshotUnavailable
        );
        assert!(groups(None, None, None).is_err());
        assert_eq!(list(&conn, None, None, 50).unwrap().2, 1);
    }
}

#[test]
#[ignore = "synthetic 10k workspace membership query capacity; not a publish benchmark"]
fn ten_thousand_workspace_groups_and_memberships_remain_paged() {
    let home = IsolatedAppHome::acquire("workspace-catalog-capacity");
    let source = registration(&home, "source");
    let mut conn = catalog::open_catalog().unwrap();
    let started = std::time::Instant::now();
    let tx = conn.transaction().unwrap();
    for n in 0..10_000 {
        let id = history(&tx, &source, &format!("synthetic {n}"));
        let path = format!("/synthetic/offline/{n}");
        let mut ws = workspace::source_workspace_from_folder(&path);
        ws.source_workspace_id = format!("ws:{n:05}");
        ws.source_identity = Some(workspace_identity::WorkspaceSourceIdentity {
            source_id: source.clone(),
            provider: "codex".into(),
            locator: format!("folder:{path}"),
        });
        let raw = serde_json::to_string(&ws).unwrap();
        tx.execute(
            "INSERT INTO workspaces(source_workspace_id,display_name,json) VALUES(?1,?2,?3)",
            params![ws.source_workspace_id, ws.display_name, raw],
        )
        .unwrap();
        storage::save(
            &tx,
            &storage::Record {
                schema_version: 1,
                import_id: id,
                version: 1,
                source_id: source.clone(),
                provider: "codex".into(),
                choice: Some(association::WorkspaceChoice {
                    source_workspace_id: ws.source_workspace_id,
                    display_name: ws.display_name,
                    revision: crate::session_migration::hash::sha256_bytes(raw.as_bytes()),
                    source_locator: Some(format!("folder:{path}")),
                }),
                expected_version: 0,
                request_id: uuid::Uuid::new_v4().to_string(),
            },
        )
        .unwrap();
    }
    tx.commit().unwrap();
    let fixture_ms = started.elapsed().as_millis();
    let started = std::time::Instant::now();
    let first = groups(None, Some(50), Some("ws:09999")).unwrap();
    let groups_ms = started.elapsed().as_millis();
    assert_eq!(first.items.len(), 50);
    assert_eq!(
        first.selected.as_ref().unwrap().source_workspace_id,
        "ws:09999"
    );
    let group_bytes = serde_json::to_vec(&first).unwrap().len();
    assert!(group_bytes < 256 * 1024);
    let second = groups(first.next_cursor.as_deref(), Some(50), None).unwrap();
    assert!(second.items.iter().all(|row| !first
        .items
        .iter()
        .any(|old| old.source_workspace_id == row.source_workspace_id)));
    let started = std::time::Instant::now();
    let filter = WorkspaceFilter::Workspace {
        source_workspace_id: "ws:05000".into(),
    };
    let filtered = list(&conn, Some(&filter), None, 50).unwrap();
    let filtered_ms = started.elapsed().as_millis();
    assert_eq!(filtered.0.len(), 1);
    assert_eq!(filtered.2, 1);
    let started = std::time::Instant::now();
    assert_eq!(
        list(&conn, Some(&WorkspaceFilter::Unassigned), None, 50)
            .unwrap()
            .2,
        0
    );
    eprintln!("workspace_capacity=10000 fixture_ms={fixture_ms} groups_ms={groups_ms} groups_ipc_bytes={group_bytes} filtered_ms={filtered_ms} unassigned_ms={}", started.elapsed().as_millis());
}
