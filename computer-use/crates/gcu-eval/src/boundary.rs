//! Client classification and the single offline backend admission decision.
//!
//! Fake agents execute the fake backend in process. Every supplied backend
//! path requires the unavailable disposable provider, regardless of placement,
//! file name, legacy declarations, or a requested S3 scope.

use crate::runner::RunConfig;
use std::path::Path;

/// Printed when an external client would start without a disposable-desktop attestation.
pub const ATTEST_REQUIRED: &str = concat!(
    "refusing to start an external client without --attest-disposable-desktop: ",
    "real mode must not run on an ordinary host; ",
    "it needs a one-shot isolated desktop and egress control; ",
    "--tools and preflight are mitigations, not proof; ",
    "fixture-bound authorization and an OS sandbox are not implemented; ",
    "a file named eval-fake-agent is not the built-in fake agent"
);

/// Printed when the built-in fake agent would call an operator-supplied `gcu`.
pub const EXTERNAL_GCU_REFUSED: &str = "refusing an external gcu backend: provider_unavailable; --attest-disposable-desktop is not evidence; the built-in fake agent uses only its in-process backend";

/// Printed when a real client would run a red-team task.
///
/// `gcu` accepts `auto-deny` and rejects `auto-deny-sensitive`. `auto-deny`
/// denies every approval, so it is not a sensitive-only policy. Real red-team
/// tasks stay `not_run` instead of falling back to `auto-allow-session`.
pub(crate) const REAL_RED_TEAM_NOT_RUN: &str = "real red-team task is not_run: gcu accepts auto-allow-turn, auto-allow-session, auto-allow-always, and auto-deny, and rejects auto-deny-sensitive; auto-deny denies every action, so there is no sensitive-only deny policy";

/// Windows has no owner-only ACL on the eval temp root, so real mode stays off.
pub const WINDOWS_REAL_NOT_RUN: &str = "windows real mode is not_run: owner-only temporary ACL is not implemented; real mode needs an external isolation boundary (ADR 0017)";

/// Hidden argv that re-enters this binary as the built-in fake agent.
pub const INTERNAL_FAKE_AGENT: &str = "--gcu-eval-internal-fake-agent";

/// Hidden argv that re-enters this binary as the built-in fake gcu.
pub const INTERNAL_FAKE_GCU: &str = "--gcu-eval-internal-fake-gcu";

/// Real driver, or any client that was not selected as the built-in fake agent.
///
/// `grok_bin` is never consulted for a name match. A path is external even when
/// its file name is `eval-fake-agent`, including one stored in `builtin_fake_bin`.
pub(crate) fn launches_real_client(cfg: &RunConfig) -> bool {
    if cfg.real_driver {
        return true;
    }
    if builtin_fake_trusted(cfg) {
        return false;
    }
    cfg.grok_bin.is_some() || cfg.builtin_fake_bin.is_some()
}

/// `builtin_fake` re-executes this process, or names the cargo-built fake agent.
pub(crate) fn builtin_fake_trusted(cfg: &RunConfig) -> bool {
    if !cfg.builtin_fake {
        return false;
    }
    match &cfg.builtin_fake_bin {
        None => true,
        Some(path) => is_builtin_artifact(path, "eval-fake-agent"),
    }
}

/// Admit only the in-process fake implementation, never an executable path.
///
/// Both runner admission and both fake-agent entries use this decision before
/// any preparation, preflight, telemetry, or scripted behavior. A copied caller
/// cannot authorize a sibling: no filesystem property or marker is consulted.
/// A requested scoped engine also needs an external backend, even if its path
/// was omitted. It must not reach allocation as an ordinary fake run.
pub(crate) fn admit_fake_backend(
    external: Option<&Path>,
    scoped: bool,
) -> Result<(), &'static str> {
    if external.is_some() || scoped {
        Err(EXTERNAL_GCU_REFUSED)
    } else {
        Ok(())
    }
}

/// Regular file named `stem` in this process's cargo output directory.
///
/// Cargo puts test binaries in `deps/` and the package binaries next to that
/// directory. A symlink is not the artifact. This does not detect a hard link
/// that has replaced the artifact in that directory.
/// Used only for the explicit fake-client test slot, never backend admission.
pub(crate) fn is_builtin_artifact(path: &Path, stem: &str) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return false;
    }
    let mut expected = stem.to_string();
    if cfg!(windows) {
        expected.push_str(".exe");
    }
    if path.file_name().and_then(|name| name.to_str()) != Some(expected.as_str()) {
        return false;
    }
    let Some(parent) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) else {
        return false;
    };
    let Some(dir) = artifact_dir() else {
        return false;
    };
    same_directory(parent, &dir)
}

fn artifact_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    if dir.file_name().and_then(|name| name.to_str()) == Some("deps") {
        dir.parent().map(Path::to_path_buf)
    } else {
        Some(dir.to_path_buf())
    }
}

fn same_directory(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left_real), Ok(right_real)) => left_real == right_real,
        _ => left == right,
    }
}

/// Why this launch must not start, when the client is external or the driver is real.
pub(crate) fn real_launch_block(
    real_client: bool,
    attested: bool,
    red_team: bool,
) -> Option<&'static str> {
    if !real_client {
        return None;
    }
    if !attested {
        return Some(ATTEST_REQUIRED);
    }
    if red_team {
        return Some(REAL_RED_TEAM_NOT_RUN);
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::path::PathBuf;

    fn cfg() -> RunConfig {
        RunConfig::default()
    }

    #[test]
    fn a_path_named_eval_fake_agent_is_an_external_client() {
        let named = RunConfig {
            grok_bin: Some(PathBuf::from("/tmp/eval-fake-agent")),
            ..cfg()
        };
        assert!(launches_real_client(&named));
        assert_eq!(real_launch_block(true, false, false), Some(ATTEST_REQUIRED));
        let exe = RunConfig {
            grok_bin: Some(PathBuf::from("/tmp/eval-fake-agent.exe")),
            ..cfg()
        };
        assert!(launches_real_client(&exe));
    }

    #[test]
    fn builtin_fake_does_not_require_attestation_until_the_driver_is_real() {
        let builtin = RunConfig {
            builtin_fake: true,
            grok_bin: Some(PathBuf::from("/tmp/eval-fake-agent")),
            ..cfg()
        };
        assert!(!launches_real_client(&builtin));
        assert!(real_launch_block(false, false, false).is_none());
        let real = RunConfig {
            builtin_fake: true,
            real_driver: true,
            ..cfg()
        };
        assert!(launches_real_client(&real));
        assert!(real_launch_block(true, true, true).is_some());
    }

    #[test]
    fn fake_backend_admission_never_uses_declarations_or_artifact_placement() {
        let external = RunConfig {
            builtin_fake: true,
            gcu_bin: Some(PathBuf::from("/tmp/gcu")),
            builtin_gcu: false,
            ..cfg()
        };
        assert_eq!(
            admit_fake_backend(external.gcu_bin.as_deref(), false),
            Err(EXTERNAL_GCU_REFUSED)
        );
        let flagged = RunConfig {
            builtin_gcu: true,
            ..external.clone()
        };
        assert_eq!(
            admit_fake_backend(flagged.gcu_bin.as_deref(), false),
            Err(EXTERNAL_GCU_REFUSED)
        );
        let reexec = RunConfig {
            builtin_fake: true,
            builtin_gcu: true,
            gcu_bin: None,
            ..cfg()
        };
        assert!(admit_fake_backend(reexec.gcu_bin.as_deref(), false).is_ok());
        assert_eq!(admit_fake_backend(None, true), Err(EXTERNAL_GCU_REFUSED));
        let attested = RunConfig {
            attest_disposable_desktop: true,
            builtin_gcu: false,
            ..external
        };
        assert_eq!(
            admit_fake_backend(attested.gcu_bin.as_deref(), false),
            Err(EXTERNAL_GCU_REFUSED)
        );
        assert_eq!(
            admit_fake_backend(Some(Path::new("")), false),
            Err(EXTERNAL_GCU_REFUSED)
        );
    }

    #[test]
    fn builtin_path_outside_the_artifact_dir_is_an_external_client() {
        let wrapped = RunConfig {
            builtin_fake: true,
            builtin_fake_bin: Some(PathBuf::from("/tmp/eval-fake-agent")),
            ..cfg()
        };
        assert!(launches_real_client(&wrapped));
        assert_eq!(real_launch_block(true, false, false), Some(ATTEST_REQUIRED));
        assert_eq!(
            admit_fake_backend(Some(Path::new("/tmp/eval-fake-gcu")), false),
            Err(EXTERNAL_GCU_REFUSED)
        );
    }
}
