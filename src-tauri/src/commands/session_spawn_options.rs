//! Store-to-retirement path shared by the real session setting commands.
use crate::{session_manager::SessionManager, store};
use tauri::AppHandle;

pub(crate) enum SessionSpawnOption {
    ForkAgent(bool),
    PluginDirs(Vec<String>),
    MaxTurns(Option<u32>),
    NoAskUser(Option<bool>),
}

pub(crate) async fn apply_session_option(
    app: Option<&AppHandle>,
    manager: &SessionManager,
    id: &str,
    option: SessionSpawnOption,
) -> Result<store::SessionMeta, String> {
    let (meta, reason) = match option {
        SessionSpawnOption::ForkAgent(armed) => {
            let meta = store::set_session_fork_agent_session(id, armed)?;
            let reason = meta.fork_agent_session.then_some("session_fork_agent");
            (meta, reason)
        }
        SessionSpawnOption::PluginDirs(dirs) => (
            store::set_session_plugin_dirs(id, dirs)?,
            Some("session_plugin_dirs"),
        ),
        SessionSpawnOption::MaxTurns(turns) => (
            store::set_session_max_agent_turns(id, turns)?,
            Some("session_max_agent_turns"),
        ),
        SessionSpawnOption::NoAskUser(value) => (
            store::set_session_no_ask_user(id, value)?,
            Some("session_no_ask_user"),
        ),
    };
    if let Some(reason) = reason {
        #[cfg(test)]
        crate::session_manager::command_spawn_options_tests::pause_after_observation(&meta.id);
        manager
            .soft_respawn_for_session_core(app, &meta.id, reason)
            .await;
    }
    Ok(meta)
}

pub(crate) async fn apply_project_sandbox(
    app: Option<&AppHandle>,
    manager: &SessionManager,
    id: &str,
    profile: Option<String>,
) -> Result<store::Project, String> {
    let project = store::set_project_sandbox_profile(id, profile)?;
    #[cfg(test)]
    crate::session_manager::command_spawn_options_tests::pause_after_observation(id);
    manager.refresh_project_sandbox_core(app, id).await?;
    Ok(project)
}
