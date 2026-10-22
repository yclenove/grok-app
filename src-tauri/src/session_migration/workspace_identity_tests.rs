use super::*;
use crate::session_migration::{
    catalog,
    test_home::IsolatedAppHome,
    types::Provider,
    workspace::WorkspaceRootChoice,
    workspace_edit::{self, WorkspaceUpdateRequest},
    workspace_import::{self as import, WorkspaceSource},
};

fn registration(home: &IsolatedAppHome, name: &str) -> sources::SourceRegistration {
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
}

fn source(home: &IsolatedAppHome, source_id: Option<String>) -> WorkspaceSource {
    let path = home.path.join("same.code-workspace");
    std::fs::write(&path, r#"{"folders":[{"path":"missing-project"}]}"#).unwrap();
    WorkspaceSource {
        path: path.to_string_lossy().into_owned(),
        kind: import::WorkspaceSourceKind::File,
        source_id,
    }
}

fn save(preview: import::WorkspacePreview) -> SourceWorkspace {
    let choices = preview
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
        .collect();
    import::save(import::WorkspaceSaveRequest {
        source: preview.source,
        expected_fingerprint: preview.fingerprint,
        expected_saved_revision: preview.saved_revision,
        choices,
    })
    .unwrap()
}

fn adoption(saved: &workspace_edit::SavedWorkspace, source_id: &str) -> WorkspaceUpdateRequest {
    WorkspaceUpdateRequest {
        source_workspace_id: saved.workspace.source_workspace_id.clone(),
        expected_revision: saved.revision.clone(),
        choices: saved.workspace.mapping_choices.clone(),
        source_id: Some(source_id.into()),
    }
}

#[test]
fn same_locator_on_distinct_installations_is_separate_and_retry_reuses_each() {
    let home = IsolatedAppHome::acquire("workspace-source-scope");
    let first = registration(&home, "machine-a");
    let second = registration(&home, "machine-b");
    let a = save(import::preview(source(&home, Some(first.source_id.clone()))).unwrap());
    let b = save(import::preview(source(&home, Some(second.source_id.clone()))).unwrap());
    assert_ne!(a.source_workspace_id, b.source_workspace_id);
    assert_ne!(a.source_fingerprint, b.source_fingerprint);
    assert_eq!(a.roots, b.roots); // Root IDs are scoped by their parent workspace.
    assert_eq!(
        save(import::preview(source(&home, Some(first.source_id))).unwrap()),
        a
    );
    assert_eq!(
        save(import::preview(source(&home, Some(second.source_id))).unwrap()),
        b
    );
    assert_eq!(import::list(None, 20).unwrap().items.len(), 2);
    assert!(
        catalog::list_entries(&catalog::open_catalog_readonly().unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(!crate::paths::workspaces_file().exists());
}

#[test]
fn offline_legacy_adoption_preserves_ids_and_repeated_confirmation_is_idempotent() {
    let home = IsolatedAppHome::acquire("workspace-source-adopt");
    let registration = registration(&home, "provider");
    let input = source(&home, None);
    let saved = save(import::preview(input.clone()).unwrap());
    let old = workspace_edit::get(&saved.source_workspace_id).unwrap();
    let before = std::fs::read(&input.path).unwrap();
    std::fs::remove_file(&input.path).unwrap();
    std::fs::remove_dir(&registration.root_path).unwrap();
    let adopted = workspace_edit::update(adoption(&old, &registration.source_id)).unwrap();
    assert_eq!(
        adopted.workspace.source_workspace_id,
        saved.source_workspace_id
    );
    assert_eq!(adopted.workspace.roots, saved.roots);
    assert_eq!(adopted.workspace.mapping_choices, saved.mapping_choices);
    assert_eq!(
        adopted.workspace.source_fingerprint,
        saved.source_fingerprint
    );
    assert_eq!(
        workspace_edit::update(adoption(&old, &registration.source_id))
            .unwrap()
            .revision,
        adopted.revision
    );
    std::fs::write(&input.path, before).unwrap();
    let preview = import::preview(input).unwrap();
    assert_eq!(
        preview.source.source_id.as_deref(),
        Some(registration.source_id.as_str())
    );
    assert_eq!(save(preview).source_workspace_id, saved.source_workspace_id);
    assert_eq!(import::list(None, 20).unwrap().items.len(), 1);
}

#[test]
fn source_relocation_keeps_workspace_identity_without_reading_provider_contents() {
    let home = IsolatedAppHome::acquire("workspace-source-moved");
    let original = registration(&home, "old-provider");
    let input = source(&home, Some(original.source_id.clone()));
    let saved = save(import::preview(input.clone()).unwrap());
    let moved = home.path.join("new-provider");
    std::fs::rename(&original.root_path, &moved).unwrap();
    std::fs::write(
        moved.join("auth.json"),
        "synthetic unreadable provider data",
    )
    .unwrap();
    let registration = sources::register(
        &catalog::open_catalog().unwrap(),
        Provider::Codex,
        &moved,
        Some(&original.source_id),
        "ignored",
    )
    .unwrap();
    assert_eq!(registration.source_id, original.source_id);
    assert_eq!(save(import::preview(input).unwrap()), saved);
}

#[test]
fn adoption_rejects_conflicts_and_preserves_saved_bytes() {
    let home = IsolatedAppHome::acquire("workspace-source-conflict");
    let first = registration(&home, "first");
    let second = registration(&home, "second");
    let legacy = save(import::preview(source(&home, None)).unwrap());
    save(import::preview(source(&home, Some(first.source_id.clone()))).unwrap());
    let old = workspace_edit::get(&legacy.source_workspace_id).unwrap();
    assert_eq!(
        workspace_edit::update(adoption(&old, &first.source_id))
            .unwrap_err()
            .code,
        ErrorCode::MappingConflict
    );
    assert_eq!(
        workspace_edit::get(&legacy.source_workspace_id)
            .unwrap()
            .revision,
        old.revision
    );
    let adopted = workspace_edit::update(adoption(&old, &second.source_id)).unwrap();
    assert_eq!(
        workspace_edit::update(adoption(&adopted, &first.source_id))
            .unwrap_err()
            .code,
        ErrorCode::MappingConflict
    );
    assert_eq!(
        workspace_edit::get(&legacy.source_workspace_id)
            .unwrap()
            .revision,
        adopted.revision
    );
    let mut stripped = adopted.workspace;
    stripped.source_identity = None;
    assert!(catalog::upsert_workspace(&catalog::open_catalog().unwrap(), &stripped).is_err());
}

#[test]
fn stale_upgrade_cannot_overwrite_mapping_edits_or_accept_a_forged_provider() {
    let home = IsolatedAppHome::acquire("workspace-source-stale");
    let registered = registration(&home, "provider");
    let legacy = save(import::preview(source(&home, None)).unwrap());
    let old = workspace_edit::get(&legacy.source_workspace_id).unwrap();
    let mut mapping = adoption(&old, &registered.source_id);
    mapping.source_id = None;
    mapping.choices[0].explicit_target = Some(home.path.to_string_lossy().into_owned());
    let edited = workspace_edit::update(mapping).unwrap();
    assert_eq!(
        workspace_edit::update(adoption(&old, &registered.source_id))
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
    let mut forged = workspace_edit::update(adoption(&edited, &registered.source_id))
        .unwrap()
        .workspace;
    forged.source_identity.as_mut().unwrap().provider = "cursor".into();
    assert!(catalog::upsert_workspace(&catalog::open_catalog().unwrap(), &forged).is_err());
}

#[test]
fn missing_registration_blocks_preview_and_disappearing_registration_blocks_save() {
    let home = IsolatedAppHome::acquire("workspace-source-missing");
    let input = source(&home, Some(uuid::Uuid::new_v4().to_string()));
    assert!(import::preview(input).is_err());
    assert!(!catalog::catalog_path().exists());
    let registered = registration(&home, "provider");
    let preview = import::preview(source(&home, Some(registered.source_id.clone()))).unwrap();
    catalog::open_catalog()
        .unwrap()
        .execute(
            "DELETE FROM sources WHERE installation_id=?1",
            [&registered.source_id],
        )
        .unwrap();
    let choices = preview
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
        .collect();
    assert_eq!(
        import::save(import::WorkspaceSaveRequest {
            source: preview.source,
            expected_fingerprint: preview.fingerprint,
            expected_saved_revision: None,
            choices,
        })
        .unwrap_err()
        .code,
        ErrorCode::MappingConflict
    );
    assert!(import::list(None, 20).unwrap().items.is_empty());
}

#[test]
fn independent_connections_cannot_publish_duplicate_identity_claims_without_projects() {
    let home = IsolatedAppHome::acquire("workspace-source-race");
    let registered = registration(&home, "provider");
    let conn = catalog::open_catalog().unwrap();
    let legacy = import::preview(source(&home, None)).unwrap().workspace;
    let mut adopted = legacy.clone();
    adopt(&conn, &mut adopted, &registered.source_id).unwrap();
    let mut scoped = legacy;
    scope(&conn, &mut scoped, &registered.source_id).unwrap();
    let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
    let outcomes = std::thread::scope(|threads| {
        let handles: Vec<_> = [adopted, scoped]
            .into_iter()
            .map(|workspace| {
                let gate = gate.clone();
                threads.spawn(move || {
                    let conn = catalog::open_catalog().unwrap();
                    gate.wait();
                    catalog::upsert_workspace(&conn, &workspace)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(import::list(None, 20).unwrap().items.len(), 1);
}

#[test]
fn empty_workspace_adoption_is_atomic_with_mapping_save_and_has_no_session_side_effects() {
    let home = IsolatedAppHome::acquire("workspace-source-empty");
    let registered = registration(&home, "provider");
    let input = source(&home, None);
    std::fs::write(&input.path, r#"{"folders":[]}"#).unwrap();
    let saved = save(import::preview(input).unwrap());
    let previous = workspace_edit::get(&saved.source_workspace_id).unwrap();
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_identity_update BEFORE UPDATE ON workspaces
        BEGIN SELECT RAISE(ABORT, 'injected identity write failure'); END;",
    )
    .unwrap();
    assert!(workspace_edit::update(adoption(&previous, &registered.source_id)).is_err());
    assert_eq!(
        workspace_edit::get(&saved.source_workspace_id)
            .unwrap()
            .revision,
        previous.revision
    );
    conn.execute_batch("DROP TRIGGER reject_identity_update")
        .unwrap();
    let adopted = workspace_edit::update(adoption(&previous, &registered.source_id)).unwrap();
    assert!(adopted.workspace.roots.is_empty());
    assert!(adopted.workspace.mapping_choices.is_empty());
    assert!(catalog::list_entries(&conn).unwrap().is_empty());
    assert!(!crate::paths::workspaces_file().exists());
}
