//! Antigravity (`agy`) registration.
//!
//! User scope does not edit JSON. It backs up `~/.gemini/config/mcp_config.json`
//! when that file exists, then runs `agy mcp add` / `agy mcp remove` with `HOME`
//! pointed at the injected home. Flags precede the name, and `--` separates the
//! `gcu` arguments (`agy` 1.3.4). `agy mcp` has no scope flag, so project scope
//! edits `.agents/mcp_config.json` directly. An unparseable user file is not
//! handed to `agy`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::error::{unexpected, unparseable, InstallError};
use super::fsutil::read_bytes;
use super::json::{self, ServerShape};
use super::plan::{FileChange, Plan, PlanAction, PlannedCommand};
use super::{
    gcu_string, push_change, require_context, status_of, ClientId, ClientInstaller, ClientStatus,
    InstallCtx, Scope, SERVER_NAME,
};

const USER_REL: &str = ".gemini/config/mcp_config.json";

pub(crate) struct AntigravityInstaller;

impl ClientInstaller for AntigravityInstaller {
    fn id(&self) -> ClientId {
        ClientId::Antigravity
    }

    fn plan(&self, ctx: &InstallCtx<'_>) -> Result<Plan, InstallError> {
        match ctx.scope {
            Scope::User => plan_user(ctx, PlanAction::Install),
            Scope::Project => plan_project(ctx, PlanAction::Install),
        }
    }

    fn plan_removal(&self, ctx: &InstallCtx<'_>) -> Result<Plan, InstallError> {
        match ctx.scope {
            Scope::User => plan_user(ctx, PlanAction::Uninstall),
            Scope::Project => plan_project(ctx, PlanAction::Uninstall),
        }
    }

    fn status(&self, ctx: &InstallCtx<'_>) -> Result<ClientStatus, InstallError> {
        require_context(ctx)?;
        match ctx.scope {
            Scope::User => {
                let gcu = gcu_string(ctx)?;
                let state = inspect(&user_config(ctx.home), &gcu)?;
                let present = matches!(state, AgyState::Present { .. });
                let matches = matches!(state, AgyState::Present { matches: true, .. });
                Ok(status_of(present, matches))
            }
            Scope::Project => {
                let path = project_config(ctx.project_dir);
                let present = json::server_present(&path, "mcpServers")?;
                let plan = self.plan(ctx)?;
                Ok(status_of(present, present && plan.is_noop()))
            }
        }
    }
}

fn plan_project(ctx: &InstallCtx<'_>, action: PlanAction) -> Result<Plan, InstallError> {
    require_context(ctx)?;
    let path = project_config(ctx.project_dir);
    let mut files = Vec::new();
    match action {
        PlanAction::Install => {
            let gcu = gcu_string(ctx)?;
            push_change(
                &mut files,
                json::plan_upsert(&path, "mcpServers", ServerShape::CommandArgs, &gcu)?,
            );
        }
        PlanAction::Uninstall => {
            push_change(&mut files, json::plan_remove(&path, "mcpServers")?);
        }
    }
    Ok(Plan::new(ClientId::Antigravity, action, files, Vec::new()))
}

fn plan_user(ctx: &InstallCtx<'_>, action: PlanAction) -> Result<Plan, InstallError> {
    require_context(ctx)?;
    let gcu = gcu_string(ctx)?;
    let path = user_config(ctx.home);
    let state = inspect(&path, &gcu)?;
    let (run, bytes) = match (&state, action) {
        (AgyState::Absent { .. }, PlanAction::Uninstall) => (false, None),
        (AgyState::Present { matches: true, .. }, PlanAction::Install) => (false, None),
        (AgyState::Absent { bytes }, PlanAction::Install) => (true, bytes.clone()),
        (AgyState::Present { bytes, .. }, _) => (true, Some(bytes.clone())),
    };
    let mut files = Vec::new();
    if run {
        if let Some(bytes) = bytes {
            files.push(FileChange::backup_only(&path, bytes));
        }
    }
    let mut commands = Vec::new();
    if run {
        commands.push(agy_command(ctx, action, &gcu)?);
    }
    Ok(Plan::new(ClientId::Antigravity, action, files, commands))
}

fn agy_command(
    ctx: &InstallCtx<'_>,
    action: PlanAction,
    gcu: &str,
) -> Result<PlannedCommand, InstallError> {
    let program = ctx
        .bins
        .agy
        .ok_or(InstallError::MissingClientBinary {
            client: ClientId::Antigravity,
        })?
        .to_path_buf();
    let home = ctx
        .home
        .to_str()
        .ok_or(InstallError::PathNotUnicode { what: "home" })?;
    let args = match action {
        PlanAction::Install => vec![
            "mcp".to_owned(),
            "add".to_owned(),
            SERVER_NAME.to_owned(),
            "--".to_owned(),
            gcu.to_owned(),
            "mcp".to_owned(),
        ],
        PlanAction::Uninstall => {
            vec![
                "mcp".to_owned(),
                "remove".to_owned(),
                SERVER_NAME.to_owned(),
            ]
        }
    };
    Ok(PlannedCommand {
        program,
        args,
        cwd: ctx.project_dir.to_path_buf(),
        env: vec![
            ("HOME".to_owned(), home.to_owned()),
            ("USERPROFILE".to_owned(), home.to_owned()),
        ],
    })
}

enum AgyState {
    Absent { bytes: Option<Vec<u8>> },
    Present { bytes: Vec<u8>, matches: bool },
}

fn inspect(path: &Path, gcu: &str) -> Result<AgyState, InstallError> {
    let Some(bytes) = read_bytes(path)? else {
        return Ok(AgyState::Absent { bytes: None });
    };
    let text = std::str::from_utf8(&bytes).map_err(|source| unparseable(path, source))?;
    if text.trim().is_empty() {
        return Ok(AgyState::Absent { bytes: Some(bytes) });
    }
    let value: Value = serde_json::from_str(text).map_err(|source| unparseable(path, source))?;
    let Some(root) = value.as_object() else {
        return Err(unexpected(path, "root must be an object"));
    };
    let Some(parent) = root.get("mcpServers") else {
        return Ok(AgyState::Absent { bytes: Some(bytes) });
    };
    let Some(parent) = parent.as_object() else {
        return Err(unexpected(path, "mcpServers must be an object"));
    };
    let Some(server) = parent.get(SERVER_NAME) else {
        return Ok(AgyState::Absent { bytes: Some(bytes) });
    };
    let matches = server.as_object().is_some_and(|server| {
        server.get("command").and_then(Value::as_str) == Some(gcu)
            && server
                .get("args")
                .and_then(Value::as_array)
                .is_some_and(|args| {
                    args.len() == 1 && args.first().and_then(Value::as_str) == Some("mcp")
                })
    });
    Ok(AgyState::Present { bytes, matches })
}

fn user_config(home: &Path) -> PathBuf {
    home.join(USER_REL)
}

fn project_config(project: &Path) -> PathBuf {
    project.join(".agents").join("mcp_config.json")
}
