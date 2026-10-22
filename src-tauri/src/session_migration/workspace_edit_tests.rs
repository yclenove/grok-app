use super::*;
use crate::session_migration::{test_home::IsolatedAppHome, workspace_import as import};

fn saved(home: &IsolatedAppHome) -> SavedWorkspace {
    let path = home.path.join("offline.code-workspace");
    std::fs::write(
        &path,
        r#"{"folders":[{"path":"missing-a"},{"path":"missing-b"}]}"#,
    )
    .unwrap();
    let preview = import::preview(import::WorkspaceSource {
        source_id: None,
        kind: import::WorkspaceSourceKind::File,
        path: path.to_string_lossy().into_owned(),
    })
    .unwrap();
    let choices = preview
        .workspace
        .roots
        .iter()
        .enumerate()
        .map(|(index, root)| WorkspaceRootChoice {
            root_id: root.root_id.clone(),
            target_project_id: None,
            explicit_target: None,
            primary: index == 0,
        })
        .collect();
    let result = import::save(import::WorkspaceSaveRequest {
        source: preview.source,
        expected_fingerprint: preview.fingerprint,
        expected_saved_revision: None,
        choices,
    })
    .unwrap();
    std::fs::remove_file(path).unwrap();
    get(&result.source_workspace_id).unwrap()
}

fn request(saved: &SavedWorkspace) -> WorkspaceUpdateRequest {
    WorkspaceUpdateRequest {
        source_id: None,
        source_workspace_id: saved.workspace.source_workspace_id.clone(),
        expected_revision: saved.revision.clone(),
        choices: saved.workspace.mapping_choices.clone(),
    }
}

#[test]
fn offline_update_preserves_provenance_and_projects_without_source_access() {
    let home = IsolatedAppHome::acquire("workspace-offline-edit");
    let original = saved(&home);
    let projects =
        br#"[{"id":"p1","name":"Keep name","path":"/offline","pathOk":false,"lastOpenedAt":"2026-09-20T00:00:00Z","trusted":false,"pinned":true,"spaces":["one"]}]"#;
    std::fs::write(crate::paths::projects_file(), projects).unwrap();
    let mut update_request = request(&original);
    update_request.choices[0].primary = false;
    update_request.choices[1].primary = true;
    update_request.choices[1].target_project_id = Some("p1".into());
    let updated = update(update_request).unwrap();
    assert_ne!(updated.revision, original.revision);
    assert_eq!(updated.workspace.roots, original.workspace.roots);
    assert_eq!(
        updated.workspace.source_fingerprint,
        original.workspace.source_fingerprint
    );
    assert_eq!(
        updated.workspace.workspace_file,
        original.workspace.workspace_file
    );
    assert_eq!(
        updated.workspace.default_root_id.as_deref(),
        Some(original.workspace.roots[1].root_id.as_str())
    );
    assert_eq!(
        get(&updated.workspace.source_workspace_id)
            .unwrap()
            .workspace,
        updated.workspace
    );
    assert_eq!(
        std::fs::read(crate::paths::projects_file()).unwrap(),
        projects
    );
    assert!(!crate::paths::workspaces_file().exists());
    assert!(
        catalog::list_entries(&catalog::open_catalog_readonly().unwrap())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn relocated_target_is_separate_from_source_and_can_later_be_offline() {
    let home = IsolatedAppHome::acquire("workspace-target-edit");
    let original = saved(&home);
    let target = home.path.join("新目录 with space");
    std::fs::create_dir(&target).unwrap();
    let mut update_request = request(&original);
    update_request.choices[0].explicit_target = Some(target.to_string_lossy().into_owned());
    let updated = update(update_request).unwrap();
    assert_eq!(updated.workspace.roots, original.workspace.roots);
    assert_eq!(
        updated.workspace.mapping_choices[0]
            .explicit_target
            .as_deref(),
        Some(target.to_string_lossy().as_ref())
    );
    std::fs::remove_dir(&target).unwrap();
    let mut retry = request(&original);
    retry.choices[0].explicit_target = Some(target.to_string_lossy().into_owned());
    assert_eq!(update(retry).unwrap().revision, updated.revision);
    let mut offline_edit = request(&updated);
    offline_edit.choices[0].primary = false;
    offline_edit.choices[1].primary = true;
    assert!(update(offline_edit).is_ok());
}

#[test]
fn concurrent_edits_conflict_and_exact_lost_reply_retry_is_idempotent() {
    let home = IsolatedAppHome::acquire("workspace-edit-cas");
    let original = saved(&home);
    let mut first = request(&original);
    first.choices[0].primary = false;
    first.choices[1].primary = true;
    let mut retry = request(&original);
    retry.choices = first.choices.clone();
    let updated = update(first).unwrap();
    assert_eq!(update(retry).unwrap().revision, updated.revision);
    assert_eq!(
        update(request(&original)).unwrap_err().code,
        ErrorCode::PlanStale
    );
    assert_eq!(
        get(&original.workspace.source_workspace_id)
            .unwrap()
            .workspace,
        updated.workspace
    );
}

#[test]
fn invalid_roots_targets_projects_or_primary_cannot_change_saved_metadata() {
    let home = IsolatedAppHome::acquire("workspace-edit-invalid");
    let original = saved(&home);
    for case in 0..7 {
        let mut invalid = request(&original);
        match case {
            0 => invalid.choices[0].root_id = "foreign".into(),
            1 => invalid.choices[0].primary = false,
            2 => invalid.choices[0].target_project_id = Some("nonexistent".into()),
            3 => invalid.choices[0].explicit_target = Some("relative".into()),
            4 => {
                invalid.choices[0].explicit_target =
                    Some(home.path.join("gone").to_string_lossy().into_owned())
            }
            5 => {
                invalid.choices[0].explicit_target = Some(home.path.to_string_lossy().into_owned());
                invalid.choices[0].target_project_id = Some("both".into());
            }
            _ => {
                invalid.choices.push(invalid.choices[0].clone());
            }
        }
        assert!(update(invalid).is_err(), "case {case}");
        assert_eq!(
            get(&original.workspace.source_workspace_id)
                .unwrap()
                .revision,
            original.revision
        );
    }
}

#[test]
fn failed_update_rolls_back_without_losing_previous_choices() {
    let home = IsolatedAppHome::acquire("workspace-edit-rollback");
    let original = saved(&home);
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch("CREATE TRIGGER reject_edit BEFORE UPDATE ON workspaces BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
    let mut change = request(&original);
    change.choices[0].primary = false;
    change.choices[1].primary = true;
    assert!(update(change).is_err());
    assert_eq!(
        get(&original.workspace.source_workspace_id)
            .unwrap()
            .revision,
        original.revision
    );
}

#[test]
fn get_is_read_only_and_rejects_mismatched_catalog_identity() {
    let home = IsolatedAppHome::acquire("workspace-edit-identity");
    assert!(get("ws:missing").is_err());
    assert!(!catalog::catalog_path().exists());
    let original = saved(&home);
    let conn = catalog::open_catalog().unwrap();
    let mut wrong = original.workspace.clone();
    wrong.source_workspace_id = "ws:different".into();
    conn.execute(
        "UPDATE workspaces SET json=?1 WHERE source_workspace_id=?2",
        rusqlite::params![
            serde_json::to_string(&wrong).unwrap(),
            original.workspace.source_workspace_id
        ],
    )
    .unwrap();
    assert_eq!(
        get(&original.workspace.source_workspace_id)
            .unwrap_err()
            .code,
        ErrorCode::SnapshotUnavailable
    );
    assert!(update(request(&original)).is_err());
}

#[test]
fn offline_commands_read_update_and_reopen_the_same_identity() {
    let home = IsolatedAppHome::acquire("workspace-edit-command");
    let original = saved(&home);
    tauri::async_runtime::block_on(async {
        let current = crate::commands::session_import_workspace_get(
            original.workspace.source_workspace_id.clone(),
        )
        .await
        .unwrap();
        let mut change = request(&current);
        change.choices[0].primary = false;
        change.choices[1].primary = true;
        let updated = crate::commands::session_import_workspace_update(change)
            .await
            .unwrap();
        let reopened = crate::commands::session_import_workspace_get(
            updated.workspace.source_workspace_id.clone(),
        )
        .await
        .unwrap();
        assert_eq!(reopened.revision, updated.revision);
        assert_eq!(reopened.workspace, updated.workspace);
    });
}
