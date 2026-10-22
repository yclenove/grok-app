use super::*;
use crate::session_migration::test_home::IsolatedAppHome;
use crate::workspace_store as workspaces;

fn project(home: &IsolatedAppHome, id: &str) -> Project {
    let path = home.path.join(id);
    fs::create_dir_all(&path).unwrap();
    ensure_import_project(id, path.to_str().unwrap())
        .unwrap()
        .project
}

fn workspace(project: &Project) -> workspaces::WorkspaceRecord {
    workspaces::upsert_workspace(None, "Reference test".into(), project.id.clone(), vec![]).unwrap()
}

fn bind(
    session: &SessionMeta,
    workspace: &workspaces::WorkspaceRecord,
) -> Result<SessionMeta, String> {
    set_session_workspace(
        &session.id,
        Some(workspace.id.clone()),
        Some(workspaces::root_snapshot(&workspace.roots)),
        Some("context_only".into()),
    )
}

#[test]
fn undo_keeps_native_workspace_references_and_unreadable_workspace_store() {
    let home = IsolatedAppHome::acquire("project-native-reference");
    let project = project(&home, "primary");
    let workspace = workspace(&project);
    assert!(remove_import_project(&project)
        .unwrap_err()
        .contains("workspaces"));
    workspaces::delete_workspace(&workspace.id).unwrap();
    let path = crate::paths::workspaces_file();
    let valid = fs::read(&path).unwrap();
    for invalid in ["[42]", "broken", r#"{"version":99,"workspaces":[]}"#] {
        fs::write(&path, invalid).unwrap();
        assert!(remove_import_project(&project).is_err());
        assert_eq!(load_projects_snapshot().unwrap()[0].id, project.id);
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    }
    fs::write(path, valid).unwrap();
    remove_import_project(&project).unwrap();
    assert!(load_projects_snapshot().unwrap().is_empty());
    assert!(Path::new(&project.path).is_dir());
}

#[test]
fn workspace_publication_rechecks_all_reviewed_projects_including_extra_targets() {
    let home = IsolatedAppHome::acquire("project-stale-copy");
    let primary = project(&home, "primary");
    let extra = project(&home, "extra");
    let mut copy = workspace(&primary);
    copy.id = "new-import-copy".into();
    copy.roots.push(workspaces::WorkspaceRoot {
        path: workspaces::canonicalize_workspace_root(&extra.path)
            .unwrap()
            .0,
        role: workspaces::WorkspaceRootRole::Extra,
        access: workspaces::WorkspaceRootAccess::Read,
        path_ok: Some(true),
    });
    let expected = vec![
        (primary.id.clone(), primary.path),
        (extra.id.clone(), extra.path),
    ];
    let moved = home.path.join("moved-extra");
    fs::create_dir(&moved).unwrap();
    relocate_project(&extra.id, moved.to_string_lossy().into_owned()).unwrap();
    let before = fs::read(crate::paths::workspaces_file()).unwrap();
    assert!(workspaces::insert_imported(copy, &expected)
        .unwrap_err()
        .contains("project target changed"));
    assert_eq!(fs::read(crate::paths::workspaces_file()).unwrap(), before);
}

#[test]
fn concurrent_native_references_and_undo_never_leave_new_orphans() {
    let home = IsolatedAppHome::acquire("project-reference-race");
    let _ = load_settings();
    for index in 0..16 {
        let project = project(&home, &format!("p-{index}"));
        let barrier = std::sync::Barrier::new(2);
        let (removed, referenced) = std::thread::scope(|scope| {
            let remove = scope.spawn(|| {
                barrier.wait();
                remove_import_project(&project)
            });
            let reference = scope.spawn(|| {
                barrier.wait();
                if index % 2 == 0 {
                    workspaces::upsert_workspace(
                        None,
                        "Concurrent".into(),
                        project.id.clone(),
                        vec![],
                    )
                    .map(|_| ())
                } else {
                    create_session(Some(project.id.clone()), None, false).map(|_| ())
                }
            });
            (remove.join().unwrap(), reference.join().unwrap())
        });
        assert_ne!(removed.is_ok(), referenced.is_ok());
        let exists = load_projects_snapshot()
            .unwrap()
            .iter()
            .any(|row| row.id == project.id);
        assert_eq!(exists, referenced.is_ok());
    }
}

#[test]
fn binding_rejects_wrong_project_and_changed_snapshot_and_deletion_waits_for_detach() {
    let home = IsolatedAppHome::acquire("workspace-bind-references");
    let primary = project(&home, "primary");
    let other = project(&home, "other");
    let workspace = workspace(&primary);
    let wrong = create_session(Some(other.id.clone()), None, false).unwrap();
    let before = fs::read(sessions_index_file()).unwrap();
    assert!(bind(&wrong, &workspace)
        .unwrap_err()
        .contains("does not match"));
    assert_eq!(fs::read(sessions_index_file()).unwrap(), before);
    let session = create_session(Some(primary.id), None, false).unwrap();
    assert!(set_session_workspace(
        &session.id,
        Some(workspace.id.clone()),
        Some("stale".into()),
        None
    )
    .is_err());
    bind(&session, &workspace).unwrap();
    assert!(workspaces::delete_workspace(&workspace.id)
        .unwrap_err()
        .contains("still has sessions"));
    let moved = set_session_project(&session.id, Some(other.id)).unwrap();
    assert!(moved.workspace_id.is_none());
    assert!(moved.workspace_root_snapshot.is_none());
    assert!(moved.workspace_capability.is_none());
    workspaces::delete_workspace(&workspace.id).unwrap();
}

#[test]
fn relocated_primary_cannot_silently_spawn_or_bind_the_old_workspace() {
    let home = IsolatedAppHome::acquire("workspace-spawn-project-drift");
    let project = project(&home, "primary");
    let workspace = workspace(&project);
    let session = create_session(Some(project.id.clone()), None, false).unwrap();
    let bound = bind(&session, &workspace).unwrap();
    let moved = home.path.join("moved");
    fs::create_dir(&moved).unwrap();
    relocate_project(&project.id, moved.to_string_lossy().into_owned()).unwrap();
    assert!(bind(&session, &workspace).is_err());
    assert!(workspaces::resolve_session_spawn("off", None, "shared", &bound).is_err());
    remove_project(&project.id).unwrap();
    assert!(workspaces::resolve_session_spawn("off", None, "shared", &bound).is_err());
    assert!(workspaces::get_workspace(&workspace.id).unwrap().is_some());
}

#[test]
fn legacy_orphans_remain_editable_but_cannot_gain_new_invalid_bindings() {
    let home = IsolatedAppHome::acquire("session-orphan-compatibility");
    let project = project(&home, "primary");
    let session = create_session(Some(project.id.clone()), None, false).unwrap();
    remove_project(&project.id).unwrap();
    update_session_row(&session.id, |row| {
        row.title = "Renamed orphan".into();
        Ok(())
    })
    .unwrap();
    assert_eq!(load_sessions_snapshot().unwrap()[0].title, "Renamed orphan");
    assert!(create_session(Some(project.id), None, false).is_err());
    assert_eq!(load_sessions_snapshot().unwrap().len(), 1);
    assert!(set_session_project(&session.id, None)
        .unwrap()
        .project_id
        .is_none());
}

#[test]
fn identical_bind_and_spawn_reject_changed_roots_while_validated_grants_stay_frozen() {
    let home = IsolatedAppHome::acquire("workspace-root-snapshot-drift");
    let project = project(&home, "primary");
    let workspace = workspace(&project);
    let session = create_session(Some(project.id.clone()), None, false).unwrap();
    let bound = bind(&session, &workspace).unwrap();
    let prepared = workspaces::resolve_session_spawn("strict", None, "shared", &bound).unwrap();
    assert_eq!(prepared.sandbox, "strict");
    assert_eq!(prepared.roots.len(), 1);
    let extra = home.path.join("extra");
    fs::create_dir(&extra).unwrap();
    let (extra, _) = workspaces::canonicalize_workspace_root(extra.to_str().unwrap()).unwrap();
    let mut roots = workspace.roots.clone();
    roots.push(workspaces::WorkspaceRoot {
        path: extra.clone(),
        role: workspaces::WorkspaceRootRole::Extra,
        access: workspaces::WorkspaceRootAccess::Read,
        path_ok: Some(true),
    });
    let changed = workspaces::upsert_workspace(
        Some(workspace.id.clone()),
        "Edited".into(),
        project.id,
        roots,
    )
    .unwrap();
    assert!(!prepared.roots.contains(&extra));
    assert!(bind(&bound, &workspace).is_err());
    assert!(workspaces::resolve_session_spawn("off", None, "shared", &bound).is_err());
    let reviewed = bind(&bound, &changed).unwrap();
    assert!(
        workspaces::resolve_session_spawn("strict", None, "shared", &reviewed)
            .unwrap()
            .roots
            .contains(&extra)
    );
    fs::remove_dir(home.path.join("extra")).unwrap();
    assert!(workspaces::resolve_session_spawn("off", None, "shared", &reviewed).is_err());
}

#[test]
fn concurrent_binding_and_workspace_deletion_cannot_commit_a_missing_workspace() {
    let home = IsolatedAppHome::acquire("workspace-binding-delete-race");
    let project = project(&home, "primary");
    for _ in 0..8 {
        let workspace = workspace(&project);
        let session = create_session(Some(project.id.clone()), None, false).unwrap();
        let barrier = std::sync::Barrier::new(2);
        let (bound, deleted) = std::thread::scope(|scope| {
            let binding = scope.spawn(|| {
                barrier.wait();
                bind(&session, &workspace)
            });
            let deletion = scope.spawn(|| {
                barrier.wait();
                workspaces::delete_workspace(&workspace.id)
            });
            (binding.join().unwrap(), deletion.join().unwrap())
        });
        assert_ne!(bound.is_ok(), deleted.is_ok());
        let stored = load_sessions_snapshot()
            .unwrap()
            .into_iter()
            .find(|row| row.id == session.id)
            .unwrap();
        assert_eq!(stored.workspace_id.is_some(), bound.is_ok());
        assert_eq!(
            workspaces::get_workspace(&workspace.id).unwrap().is_some(),
            bound.is_ok()
        );
    }
}
