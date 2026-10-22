//! Permission command cores; tests share the production store/runtime path.
use crate::{permission::PermissionPolicy, session_manager::SessionManager, store};
use tauri::AppHandle;

pub(crate) async fn save_project_permission(
    app: Option<&AppHandle>,
    manager: &SessionManager,
    id: &str,
    policy: Option<String>,
) -> Result<store::Project, String> {
    let project = store::set_project_permission_policy(id, policy)?;
    #[cfg(test)]
    crate::session_manager::permission_policy_tests::pause_after_save(id);
    if let Err(error) = manager.refresh_permission_policies_core(app).await {
        tracing::warn!("project_set_permission_policy refresh: {error}");
    }
    Ok(project)
}

pub(crate) async fn save_session_permission(
    app: Option<&AppHandle>,
    manager: &SessionManager,
    policy: &str,
    project_id: Option<&str>,
    session_id: Option<&str>,
) -> Result<store::ComposerPrefs, String> {
    let policy = PermissionPolicy::parse(policy);
    let project_id = permission_project_id(project_id, session_id)?;
    let prefs = store::save_composer_prefs(
        project_id.as_deref(),
        session_id,
        None,
        None,
        None,
        Some(policy.as_str().into()),
    )?;
    manager.refresh_permission_policies_core(app).await?;
    Ok(prefs)
}

/// A persisted chat owns its project even if a delayed UI callback sends a
/// different activeProject. Null session ids remain drafts, not current focus.
pub(crate) fn permission_project_id(
    project_id: Option<&str>,
    session_id: Option<&str>,
) -> Result<Option<String>, String> {
    match session_id {
        Some(id) => store::load_sessions_snapshot()?
            .into_iter()
            .find(|session| session.id == id)
            .map(|session| session.project_id)
            .ok_or_else(|| "session not found".into()),
        None => Ok(project_id.map(str::to_owned)),
    }
}
