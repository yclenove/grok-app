//! Approval parsing, host platform, and user-config path checks.
//!
//! These helpers do not read config files and do not launch a client.

use crate::Error;
use std::path::{Path, PathBuf};

/// `GCU_APPROVAL` values `gcu` accepts on an eval-autoapprove debug build.
///
/// `auto-deny-sensitive` is absent: `gcu-cli` rejects it. `auto-deny` denies
/// every approval, including ordinary ones, so it is not a sensitive-only policy.
pub const GCU_ACCEPTED_APPROVAL: &[&str] = &[
    "auto-allow-turn",
    "auto-allow-session",
    "auto-allow-always",
    "auto-deny",
];

/// CU approval values accepted only by a debug `eval-autoapprove` build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalMode {
    /// Allow the current turn.
    AllowTurn,
    /// Allow the current session.
    AllowSession,
    /// Allow for the session. The in-process host does not persist this.
    AllowAlways,
    /// Deny every approval. This is not limited to sensitive actions.
    Deny,
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
        "GCU_EVAL_ATTESTED",
        "GCU_EVAL_BUILTIN_GCU",
        "GCU_EVAL_GCU_REEXEC",
        "GCU_BIN",
        "GCU_APPROVAL",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
    ]
}

/// Proxy settings a real client needs to reach its model endpoint. They are network configuration,
/// not secrets, and are forwarded only when the launch can start a real client. A proxy URL with
/// credentials is the operator's own choice. The eval fake agent never receives them.
#[must_use]
pub const fn proxy_env_keys() -> &'static [&'static str] {
    &[
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
    ]
}

/// Personal config directories the harness must not seed from or provision inside.
///
/// The list covers `HOME` and `USERPROFILE`, the OS account home from
/// `getpwuid_r` or the Windows profile folder (not only the environment),
/// `GROK_HOME`, `CODEX_HOME`, and `GROK_APP_HOME`, absolute `GEMINI_*` and
/// `CLAUDE_*` values, and this app's `~/.grok-app` tree (`agent-home` and
/// `agent-home-official`). It does not open those directories. A path check
/// cannot prove that file bytes are an evaluation-only identity.
#[must_use]
pub fn user_config_dirs() -> Vec<PathBuf> {
    config_dirs_from(
        &std::env::vars_os().collect::<Vec<_>>(),
        crate::ffi::os_home_dir(),
    )
}

/// Build the exclusion list from an explicit environment and OS home.
///
/// `env` is `(name, value)` pairs. `os_home` is the account home, if known.
#[must_use]
pub fn config_dirs_from(
    env: &[(std::ffi::OsString, std::ffi::OsString)],
    os_home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut homes = Vec::new();
    for key in ["HOME", "USERPROFILE"] {
        if let Some(value) = env_value(env, key) {
            homes.push(PathBuf::from(value));
        }
    }
    if let Some(home) = os_home {
        homes.push(home);
    }
    let mut out = Vec::new();
    for home in homes {
        for name in [".grok", ".codex", ".gemini", ".claude"] {
            push_unique(&mut out, home.join(name));
        }
        let app = home.join(".grok-app");
        push_unique(&mut out, app.join("agent-home"));
        push_unique(&mut out, app.join("agent-home-official"));
        push_unique(&mut out, app);
    }
    for key in ["GROK_HOME", "CODEX_HOME", "GROK_APP_HOME"] {
        if let Some(value) = env_value(env, key) {
            let path = PathBuf::from(value);
            if key == "GROK_APP_HOME" {
                push_unique(&mut out, path.join("agent-home"));
                push_unique(&mut out, path.join("agent-home-official"));
            }
            push_unique(&mut out, path);
        }
    }
    for (key, value) in env {
        let name = key.to_string_lossy();
        if name.starts_with("GEMINI_") || name.starts_with("CLAUDE_") {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                push_unique(&mut out, path);
            }
        }
    }
    out
}

fn env_value(
    env: &[(std::ffi::OsString, std::ffi::OsString)],
    key: &str,
) -> Option<std::ffi::OsString> {
    env.iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
}

fn push_unique(out: &mut Vec<PathBuf>, path: PathBuf) {
    if !path.as_os_str().is_empty() && !out.iter().any(|item| item == &path) {
        out.push(path);
    }
}

/// True when `path` is `configs` or a descendant of one of them.
///
/// macOS and Windows compare components without case. Linux does not.
#[must_use]
pub fn path_under_configs(path: &Path, configs: &[PathBuf]) -> bool {
    configs
        .iter()
        .any(|dir| path_within(path, dir, case_insensitive_paths()))
}

/// True on platforms whose canonical paths are not case-sensitive.
#[must_use]
pub const fn case_insensitive_paths() -> bool {
    cfg!(windows) || cfg!(target_os = "macos")
}

/// True when `path` is `root` or a descendant.
///
/// `case_insensitive` folds ASCII case on normal and prefix components.
/// Root components stay exact. This does not touch the filesystem. The unit
/// test is not a live macOS or Windows filesystem proof.
#[must_use]
pub fn path_within(path: &Path, root: &Path, case_insensitive: bool) -> bool {
    let path: Vec<_> = path.components().collect();
    let root: Vec<_> = root.components().collect();
    if root.is_empty() || path.len() < root.len() {
        return false;
    }
    path.iter()
        .zip(root.iter())
        .all(|(left, right)| component_eq(left, right, case_insensitive))
}

fn component_eq(
    left: &std::path::Component<'_>,
    right: &std::path::Component<'_>,
    case_insensitive: bool,
) -> bool {
    use std::path::Component;
    match (left, right) {
        (Component::Normal(left), Component::Normal(right)) if case_insensitive => left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy()),
        (Component::Prefix(left), Component::Prefix(right)) if case_insensitive => left
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy()),
        _ => left == right,
    }
}

/// True when the canonical form of `path` is one of `configs` or a descendant.
///
/// Comparison uses canonical paths, so a symlink ancestor cannot hide a personal
/// config directory. Missing config directories are resolved through the longest
/// existing ancestor. macOS and Windows compare without case. This does not read
/// directory entries. Bind mounts are not detected.
pub(crate) fn canonical_under_configs(path: &Path, configs: &[PathBuf]) -> Result<bool, Error> {
    let path = canonical_prefix(path)?;
    let folded = case_insensitive_paths();
    for config in configs {
        let config = canonical_prefix(config)?;
        if path_within(&path, &config, folded) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn canonical_prefix(path: &Path) -> Result<PathBuf, Error> {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return Ok(canon);
    }
    let mut suffix = Vec::new();
    let mut cursor = path.to_path_buf();
    loop {
        if let Ok(canon) = std::fs::canonicalize(&cursor) {
            let mut out = canon;
            for part in suffix.iter().rev() {
                out.push(part);
            }
            return Ok(out);
        }
        let Some(name) = cursor.file_name().map(std::ffi::OsStr::to_os_string) else {
            return Err(Error::msg("could not canonicalize path"));
        };
        suffix.push(name);
        if !cursor.pop() {
            return Err(Error::msg("could not canonicalize path"));
        }
    }
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn path_within_folds_ascii_case_only_when_asked() {
        let root = Path::new("/Home/User/.grok");
        let child = Path::new("/home/user/.grok/auth.json");
        assert!(!path_within(child, root, false));
        assert!(path_within(child, root, true));
        assert!(path_within(root, root, true));
        assert!(!path_within(
            Path::new("/home/user/.grok-extra"),
            root,
            true
        ));
        assert!(!path_within(Path::new("/tmp/other"), root, true));
        assert!(!path_within(Path::new("/home/user"), root, true));
    }

    /// Absolute on every platform: a bare `/x` is only rooted, not absolute, on Windows.
    fn abs(path: &str) -> PathBuf {
        let root = if cfg!(windows) { "C:" } else { "" };
        PathBuf::from(format!("{root}{path}"))
    }

    #[test]
    fn config_dirs_cover_env_homes_os_home_and_absolute_overrides() {
        let os = |path: &str| abs(path).into_os_string();
        let env = vec![
            (OsString::from("HOME"), os("/envhome")),
            (OsString::from("USERPROFILE"), os("/profile")),
            (OsString::from("GROK_HOME"), os("/custom/grok")),
            (OsString::from("CODEX_HOME"), os("/custom/codex")),
            (OsString::from("GROK_APP_HOME"), os("/custom/app")),
            (OsString::from("GEMINI_HOME"), os("/abs/gemini")),
            (OsString::from("CLAUDE_CONFIG_DIR"), os("/abs/claude")),
            (
                OsString::from("GEMINI_RELATIVE"),
                OsString::from("relative/gemini"),
            ),
        ];
        let dirs = config_dirs_from(&env, Some(abs("/oshome")));
        let expected = [
            abs("/envhome").join(".grok"),
            abs("/envhome").join(".codex"),
            abs("/envhome").join(".gemini"),
            abs("/envhome").join(".claude"),
            abs("/envhome").join(".grok-app"),
            abs("/envhome").join(".grok-app").join("agent-home"),
            abs("/envhome")
                .join(".grok-app")
                .join("agent-home-official"),
            abs("/profile").join(".grok"),
            abs("/profile")
                .join(".grok-app")
                .join("agent-home-official"),
            abs("/oshome").join(".grok"),
            abs("/oshome").join(".grok-app").join("agent-home"),
            abs("/oshome").join(".grok-app").join("agent-home-official"),
            abs("/custom/grok"),
            abs("/custom/codex"),
            abs("/custom/app"),
            abs("/custom/app").join("agent-home"),
            abs("/custom/app").join("agent-home-official"),
            abs("/abs/gemini"),
            abs("/abs/claude"),
        ];
        for item in expected {
            assert!(
                dirs.iter().any(|path| path == &item),
                "{item:?} missing from {dirs:?}"
            );
        }
        assert!(
            !dirs.iter().any(|path| path == Path::new("relative/gemini")),
            "a relative GEMINI_* value is not a config root: {dirs:?}"
        );
    }
}
