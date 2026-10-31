//! The single admission decision for every offline client/backend launch.
//!
//! Fake clients and backends run in process. Every supplied executable
//! path requires the unavailable disposable provider, regardless of placement,
//! file name, legacy declarations, or a requested S3 scope.

use crate::runner::RunConfig;
use std::path::Path;

/// Printed when an external client lacks the unavailable disposable provider.
pub const ATTEST_REQUIRED: &str = concat!(
    "refusing to start an external client: provider_unavailable; ",
    "--attest-disposable-desktop is not evidence; ",
    "real mode must not run on an ordinary host; ",
    "it needs a one-shot isolated desktop and egress control; ",
    "--tools and preflight are mitigations, not proof; ",
    "fixture-bound authorization and an OS sandbox are not implemented; ",
    "a file named eval-fake-agent is not the built-in fake agent"
);

/// Printed when any launch requests an external backend or scoped engine.
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

/// Real driver or any explicit external client, irrespective of fake selection.
pub(crate) fn launches_real_client(cfg: &RunConfig) -> bool {
    cfg.real_driver || cfg.grok_bin.is_some()
}

/// Admit only compiled, in-process implementations, never executable paths.
///
/// Runner, client resolution/command construction and standalone fake-agent
/// entries all use this decision. No location, name or declaration grants trust.
/// A scoped engine request also requires the unavailable disposable provider.
pub(crate) fn admit_launch(
    client: Option<&Path>,
    backend: Option<&Path>,
    scoped: bool,
    real_driver: bool,
) -> Result<(), &'static str> {
    if backend.is_some() || scoped {
        Err(EXTERNAL_GCU_REFUSED)
    } else if client.is_some() || real_driver {
        Err(ATTEST_REQUIRED)
    } else {
        Ok(())
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
    fn explicit_client_is_external_even_with_builtin_fake_selected() {
        let builtin = RunConfig {
            builtin_fake: true,
            grok_bin: Some(PathBuf::from("/tmp/eval-fake-agent")),
            ..cfg()
        };
        assert!(launches_real_client(&builtin));
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
            admit_launch(None, external.gcu_bin.as_deref(), false, false),
            Err(EXTERNAL_GCU_REFUSED)
        );
        let flagged = RunConfig {
            builtin_gcu: true,
            ..external.clone()
        };
        assert_eq!(
            admit_launch(None, flagged.gcu_bin.as_deref(), false, false),
            Err(EXTERNAL_GCU_REFUSED)
        );
        let in_process = RunConfig {
            builtin_fake: true,
            builtin_gcu: true,
            gcu_bin: None,
            ..cfg()
        };
        assert!(admit_launch(None, in_process.gcu_bin.as_deref(), false, false).is_ok());
        assert_eq!(
            admit_launch(None, None, true, false),
            Err(EXTERNAL_GCU_REFUSED)
        );
        let attested = RunConfig {
            attest_disposable_desktop: true,
            builtin_gcu: false,
            ..external
        };
        assert_eq!(
            admit_launch(None, attested.gcu_bin.as_deref(), false, false),
            Err(EXTERNAL_GCU_REFUSED)
        );
        assert_eq!(
            admit_launch(None, Some(Path::new("")), false, false),
            Err(EXTERNAL_GCU_REFUSED)
        );
    }

    #[test]
    fn explicit_paths_never_select_in_process_code() {
        let wrapped = RunConfig {
            builtin_fake: true,
            grok_bin: Some(PathBuf::from("/tmp/eval-fake-agent")),
            ..cfg()
        };
        assert!(launches_real_client(&wrapped));
        assert_eq!(
            admit_launch(wrapped.grok_bin.as_deref(), None, false, false),
            Err(ATTEST_REQUIRED)
        );
        assert_eq!(
            admit_launch(Some(Path::new("")), None, false, false),
            Err(ATTEST_REQUIRED)
        );
        assert!(admit_launch(None, None, false, false).is_ok());
        assert_eq!(real_launch_block(true, false, false), Some(ATTEST_REQUIRED));
        assert_eq!(
            admit_launch(None, Some(Path::new("/tmp/eval-fake-gcu")), false, false),
            Err(EXTERNAL_GCU_REFUSED)
        );
    }
}
