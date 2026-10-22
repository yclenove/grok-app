use super::*;
use crate::session_migration::{
    test_home::IsolatedAppHome, workspace::WorkspaceRootChoice, workspace_edit, workspace_import,
};

fn saved(home: &IsolatedAppHome, count: usize) -> workspace_edit::SavedWorkspace {
    let primary = home.path.join("primary");
    std::fs::create_dir_all(&primary).unwrap();
    let project = crate::store::add_project(primary.to_str().unwrap().into(), false).unwrap();
    let source = home.path.join("offline.code-workspace");
    let folders: Vec<_> = (0..count)
        .map(|index| {
            serde_json::json!({
                "uri": format!("vscode-remote://ssh-remote+fixture-host/remote/root-{index}")
            })
        })
        .collect();
    std::fs::write(
        &source,
        serde_json::to_vec(&serde_json::json!({"folders": folders})).unwrap(),
    )
    .unwrap();
    let preview = workspace_import::preview(workspace_import::WorkspaceSource {
        source_id: None,
        path: source.to_string_lossy().into_owned(),
        kind: workspace_import::WorkspaceSourceKind::File,
    })
    .unwrap();
    let choices = preview
        .workspace
        .roots
        .iter()
        .enumerate()
        .map(|(index, root)| {
            let target = home.path.join(format!("local-{index}"));
            std::fs::create_dir_all(&target).unwrap();
            WorkspaceRootChoice {
                root_id: root.root_id.clone(),
                primary: index == 0,
                target_project_id: (index == 0).then(|| project.id.clone()),
                explicit_target: (index != 0).then(|| target.to_string_lossy().into_owned()),
            }
        })
        .collect();
    let workspace = workspace_import::save(workspace_import::WorkspaceSaveRequest {
        source: preview.source,
        expected_fingerprint: preview.fingerprint,
        expected_saved_revision: None,
        choices,
    })
    .unwrap();
    std::fs::remove_file(source).unwrap();
    workspace_edit::get(&workspace.source_workspace_id).unwrap()
}

fn request(saved: &workspace_edit::SavedWorkspace) -> WorkspaceApplyRequest {
    WorkspaceApplyRequest {
        source_workspace_id: saved.workspace.source_workspace_id.clone(),
        expected_revision: saved.revision.clone(),
    }
}

#[test]
fn creates_native_copy_offline_and_retries_without_touching_source_projects_or_sessions() {
    let home = IsolatedAppHome::acquire("workspace-apply-copy");
    let saved = saved(&home, 3);
    let projects_before = std::fs::read(crate::paths::projects_file()).unwrap();
    let record = apply(request(&saved)).unwrap();
    assert_eq!(record.capability, WorkspaceCapability::ContextOnly);
    assert_eq!(record.roots.len(), 3);
    assert_eq!(record.roots[0].access, WorkspaceRootAccess::Write);
    assert!(record.roots[1..]
        .iter()
        .all(|root| root.access == WorkspaceRootAccess::Read));
    assert!(record.roots.iter().all(|root| root.path_ok == Some(true)));
    assert!(record.profile_ref.is_none());
    let native_bytes = std::fs::read(crate::paths::workspaces_file()).unwrap();
    assert_eq!(apply(request(&saved)).unwrap(), record);
    assert_eq!(
        std::fs::read(crate::paths::workspaces_file()).unwrap(),
        native_bytes
    );
    assert_eq!(
        native::workspaces_for_project(&record.primary_project_id).unwrap(),
        vec![record]
    );
    assert_eq!(
        std::fs::read(crate::paths::projects_file()).unwrap(),
        projects_before
    );
    assert_eq!(
        workspace_edit::get(&saved.workspace.source_workspace_id)
            .unwrap()
            .workspace,
        saved.workspace
    );
    assert!(!crate::paths::sessions_index_file().exists());
    assert!(!crate::paths::agent_home_dir().join("sandbox.toml").exists());
}

#[test]
fn stale_revision_is_rejected_before_any_native_write() {
    let home = IsolatedAppHome::acquire("workspace-apply-stale");
    let saved = saved(&home, 2);
    let mut changed = saved.workspace.clone();
    changed.mapping_choices[1].explicit_target = None;
    workspace_import::persist_workspace(changed, Some(&saved.revision)).unwrap();
    assert_eq!(
        apply(request(&saved)).unwrap_err().code,
        ErrorCode::PlanStale
    );
    assert!(!crate::paths::workspaces_file().exists());
}

#[test]
fn all_roots_must_be_mapped_no_source_path_fallback_or_silent_truncation() {
    for count in [0, 2, 10] {
        let home = IsolatedAppHome::acquire("workspace-apply-unresolved");
        let saved = saved(&home, count);
        let mut workspace = saved.workspace;
        if count == 2 {
            workspace.mapping_choices[1].explicit_target = None;
        }
        let workspace =
            workspace_import::persist_workspace(workspace, Some(&saved.revision)).unwrap();
        let saved = workspace_edit::get(&workspace.source_workspace_id).unwrap();
        let error = apply(request(&saved)).unwrap_err();
        assert_eq!(
            error.code,
            if count == 2 {
                ErrorCode::MappingConflict
            } else {
                ErrorCode::ResourceLimit
            }
        );
        assert!(!crate::paths::workspaces_file().exists());
    }
}

#[test]
fn primary_requires_existing_local_project_and_extras_require_distinct_available_targets() {
    let home = IsolatedAppHome::acquire("workspace-apply-invalid-mapping");
    let saved = saved(&home, 2);
    let mut workspace = saved.workspace.clone();
    let primary_path = native::load_projects_snapshot().unwrap()[0].path.clone();
    for variant in ["primary-directory", "duplicate", "missing", "network"] {
        workspace.mapping_choices = saved.workspace.mapping_choices.clone();
        match variant {
            "primary-directory" => {
                workspace.mapping_choices[0].target_project_id = None;
                workspace.mapping_choices[0].explicit_target = Some(primary_path.clone());
            }
            "duplicate" => {
                workspace.mapping_choices[1].explicit_target = Some(primary_path.clone())
            }
            "network" => {
                workspace.mapping_choices[1].explicit_target =
                    Some("//unreachable.invalid/share".into())
            }
            _ => {
                workspace.mapping_choices[1].explicit_target =
                    Some(home.path.join("missing").to_string_lossy().into_owned())
            }
        }
        let current = workspace_edit::get(&workspace.source_workspace_id).unwrap();
        workspace_import::persist_workspace(workspace.clone(), Some(&current.revision)).unwrap();
        let current = workspace_edit::get(&workspace.source_workspace_id).unwrap();
        assert!(apply(request(&current)).is_err(), "{variant}");
        assert!(!crate::paths::workspaces_file().exists());
    }
}

#[test]
fn edited_native_copy_is_not_overwritten_by_retry() {
    let home = IsolatedAppHome::acquire("workspace-apply-edited");
    let saved = saved(&home, 2);
    let native = apply(request(&saved)).unwrap();
    let edited = native::upsert_workspace(
        Some(native.id.clone()),
        "User name".into(),
        native.primary_project_id,
        native.roots,
    )
    .unwrap();
    let bytes = std::fs::read(crate::paths::workspaces_file()).unwrap();
    assert_eq!(
        apply(request(&saved)).unwrap_err().code,
        ErrorCode::MappingConflict
    );
    assert_eq!(
        std::fs::read(crate::paths::workspaces_file()).unwrap(),
        bytes
    );
    assert_eq!(native::get_workspace(&edited.id).unwrap(), Some(edited));
}

#[test]
fn corrupt_native_store_is_preserved_and_retry_after_repair_works() {
    let home = IsolatedAppHome::acquire("workspace-apply-corrupt");
    let saved = saved(&home, 1);
    std::fs::write(crate::paths::workspaces_file(), b"{corrupt").unwrap();
    assert_eq!(
        apply(request(&saved)).unwrap_err().code,
        ErrorCode::SnapshotUnavailable
    );
    assert_eq!(
        std::fs::read(crate::paths::workspaces_file()).unwrap(),
        b"{corrupt"
    );
    std::fs::write(
        crate::paths::workspaces_file(),
        br#"{"version":1,"workspaces":[]}"#,
    )
    .unwrap();
    assert!(apply(request(&saved)).is_ok());
}

#[test]
fn concurrent_identical_requests_publish_one_native_record() {
    let home = IsolatedAppHome::acquire("workspace-apply-concurrent");
    let saved = saved(&home, 2);
    let results = std::thread::scope(|scope| {
        let tasks: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| apply(request(&saved)).unwrap()))
            .collect();
        tasks
            .into_iter()
            .map(|task| task.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(results.iter().all(|row| row == &results[0]));
    assert_eq!(native::list_workspaces().unwrap().len(), 1);
}
