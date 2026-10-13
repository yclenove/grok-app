//! Install and uninstall the `computer-use` MCP entry.
//!
//! [`ClientInstaller::plan`] and [`ClientInstaller::plan_removal`] only read.
//! They are the `--dry-run` path: call [`Plan::render_diff`] and write nothing.
//! [`ClientInstaller::apply`] and [`ClientInstaller::uninstall`] are synchronous.
//! They block on filesystem IO and, for Antigravity user scope, on the injected
//! [`CommandExecutor`] until that executor finishes or times out. There is no
//! cancellation token. A noop plan does not prompt, back up, or write.
//!
//! Writers, after each client's `--help` and a temporary-home probe where that
//! probe could not touch a real config:
//!
//! - Grok Build 1.0.52 `grok mcp add` strips TOML comments, so MCP is edited
//!   with `toml_edit`. The Stop hook is a file under `<home>/.grok/hooks`
//!   (ADR 0001). Any real change there asks for confirmation first.
//! - Codex 0.162.1 `codex mcp add` cannot chain `notify`, so MCP and `notify`
//!   are one edit. Project-scope `notify` still goes in the user config.
//! - Claude Code 2.1.296 reserves the name `computer-use` and rewrites
//!   `~/.claude.json` even when add fails, so the JSON file is edited directly.
//! - Antigravity user scope calls `agy mcp add` / `agy mcp remove`. Project
//!   scope has no scope flag, so `.agents/mcp_config.json` is JSON.
//! - Gemini CLI, OpenCode, Cursor, and VS Code are JSON edits. An unparseable
//!   file, including OpenCode JSONC, is left untouched.
//!
//! Backups are `<filename>.gcu-backup-<stamp>` next to the original, written
//! only when a real change or external CLI write is about to happen.

mod antigravity;
mod codex;
mod error;
mod exec;
mod fsutil;
mod grok;
mod json;
mod json_clients;
mod plan;
mod toml;

pub use error::InstallError;
pub use exec::{
    ClientBins, CommandExecutor, CommandOutput, CommandSpec, Confirmer, ProcessExecutor,
    RecordingExecutor, StaticConfirmer,
};
pub use plan::{ApplyOutcome, ClientStatus, FileChange, Plan, PlanAction, PlannedCommand};

use std::path::Path;

use error::InstallError as Error;
use plan::ClientStatus as Status;

/// MCP entry name owned by this installer. No other key is added or removed.
pub const SERVER_NAME: &str = "computer-use";

/// Client whose MCP registration this library can edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientId {
    /// Grok Build. User config `<home>/.grok/config.toml`, project `<project>/.grok/config.toml`.
    Grok,
    /// Codex. User `<home>/.codex/config.toml`, project `<project>/.codex/config.toml`.
    Codex,
    /// Claude Code. User `<home>/.claude.json`, project `<project>/.mcp.json`.
    Claude,
    /// Antigravity. User scope calls `agy`; project scope edits `<project>/.agents/mcp_config.json`.
    Antigravity,
    /// Gemini CLI. `<home>/.gemini/settings.json` or `<project>/.gemini/settings.json`.
    Gemini,
    /// OpenCode. `<home>/.config/opencode/opencode.json` or `<project>/opencode.json`.
    Opencode,
    /// Cursor. `<home>/.cursor/mcp.json` or `<project>/.cursor/mcp.json`.
    Cursor,
    /// VS Code. User profile `mcp.json`, or `<project>/.vscode/mcp.json`.
    VsCode,
}

impl ClientId {
    /// Stable name used in diffs and errors.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Grok => "grok",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Antigravity => "antigravity",
            Self::Gemini => "gemini",
            Self::Opencode => "opencode",
            Self::Cursor => "cursor",
            Self::VsCode => "vscode",
        }
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Where the MCP entry is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The injected home directory.
    User,
    /// The injected project directory. Grok's Stop hook and Codex `notify` still use the home.
    Project,
}

impl std::fmt::Display for Scope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::User => "user",
            Self::Project => "project",
        })
    }
}

/// Everything an installer needs, with no ambient home or process environment.
///
/// Paths are absolute so tests can point them at a temporary directory. The
/// library never reads the process home and never sets process-global env vars.
#[derive(Clone, Copy)]
pub struct InstallCtx<'a> {
    /// Absolute home. Client files live under this directory, not `std::env::home_dir`.
    pub home: &'a Path,
    /// Absolute project root for project-scoped files and Antigravity's working directory.
    pub project_dir: &'a Path,
    /// Absolute `gcu` executable registered as the MCP command.
    pub gcu_exe: &'a Path,
    /// User or project scope.
    pub scope: Scope,
    /// When true and `yes` is false, ask `confirmer` before changing `<home>/.grok`.
    pub interactive: bool,
    /// Skip the `~/.grok` confirmation. `yes` wins over `interactive`.
    pub yes: bool,
    /// Backup suffix. `None` uses the unix time in seconds. Must not contain separators.
    pub backup_stamp: Option<&'a str>,
    /// Answers the `~/.grok` prompt. Ignored when the plan is a noop or `yes` is set.
    pub confirmer: &'a dyn Confirmer,
    /// Spawns client CLIs. Blocking, bounded by the implementation's timeout.
    pub executor: &'a dyn CommandExecutor,
    /// Optional client binaries. Only Antigravity user scope reads `agy`.
    pub bins: ClientBins<'a>,
}

/// One client's install, uninstall, dry-run, and status.
///
/// `plan` and `plan_removal` do not write, prompt, or spawn. `apply` and
/// `uninstall` do. All methods block the caller for filesystem IO. `apply` of
/// an Antigravity user-scope plan also blocks inside [`CommandExecutor::run`].
pub trait ClientInstaller: Send + Sync {
    /// Client this installer edits.
    fn id(&self) -> ClientId;

    /// Read-only install plan. Does not create directories.
    fn plan(&self, ctx: &InstallCtx<'_>) -> Result<Plan, InstallError>;

    /// Read-only uninstall plan. Does not create directories.
    fn plan_removal(&self, ctx: &InstallCtx<'_>) -> Result<Plan, InstallError>;

    /// Read-only view of whether `computer-use` is already registered.
    ///
    /// Blocking filesystem reads. Does not write. A file that cannot be parsed
    /// returns [`InstallError::Unparseable`] or [`InstallError::UnexpectedShape`].
    fn status(&self, ctx: &InstallCtx<'_>) -> Result<ClientStatus, InstallError>;

    /// Back up, write `plan`, then run its commands.
    ///
    /// Blocking. Prompts before a real change under `<home>/.grok` unless `yes`
    /// is set. A declined or missing confirmation writes nothing.
    fn apply(&self, ctx: &InstallCtx<'_>) -> Result<ApplyOutcome, InstallError> {
        let plan = self.plan(ctx)?;
        fsutil::apply_plan(ctx, &plan)
    }

    /// Back up, apply `plan_removal`, then run its commands.
    ///
    /// Blocking, with the same `~/.grok` confirmation rules as [`Self::apply`].
    fn uninstall(&self, ctx: &InstallCtx<'_>) -> Result<ApplyOutcome, InstallError> {
        let plan = self.plan_removal(ctx)?;
        fsutil::apply_plan(ctx, &plan)
    }
}

/// Installer for `id`. The concrete types stay private; callers use the trait.
#[must_use]
pub fn installer(id: ClientId) -> Box<dyn ClientInstaller> {
    match id {
        ClientId::Grok => Box::new(grok::GrokInstaller),
        ClientId::Codex => Box::new(codex::CodexInstaller),
        ClientId::Antigravity => Box::new(antigravity::AntigravityInstaller),
        ClientId::Claude => json_clients::claude(),
        ClientId::Gemini => json_clients::gemini(),
        ClientId::Opencode => json_clients::opencode(),
        ClientId::Cursor => json_clients::cursor(),
        ClientId::VsCode => json_clients::vscode(),
    }
}

pub(crate) fn require_context(ctx: &InstallCtx<'_>) -> Result<(), Error> {
    if !ctx.gcu_exe.is_absolute() {
        return Err(Error::GcuPathNotAbsolute);
    }
    if !ctx.home.is_absolute() {
        return Err(Error::PathNotAbsolute { what: "home" });
    }
    if !ctx.project_dir.is_absolute() {
        return Err(Error::PathNotAbsolute {
            what: "project_dir",
        });
    }
    Ok(())
}

pub(crate) fn gcu_string(ctx: &InstallCtx<'_>) -> Result<String, Error> {
    ctx.gcu_exe
        .to_str()
        .map(str::to_owned)
        .ok_or(Error::PathNotUnicode { what: "gcu" })
}

pub(crate) fn status_of(present: bool, matches_gcu: bool) -> Status {
    let mut notes = Vec::new();
    if present && !matches_gcu {
        notes.push("computer-use is present but does not match this gcu executable".to_owned());
    }
    Status {
        present,
        matches_gcu,
        notes,
    }
}

pub(crate) fn push_change(files: &mut Vec<plan::FileChange>, change: Option<plan::FileChange>) {
    if let Some(change) = change {
        files.push(change);
    }
}
