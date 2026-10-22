use super::*;
use crate::session_migration::test_home::IsolatedAppHome;

fn project(home: &IsolatedAppHome, id: &str) -> Project {
    let path = home.path.join(id);
    fs::create_dir_all(&path).unwrap();
    ensure_import_project(id, path.to_str().unwrap())
        .unwrap()
        .project
}

#[test]
fn imported_project_is_untrusted_and_exact_retry_preserves_user_settings_offline() {
    let home = IsolatedAppHome::acquire("project-create-retry");
    let project = project(&home, "stable-id");
    assert!(!project.trusted);
    assert!(project.permission_policy.is_none());
    update_projects(|list| {
        list[0].name = "User name".into();
        list[0].pinned = true;
        list[0].color = Some("blue".into());
        Ok(())
    })
    .unwrap();
    fs::remove_dir(&project.path).unwrap();
    let bytes = fs::read(projects_file()).unwrap();
    let retry = ensure_import_project(&project.id, &project.path).unwrap();
    assert!(!retry.created);
    assert_eq!(retry.project.name, "User name");
    assert!(retry.project.pinned);
    assert_eq!(retry.project.color.as_deref(), Some("blue"));
    assert_eq!(fs::read(projects_file()).unwrap(), bytes);
    assert!(!crate::paths::workspaces_file().exists());
}

#[test]
fn reuse_does_not_upgrade_trust_or_drop_unknown_fields() {
    let home = IsolatedAppHome::acquire("project-reuse-fields");
    let project = project(&home, "existing");
    let mut rows: serde_json::Value =
        serde_json::from_slice(&fs::read(projects_file()).unwrap()).unwrap();
    rows[0]["spaces"] = serde_json::json!(["team", "private"]);
    rows[0]["futureSettings"] = serde_json::json!({"keep": true});
    rows[0]["modelId"] = serde_json::json!("custom-fixture");
    write_json(&projects_file(), &rows).unwrap();
    let before = fs::read(projects_file()).unwrap();
    let reused = ensure_import_project("new-request", &project.path).unwrap();
    assert_eq!(reused.project.id, project.id);
    assert!(!reused.created);
    assert!(!reused.project.trusted);
    assert_eq!(fs::read(projects_file()).unwrap(), before);
    rename_project(&project.id, "Rename").unwrap();
    update_projects(|rows| {
        rows[0].model_id = None;
        Ok(())
    })
    .unwrap();
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(projects_file()).unwrap()).unwrap();
    assert_eq!(after[0]["spaces"], rows[0]["spaces"]);
    assert_eq!(after[0]["futureSettings"], rows[0]["futureSettings"]);
    assert!(after[0].get("modelId").is_none());
}

#[test]
fn import_creation_and_native_mutations_preserve_concurrent_records_and_fields() {
    let home = IsolatedAppHome::acquire("project-concurrent");
    let existing = project(&home, "existing");
    let mut targets = Vec::new();
    for index in 0..12 {
        let path = home.path.join(format!("new-{index}"));
        fs::create_dir(&path).unwrap();
        targets.push(path);
    }
    let _ = load_settings();
    std::thread::scope(|scope| {
        for (index, path) in targets.iter().enumerate() {
            scope.spawn(move || {
                ensure_import_project(&format!("request-{index}"), path.to_str().unwrap()).unwrap()
            });
        }
        scope.spawn(|| rename_project(&existing.id, "Concurrent name").unwrap());
        scope.spawn(|| set_project_pinned(&existing.id, true).unwrap());
        scope.spawn(|| set_project_color(&existing.id, Some("purple".into())).unwrap());
    });
    let projects = load_projects_snapshot().unwrap();
    assert_eq!(projects.len(), 13);
    let updated = projects.iter().find(|row| row.id == existing.id).unwrap();
    assert_eq!(updated.name, "Concurrent name");
    assert!(updated.pinned);
    assert_eq!(updated.color.as_deref(), Some("purple"));
}

#[test]
fn concurrent_retries_create_one_untrusted_record() {
    let home = IsolatedAppHome::acquire("project-idempotent-race");
    let path = home.path.join("target");
    fs::create_dir(&path).unwrap();
    let results = std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    ensure_import_project("same-request", path.to_str().unwrap()).unwrap()
                })
            })
            .collect();
        jobs.into_iter()
            .map(|job| job.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|row| row.created).count(), 1);
    assert!(results
        .iter()
        .all(|row| row.project.id == "same-request" && !row.project.trusted));
    assert_eq!(load_projects_snapshot().unwrap().len(), 1);
}

#[test]
fn corrupted_project_file_is_preserved_by_every_native_mutation_family() {
    let home = IsolatedAppHome::acquire("project-corrupt-preserved");
    let path = home.path.join("target");
    fs::create_dir(&path).unwrap();
    fs::write(projects_file(), b"{bad-json").unwrap();
    assert!(load_projects_checked().is_err());
    assert!(ensure_import_project("new", path.to_str().unwrap()).is_err());
    assert!(add_project(path.to_string_lossy().into_owned(), false).is_err());
    assert!(rename_project("old", "new").is_err());
    assert!(remove_project("old").is_err());
    assert!(set_project_pinned("old", true).is_err());
    assert!(reorder_projects(vec![]).is_err());
    assert!(trust_project("old").is_err());
    assert!(set_project_color("old", None).is_err());
    assert!(set_project_permission_policy("old", None).is_err());
    assert!(set_project_sandbox_profile("old", None).is_err());
    assert_eq!(fs::read(projects_file()).unwrap(), b"{bad-json");
}

#[test]
fn failed_transaction_keeps_bytes_and_request_id_cannot_change_targets() {
    let home = IsolatedAppHome::acquire("project-abort");
    let first = project(&home, "first");
    let second = project(&home, "second");
    let before = fs::read(projects_file()).unwrap();
    let result: Result<(), String> = update_projects(|list| {
        list.clear();
        Err("injected".into())
    });
    assert!(result.is_err());
    assert!(ensure_import_project(&first.id, &second.path).is_err());
    assert_eq!(fs::read(projects_file()).unwrap(), before);
}

#[test]
fn no_op_and_failed_replace_do_not_report_success_or_corrupt_old_file() {
    let home = IsolatedAppHome::acquire("project-replace-failure");
    let existing = project(&home, "existing");
    let before = fs::read(projects_file()).unwrap();
    update_projects(|_| Ok(())).unwrap();
    assert_eq!(fs::read(projects_file()).unwrap(), before);
    let tmp = projects_file().with_file_name("projects.json.tmp");
    fs::create_dir(&tmp).unwrap();
    assert!(rename_project(&existing.id, "cannot persist").is_err());
    assert_eq!(fs::read(projects_file()).unwrap(), before);
}

#[test]
fn duplicate_ids_and_oversized_stores_are_rejected_without_recovery() {
    let home = IsolatedAppHome::acquire("project-store-limits");
    let project = project(&home, "duplicated");
    write_json(&projects_file(), &vec![project.clone(), project]).unwrap();
    assert!(load_projects_checked().unwrap_err().contains("duplicate"));
    assert_eq!(
        serde_json::from_slice::<Vec<Project>>(&fs::read(projects_file()).unwrap())
            .unwrap()
            .len(),
        2
    );
    fs::File::create(projects_file())
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    assert!(load_projects_checked().is_err());
    assert_eq!(
        fs::metadata(projects_file()).unwrap().len(),
        16 * 1024 * 1024 + 1
    );
}

#[test]
fn undo_checks_the_current_row_under_lock_and_preserves_later_edits() {
    let home = IsolatedAppHome::acquire("project-undo-conflict");
    let before = project(&home, "original");
    rename_project(&before.id, "User edit").unwrap();
    assert!(remove_import_project(&before)
        .unwrap_err()
        .contains("changed"));
    assert_eq!(load_projects_snapshot().unwrap()[0].name, "User edit");
}

#[test]
fn undo_preserves_projects_with_unknown_customizations_or_invalid_session_index() {
    let home = IsolatedAppHome::acquire("project-undo-invalid-index");
    let project = project(&home, "original");
    let before = fs::read(projects_file()).unwrap();
    for invalid in ["[42]", "[{}]", "{bad-json", "[{\"projectId\":42}]"] {
        fs::write(sessions_index_file(), invalid).unwrap();
        assert!(remove_import_project(&project).is_err());
        assert_eq!(fs::read(projects_file()).unwrap(), before);
        assert_eq!(fs::read_to_string(sessions_index_file()).unwrap(), invalid);
    }
    fs::write(sessions_index_file(), "[]").unwrap();
    let mut rows: serde_json::Value = serde_json::from_slice(&before).unwrap();
    rows[0]["futureSettings"] = serde_json::json!({"keep": true});
    write_json(&projects_file(), &rows).unwrap();
    let customized = fs::read(projects_file()).unwrap();
    assert!(remove_import_project(&project)
        .unwrap_err()
        .contains("additional settings"));
    assert_eq!(fs::read(projects_file()).unwrap(), customized);
}
