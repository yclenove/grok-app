//! Whether a launch is allowed to start an external client.
//!
//! The built-in fake agent is selected by [`crate::runner::RunConfig::builtin_fake`],
//! not by the file name of a path. Every other executable, and every real-driver
//! launch, refuses until `--attest-disposable-desktop` is set. The flag is an
//! attestation, not a confinement proof. A same-named copy, hard link, or
//! wrapper is an external client.

use crate::runner::RunConfig;

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
pub const EXTERNAL_GCU_REFUSED: &str = "refusing an external gcu backend without --attest-disposable-desktop: the built-in fake agent may call only the built-in fake gcu unless the operator attests a disposable desktop";

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
/// its file name is `eval-fake-agent`.
pub(crate) fn launches_real_client(cfg: &RunConfig) -> bool {
    cfg.real_driver || (!cfg.builtin_fake && cfg.grok_bin.is_some())
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

/// Built-in fake agent plus an operator `gcu` path, without attestation.
pub(crate) fn external_backend_block(cfg: &RunConfig) -> Option<&'static str> {
    if cfg.attest_disposable_desktop || !cfg.builtin_fake {
        return None;
    }
    if cfg.gcu_bin.is_some() && !cfg.builtin_gcu {
        return Some(EXTERNAL_GCU_REFUSED);
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
    fn builtin_fake_refuses_an_external_gcu_without_attestation() {
        let external = RunConfig {
            builtin_fake: true,
            gcu_bin: Some(PathBuf::from("/tmp/gcu")),
            builtin_gcu: false,
            ..cfg()
        };
        assert_eq!(
            external_backend_block(&external),
            Some(EXTERNAL_GCU_REFUSED)
        );
        let allowed = RunConfig {
            builtin_gcu: true,
            ..external.clone()
        };
        assert!(external_backend_block(&allowed).is_none());
        let attested = RunConfig {
            attest_disposable_desktop: true,
            builtin_gcu: false,
            ..external.clone()
        };
        assert!(external_backend_block(&attested).is_none());
    }
}
