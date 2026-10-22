//! Default and custom source roots for Codex, Claude Code, and Cursor.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::types::Provider;
use crate::process_util;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredRoot {
    pub provider: Provider,
    pub kind: String,
    pub path: String,
    pub exists: bool,
    pub readable: bool,
}

/// Path list only. Does not stat or list the directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootCandidate {
    pub provider: Provider,
    pub kind: &'static str,
    pub path: PathBuf,
}

pub fn default_root_candidates() -> Vec<RootCandidate> {
    let home = process_util::user_home();
    let codex = std::env::var("CODEX_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    let claude = std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"));
    vec![
        RootCandidate {
            provider: Provider::Codex,
            kind: "codex-home",
            path: codex,
        },
        RootCandidate {
            provider: Provider::ClaudeCode,
            kind: "claude-config",
            path: claude,
        },
        RootCandidate {
            provider: Provider::Cursor,
            kind: "cursor-user",
            path: cursor_user_dir(),
        },
        RootCandidate {
            provider: Provider::Cursor,
            kind: "cursor-home",
            path: home.join(".cursor"),
        },
    ]
}

pub fn default_roots() -> Vec<DiscoveredRoot> {
    discover_default_roots(default_root_candidates)
}

fn discover_default_roots(candidates: impl FnOnce() -> Vec<RootCandidate>) -> Vec<DiscoveredRoot> {
    // An explicit opt-out must also isolate optimized builds. Preserve the
    // existing presence-based switch and avoid even resolving default paths.
    if std::env::var_os("GROK_DISABLE_DEFAULT_ROOTS").is_some() {
        return Vec::new();
    }
    candidates()
        .into_iter()
        .map(|candidate| probe_root(candidate.provider, candidate.kind, &candidate.path))
        .collect()
}

pub fn cursor_user_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("Cursor").join("User");
        }
        process_util::user_home()
            .join("AppData")
            .join("Roaming")
            .join("Cursor")
            .join("User")
    }
    #[cfg(target_os = "macos")]
    {
        process_util::user_home()
            .join("Library")
            .join("Application Support")
            .join("Cursor")
            .join("User")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let xdg = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| process_util::user_home().join(".config"));
        xdg.join("Cursor").join("User")
    }
}

pub fn probe_root(provider: Provider, kind: &str, path: &Path) -> DiscoveredRoot {
    let exists = path.exists();
    let readable = exists && std::fs::read_dir(path).is_ok();
    DiscoveredRoot {
        provider,
        kind: kind.into(),
        path: path.to_string_lossy().into_owned(),
        exists,
        readable,
    }
}

pub fn current_platform() -> String {
    if cfg!(target_os = "windows") {
        "windows".into()
    } else if cfg!(target_os = "macos") {
        "macos".into()
    } else {
        "linux".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roots_opt_out_is_profile_independent() {
        const CASE: &str = "GROK_DETECT_ROOTS_CONTRACT_CASE";
        const ROOT: &str = "GROK_DETECT_ROOTS_CONTRACT_ROOT";
        if let Ok(case) = std::env::var(CASE) {
            let root = PathBuf::from(std::env::var_os(ROOT).expect("synthetic root"));
            let resolved = std::cell::Cell::new(false);
            let roots = discover_default_roots(|| {
                resolved.set(true);
                vec![RootCandidate {
                    provider: Provider::GenericFile,
                    kind: "synthetic-contract",
                    path: root.clone(),
                }]
            });
            if case == "unset" {
                assert!(resolved.get());
                assert_eq!(roots.len(), 1);
                assert_eq!(roots[0].path, root.to_string_lossy());
                assert!(roots[0].exists && roots[0].readable);
            } else {
                assert!(
                    !resolved.get(),
                    "disabled discovery resolved candidate paths"
                );
                assert!(roots.is_empty());
            }
            return;
        }

        let root = std::env::temp_dir().join(format!("grok-detect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let child =
            "session_migration::detect::tests::default_roots_opt_out_is_profile_independent";
        for (case, value) in [("unset", None), ("one", Some("1")), ("zero", Some("0"))] {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", child, "--test-threads=1", "--nocapture"])
                .env(CASE, case)
                .env(ROOT, &root)
                .env_remove("GROK_DISABLE_DEFAULT_ROOTS");
            if let Some(value) = value {
                command.env("GROK_DISABLE_DEFAULT_ROOTS", value);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "root opt-out contract failed for {case}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
        }
        std::fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn default_root_candidates_cover_three_providers_without_listing() {
        let roots = default_root_candidates();
        assert!(roots
            .iter()
            .any(|r| r.provider == Provider::Codex && r.kind == "codex-home"));
        assert!(roots
            .iter()
            .any(|r| r.provider == Provider::ClaudeCode && r.kind == "claude-config"));
        assert!(roots
            .iter()
            .any(|r| r.provider == Provider::Cursor && r.kind == "cursor-user"));
        assert!(roots
            .iter()
            .any(|r| r.provider == Provider::Cursor && r.kind == "cursor-home"));
    }

    #[test]
    fn probe_root_reads_only_the_directory_it_is_given() {
        let dir = std::env::temp_dir().join(format!(
            "probe-root-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let found = probe_root(Provider::GenericFile, "temp", &dir);
        assert!(found.exists && found.readable, "{found:?}");
        let missing = dir.join("missing-child");
        let absent = probe_root(Provider::GenericFile, "temp", &missing);
        assert!(!absent.exists && !absent.readable, "{absent:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
