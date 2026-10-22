//! Integration boundary between native workspaces and read-only imported history.

use super::*;
use crate::commands::bind_session_workspace;
use crate::session_migration::test_home::IsolatedAppHome;
use crate::workspace_store::{
    self, WorkspaceCapability, WorkspaceRecord, WorkspaceRoot, WorkspaceRootAccess,
    WorkspaceRootRole, WorkspaceStoreFile,
};

struct Fixture {
    home: IsolatedAppHome,
    _scope_lock: tokio::sync::MutexGuard<'static, ()>,
    previous_roots: Vec<PathBuf>,
    previous_grants: Vec<PathBuf>,
    extra: PathBuf,
    workspace: WorkspaceRecord,
}

impl Fixture {
    fn new() -> Self {
        let scope_lock = TEST_LOCK.blocking_lock();
        let home = IsolatedAppHome::acquire("workspace-import-boundary");
        let allowed = home.path.join("allowed");
        let extra = home.path.join("extra");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&extra).unwrap();
        let project =
            crate::store::ensure_import_project("fixture-project", allowed.to_str().unwrap())
                .unwrap()
                .project;
        let previous_roots = std::mem::replace(&mut *roots().write(), vec![allowed]);
        let previous_grants = std::mem::take(&mut *extra_grants().write());
        let workspace = WorkspaceRecord {
            id: "fixture-workspace".into(),
            name: "Synthetic workspace".into(),
            primary_project_id: project.id,
            roots: vec![
                WorkspaceRoot {
                    path: project.path,
                    role: WorkspaceRootRole::Primary,
                    access: WorkspaceRootAccess::Write,
                    path_ok: Some(true),
                },
                WorkspaceRoot {
                    path: extra.to_string_lossy().into_owned(),
                    role: WorkspaceRootRole::Extra,
                    access: WorkspaceRootAccess::Read,
                    path_ok: Some(true),
                },
            ],
            profile_ref: None,
            capability: WorkspaceCapability::ContextOnly,
            capability_reason: None,
            updated_at: chrono::Utc::now(),
        };
        workspace_store::save_workspace_store(&WorkspaceStoreFile {
            workspaces: vec![workspace.clone()],
            ..WorkspaceStoreFile::default()
        })
        .unwrap();
        Self {
            home,
            _scope_lock: scope_lock,
            previous_roots,
            previous_grants,
            extra,
            workspace,
        }
    }

    fn assert_no_grant(&self) {
        assert!(extra_grants().read().is_empty());
        assert!(!is_allowed(&self.extra));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        *roots().write() = std::mem::take(&mut self.previous_roots);
        *extra_grants().write() = std::mem::take(&mut self.previous_grants);
    }
}

#[test]
fn imported_id_is_rejected_before_workspace_lookup() {
    let fixture = Fixture::new();
    let id = crate::session_migration::ids::new_import_id();
    let error = bind_session_workspace(&id, Some("missing-workspace".into())).unwrap_err();
    assert!(error.starts_with("IMPORTED_HISTORY_READ_ONLY"), "{error}");
    fixture.assert_no_grant();
    assert!(!crate::paths::sessions_index_file().exists());
}

#[test]
fn imported_id_cannot_grant_workspace_roots_or_change_native_index() {
    let fixture = Fixture::new();
    crate::store::create_session(None, Some("unrelated native chat".into()), false).unwrap();
    let before = std::fs::read(crate::paths::sessions_index_file()).unwrap();
    let id = crate::session_migration::ids::new_import_id();
    let error = bind_session_workspace(&id, Some(fixture.workspace.id.clone())).unwrap_err();
    fixture.assert_no_grant();
    assert!(error.starts_with("IMPORTED_HISTORY_READ_ONLY"), "{error}");
    assert_eq!(
        before,
        std::fs::read(crate::paths::sessions_index_file()).unwrap()
    );
}

#[test]
fn missing_native_session_does_not_grant_workspace_roots() {
    let fixture = Fixture::new();
    let id = uuid::Uuid::new_v4().to_string();
    let error = bind_session_workspace(&id, Some(fixture.workspace.id.clone())).unwrap_err();
    assert!(error.contains("session not found"), "{error}");
    fixture.assert_no_grant();
}

#[test]
fn direct_store_binding_rejects_imported_history_before_io() {
    let fixture = Fixture::new();
    let id = crate::session_migration::ids::new_import_id();
    for workspace_id in [None, Some(fixture.workspace.id.clone())] {
        let error = crate::store::set_session_workspace(&id, workspace_id, None, None).unwrap_err();
        assert!(error.starts_with("IMPORTED_HISTORY_READ_ONLY"), "{error}");
    }
    fixture.assert_no_grant();
    assert!(!fixture.home.path.join("sessions_index.json").exists());
}

#[test]
fn native_binding_persists_then_grants_and_can_be_cleared() {
    let fixture = Fixture::new();
    let native = crate::store::create_session(
        Some(fixture.workspace.primary_project_id.clone()),
        Some("native fixture".into()),
        false,
    )
    .unwrap();
    fixture.assert_no_grant();
    let meta = bind_session_workspace(&native.id, Some(fixture.workspace.id.clone())).unwrap();
    assert_eq!(
        meta.workspace_id.as_deref(),
        Some(fixture.workspace.id.as_str())
    );
    assert_eq!(meta.workspace_capability.as_deref(), Some("context_only"));
    assert_eq!(
        meta.workspace_root_snapshot,
        Some(workspace_store::root_snapshot(&fixture.workspace.roots))
    );
    assert!(is_allowed(&fixture.extra));
    let stored = crate::store::load_sessions_index();
    assert_eq!(stored[0].workspace_id, meta.workspace_id);
    let cleared = bind_session_workspace(&native.id, None).unwrap();
    assert!(cleared.workspace_id.is_none());
    assert!(cleared.workspace_root_snapshot.is_none());
    assert!(cleared.workspace_capability.is_none());
}
