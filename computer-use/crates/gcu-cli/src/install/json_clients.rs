//! JSON MCP configs for clients whose official CLI is missing or unsafe to call.
//!
//! Claude Code 2.1.296 rejects the server name `computer-use` and still rewrites
//! `~/.claude.json` when `claude mcp add` fails, so Claude is edited in place.
//! Gemini CLI, OpenCode, Cursor, and VS Code are file edits by task assignment.
//! OpenCode JSONC comments are not parsed: an unparseable file is left untouched
//! rather than pulling in a JSONC crate.

use std::path::{Path, PathBuf};

use super::error::InstallError;
use super::json::{self, ServerShape};
use super::plan::{Plan, PlanAction};
use super::{
    gcu_string, push_change, require_context, status_of, ClientId, ClientInstaller, ClientStatus,
    InstallCtx, Scope,
};

pub(crate) struct JsonInstaller {
    id: ClientId,
    parent: &'static str,
    shape: ServerShape,
    user_path: fn(&Path) -> PathBuf,
    project_path: fn(&Path) -> PathBuf,
}

impl JsonInstaller {
    fn path(&self, ctx: &InstallCtx<'_>) -> PathBuf {
        match ctx.scope {
            Scope::User => (self.user_path)(ctx.home),
            Scope::Project => (self.project_path)(ctx.project_dir),
        }
    }
}

impl ClientInstaller for JsonInstaller {
    fn id(&self) -> ClientId {
        self.id
    }

    fn plan(&self, ctx: &InstallCtx<'_>) -> Result<Plan, InstallError> {
        require_context(ctx)?;
        let gcu = gcu_string(ctx)?;
        let path = self.path(ctx);
        let mut files = Vec::new();
        push_change(
            &mut files,
            json::plan_upsert(&path, self.parent, self.shape, &gcu)?,
        );
        Ok(Plan::new(self.id, PlanAction::Install, files, Vec::new()))
    }

    fn plan_removal(&self, ctx: &InstallCtx<'_>) -> Result<Plan, InstallError> {
        require_context(ctx)?;
        let path = self.path(ctx);
        let mut files = Vec::new();
        push_change(&mut files, json::plan_remove(&path, self.parent)?);
        Ok(Plan::new(self.id, PlanAction::Uninstall, files, Vec::new()))
    }

    fn status(&self, ctx: &InstallCtx<'_>) -> Result<ClientStatus, InstallError> {
        let path = self.path(ctx);
        let present = json::server_present(&path, self.parent)?;
        let plan = self.plan(ctx)?;
        Ok(status_of(present, present && plan.is_noop()))
    }
}

fn make(
    id: ClientId,
    parent: &'static str,
    shape: ServerShape,
    user_path: fn(&Path) -> PathBuf,
    project_path: fn(&Path) -> PathBuf,
) -> Box<dyn ClientInstaller> {
    Box::new(JsonInstaller {
        id,
        parent,
        shape,
        user_path,
        project_path,
    })
}

pub(crate) fn claude() -> Box<dyn ClientInstaller> {
    make(
        ClientId::Claude,
        "mcpServers",
        ServerShape::CommandArgs,
        claude_user,
        claude_project,
    )
}

pub(crate) fn gemini() -> Box<dyn ClientInstaller> {
    make(
        ClientId::Gemini,
        "mcpServers",
        ServerShape::CommandArgs,
        gemini_user,
        gemini_project,
    )
}

pub(crate) fn opencode() -> Box<dyn ClientInstaller> {
    make(
        ClientId::Opencode,
        "mcp",
        ServerShape::LocalCommand,
        opencode_user,
        opencode_project,
    )
}

pub(crate) fn cursor() -> Box<dyn ClientInstaller> {
    make(
        ClientId::Cursor,
        "mcpServers",
        ServerShape::Stdio,
        cursor_user,
        cursor_project,
    )
}

pub(crate) fn vscode() -> Box<dyn ClientInstaller> {
    make(
        ClientId::VsCode,
        "servers",
        ServerShape::Stdio,
        vscode_user,
        vscode_project,
    )
}

fn claude_user(home: &Path) -> PathBuf {
    home.join(".claude.json")
}

fn claude_project(project: &Path) -> PathBuf {
    project.join(".mcp.json")
}

fn gemini_user(home: &Path) -> PathBuf {
    home.join(".gemini").join("settings.json")
}

fn gemini_project(project: &Path) -> PathBuf {
    project.join(".gemini").join("settings.json")
}

fn opencode_user(home: &Path) -> PathBuf {
    home.join(".config").join("opencode").join("opencode.json")
}

fn opencode_project(project: &Path) -> PathBuf {
    project.join("opencode.json")
}

fn cursor_user(home: &Path) -> PathBuf {
    home.join(".cursor").join("mcp.json")
}

fn cursor_project(project: &Path) -> PathBuf {
    project.join(".cursor").join("mcp.json")
}

fn vscode_user(home: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        return home
            .join("AppData")
            .join("Roaming")
            .join("Code")
            .join("User")
            .join("mcp.json");
    }
    #[cfg(target_os = "macos")]
    {
        return home
            .join("Library")
            .join("Application Support")
            .join("Code")
            .join("User")
            .join("mcp.json");
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        home.join(".config")
            .join("Code")
            .join("User")
            .join("mcp.json")
    }
}

fn vscode_project(project: &Path) -> PathBuf {
    project.join(".vscode").join("mcp.json")
}
