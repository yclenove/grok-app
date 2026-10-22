use super::*;
use crate::session_migration::{
    test_home::IsolatedAppHome,
    workspace::{source_workspace_from_folder, WorkspaceRootChoice},
};

fn fixture(home: &IsolatedAppHome) -> (crate::store::Project, SourceWorkspace) {
    let path = home.path.join("project");
    std::fs::create_dir_all(&path).unwrap();
    let project = crate::store::ensure_import_project("project", path.to_str().unwrap())
        .unwrap()
        .project;
    let mut workspace = source_workspace_from_folder(path.to_str().unwrap());
    workspace.mapping_choices = vec![WorkspaceRootChoice {
        root_id: workspace.roots[0].root_id.clone(),
        target_project_id: Some(project.id.clone()),
        explicit_target: None,
        primary: true,
    }];
    (project, workspace)
}

fn mapping(project_id: &str) -> WorkspaceMapping {
    WorkspaceMapping {
        source: None,
        root_id: "import:fixture".into(),
        target_project_id: Some(project_id.into()),
        explicit_target: None,
        unassigned: false,
        user_choice_version: 1,
        primary: true,
        suggested_target: None,
        create_project: false,
        import_unassigned_anyway: false,
    }
}

#[test]
fn saved_workspaces_and_mapping_references_prevent_removal() {
    let home = IsolatedAppHome::acquire("catalog-project-references");
    let (project, workspace) = fixture(&home);
    let conn = catalog::open_catalog().unwrap();
    catalog::upsert_workspace(&conn, &workspace).unwrap();
    assert!(ensure_unreferenced(&conn, &project.id)
        .unwrap_err()
        .message
        .contains("saved imported workspace"));
    conn.execute("DELETE FROM workspaces", []).unwrap();
    catalog::upsert_mapping(&conn, &mapping(&project.id)).unwrap();
    assert!(ensure_unreferenced(&conn, &project.id).is_err()); // Unpublished mappings also retain ownership.
    let entry = catalog::CatalogEntry {
        import_id: "fixture".into(),
        installation_id: "installation".into(),
        provider: "fixture".into(),
        native_session_id: "native".into(),
        title: "Fixture".into(),
        alias: None,
        hidden: true,
        current_revision: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    catalog::upsert_entry(&conn, &entry).unwrap();
    assert!(ensure_unreferenced(&conn, &project.id).is_err()); // Hidden alone is not deletion.
    catalog::insert_tombstone(
        &conn,
        &entry.installation_id,
        &entry.provider,
        &entry.native_session_id,
    )
    .unwrap();
    ensure_unreferenced(&conn, &project.id).unwrap();
}

#[test]
fn corrupt_reference_rows_are_not_interpreted_as_no_references() {
    let home = IsolatedAppHome::acquire("catalog-project-corrupt-references");
    let (project, workspace) = fixture(&home);
    let conn = catalog::open_catalog().unwrap();
    catalog::upsert_workspace(&conn, &workspace).unwrap();
    conn.execute("UPDATE workspaces SET json='{}'", []).unwrap();
    assert!(ensure_unreferenced(&conn, &project.id).is_err());
    conn.execute("DELETE FROM workspaces", []).unwrap();
    conn.execute(
        "INSERT INTO mappings(root_id,json) VALUES ('broken','{bad')",
        [],
    )
    .unwrap();
    assert!(ensure_unreferenced(&conn, &project.id).is_err());
    assert_eq!(
        crate::store::load_projects_snapshot().unwrap()[0].id,
        project.id
    );
}

#[test]
fn catalog_publication_rechecks_targets_instead_of_publishing_removed_ids() {
    let home = IsolatedAppHome::acquire("catalog-project-stale-reference");
    let (project, workspace) = fixture(&home);
    let conn = catalog::open_catalog().unwrap();
    crate::store::remove_import_project(&project).unwrap();
    assert!(catalog::upsert_workspace(&conn, &workspace).is_err());
    assert!(catalog::upsert_mapping(&conn, &mapping(&project.id)).is_err());
    assert!(catalog::list_workspaces(&conn).unwrap().is_empty());
    assert!(catalog::list_mappings(&conn).unwrap().is_empty());
}

#[test]
fn concurrent_catalog_reference_and_undo_are_serialized_in_catalog_then_store_order() {
    let home = IsolatedAppHome::acquire("catalog-project-removal-race");
    let (project, workspace) = fixture(&home);
    let conn = catalog::open_catalog().unwrap();
    drop(conn);
    let barrier = std::sync::Barrier::new(2);
    let (removed, mapped) = std::thread::scope(|scope| {
        let remove = scope.spawn(|| {
            let conn = Connection::open(catalog::catalog_path()).unwrap();
            barrier.wait();
            let tx = Transaction::new_unchecked(&conn, TransactionBehavior::Immediate).unwrap();
            ensure_unreferenced(&tx, &project.id)?;
            crate::store::remove_import_project(&project).map_err(error)
        });
        let map = scope.spawn(|| {
            let conn = Connection::open(catalog::catalog_path()).unwrap();
            barrier.wait();
            catalog::upsert_workspace(&conn, &workspace)
        });
        (remove.join().unwrap(), map.join().unwrap())
    });
    assert_ne!(removed.is_ok(), mapped.is_ok());
    assert_eq!(
        !crate::store::load_projects_snapshot().unwrap().is_empty(),
        mapped.is_ok()
    );
}
