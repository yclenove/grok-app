//! Approval parsing, host platform, and user-config path checks.
//!
//! These helpers do not read config files and do not launch a client.

use crate::Error;
use std::path::{Path, PathBuf};

/// CU approval values accepted only by a debug `eval-autoapprove` build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalMode {
    /// Allow the current turn.
    AllowTurn,
    /// Allow the current session.
    AllowSession,
    /// Allow for the session. The in-process host does not persist this.
    AllowAlways,
    /// Deny.
    Deny,
    /// Deny sensitive computer-use actions. Red-team tasks use this.
    DenySensitive,
}

impl ApprovalMode {
    /// Value assigned to `GCU_APPROVAL`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AllowTurn => "auto-allow-turn",
            Self::AllowSession => "auto-allow-session",
            Self::AllowAlways => "auto-allow-always",
            Self::Deny => "auto-deny",
            Self::DenySensitive => "auto-deny-sensitive",
        }
    }
}

/// Accept `GCU_APPROVAL` only when `eval_build` is true.
///
/// Release and non-`eval-autoapprove` builds must pass `eval_build = false`.
/// Any set value is then an error, including values that do not start with `auto-`.
/// Blocking: none.
pub fn parse_approval(raw: Option<&str>, eval_build: bool) -> Result<Option<ApprovalMode>, Error> {
    let Some(value) = raw else {
        return Ok(None);
    };
    if value.is_empty() {
        return Err(Error::Approval("GCU_APPROVAL must be valid text".into()));
    }
    if !eval_build {
        return Err(Error::Approval(
            "GCU_APPROVAL is forbidden in release or non eval-autoapprove builds".into(),
        ));
    }
    let mode = match value {
        "auto-allow-turn" => ApprovalMode::AllowTurn,
        "auto-allow-session" => ApprovalMode::AllowSession,
        "auto-allow-always" => ApprovalMode::AllowAlways,
        "auto-deny" => ApprovalMode::Deny,
        "auto-deny-sensitive" => ApprovalMode::DenySensitive,
        _ => return Err(Error::Approval("Unknown GCU_APPROVAL mode".into())),
    };
    Ok(Some(mode))
}

/// Platform name for the host this binary was compiled for.
#[must_use]
pub const fn host_platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// `eval/tasks` next to this crate, when the source tree is present.
#[must_use]
pub fn bundled_tasks_dir() -> Option<PathBuf> {
    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../eval/tasks");
    std::fs::canonicalize(candidate)
        .ok()
        .filter(|path| path.is_dir())
}

/// Environment keys the child is allowed to see. Anything else is dropped.
#[must_use]
pub const fn child_env_keys() -> &'static [&'static str] {
    &[
        "GROK_HOME",
        "HOME",
        "USERPROFILE",
        "TMPDIR",
        "TEMP",
        "TMP",
        "PATH",
        "LANG",
        "GCU_FIXTURE_STATE",
        "GCU_EVAL_TASK_ID",
        "GCU_EVAL_CATEGORY",
        "GCU_EVAL_PASS_SCRIPT",
        "GCU_EVAL_TOOL_LOG",
        "GCU_EVAL_SIDE_LOG",
        "GCU_EVAL_PROBE",
        "GCU_EVAL_BEHAVIOR",
        "GCU_EVAL_WATCH_FILE",
        "GCU_BIN",
        "GCU_APPROVAL",
    ]
}

/// `~/.grok`, `~/.codex`, `~/.gemini`, and `~/.claude` for the current user.
///
/// This reads `HOME` and `USERPROFILE` and does not open those directories.
#[must_use]
pub fn user_config_dirs() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        homes.push(PathBuf::from(home));
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        homes.push(PathBuf::from(home));
    }
    let mut out = Vec::new();
    for home in homes {
        for name in [".grok", ".codex", ".gemini", ".claude"] {
            out.push(home.join(name));
        }
    }
    out
}

/// True when `path` is `configs` or a descendant of one of them.
#[must_use]
pub fn path_under_configs(path: &Path, configs: &[PathBuf]) -> bool {
    configs.iter().any(|dir| path.starts_with(dir))
}

/// Drive `future` on a current-thread runtime.
///
/// Blocking: until `future` completes. Timeout and cancellation belong to the
/// future. Building the runtime fails as [`Error::Message`].
pub fn block_on<F: std::future::Future>(future: F) -> Result<F::Output, Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| Error::msg(format!("runtime: {err}")))?;
    Ok(runtime.block_on(future))
}
