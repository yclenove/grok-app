use super::*;
use crate::session_migration::test_home::IsolatedAppHome;

fn source(home: &IsolatedAppHome, text: &str) -> WorkspaceSource {
    let path = home.path.join("test.code-workspace");
    std::fs::write(&path, text).unwrap();
    WorkspaceSource {
        path: path.to_string_lossy().into_owned(),
        kind: WorkspaceSourceKind::File,
        source_id: None,
    }
}

fn request(preview: &WorkspacePreview) -> WorkspaceSaveRequest {
    WorkspaceSaveRequest {
        source: preview.source.clone(),
        expected_fingerprint: preview.fingerprint.clone(),
        expected_saved_revision: preview.saved_revision.clone(),
        choices: preview
            .workspace
            .roots
            .iter()
            .enumerate()
            .map(|(i, root)| WorkspaceRootChoice {
                root_id: root.root_id.clone(),
                target_project_id: None,
                explicit_target: None,
                primary: i == 0,
            })
            .collect(),
    }
}

#[test]
fn preview_and_list_do_not_create_catalog_or_scan_project_files() {
    let home = IsolatedAppHome::acquire("workspace-preview");
    let input = source(&home, r#"{"folders":[{"path":".","name":"项目"}]}"#);
    std::fs::write(
        home.path.join("conversation.json"),
        r#"{"messages":[{"role":"user","content":"not selected"}]}"#,
    )
    .unwrap();
    let before = std::fs::read(&input.path).unwrap();
    let parsed = preview(input.clone()).unwrap();
    assert_eq!(
        parsed.workspace.roots[0].display_name.as_deref(),
        Some("项目")
    );
    assert!(!catalog::catalog_path().exists());
    assert!(list(None, 20).unwrap().items.is_empty());
    assert!(!catalog::catalog_path().exists());
    save(request(&parsed)).unwrap();
    let conn = catalog::open_catalog_readonly().unwrap();
    assert!(catalog::list_entries(&conn).unwrap().is_empty());
    assert_eq!(std::fs::read(&input.path).unwrap(), before);
}

#[test]
fn empty_workspace_roundtrips_without_parent_or_session() {
    let home = IsolatedAppHome::acquire("workspace-empty");
    let parsed = preview(source(&home, r#"{"folders":[]}"#)).unwrap();
    assert!(parsed.workspace.roots.is_empty());
    assert!(parsed.workspace.default_root_id.is_none());
    save(request(&parsed)).unwrap();
    let page = list(None, 20).unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].roots.is_empty());
    assert!(page.items[0].default_root_id.is_none());
    std::fs::remove_file(&parsed.source.path).unwrap();
    assert_eq!(list(None, 20).unwrap().items[0], page.items[0]);
}

#[test]
fn malformed_inputs_are_rejected_instead_of_empty_workspace() {
    let home = IsolatedAppHome::acquire("workspace-format");
    for text in [
        "null",
        "[]",
        "42",
        "{} /*oops",
        r#"{"folders":[,]}"#,
        r#"{"folders":{},}"#,
        r#"{"folders":[{"uri":"file:///tmp","path":"ambiguous"}]}"#,
    ] {
        assert_eq!(
            preview(source(&home, text)).unwrap_err().code,
            ErrorCode::FormatUnsupported,
            "{text}"
        );
    }
    assert!(!catalog::catalog_path().exists());
}

#[test]
fn jsonc_comments_and_trailing_commas_preserve_string_contents() {
    let home = IsolatedAppHome::acquire("workspace-jsonc");
    let parsed = preview(source(&home, "\u{feff}{ /*注释*/ \"folders\": [ { \"path\": \"missing//folder\", \"name\": \"[,] /*name*/\", }, ], // comment\n }" )).unwrap();
    assert_eq!(parsed.workspace.roots.len(), 1);
    assert_eq!(
        parsed.workspace.roots[0].display_name.as_deref(),
        Some("[,] /*name*/")
    );
    assert!(parsed.workspace.roots[0]
        .original_path
        .ends_with("missing//folder"));
}

#[test]
fn oversized_file_is_bounded() {
    let home = IsolatedAppHome::acquire("workspace-oversize");
    let input = source(&home, "{}");
    File::options()
        .write(true)
        .open(&input.path)
        .unwrap()
        .set_len(MAX_FILE_BYTES as u64 + 1)
        .unwrap();
    assert_eq!(preview(input).unwrap_err().code, ErrorCode::ResourceLimit);
    assert!(!catalog::catalog_path().exists());
}

#[test]
fn changed_source_and_invalid_choices_do_not_publish() {
    let home = IsolatedAppHome::acquire("workspace-stale");
    let parsed = preview(source(&home, r#"{"folders":[{"path":"missing"}]}"#)).unwrap();
    let mut invalid = request(&parsed);
    invalid.choices[0].root_id = "foreign-root".into();
    assert_eq!(save(invalid).unwrap_err().code, ErrorCode::MappingConflict);
    let mut no_primary = request(&parsed);
    no_primary.choices[0].primary = false;
    assert_eq!(
        save(no_primary).unwrap_err().code,
        ErrorCode::MappingConflict
    );
    std::fs::write(&parsed.source.path, "{}").unwrap();
    assert_eq!(
        save(request(&parsed)).unwrap_err().code,
        ErrorCode::SourceChanged
    );
    assert!(!catalog::catalog_path().exists());
}

#[test]
fn save_preserves_existing_projects_and_is_idempotent_with_conflict_detection() {
    let home = IsolatedAppHome::acquire("workspace-mapping");
    let projects = br#"[{"id":"existing","name":"Do not rename","path":"/offline","pathOk":false,"lastOpenedAt":"2026-09-20T00:00:00Z","trusted":false,"pinned":true,"spaces":["one"]}]"#;
    std::fs::write(crate::paths::projects_file(), projects).unwrap();
    let parsed = preview(source(&home, r#"{"folders":[{"path":"missing"}]}"#)).unwrap();
    let mut first = request(&parsed);
    first.choices[0].target_project_id = Some("existing".into());
    let stored = save(first).unwrap();
    let mut retry = request(&parsed);
    retry.choices = stored.mapping_choices.clone();
    assert_eq!(save(retry).unwrap(), stored);
    assert_eq!(
        save(request(&parsed)).unwrap_err().code,
        ErrorCode::PlanStale
    );
    assert_eq!(
        std::fs::read(crate::paths::projects_file()).unwrap(),
        projects
    );
    let conn = catalog::open_catalog_readonly().unwrap();
    assert!(catalog::list_mappings(&conn).unwrap().is_empty());
    assert_eq!(list(None, 50).unwrap().items, vec![stored]);
}

#[test]
fn workspace_choices_remain_scoped_and_folder_import_does_not_read_children() {
    let home = IsolatedAppHome::acquire("workspace-scoped");
    let folder = WorkspaceSource {
        source_id: None,
        path: home.path.to_string_lossy().into_owned(),
        kind: WorkspaceSourceKind::Folder,
    };
    let parsed = preview(folder.clone()).unwrap();
    save(request(&parsed)).unwrap();
    std::fs::write(home.path.join("broken.json"), "not JSON").unwrap();
    assert_eq!(preview(folder).unwrap().fingerprint, parsed.fingerprint);
    let other = preview(source(
        &home,
        &serde_json::json!({"folders":[{"path":home.path}]}).to_string(),
    ))
    .unwrap();
    save(request(&other)).unwrap();
    let first = list(None, 1).unwrap();
    assert_eq!(first.items.len(), 1);
    let second = list(first.next_cursor.as_deref(), 1).unwrap();
    assert_eq!(second.items.len(), 1);
    assert_ne!(
        first.items[0].source_workspace_id,
        second.items[0].source_workspace_id
    );
    assert!(second.next_cursor.is_none());
}

#[test]
fn catalog_failure_rolls_back_workspace_save() {
    let home = IsolatedAppHome::acquire("workspace-atomic");
    let parsed = preview(source(&home, "{}")).unwrap();
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch("CREATE TRIGGER reject_workspace AFTER INSERT ON workspaces BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
    assert!(save(request(&parsed)).is_err());
    assert!(list(None, 20).unwrap().items.is_empty());
}

#[test]
fn command_preview_save_list_roundtrip_without_session_batch() {
    let home = IsolatedAppHome::acquire("workspace-command");
    let source = source(&home, r#"{"folders":[]}"#);
    tauri::async_runtime::block_on(async {
        let preview = crate::commands::session_import_workspace_preview(source)
            .await
            .unwrap();
        assert!(!catalog::catalog_path().exists());
        let saved = crate::commands::session_import_workspace_save(request(&preview))
            .await
            .unwrap();
        let list = crate::commands::session_import_workspaces(None, Some(1))
            .await
            .unwrap();
        assert_eq!(list.items, vec![saved]);
        assert!(
            catalog::list_entries(&catalog::open_catalog_readonly().unwrap())
                .unwrap()
                .is_empty()
        );
    });
}

#[test]
fn duplicate_roots_do_not_create_conflicting_mapping_controls() {
    let home = IsolatedAppHome::acquire("workspace-duplicate");
    let parsed = preview(source(
        &home,
        r#"{"folders":[{"path":"same","name":"first"},{"path":"same","name":"alias"}]}"#,
    ))
    .unwrap();
    assert_eq!(parsed.workspace.roots.len(), 1);
    assert_eq!(
        parsed.workspace.roots[0].display_name.as_deref(),
        Some("first")
    );
    save(request(&parsed)).unwrap();
}

#[test]
fn corrupt_catalog_is_not_treated_as_empty_or_overwritten() {
    let home = IsolatedAppHome::acquire("workspace-corrupt");
    let input = source(&home, "{}");
    let parsed = preview(input.clone()).unwrap();
    let conn = catalog::open_catalog().unwrap();
    conn.execute(
        "INSERT INTO workspaces(source_workspace_id,display_name,json) VALUES (?1,'broken','{')",
        [&parsed.workspace.source_workspace_id],
    )
    .unwrap();
    assert!(preview(input).is_err());
    assert!(list(None, 20).is_err());
    assert_eq!(
        save(request(&parsed)).unwrap_err().code,
        ErrorCode::PlanStale
    );
    assert_eq!(
        stored_json(&conn, &parsed.workspace.source_workspace_id)
            .unwrap()
            .as_deref(),
        Some("{")
    );
}
