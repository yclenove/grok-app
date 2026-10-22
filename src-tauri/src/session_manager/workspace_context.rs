//! Immutable execution context, separate from mutable runtime/journal metadata.
use crate::{store, workspace_store as ws};
use serde::{Deserialize, Serialize};

#[cfg(test)]
#[path = "workspace_runtime_tests.rs"]
mod tests;

pub(super) const CHANGED: &str = "WORKSPACE_CHANGED";

pub(super) fn changed(error: impl std::fmt::Display) -> String {
    format!("{CHANGED}: {error}")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Binding {
    project_id: Option<String>,
    workspace_id: Option<String>,
    roots: Option<String>,
    capability: Option<String>,
    worktree: Option<String>,
}

impl From<&store::SessionMeta> for Binding {
    fn from(meta: &store::SessionMeta) -> Self {
        Self {
            project_id: meta.project_id.clone(),
            workspace_id: meta.workspace_id.clone(),
            roots: meta.workspace_root_snapshot.clone(),
            capability: meta.workspace_capability.clone(),
            worktree: meta.worktree_path.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ProjectContext {
    path: String,
    canonical: Option<String>,
    ssh: Option<String>,
    trusted: bool,
    sandbox: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct WorkspaceContext {
    roots: String,
    profile: Option<String>,
    capability: ws::WorkspaceCapability,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeWorkspaceContext {
    binding: Binding,
    pub(super) cwd: String,
    canonical_cwd: Option<String>,
    project: Option<ProjectContext>,
    workspace: Option<WorkspaceContext>,
    default_sandbox: String,
    session_data_mode: String,
    pub(super) sandbox: String,
    pub(super) roots: Vec<String>,
}

fn directory(path: &str) -> Result<String, String> {
    let (canonical, available) = ws::canonicalize_workspace_root(path)?;
    if !available {
        return Err("execution directory is unavailable".into());
    }
    Ok(canonical)
}

pub(super) fn store_locks() -> Result<crate::store_lock::ExclusiveLock, String> {
    crate::store_lock::lock_exclusive_many(&[
        crate::paths::projects_file(),
        crate::paths::workspaces_file(),
        crate::paths::sessions_index_file(),
    ])
}

pub(super) fn session(id: &str) -> Result<store::SessionMeta, String> {
    store::load_sessions_snapshot()?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or_else(|| "session no longer exists".into())
}

impl RuntimeWorkspaceContext {
    /// Store locks are held; only spawn capture may create a sandbox profile.
    fn read(
        meta: &store::SessionMeta,
        cwd: &str,
        settings: &store::AppSettings,
        remote: bool,
    ) -> Result<Self, String> {
        let project = match meta.project_id.as_deref() {
            Some(id) => Some(
                store::load_projects_snapshot()?
                    .into_iter()
                    .find(|p| p.id == id)
                    .ok_or("session project no longer exists")?,
            ),
            None => None,
        };
        let mut roots = Vec::new();
        let workspace = if let Some(id) = meta
            .workspace_id
            .as_deref()
            .filter(|id| ws::is_bound_workspace_id(Some(id)))
        {
            if remote {
                return Err("local workspace cannot execute remotely".into());
            }
            let workspace = ws::get_workspace(id)?.ok_or("bound workspace no longer exists")?;
            ws::validate_session_workspace(meta, &workspace)?;
            for root in &workspace.roots {
                let canonical = directory(&root.path)?;
                if std::path::Path::new(&canonical) != std::path::Path::new(&root.path) {
                    return Err("workspace directory target changed".into());
                }
                roots.push(canonical);
            }
            let primary = workspace
                .roots
                .iter()
                .find(|r| r.role == ws::WorkspaceRootRole::Primary)
                .ok_or("workspace primary directory missing")?;
            if directory(cwd)? != directory(&primary.path)? {
                return Err("execution directory differs from reviewed workspace".into());
            }
            Some(WorkspaceContext {
                roots: ws::root_snapshot(&workspace.roots),
                profile: workspace.profile_ref,
                capability: workspace.capability,
            })
        } else {
            None
        };
        let default_sandbox = store::resolve_sandbox_profile(
            &settings.sandbox_profile,
            project.as_ref().and_then(|p| p.sandbox_profile.as_deref()),
        );
        let project = project
            .map(|p| -> Result<_, String> {
                Ok(ProjectContext {
                    canonical: if remote {
                        None
                    } else {
                        Some(directory(&p.path)?)
                    },
                    path: p.path,
                    ssh: p.ssh_alias,
                    trusted: p.trusted,
                    sandbox: p.sandbox_profile,
                })
            })
            .transpose()?;
        Ok(Self {
            binding: Binding::from(meta),
            cwd: cwd.to_string(),
            canonical_cwd: if remote { None } else { Some(directory(cwd)?) },
            project,
            workspace,
            roots,
            sandbox: default_sandbox.clone(),
            default_sandbox,
            session_data_mode: settings.session_data_mode.clone(),
        })
    }

    pub(super) fn capture(
        meta: &store::SessionMeta,
        cwd: &str,
        settings: &store::AppSettings,
        remote: bool,
    ) -> Result<Self, String> {
        let _locks = store_locks()?;
        let current = session(&meta.id)?;
        if Binding::from(&current) != Binding::from(meta) {
            return Err("session binding changed during connection".into());
        }
        let mut context = Self::read(meta, cwd, settings, remote)?;
        if let Some(id) = meta
            .workspace_id
            .as_deref()
            .filter(|id| ws::is_bound_workspace_id(Some(id)))
        {
            let spawn = ws::resolve_bound_spawn(
                &context.default_sandbox,
                &settings.session_data_mode,
                meta,
                id,
            )?;
            context.sandbox = spawn.sandbox;
            context.roots = spawn.roots;
        }
        Ok(context)
    }

    pub(super) fn validate(&self, id: &str) -> Result<(), String> {
        // Settings migration may write; it must never happen inside store_locks.
        let settings = store::load_settings();
        let _locks = store_locks()?;
        let current = session(id)?;
        self.validate_row_locked(&current, &settings)
    }

    /// Compare the captured project setting, not a workspace's custom applied
    /// profile. Even an equal effective profile may have a changed raw binding
    /// that would otherwise fail the next read-only execution-context review.
    pub(super) fn matches_project_sandbox(
        &self,
        meta: &store::SessionMeta,
        project: &store::Project,
        settings: &store::AppSettings,
    ) -> bool {
        self.binding == Binding::from(meta)
            && self.project.as_ref().is_some_and(|captured| {
                captured.path == project.path
                    && captured.ssh == project.ssh_alias
                    && captured.trusted == project.trusted
                    && captured.sandbox == project.sandbox_profile
            })
            && self.default_sandbox
                == store::resolve_sandbox_profile(
                    &settings.sandbox_profile,
                    project.sandbox_profile.as_deref(),
                )
            && self.session_data_mode == settings.session_data_mode
    }

    /// Caller holds the project/workspace/session writer locks. Never acquire
    /// them recursively or run the migratory settings loader inside a writer.
    pub(crate) fn validate_row_locked(
        &self,
        current: &store::SessionMeta,
        settings: &store::AppSettings,
    ) -> Result<(), String> {
        let mut next = Self::read(current, &self.cwd, settings, self.canonical_cwd.is_none())?;
        // Applied profile belongs to the process. Validation is read-only and
        // must not rewrite sandbox.toml or run write probes on each send.
        next.sandbox.clone_from(&self.sandbox);
        if next != *self {
            return Err("execution context changed; review and reconnect".into());
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct SendWorkspaceGuard {
    process_id: String,
    turn_open_epoch: u64,
    context: Option<RuntimeWorkspaceContext>,
}

impl SendWorkspaceGuard {
    pub(super) fn matches(&self, session: &super::LiveSession) -> Result<(), String> {
        if self.turn_open_epoch != session.turn_open_epoch {
            return Err("SEND_CANCELLED: stopped before opening the turn".into());
        }
        if self.process_id != session.process_id || self.context != session.workspace_context {
            return Err(changed("agent changed during directory validation; retry"));
        }
        Ok(())
    }

    pub(super) async fn validate(&self, id: &str) -> Result<(), String> {
        let context = self.context.clone();
        let sid = id.to_string();
        tokio::task::spawn_blocking(move || {
            if let Some(context) = context {
                context.validate(&sid).map_err(changed)?;
            }
            Ok::<_, String>(())
        })
        .await
        .map_err(changed)??;
        Ok(())
    }
}

impl super::SessionManager {
    /// Only an existing usable process needs a reuse review. A confirmed soft
    /// disconnect must be allowed to capture the new configuration on connect.
    pub(super) fn reusable_workspace_context(&self, id: &str) -> Option<RuntimeWorkspaceContext> {
        self.with_session_mut(id, |s| {
            (s.fsm.state() == crate::session_fsm::SessionState::Ready
                && !Self::live_session_is_busy(s)
                && (s.backend == "mock_acp" || s.acp.as_ref().is_some_and(|c| c.is_alive())))
            .then(|| s.workspace_context.clone())
            .flatten()
        })
        .flatten()
        .or_else(|| {
            self.parked
                .lock()
                .get(id)
                .filter(|p| p.acp.is_alive())
                .and_then(|p| p.workspace_context.clone())
        })
    }

    pub(super) fn snapshot_send_workspace(&self, id: &str) -> Result<SendWorkspaceGuard, String> {
        let guard = self
            .with_session_mut(id, |s| SendWorkspaceGuard {
                process_id: s.process_id.clone(),
                context: s.workspace_context.clone(),
                turn_open_epoch: s.turn_open_epoch,
            })
            .ok_or_else(|| changed("session has no runtime"))?;
        if guard.context.is_none() {
            return Err(changed("execution context unavailable; reconnect"));
        }
        Ok(guard)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRuntimeReview {
    pub session_id: String,
    pub cwd: String,
    pub ssh_alias: Option<String>,
    pub roots: Vec<ws::WorkspaceRoot>,
    pub request_hash: String,
}

fn review_locked(
    meta: &store::SessionMeta,
    settings: &store::AppSettings,
) -> Result<(WorkspaceRuntimeReview, store::SessionMeta), String> {
    let mut next = meta.clone();
    if let Some(id) = meta
        .workspace_id
        .as_deref()
        .filter(|id| ws::is_bound_workspace_id(Some(id)))
    {
        let workspace = ws::get_workspace(id)?
            .ok_or("bound workspace missing; detach or choose another workspace")?;
        next.workspace_root_snapshot = Some(ws::root_snapshot(&workspace.roots));
        next.workspace_capability = Some(
            match workspace.capability {
                ws::WorkspaceCapability::None => "none",
                ws::WorkspaceCapability::ContextOnly => "context_only",
                ws::WorkspaceCapability::EnforcedRead => "enforced_read",
                ws::WorkspaceCapability::ExtraWriteActive => "extra_write_active",
                ws::WorkspaceCapability::Blocked => "blocked",
            }
            .to_string(),
        );
    }
    let project = if let Some(id) = meta.project_id.as_deref() {
        Some(
            store::load_projects_snapshot()?
                .into_iter()
                .find(|p| p.id == id)
                .ok_or("project missing; choose a project")?,
        )
    } else {
        None
    };
    let ssh_alias = project.as_ref().and_then(|p| p.ssh_alias.clone());
    let cwd = meta
        .worktree_path
        .clone()
        .or_else(|| project.map(|p| p.path))
        .unwrap_or_else(|| {
            crate::paths::general_workspace_dir()
                .to_string_lossy()
                .into_owned()
        });
    let context = RuntimeWorkspaceContext::read(&next, &cwd, settings, ssh_alias.is_some())?;
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(&(
        "workspace-review-v1",
        &meta.id,
        Binding::from(meta),
        &meta.agent_session_id,
        &meta.agent_execution,
        &context,
    ))
    .map_err(|e| e.to_string())?;
    let request_hash = format!("{:x}", Sha256::digest(bytes));
    let roots = match meta
        .workspace_id
        .as_deref()
        .filter(|id| ws::is_bound_workspace_id(Some(id)))
    {
        Some(id) => ws::get_workspace(id)?.ok_or("workspace missing")?.roots,
        None => Vec::new(),
    };
    Ok((
        WorkspaceRuntimeReview {
            session_id: meta.id.clone(),
            cwd,
            ssh_alias,
            roots,
            request_hash,
        },
        next,
    ))
}

pub fn preview_workspace_runtime(id: &str) -> Result<WorkspaceRuntimeReview, String> {
    crate::session_migration::reject_imported_history_id(Some(id))?;
    let settings = store::load_settings();
    let _locks = store_locks()?;
    review_locked(&session(id)?, &settings)
        .map(|(review, _)| review)
        .map_err(changed)
}

/// CAS only directory configuration, not unrelated title/activity changes.
fn confirm_review(id: &str, hash: &str) -> Result<(), String> {
    let settings = store::load_settings();
    store::update_sessions_index_strict(|rows| {
        let meta = rows
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or("session no longer exists")?;
        let (review, next) = review_locked(meta, &settings)?;
        if review.request_hash != hash {
            return Err("directory review expired; review again".into());
        }
        meta.workspace_root_snapshot = next.workspace_root_snapshot;
        meta.workspace_capability = next.workspace_capability;
        meta.agent_session_id = None;
        meta.agent_execution = None;
        Ok(())
    })
    .map_err(changed)
}

impl super::SessionManager {
    pub async fn confirm_workspace_runtime(
        &self,
        app: &tauri::AppHandle,
        id: &str,
        hash: &str,
    ) -> Result<(), String> {
        let _guard = self.acquire_send_connect_lock().await?;
        if self
            .with_session_mut(id, |s| Self::live_session_is_busy(s))
            .unwrap_or(false)
        {
            return Err(changed(
                "wait for the current turn to finish before reconnecting",
            ));
        }
        let (sid, hash) = (id.to_string(), hash.to_string());
        tokio::task::spawn_blocking(move || confirm_review(&sid, &hash))
            .await
            .map_err(changed)??;
        self.invalidate_spawn_flags_for_session(app, id, "workspace_review")
            .await;
        Ok(())
    }
}
