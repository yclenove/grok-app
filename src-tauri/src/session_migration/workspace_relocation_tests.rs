use super::*;
use crate::session_migration::{sources, test_home::IsolatedAppHome, types::Provider};
use std::path::Path;

#[test]
fn membership_and_receipts_survive_relocation_and_catalog_rebuild() {
    use crate::session_migration::workspace_association as association;
    let home = IsolatedAppHome::acquire("workspace-relocation-membership");
    let provider = home.path.join("provider");
    std::fs::create_dir(&provider).unwrap();
    let conn = catalog::open_catalog().unwrap();
    let registration =
        sources::register(&conn, Provider::Codex, &provider, None, "synthetic").unwrap();
    let old = home.path.join("old.code-workspace");
    let new = home.path.join("new.code-workspace");
    let saved = initial(
        &old,
        r#"{"folders":[]}"#,
        Some(registration.source_id.clone()),
    );
    let id = format!("imp:{}", uuid::Uuid::new_v4());
    catalog::upsert_entry(
        &conn,
        &catalog::CatalogEntry {
            import_id: id.clone(),
            installation_id: registration.source_id,
            provider: "codex".into(),
            native_session_id: uuid::Uuid::new_v4().to_string(),
            title: "Synthetic".into(),
            alias: None,
            hidden: false,
            current_revision: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        },
    )
    .unwrap();
    association::update(association::UpdateRequest {
        import_id: id.clone(),
        expected_version: 0,
        request_id: uuid::Uuid::new_v4().to_string(),
        choice: Some(association::WorkspaceChoice {
            source_workspace_id: saved.workspace.source_workspace_id.clone(),
            revision: saved.revision.clone(),
            display_name: saved.workspace.display_name.clone(),
            source_locator: saved
                .workspace
                .source_identity
                .as_ref()
                .map(|s| s.locator.clone()),
        }),
    })
    .unwrap();
    std::fs::rename(&old, &new).unwrap();
    let request = confirm(preview(input(&saved, &new)).unwrap());
    let done = save(request.clone()).unwrap();
    let linked = association::get(&id).unwrap();
    assert_eq!(
        linked.choice.unwrap().source_workspace_id,
        saved.workspace.source_workspace_id
    );
    assert_eq!(linked.status, association::AssociationStatus::Stale);
    assert_eq!(linked.version, 1);
    catalog::run_catalog_rebuild().unwrap();
    std::fs::remove_file(&new).unwrap();
    assert_eq!(save(request).unwrap().revision, done.revision);
    assert_eq!(
        association::get(&id)
            .unwrap()
            .choice
            .unwrap()
            .source_workspace_id,
        saved.workspace.source_workspace_id
    );
}

#[test]
fn source_unavailability_and_unknown_receipt_schema_never_mutate_saved_metadata() {
    let home = IsolatedAppHome::acquire("workspace-relocation-schema");
    let old = home.path.join("old.code-workspace");
    let new = home.path.join("new.code-workspace");
    let saved = initial(&old, r#"{"folders":[]}"#, None);
    assert_eq!(
        preview(input(&saved, &new)).unwrap_err().code,
        ErrorCode::SourceNotFound
    );
    std::fs::write(&new, "invalid workspace").unwrap();
    assert_eq!(
        preview(input(&saved, &new)).unwrap_err().code,
        ErrorCode::FormatUnsupported
    );
    std::fs::copy(&old, &new).unwrap();
    let request = confirm(preview(input(&saved, &new)).unwrap());
    let conn = catalog::open_catalog().unwrap();
    conn.execute(
        "INSERT INTO meta(key,value) VALUES('workspace_relocations_version','2')",
        [],
    )
    .unwrap();
    assert!(save(request).is_err());
    assert_eq!(
        workspace_edit::get(&saved.workspace.source_workspace_id)
            .unwrap()
            .revision,
        saved.revision
    );
    assert!(!conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='workspace_relocations')",
            [],
            |row| row.get::<_, bool>(0)
        )
        .unwrap());
}

fn source(path: &Path, source_id: Option<String>) -> WorkspaceSource {
    WorkspaceSource {
        path: path.to_string_lossy().into(),
        kind: WorkspaceSourceKind::File,
        source_id,
    }
}

fn initial(
    path: &Path,
    content: &str,
    source_id: Option<String>,
) -> workspace_edit::SavedWorkspace {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
    let p = workspace_import::preview(source(path, source_id)).unwrap();
    let choices = p
        .workspace
        .roots
        .iter()
        .enumerate()
        .map(|(i, r)| WorkspaceRootChoice {
            root_id: r.root_id.clone(),
            target_project_id: None,
            explicit_target: None,
            primary: i == 0,
        })
        .collect();
    let w = workspace_import::save(workspace_import::WorkspaceSaveRequest {
        source: p.source,
        expected_fingerprint: p.fingerprint,
        expected_saved_revision: p.saved_revision,
        choices,
    })
    .unwrap();
    workspace_edit::get(&w.source_workspace_id).unwrap()
}

fn input(saved: &workspace_edit::SavedWorkspace, path: &Path) -> PreviewRequest {
    PreviewRequest {
        source_workspace_id: saved.workspace.source_workspace_id.clone(),
        expected_revision: saved.revision.clone(),
        path: path.to_string_lossy().into(),
    }
}

fn confirm(p: RelocationPreview) -> SaveRequest {
    SaveRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        preview: p.request,
        fingerprint: p.fingerprint,
        proposal_hash: p.proposal_hash,
    }
}

#[test]
fn offline_move_preserves_identity_and_exact_roots_then_reopens_without_duplicate() {
    let home = IsolatedAppHome::acquire("workspace-relocation-offline");
    let old = home.path.join("old.code-workspace");
    let new = home.path.join("new.code-workspace");
    let saved = initial(&old, r#"{"folders":[{"path":"missing"}]}"#, None);
    std::fs::rename(&old, &new).unwrap();
    let p = preview(input(&saved, &new)).unwrap();
    assert_eq!(
        p.retained_root_ids,
        vec![saved.workspace.roots[0].root_id.clone()]
    );
    assert!(p.added_root_ids.is_empty());
    assert_eq!(
        workspace_edit::get(&saved.workspace.source_workspace_id)
            .unwrap()
            .revision,
        saved.revision
    );
    let result = save(confirm(p)).unwrap();
    assert_eq!(
        result.workspace.source_workspace_id,
        saved.workspace.source_workspace_id
    );
    assert_eq!(
        result.workspace.mapping_choices,
        saved.workspace.mapping_choices
    );
    let reopened = workspace_import::preview(source(&new, None)).unwrap();
    assert_eq!(
        reopened.workspace.source_workspace_id,
        saved.workspace.source_workspace_id
    );
    assert_eq!(
        reopened.saved_revision.as_deref(),
        Some(result.revision.as_str())
    );
    assert_eq!(workspace_import::list(None, 50).unwrap().items.len(), 1);
    assert!(!crate::paths::workspaces_file().exists());
    assert!(!crate::paths::sessions_index_file().exists());
    assert!(!crate::paths::projects_file().exists());
    // The former path cannot silently move the saved record back.
    std::fs::copy(&new, &old).unwrap();
    assert!(workspace_import::preview(source(&old, None)).is_err());
    let returned = save(confirm(preview(input(&result, &old)).unwrap())).unwrap();
    assert_eq!(
        returned.workspace.source_workspace_id,
        saved.workspace.source_workspace_id
    );
}

#[test]
fn moved_relative_roots_and_changed_remote_hosts_drop_only_affected_mappings() {
    let home = IsolatedAppHome::acquire("workspace-relocation-roots");
    let old = home.path.join("old/w.code-workspace");
    let new = home.path.join("new/w.code-workspace");
    let fixed = home.path.join("fixed");
    std::fs::create_dir(&fixed).unwrap();
    let content = serde_json::json!({"folders":[{"path":fixed},{"path":"relative"},{"uri":"vscode-remote://ssh-remote+one/home/project"}]}).to_string();
    let saved = initial(&old, &content, None);
    let mut edit = workspace_edit::WorkspaceUpdateRequest {
        source_workspace_id: saved.workspace.source_workspace_id.clone(),
        expected_revision: saved.revision,
        choices: saved.workspace.mapping_choices,
        source_id: None,
    };
    for choice in &mut edit.choices {
        choice.explicit_target = Some(fixed.to_string_lossy().into());
    }
    let saved = workspace_edit::update(edit).unwrap();
    std::fs::create_dir(new.parent().unwrap()).unwrap();
    std::fs::write(&new, content.replace("ssh-remote+one", "ssh-remote+two")).unwrap();
    let p = preview(input(&saved, &new)).unwrap();
    assert_eq!(p.retained_root_ids.len(), 1);
    assert_eq!(p.added_root_ids.len(), 2);
    assert_eq!(p.removed_roots.len(), 2);
    assert!(p.after.mapping_choices[0].explicit_target.is_some());
    assert!(p.after.mapping_choices[1..]
        .iter()
        .all(|c| c.explicit_target.is_none() && c.target_project_id.is_none()));
    let bytes = std::fs::read(&new).unwrap();
    let result = save(confirm(p)).unwrap();
    assert_eq!(result.workspace.roots.len(), 3);
    assert_eq!(std::fs::read(&new).unwrap(), bytes);
}

#[test]
fn installation_is_preserved_offline_and_other_installations_remain_separate() {
    let home = IsolatedAppHome::acquire("workspace-relocation-source");
    let source_root = home.path.join("provider");
    std::fs::create_dir(&source_root).unwrap();
    let conn = catalog::open_catalog().unwrap();
    let a = sources::register(&conn, Provider::Codex, &source_root, None, "a").unwrap();
    let other_root = home.path.join("other-provider");
    std::fs::create_dir(&other_root).unwrap();
    let b = sources::register(&conn, Provider::Codex, &other_root, None, "b").unwrap();
    let old = home.path.join("old.code-workspace");
    let new = home.path.join("new.code-workspace");
    let saved = initial(&old, r#"{"folders":[]}"#, Some(a.source_id.clone()));
    let other = initial(&new, r#"{"folders":[]}"#, Some(b.source_id));
    std::fs::remove_file(&old).unwrap();
    std::fs::remove_dir(&source_root).unwrap();
    let result = save(confirm(preview(input(&saved, &new)).unwrap())).unwrap();
    assert_eq!(
        result.workspace.source_identity.as_ref().unwrap().source_id,
        a.source_id
    );
    assert_eq!(
        workspace_edit::get(&other.workspace.source_workspace_id)
            .unwrap()
            .revision,
        other.revision
    );
    let reopened = workspace_import::preview(source(&new, Some(a.source_id))).unwrap();
    assert_eq!(
        reopened.workspace.source_workspace_id,
        saved.workspace.source_workspace_id
    );
    assert!(result.workspace.roots.is_empty());
    assert!(result.workspace.default_root_id.is_none());
}

#[test]
fn changed_source_or_mapping_refuses_without_publishing_a_receipt() {
    let home = IsolatedAppHome::acquire("workspace-relocation-stale");
    let old = home.path.join("old.code-workspace");
    let new = home.path.join("new.code-workspace");
    let saved = initial(&old, r#"{"folders":[{"path":"one"},{"path":"two"}]}"#, None);
    std::fs::copy(&old, &new).unwrap();
    let request = confirm(preview(input(&saved, &new)).unwrap());
    std::fs::write(&new, r#"{"folders":[]}"#).unwrap();
    assert_eq!(
        save(request.clone()).unwrap_err().code,
        ErrorCode::SourceChanged
    );
    std::fs::copy(&old, &new).unwrap();
    let mut choices = saved.workspace.mapping_choices.clone();
    choices[0].primary = false;
    choices[1].primary = true;
    let changed = workspace_edit::update(workspace_edit::WorkspaceUpdateRequest {
        source_workspace_id: saved.workspace.source_workspace_id.clone(),
        expected_revision: saved.revision,
        choices,
        source_id: None,
    })
    .unwrap();
    assert_eq!(
        save(request.clone()).unwrap_err().code,
        ErrorCode::PlanStale
    );
    assert_eq!(
        workspace_edit::get(&saved.workspace.source_workspace_id)
            .unwrap()
            .revision,
        changed.revision
    );
    assert!(
        receipt(&catalog::open_catalog_readonly().unwrap(), &request)
            .unwrap()
            .is_none()
    );
}

#[test]
fn destination_claimed_after_preview_is_not_overwritten() {
    let home = IsolatedAppHome::acquire("workspace-relocation-collision");
    let old = home.path.join("old.code-workspace");
    let new = home.path.join("new.code-workspace");
    let saved = initial(&old, r#"{"folders":[]}"#, None);
    std::fs::copy(&old, &new).unwrap();
    let request = confirm(preview(input(&saved, &new)).unwrap());
    let other = initial(&new, r#"{"folders":[]}"#, None);
    assert_eq!(save(request).unwrap_err().code, ErrorCode::MappingConflict);
    assert_eq!(
        workspace_edit::get(&saved.workspace.source_workspace_id)
            .unwrap()
            .revision,
        saved.revision
    );
    assert_eq!(
        workspace_edit::get(&other.workspace.source_workspace_id)
            .unwrap()
            .revision,
        other.revision
    );
}

#[test]
fn lost_reply_retries_offline_return_current_metadata_without_reapplying_or_resurrecting() {
    let home = IsolatedAppHome::acquire("workspace-relocation-retry");
    let old = home.path.join("old.code-workspace");
    let new = home.path.join("new.code-workspace");
    let saved = initial(&old, r#"{"folders":[{"path":"one"},{"path":"two"}]}"#, None);
    std::fs::rename(&old, &new).unwrap();
    let request = confirm(preview(input(&saved, &new)).unwrap());
    let done = save(request.clone()).unwrap();
    std::fs::remove_file(&new).unwrap();
    assert_eq!(save(request.clone()).unwrap().revision, done.revision);
    let mut choices = done.workspace.mapping_choices.clone();
    choices[0].primary = false;
    choices[1].primary = true;
    let newer = workspace_edit::update(workspace_edit::WorkspaceUpdateRequest {
        source_workspace_id: done.workspace.source_workspace_id.clone(),
        expected_revision: done.revision,
        choices,
        source_id: None,
    })
    .unwrap();
    assert_eq!(save(request.clone()).unwrap().revision, newer.revision);
    let mut altered = request.clone();
    altered.preview.path.push('x');
    assert_eq!(save(altered).unwrap_err().code, ErrorCode::PlanStale);
    let conn = catalog::open_catalog().unwrap();
    conn.execute(
        "DELETE FROM workspaces WHERE source_workspace_id=?1",
        [&done.workspace.source_workspace_id],
    )
    .unwrap();
    assert!(save(request).is_err());
    assert!(workspace_import::list(None, 20).unwrap().items.is_empty());
}

#[test]
fn concurrent_same_request_publishes_once_and_distinct_stale_request_fails() {
    let home = IsolatedAppHome::acquire("workspace-relocation-concurrent");
    let old = home.path.join("old.code-workspace");
    let new = home.path.join("new.code-workspace");
    let saved = initial(&old, r#"{"folders":[]}"#, None);
    std::fs::rename(&old, &new).unwrap();
    let request = confirm(preview(input(&saved, &new)).unwrap());
    let results = std::thread::scope(|s| {
        let handles: Vec<_> = (0..4).map(|_| s.spawn(|| save(request.clone()))).collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap().unwrap().revision)
            .collect::<Vec<_>>()
    });
    assert!(results.iter().all(|r| *r == results[0]));
    let count: i64 = catalog::open_catalog()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM workspace_relocations", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    let mut stale = request;
    stale.request_id = "different".into();
    assert_eq!(save(stale).unwrap_err().code, ErrorCode::PlanStale);
}

#[test]
fn receipt_failure_rolls_back_workspace_and_corruption_is_not_success() {
    let home = IsolatedAppHome::acquire("workspace-relocation-rollback");
    let old = home.path.join("old.code-workspace");
    let new = home.path.join("new.code-workspace");
    let saved = initial(&old, r#"{"folders":[]}"#, None);
    std::fs::copy(&old, &new).unwrap();
    let request = confirm(preview(input(&saved, &new)).unwrap());
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch("CREATE TABLE workspace_relocations(request_id TEXT PRIMARY KEY,json TEXT NOT NULL,digest TEXT NOT NULL); INSERT INTO meta(key,value) VALUES('workspace_relocations_version','1'); CREATE TRIGGER reject_receipt BEFORE INSERT ON workspace_relocations BEGIN SELECT RAISE(ABORT, 'synthetic write failure'); END;").unwrap();
    assert!(save(request.clone()).is_err());
    assert_eq!(
        workspace_edit::get(&saved.workspace.source_workspace_id)
            .unwrap()
            .revision,
        saved.revision
    );
    conn.execute_batch("DROP TRIGGER reject_receipt").unwrap();
    let done = save(request.clone()).unwrap();
    conn.execute("UPDATE workspace_relocations SET digest='broken'", [])
        .unwrap();
    assert!(save(request).is_err());
    assert_eq!(
        workspace_edit::get(&done.workspace.source_workspace_id)
            .unwrap()
            .revision,
        done.revision
    );
}

#[test]
fn folder_relocation_is_explicit_and_keeps_stable_identity() {
    let home = IsolatedAppHome::acquire("workspace-relocation-folder");
    let old = home.path.join("old");
    let new = home.path.join("new");
    std::fs::create_dir(&old).unwrap();
    let p = workspace_import::preview(WorkspaceSource {
        path: old.to_string_lossy().into(),
        kind: WorkspaceSourceKind::Folder,
        source_id: None,
    })
    .unwrap();
    let saved = workspace_import::save(workspace_import::WorkspaceSaveRequest {
        source: p.source,
        expected_fingerprint: p.fingerprint,
        expected_saved_revision: None,
        choices: vec![WorkspaceRootChoice {
            root_id: p.workspace.roots[0].root_id.clone(),
            target_project_id: None,
            explicit_target: None,
            primary: true,
        }],
    })
    .unwrap();
    let saved = workspace_edit::get(&saved.source_workspace_id).unwrap();
    std::fs::rename(&old, &new).unwrap();
    let p = preview(input(&saved, &new)).unwrap();
    assert_eq!(p.removed_roots.len(), 1);
    assert_eq!(p.added_root_ids.len(), 1);
    let done = save(confirm(p)).unwrap();
    assert_eq!(
        done.workspace.source_workspace_id,
        saved.workspace.source_workspace_id
    );
    assert_eq!(
        workspace_import::preview(WorkspaceSource {
            path: new.to_string_lossy().into(),
            kind: WorkspaceSourceKind::Folder,
            source_id: None
        })
        .unwrap()
        .workspace
        .source_workspace_id,
        saved.workspace.source_workspace_id
    );
}
