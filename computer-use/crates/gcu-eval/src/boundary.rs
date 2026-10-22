//! Whether a launch is allowed to start a real client.
//!
//! The fake agent is the only client that may run without an operator
//! attestation. Every other binary, and every real-driver launch, refuses
//! until `--attest-disposable-desktop` is set. The flag is an attestation,
//! not a confinement proof.

use std::path::Path;

/// Printed when a real client would start without a disposable-desktop attestation.
pub const ATTEST_REQUIRED: &str = concat!(
    "refusing to start a real client without --attest-disposable-desktop: ",
    "real mode must not run on an ordinary host; ",
    "it needs a one-shot isolated desktop and egress control; ",
    "--tools and preflight are mitigations, not proof; ",
    "fixture-bound authorization and an OS sandbox are not implemented"
);

/// Printed when a real client would run a red-team task.
///
/// `gcu` accepts `auto-deny` and rejects `auto-deny-sensitive`. `auto-deny`
/// denies every approval, so it is not a sensitive-only policy. Real red-team
/// tasks stay `not_run` instead of falling back to `auto-allow-session`.
pub(crate) const REAL_RED_TEAM_NOT_RUN: &str = "real red-team task is not_run: gcu accepts auto-allow-turn, auto-allow-session, auto-allow-always, and auto-deny, and rejects auto-deny-sensitive; auto-deny denies every action, so there is no sensitive-only deny policy";

/// True when `path` is the eval fake agent and not a symlink to something else.
///
/// A missing `eval-fake-agent` path still counts, so the runner can report
/// "client binary missing". A symlink with that name does not count.
pub fn is_eval_fake_agent(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|part| part.to_str()) else {
        return false;
    };
    if name != "eval-fake-agent" && name != "eval-fake-agent.exe" {
        return false;
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) => meta.is_file() && !meta.file_type().is_symlink(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

/// Real driver, or any client that is not the eval fake agent.
pub(crate) fn launches_real_client(real_driver: bool, grok: &Path) -> bool {
    real_driver || !is_eval_fake_agent(grok)
}

/// Real driver, or a configured client that is not the eval fake agent.
///
/// `None` is real only when the driver itself is real. Callers that already
/// have a path should use [`launches_real_client`].
pub(crate) fn client_is_real(real_driver: bool, grok: Option<&Path>) -> bool {
    match grok {
        Some(path) => launches_real_client(real_driver, path),
        None => real_driver,
    }
}

/// Why this launch must not start, when the client is real.
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

#[cfg(all(test, unix))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn scratch(label: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("gcu-boundary-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn symlink_named_like_the_fake_agent_is_a_real_client() {
        let dir = scratch("link");
        let real = dir.join("grok");
        std::fs::write(&real, b"x").unwrap();
        let link = dir.join("eval-fake-agent");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(!is_eval_fake_agent(&link));
        assert!(launches_real_client(false, &link));
        let file = dir.join("bin");
        std::fs::create_dir(&file).unwrap();
        let agent = file.join("eval-fake-agent");
        std::fs::write(&agent, b"x").unwrap();
        assert!(is_eval_fake_agent(&agent));
        assert!(!launches_real_client(false, &agent));
        assert!(launches_real_client(true, &agent));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
