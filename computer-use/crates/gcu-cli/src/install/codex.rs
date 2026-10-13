//! Codex MCP server and `notify` chain.
//!
//! `codex mcp add` can preserve comments (probed on codex-cli 0.162.1) but it
//! cannot install the `notify` chain. MCP and `notify` are one `toml_edit`
//! transaction so a second CLI pass cannot rewrite the file. Project-scope MCP
//! is `.codex/config.toml` in the project. Codex ignores a project-local
//! `notify`, so that key always stays in the user config.

use std::path::PathBuf;

use super::error::InstallError;
use super::plan::{Plan, PlanAction};
use super::toml::{self, codex_user_config, edit_toml};
use super::{
    gcu_string, push_change, require_context, status_of, ClientId, ClientInstaller, ClientStatus,
    InstallCtx, Scope,
};

pub(crate) struct CodexInstaller;

impl ClientInstaller for CodexInstaller {
    fn id(&self) -> ClientId {
        ClientId::Codex
    }

    fn plan(&self, ctx: &InstallCtx<'_>) -> Result<Plan, InstallError> {
        require_context(ctx)?;
        let gcu = gcu_string(ctx)?;
        let mut files = Vec::new();
        match ctx.scope {
            Scope::User => {
                let path = codex_user_config(ctx.home);
                push_change(
                    &mut files,
                    edit_toml(&path, |doc| install_user(doc, &path, &gcu))?,
                );
            }
            Scope::Project => {
                let project = project_config(ctx);
                let user = codex_user_config(ctx.home);
                push_change(
                    &mut files,
                    edit_toml(&project, |doc| {
                        toml::install_mcp(doc, &project, &gcu, false)
                    })?,
                );
                push_change(
                    &mut files,
                    edit_toml(&user, |doc| toml::install_notify(doc, &user, &gcu))?,
                );
            }
        }
        Ok(Plan::new(
            ClientId::Codex,
            PlanAction::Install,
            files,
            Vec::new(),
        ))
    }

    fn plan_removal(&self, ctx: &InstallCtx<'_>) -> Result<Plan, InstallError> {
        require_context(ctx)?;
        let mut files = Vec::new();
        let user = codex_user_config(ctx.home);
        match ctx.scope {
            Scope::User => {
                push_change(&mut files, edit_toml(&user, |doc| remove_user(doc, &user))?);
            }
            Scope::Project => {
                let project = project_config(ctx);
                push_change(
                    &mut files,
                    edit_toml(&project, |doc| toml::remove_mcp(doc, &project))?,
                );
                // User-scope MCP shares this notify. Leave it while that entry exists.
                if !toml::mcp_present(&user)? {
                    push_change(
                        &mut files,
                        edit_toml(&user, |doc| toml::remove_notify(doc, &user))?,
                    );
                }
            }
        }
        Ok(Plan::new(
            ClientId::Codex,
            PlanAction::Uninstall,
            files,
            Vec::new(),
        ))
    }

    fn status(&self, ctx: &InstallCtx<'_>) -> Result<ClientStatus, InstallError> {
        let present = toml::mcp_present(&mcp_path(ctx))?;
        let plan = self.plan(ctx)?;
        Ok(status_of(present, present && plan.is_noop()))
    }
}

fn project_config(ctx: &InstallCtx<'_>) -> PathBuf {
    ctx.project_dir.join(".codex/config.toml")
}

fn mcp_path(ctx: &InstallCtx<'_>) -> PathBuf {
    match ctx.scope {
        Scope::User => codex_user_config(ctx.home),
        Scope::Project => project_config(ctx),
    }
}

fn install_user(
    doc: &mut toml_edit::DocumentMut,
    path: &std::path::Path,
    gcu: &str,
) -> Result<bool, InstallError> {
    let mcp = toml::install_mcp(doc, path, gcu, false)?;
    let notify = toml::install_notify(doc, path, gcu)?;
    Ok(mcp || notify)
}

fn remove_user(
    doc: &mut toml_edit::DocumentMut,
    path: &std::path::Path,
) -> Result<bool, InstallError> {
    let mcp = toml::remove_mcp(doc, path)?;
    let notify = toml::remove_notify(doc, path)?;
    Ok(mcp || notify)
}
