//! Multi-root workspace persistence (#1194 MVP-0).
//!
//! Declares primary + extra local roots for a chat. Extra roots default to
//! read-only; without a verified custom CLI sandbox profile the capability is
//! always `context_only` (honest: CLI `workspace` profile may still read the
//! whole disk). Does not rewrite `~/.grok` or invent write powers CLI lacks.

use crate::paths::workspaces_file;
use crate::store;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[path = "workspace_binding.rs"]
mod binding;
#[path = "workspace_store_paths.rs"]
mod local_paths;
#[path = "workspace_store_io.rs"]
mod persistence;
#[cfg(test)]
pub(crate) use binding::resolve_session_spawn;
pub(crate) use binding::{resolve_bound_spawn, validate_session_workspace};
pub use local_paths::canonicalize_workspace_root;
use local_paths::paths_equal;

pub const MAX_EXTRA_ROOTS: usize = 8;
pub const WORKSPACE_STORE_VERSION: u32 = 1;
/// Stored on a session after **Detach from chat**. Distinct from `None`
/// (never bound — new chats inherit the project's workspace).
pub const UNBOUND_WORKSPACE_ID: &str = "-";

pub fn is_bound_workspace_id(id: Option<&str>) -> bool {
    id.map(str::trim)
        .is_some_and(|s| !s.is_empty() && s != UNBOUND_WORKSPACE_ID)
}

#[cfg(test)]
#[path = "workspace_store_tests.rs"]
mod transaction_tests;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceRootRole {
    Primary,
    Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceRootAccess {
    Read,
    Write,
}

/// Honest CLI capability for this workspace snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceCapability {
    /// Old CLI / unknown — hide multi-root write.
    None,
    /// Roots declared in App only; CLI may still read whole disk.
    #[default]
    ContextOnly,
    /// Verified custom strict profile covers declared roots (MVP-1+).
    EnforcedRead,
    /// Verified write roots (v1+).
    ExtraWriteActive,
    /// Profile/platform failure — do not silently fall to `off`.
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRoot {
    pub path: String,
    pub role: WorkspaceRootRole,
    pub access: WorkspaceRootAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_ok: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRecord {
    pub id: String,
    pub name: String,
    pub primary_project_id: String,
    pub roots: Vec<WorkspaceRoot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_ref: Option<String>,
    #[serde(default)]
    pub capability: WorkspaceCapability,
    /// Human-readable reason for the current capability (Doctor / modal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStoreFile {
    pub version: u32,
    pub workspaces: Vec<WorkspaceRecord>,
}

impl Default for WorkspaceStoreFile {
    fn default() -> Self {
        Self {
            version: WORKSPACE_STORE_VERSION,
            workspaces: Vec::new(),
        }
    }
}

pub fn load_workspace_store() -> Result<WorkspaceStoreFile, String> {
    persistence::read(&workspaces_file())
}

#[cfg(test)]
pub fn save_workspace_store(file: &WorkspaceStoreFile) -> Result<(), String> {
    persistence::update(&workspaces_file(), |stored| {
        *stored = file.clone();
        Ok(())
    })
}

pub fn list_workspaces() -> Result<Vec<WorkspaceRecord>, String> {
    Ok(load_workspace_store()?.workspaces)
}

pub fn get_workspace(id: &str) -> Result<Option<WorkspaceRecord>, String> {
    let id = id.trim();
    if id.is_empty() {
        return Ok(None);
    }
    Ok(list_workspaces()?.into_iter().find(|w| w.id == id))
}

/// No migrations, trust changes, or project writes during workspace resolution.
pub(crate) fn load_projects_snapshot() -> Result<Vec<store::Project>, String> {
    store::load_projects_snapshot()
}

/// Build / update a workspace. Extra-root write is allowed in the record;
/// Capability / spawn profile are resolved under the workspace transaction.
pub fn upsert_workspace(
    id: Option<String>,
    name: String,
    primary_project_id: String,
    roots: Vec<WorkspaceRoot>,
) -> Result<WorkspaceRecord, String> {
    let primary_project_id = primary_project_id.trim().to_string();
    if primary_project_id.is_empty() {
        return Err("primary project required".into());
    }
    let projects = load_projects_snapshot()?;
    let project = projects
        .iter()
        .find(|p| p.id == primary_project_id)
        .ok_or_else(|| "primary project not found".to_string())?;
    if project.is_ssh_remote() {
        return Err("SSH projects cannot host a multi-root workspace in MVP-0".into());
    }
    let (project_path, project_ok) = canonicalize_workspace_root(&project.path)?;

    let name = {
        let n = name.trim();
        if n.is_empty() {
            format!("{} workspace", project.name)
        } else {
            n.to_string()
        }
    };

    let mut normalized: Vec<WorkspaceRoot> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut primary_count = 0usize;
    let mut extra_count = 0usize;

    for root in roots {
        let (path, path_ok) = canonicalize_workspace_root(&root.path)?;
        if seen.iter().any(|s| paths_equal(s, &path)) {
            return Err(format!("duplicate root: {path}"));
        }
        seen.push(path.clone());
        match root.role {
            WorkspaceRootRole::Primary => {
                if !paths_equal(&path, &project_path) {
                    return Err("primary root must match the primary project directory".into());
                }
                primary_count += 1;
                if primary_count > 1 {
                    return Err("exactly one primary root required".into());
                }
                // Primary stays write for the main project cwd (single-project write).
                normalized.push(WorkspaceRoot {
                    path,
                    role: WorkspaceRootRole::Primary,
                    access: WorkspaceRootAccess::Write,
                    path_ok: Some(path_ok),
                });
            }
            WorkspaceRootRole::Extra => {
                extra_count += 1;
                if extra_count > MAX_EXTRA_ROOTS {
                    return Err(format!("at most {MAX_EXTRA_ROOTS} extra roots"));
                }
                normalized.push(WorkspaceRoot {
                    path,
                    role: WorkspaceRootRole::Extra,
                    access: root.access,
                    path_ok: Some(path_ok),
                });
            }
        }
    }

    if primary_count != 1 {
        // Default primary from project path when caller omitted it.
        let (path, path_ok) = (project_path, project_ok);
        if seen.iter().any(|s| paths_equal(s, &path)) {
            return Err("primary root conflicts with an extra root".into());
        }
        normalized.insert(
            0,
            WorkspaceRoot {
                path,
                role: WorkspaceRootRole::Primary,
                access: WorkspaceRootAccess::Write,
                path_ok: Some(path_ok),
            },
        );
    }

    let now = Utc::now();
    // Settings loading may write migrations and acquire the non-reentrant store
    // mutex. Resolve it before entering the workspace transaction.
    let mode = store::load_settings().session_data_mode;
    persistence::update(&workspaces_file(), |file| {
        validate_project_targets(&[(project.id.clone(), project.path.clone())])?;
        let record =
            if let Some(existing_id) = id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
                let idx = file
                    .workspaces
                    .iter()
                    .position(|w| w.id == existing_id)
                    .ok_or_else(|| "workspace not found".to_string())?;
                let mut record = WorkspaceRecord {
                    id: existing_id,
                    name,
                    primary_project_id,
                    roots: normalized,
                    profile_ref: None,
                    capability: WorkspaceCapability::ContextOnly,
                    capability_reason: None,
                    updated_at: now,
                };
                apply_capability_plan_for_mode(&mut record, &mode)?;
                file.workspaces[idx] = record.clone();
                record
            } else {
                let mut record = WorkspaceRecord {
                    id: format!("ws_{}", Uuid::new_v4()),
                    name,
                    primary_project_id,
                    roots: normalized,
                    profile_ref: None,
                    capability: WorkspaceCapability::ContextOnly,
                    capability_reason: None,
                    updated_at: now,
                };
                apply_capability_plan_for_mode(&mut record, &mode)?;
                file.workspaces.insert(0, record.clone());
                record
            };
        Ok(record)
    })
}

/// Resolve capability from session_data_mode + sandbox plan. Downgrades extra
/// write roots to read when the plan cannot activate write.
fn apply_capability_plan_for_mode(record: &mut WorkspaceRecord, mode: &str) -> Result<(), String> {
    let plan = crate::workspace_sandbox::plan_for_session_mode(mode, record)?;
    if plan.spawn_sandbox.is_none() {
        for root in &mut record.roots {
            if root.role == WorkspaceRootRole::Extra {
                root.access = WorkspaceRootAccess::Read;
            }
        }
    }
    record.capability = plan.capability;
    record.profile_ref = plan.profile_ref;
    record.capability_reason = Some(plan.reason);
    Ok(())
}

/// Recompute capability for every stored workspace (Doctor refresh).
pub fn refresh_all_capabilities() -> Result<Vec<WorkspaceRecord>, String> {
    let mode = store::load_settings().session_data_mode;
    persistence::update(&workspaces_file(), |file| {
        for ws in &mut file.workspaces {
            apply_capability_plan_for_mode(ws, &mode)?;
        }
        Ok(file.workspaces.clone())
    })
}

impl WorkspaceCapability {
    pub fn as_token(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ContextOnly => "context_only",
            Self::EnforcedRead => "enforced_read",
            Self::ExtraWriteActive => "extra_write_active",
            Self::Blocked => "blocked",
        }
    }
}

/// Project's default workspace: Settings `recentWorkspaceId` when it belongs
/// to this project, otherwise the first stored workspace for the project.
pub fn default_workspace_for_project(project_id: &str) -> Result<Option<WorkspaceRecord>, String> {
    let pid = project_id.trim();
    if pid.is_empty() {
        return Ok(None);
    }
    let list = workspaces_for_project(pid)?;
    if list.is_empty() {
        return Ok(None);
    }
    if let Some(rid) = store::load_settings()
        .recent_workspace_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some(ws) = list.iter().find(|w| w.id == rid) {
            return Ok(Some(ws.clone()));
        }
    }
    Ok(list.into_iter().next())
}

/// New chats in a project inherit that project's workspace so extra-root
/// write does not require opening the modal again (#1233).
///
/// `workspace_id: None` / empty = never bound → fill from the project.
/// `workspace_id: "-"` = explicit detach → leave alone.
pub fn bind_default_workspace_if_unbound(session_id: &str) -> Result<store::SessionMeta, String> {
    let meta = store::load_sessions_snapshot()?
        .into_iter()
        .find(|s| s.id == session_id)
        .ok_or_else(|| format!("session not found: {session_id}"))?;
    if is_bound_workspace_id(meta.workspace_id.as_deref())
        || meta.workspace_id.as_deref() == Some(UNBOUND_WORKSPACE_ID)
    {
        return Ok(meta);
    }
    let Some(pid) = meta
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(meta);
    };
    let Some(ws) = default_workspace_for_project(pid)? else {
        return Ok(meta);
    };
    validate_primary_location(&ws)?;
    let extra_roots: Vec<String> = ws
        .roots
        .iter()
        .filter(|root| root.role == WorkspaceRootRole::Extra)
        .map(|root| root.path.clone())
        .collect();
    let meta = store::set_session_workspace(
        session_id,
        Some(ws.id),
        Some(root_snapshot(&ws.roots)),
        Some(ws.capability.as_token().to_string()),
    )?;
    for root in extra_roots {
        crate::path_scope::grant_path(std::path::Path::new(&root));
    }
    Ok(meta)
}

pub fn delete_workspace(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("workspace id empty".into());
    }
    persistence::update(&workspaces_file(), |file| {
        if store::load_sessions_snapshot()?
            .iter()
            .any(|session| session.workspace_id.as_deref() == Some(id))
        {
            return Err("workspace still has sessions; detach it before deleting".into());
        }
        let before = file.workspaces.len();
        file.workspaces.retain(|w| w.id != id);
        if file.workspaces.len() == before {
            return Err("workspace not found".into());
        }
        Ok(())
    })
}

pub fn workspaces_for_project(project_id: &str) -> Result<Vec<WorkspaceRecord>, String> {
    let pid = project_id.trim();
    Ok(list_workspaces()?
        .into_iter()
        .filter(|w| w.primary_project_id == pid)
        .collect())
}

/// Immutable import copy: repeated requests reuse it, user edits are never
/// overwritten. Extra roots carry context only; creation grants no Host paths.
pub(crate) fn insert_imported(
    record: WorkspaceRecord,
    project_targets: &[(String, String)],
) -> Result<WorkspaceRecord, String> {
    persistence::update(&workspaces_file(), |file| {
        validate_project_targets(project_targets)?;
        if let Some(existing) = file.workspaces.iter().find(|row| row.id == record.id) {
            if existing.name != record.name
                || existing.primary_project_id != record.primary_project_id
                || existing.roots != record.roots
                || existing.capability != record.capability
                || existing.profile_ref != record.profile_ref
            {
                return Err("MAPPING_CONFLICT: native workspace was edited; use its existing project workspace controls".into());
            }
            return Ok(existing.clone());
        }
        file.workspaces.insert(0, record.clone());
        Ok(record)
    })
}

/// Called while the projects lock is held. The saved target is a reviewed
/// location, so even a valid relocation must require a fresh user review.
fn validate_project_targets(targets: &[(String, String)]) -> Result<(), String> {
    let projects = load_projects_snapshot()?;
    for (id, path) in targets {
        if !projects.iter().any(|project| {
            &project.id == id
                && &project.path == path
                && !project.is_ssh_remote()
                && !project.is_legacy_general()
        }) {
            return Err(
                "MAPPING_CONFLICT: project target changed; review the workspace again".into(),
            );
        }
    }
    Ok(())
}

/// A saved workspace cannot silently follow a moved/deleted project on bind or
/// spawn. Source metadata remains available so the user can explicitly repair it.
pub(crate) fn validate_primary_location(record: &WorkspaceRecord) -> Result<(), String> {
    let projects = load_projects_snapshot()?;
    let project = projects
        .iter()
        .find(|project| {
            project.id == record.primary_project_id
                && !project.is_ssh_remote()
                && !project.is_legacy_general()
        })
        .ok_or("workspace primary project no longer exists")?;
    let (path, available) = canonicalize_workspace_root(&project.path)?;
    let primary: Vec<_> = record
        .roots
        .iter()
        .filter(|root| root.role == WorkspaceRootRole::Primary)
        .collect();
    if !available || primary.len() != 1 || !paths_equal(&primary[0].path, &path) {
        return Err(
            "workspace primary folder changed or is unavailable; review the workspace again".into(),
        );
    }
    Ok(())
}

/// Snapshot string for SessionMeta (stable, path-order sensitive).
pub fn root_snapshot(roots: &[WorkspaceRoot]) -> String {
    roots
        .iter()
        .map(|r| {
            format!(
                "{}:{}:{}",
                match r.role {
                    WorkspaceRootRole::Primary => "p",
                    WorkspaceRootRole::Extra => "e",
                },
                match r.access {
                    WorkspaceRootAccess::Read => "r",
                    WorkspaceRootAccess::Write => "w",
                },
                r.path
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("grok-ws-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_spawn_sandbox_falls_back_without_workspace() {
        let _home = crate::session_migration::test_home::IsolatedAppHome::acquire(
            "workspace-unbound-spawn",
        );
        let session = store::create_session(None, None, false).unwrap();
        assert_eq!(
            resolve_session_spawn("workspace", None, "shared", &session)
                .unwrap()
                .sandbox,
            "workspace"
        );
        assert_eq!(
            resolve_session_spawn("workspace", Some("strict"), "shared", &session)
                .unwrap()
                .sandbox,
            "strict"
        );
    }

    #[test]
    fn canonicalize_rejects_parent_segments() {
        let path = std::env::temp_dir().join("foo").join("..").join("bar");
        let err = canonicalize_workspace_root(path.to_str().unwrap()).unwrap_err();
        assert!(err.contains(".."), "{err}");
    }

    #[test]
    fn canonicalize_marks_missing_path_ok_false() {
        let missing =
            std::env::temp_dir().join(format!("grok-app-ws-missing-dir-{}", Uuid::new_v4()));
        let (path, ok) = canonicalize_workspace_root(missing.to_str().unwrap()).unwrap();
        assert!(!ok);
        assert!(path.contains("grok-app-ws-missing"));
    }

    #[test]
    fn canonicalize_accepts_real_dir() {
        let dir = temp_dir("ok");
        let (path, ok) = canonicalize_workspace_root(dir.to_str().unwrap()).unwrap();
        assert!(ok);
        assert!(!path.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn root_snapshot_is_stable() {
        let snap = root_snapshot(&[
            WorkspaceRoot {
                path: "/a".into(),
                role: WorkspaceRootRole::Primary,
                access: WorkspaceRootAccess::Write,
                path_ok: Some(true),
            },
            WorkspaceRoot {
                path: "/b".into(),
                role: WorkspaceRootRole::Extra,
                access: WorkspaceRootAccess::Read,
                path_ok: Some(true),
            },
        ]);
        assert_eq!(snap, "p:w:/a|e:r:/b");
    }

    #[test]
    fn legacy_empty_file_deserializes() {
        let raw = r#"{"version":1,"workspaces":[]}"#;
        let f: WorkspaceStoreFile = serde_json::from_str(raw).unwrap();
        assert!(f.workspaces.is_empty());
    }

    #[test]
    fn bind_default_workspace_if_unbound_inherits_project_workspace() {
        let _g = crate::paths::APP_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!(
            "grok-ws-inherit-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();

        let primary = tmp.join("primary");
        let extra = tmp.join("extra");
        fs::create_dir_all(&primary).unwrap();
        fs::create_dir_all(&extra).unwrap();

        let mut settings = crate::store::load_settings();
        settings.session_data_mode = "independent".into();
        crate::store::save_settings(&settings).unwrap();

        let proj = crate::store::add_project(primary.to_string_lossy().to_string(), true).unwrap();
        let ws = upsert_workspace(
            None,
            "ws".into(),
            proj.id.clone(),
            vec![
                WorkspaceRoot {
                    path: primary.to_string_lossy().to_string(),
                    role: WorkspaceRootRole::Primary,
                    access: WorkspaceRootAccess::Write,
                    path_ok: Some(true),
                },
                WorkspaceRoot {
                    path: extra.to_string_lossy().to_string(),
                    role: WorkspaceRootRole::Extra,
                    access: WorkspaceRootAccess::Write,
                    path_ok: Some(true),
                },
            ],
        )
        .expect("upsert workspace");
        assert_eq!(ws.capability, WorkspaceCapability::ExtraWriteActive);

        let meta = crate::store::create_session(Some(proj.id.clone()), Some("n".into()), false)
            .expect("create");
        assert!(
            meta.workspace_id.is_none(),
            "new session starts unbound: {:?}",
            meta.workspace_id
        );

        let bound = bind_default_workspace_if_unbound(&meta.id).expect("bind");
        assert_eq!(bound.workspace_id.as_deref(), Some(ws.id.as_str()));

        let sandbox = resolve_session_spawn("workspace", None, "independent", &bound)
            .unwrap()
            .sandbox;
        assert!(
            sandbox.starts_with(crate::workspace_sandbox::APP_PROFILE_PREFIX),
            "expected app-managed profile, got {sandbox}"
        );

        let again = bind_default_workspace_if_unbound(&meta.id).expect("idempotent");
        assert_eq!(again.workspace_id.as_deref(), Some(ws.id.as_str()));

        crate::store::set_session_workspace(
            &meta.id,
            Some(UNBOUND_WORKSPACE_ID.to_string()),
            None,
            None,
        )
        .expect("detach");
        let detached = bind_default_workspace_if_unbound(&meta.id).expect("keep detach");
        assert_eq!(
            detached.workspace_id.as_deref(),
            Some(UNBOUND_WORKSPACE_ID),
            "explicit detach must not re-inherit the project workspace"
        );
        assert_eq!(
            resolve_session_spawn("workspace", None, "independent", &detached)
                .unwrap()
                .sandbox,
            "workspace"
        );

        std::env::remove_var("GROK_APP_HOME");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unbound_sentinel_is_not_a_bound_workspace() {
        assert!(!is_bound_workspace_id(None));
        assert!(!is_bound_workspace_id(Some("")));
        assert!(!is_bound_workspace_id(Some(UNBOUND_WORKSPACE_ID)));
        assert!(is_bound_workspace_id(Some("ws_abc")));
    }
}
