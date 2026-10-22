use super::*;
use crate::session_migration::test_home::IsolatedAppHome;

fn project(home: &IsolatedAppHome) -> store::Project {
    let path = home.path.join("project");
    std::fs::create_dir_all(&path).unwrap();
    store::add_project(path.to_str().unwrap().into(), false).unwrap()
}

#[test]
fn corrupt_future_and_incomplete_files_are_not_quarantined_or_overwritten() {
    let home = IsolatedAppHome::acquire("workspace-strict-store");
    let project = project(&home);
    for bytes in [
        "broken",
        "{}",
        r#"{"version":99,"workspaces":[]}"#,
        r#"{"version":0,"workspaces":[]}"#,
    ] {
        std::fs::write(workspaces_file(), bytes).unwrap();
        assert!(list_workspaces().is_err());
        assert!(upsert_workspace(None, "copy".into(), project.id.clone(), vec![]).is_err());
        assert!(delete_workspace("missing").is_err());
        assert!(refresh_all_capabilities().is_err());
        assert_eq!(std::fs::read_to_string(workspaces_file()).unwrap(), bytes);
    }
}

#[test]
fn independent_workspace_writers_keep_all_records_and_deletes() {
    let home = IsolatedAppHome::acquire("workspace-concurrent-store");
    let project = project(&home);
    let _ = store::load_settings();
    let records = std::thread::scope(|scope| {
        let tasks: Vec<_> = (0..12)
            .map(|i| {
                let project_id = project.id.clone();
                scope.spawn(move || {
                    upsert_workspace(None, format!("workspace {i}"), project_id, vec![]).unwrap()
                })
            })
            .collect();
        tasks
            .into_iter()
            .map(|task| task.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(list_workspaces().unwrap().len(), 12);
    std::thread::scope(|scope| {
        for record in records.iter().take(6) {
            scope.spawn(|| delete_workspace(&record.id).unwrap());
        }
        scope.spawn(|| refresh_all_capabilities().unwrap());
    });
    let remaining = list_workspaces().unwrap();
    assert_eq!(remaining.len(), 6);
    assert!(records[6..]
        .iter()
        .all(|record| remaining.iter().any(|row| row.id == record.id)));
}

#[test]
fn failed_transaction_keeps_existing_bytes() {
    let home = IsolatedAppHome::acquire("workspace-store-abort");
    let project = project(&home);
    upsert_workspace(None, "existing".into(), project.id, vec![]).unwrap();
    let before = std::fs::read(workspaces_file()).unwrap();
    let result: Result<(), String> = persistence::update(&workspaces_file(), |file| {
        file.workspaces.clear();
        Err("fixture failure".into())
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read(workspaces_file()).unwrap(), before);
}

#[test]
fn bound_workspace_errors_do_not_fall_back_to_global_spawn_permissions() {
    let _home = IsolatedAppHome::acquire("workspace-spawn-fail-closed");
    let mut session = store::create_session(None, None, false).unwrap();
    session.workspace_id = Some("missing".into());
    assert!(resolve_session_spawn("off", None, "shared", &session).is_err());
    std::fs::write(workspaces_file(), b"corrupt").unwrap();
    assert!(resolve_session_spawn("off", None, "shared", &session).is_err());
    assert_eq!(std::fs::read(workspaces_file()).unwrap(), b"corrupt");
    // Unbound sessions do not depend on workspace storage.
    session.workspace_id = None;
    assert_eq!(
        resolve_session_spawn("strict", None, "shared", &session)
            .unwrap()
            .sandbox,
        "strict"
    );
}

#[test]
fn primary_cannot_be_replaced_with_unrelated_directory() {
    let home = IsolatedAppHome::acquire("workspace-primary-consistency");
    let project = project(&home);
    let result = upsert_workspace(
        None,
        "wrong".into(),
        project.id,
        vec![WorkspaceRoot {
            path: home.path.to_string_lossy().into_owned(),
            role: WorkspaceRootRole::Primary,
            access: WorkspaceRootAccess::Read,
            path_ok: None,
        }],
    );
    assert!(result.unwrap_err().contains("primary root must match"));
    assert!(!workspaces_file().exists());
}

#[test]
fn native_relative_and_foreign_paths_are_rejected_before_filesystem_lookup() {
    for path in ["relative", "C:relative", " ", "bad\0path"] {
        assert!(canonicalize_workspace_root(path).is_err());
    }
    #[cfg(windows)]
    assert!(canonicalize_workspace_root("/foreign/posix/path").is_err());
    #[cfg(not(windows))]
    assert!(canonicalize_workspace_root(r"C:\foreign\windows").is_err());
}

#[cfg(not(windows))]
#[test]
fn posix_spaces_backslashes_and_case_remain_significant() {
    let home = IsolatedAppHome::acquire("workspace-posix-spelling");
    // Default APFS is case-insensitive: "case" then names the "Case" folder.
    let case_insensitive = {
        std::fs::create_dir(home.path.join("CaseProbe")).unwrap();
        home.path.join("caseprobe").exists()
    };
    for name in ["Case", "case", "with space ", r"literal\backslash"] {
        let path = home.path.join(name);
        if !(case_insensitive && name == "case") {
            std::fs::create_dir(&path).unwrap();
        }
        let (canonical, exists) = canonicalize_workspace_root(path.to_str().unwrap()).unwrap();
        assert!(exists);
        // A case-insensitive volume canonicalizes to the on-disk spelling.
        let expected = if case_insensitive && name == "case" {
            "Case"
        } else {
            name
        };
        assert!(canonical.ends_with(expected), "{canonical}");
    }
    assert!(!paths_equal("/Case", "/case"));
    assert!(!paths_equal("/space", "/space "));
}
