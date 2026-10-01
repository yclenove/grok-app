//! Local interactive chat only. Remote IM, scheduler, and SSH do not inherit grants.

use grok_computer_use_core::surface::{classify_surface, surface_allows_local, ComputerUseSurface};

use super::feature::set_session_surface;

pub fn classify_session(session_id: &str) -> ComputerUseSurface {
    let id = session_id.trim();
    if id.is_empty() {
        return ComputerUseSurface::LocalInteractive;
    }
    let meta = crate::store::load_sessions_index()
        .into_iter()
        .find(|s| s.id == id);
    let scheduled = meta.as_ref().is_some_and(|s| s.scheduled);
    let ssh = meta
        .as_ref()
        .and_then(|s| s.project_id.as_deref())
        .and_then(|pid| {
            crate::store::load_projects()
                .into_iter()
                .find(|p| p.id == pid)
        })
        .is_some_and(|p| p.is_ssh_remote());
    let remote_im = crate::remote_im::binds_app_session(id);
    classify_surface(scheduled, ssh, remote_im)
}

pub fn refuse_if_not_local(session_id: &str) -> Result<(), String> {
    let surface = classify_session(session_id);
    set_session_surface(session_id, surface);
    if !surface_allows_local(surface) {
        return Err("Computer Use is only available on a local interactive chat".into());
    }
    Ok(())
}

#[cfg(any(test, feature = "computer-use-probe"))]
pub fn run_classify_session_gates() -> Result<(), String> {
    use chrono::Utc;
    use grok_computer_use_core::surface::ComputerUseSurface;

    use crate::store::{self, Project, SessionMeta};

    let _g = crate::paths::APP_HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tmp = std::env::temp_dir().join(format!(
        "cu-classify-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let _ = std::fs::create_dir_all(&tmp);
    std::env::set_var("GROK_APP_HOME", &tmp);
    let _ = store::load_projects();

    let meta = |id: &str, project_id: Option<&str>, scheduled: bool| SessionMeta {
        id: id.into(),
        project_id: project_id.map(|s| s.into()),
        title: id.into(),
        agent_session_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        model_id: None,
        archived: false,
        pinned: false,
        effort: None,
        mode: None,
        permission_policy: None,
        json_schema: None,
        scheduled,
        worktree_path: None,
        worktree_branch: None,
        is_worktree_session: false,
        plugin_dirs: Vec::new(),
        extra_rules: None,
        max_agent_turns: None,
        system_prompt_override: None,
        fork_agent_session: false,
        fork_rewind_prompt_index: None,
        no_ask_user: None,
        workspace_id: None,
        workspace_root_snapshot: None,
        workspace_capability: None,
        provider_id: None,
    };

    store::save_sessions_index(&[meta("cu-sched", None, true)]).map_err(|e| e.to_string())?;
    if classify_session("cu-sched") != ComputerUseSurface::Scheduled {
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::remove_var("GROK_APP_HOME");
        return Err("scheduled session must classify as Scheduled".into());
    }
    if refuse_if_not_local("cu-sched").is_ok() {
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::remove_var("GROK_APP_HOME");
        return Err("scheduled session must not inherit Computer Use".into());
    }

    let ssh = Project {
        id: "p-ssh".into(),
        name: "UTS:/home/u".into(),
        path: "/home/u".into(),
        trusted: true,
        last_opened_at: Utc::now(),
        path_ok: true,
        pinned: false,
        system: false,
        model_id: None,
        effort: None,
        mode: None,
        permission_policy: None,
        sandbox_profile: None,
        color: None,
        ssh_alias: Some("UTS".into()),
    };
    store::save_projects(&[ssh]).map_err(|e| e.to_string())?;
    store::save_sessions_index(&[meta("cu-ssh", Some("p-ssh"), false)])
        .map_err(|e| e.to_string())?;
    if classify_session("cu-ssh") != ComputerUseSurface::Ssh {
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::remove_var("GROK_APP_HOME");
        return Err(format!(
            "ssh session must classify as Ssh, got {:?}",
            classify_session("cu-ssh")
        ));
    }
    if refuse_if_not_local("cu-ssh").is_ok() {
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::remove_var("GROK_APP_HOME");
        return Err("ssh session must not inherit Computer Use".into());
    }

    store::save_sessions_index(&[meta("cu-im", None, false)]).map_err(|e| e.to_string())?;
    crate::remote_im::with_live_bindings_cleared(|| {
        crate::remote_im::seed_disk_im_binding("cu-im");
        if classify_session("cu-im") != ComputerUseSurface::RemoteIm {
            return Err(format!(
                "disk IM session must classify as RemoteIm, got {:?}",
                classify_session("cu-im")
            ));
        }
        if refuse_if_not_local("cu-im").is_ok() {
            return Err("disk IM session must not inherit Computer Use".into());
        }
        Ok(())
    })?;

    std::env::remove_var("GROK_APP_HOME");
    let _ = std::fs::remove_dir_all(&tmp);
    println!("gate: classify_session_disk_im_scheduled_ssh");
    Ok(())
}
