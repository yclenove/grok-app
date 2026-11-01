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

/// Which process an [`AdmittedLaunch`] would start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessRole {
    Client,
    Backend,
    Fixture,
}

/// Unforgeable permission to build one client, backend, or fixture process.
///
/// The fields are private. Production code obtains a value only from
/// [`admit_client`], [`admit_scoped_backend`], or [`admit_fixture`]. Those
/// functions run the single [`admit_launch`] decision and then the provider
/// gate. A scoped backend also requires that provider's ACK. No accepting
/// provider is installed, so every executable is refused. [`std_command`] and
/// [`tokio_command`] consume the value; nothing else may call `Command::new`
/// for a client or backend.
#[derive(Debug)]
pub struct AdmittedLaunch {
    program: std::path::PathBuf,
    role: ProcessRole,
}

/// External client process. Refused until a provider admits it.
pub(crate) fn admit_client(program: &Path) -> Result<AdmittedLaunch, &'static str> {
    admit_launch(Some(program), None, false, false)?;
    grant_process(ProcessRole::Client, program, false)
}

/// Scoped backend process. Refused until a provider admits it and ACKs.
pub(crate) fn admit_scoped_backend(program: &Path) -> Result<AdmittedLaunch, &'static str> {
    admit_launch(None, Some(program), true, false)?;
    grant_process(ProcessRole::Backend, program, true)
}

/// Fixture process used only by a real driver. Refused with that driver.
pub(crate) fn admit_fixture(program: &Path) -> Result<AdmittedLaunch, &'static str> {
    admit_launch(None, None, false, true)?;
    grant_process(ProcessRole::Fixture, program, false)
}

fn grant_process(
    role: ProcessRole,
    program: &Path,
    scoped: bool,
) -> Result<AdmittedLaunch, &'static str> {
    if !provider_gate_allows_process(scoped) {
        return Err(refusal_for(role));
    }
    Ok(AdmittedLaunch {
        program: program.to_path_buf(),
        role,
    })
}

/// Provider gate. A scoped engine additionally needs the provider's ACK.
///
/// Both halves stay closed: this crate does not link an accepting provider,
/// and no ACK is recorded. Callers cannot pass a boolean that skips the gate.
fn provider_gate_allows_process(scoped: bool) -> bool {
    accepting_provider_linked() && (!scoped || scoped_provider_ack())
}

fn accepting_provider_linked() -> bool {
    // Closed. Nothing in this crate stores `true`. The load is not a const so
    // the admit arm stays compiled for a future provider in this module.
    // Callers cannot pass a boolean that skips the gate.
    static LINKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    LINKED.load(std::sync::atomic::Ordering::Relaxed)
}

fn scoped_provider_ack() -> bool {
    static ACK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    ACK.load(std::sync::atomic::Ordering::Relaxed)
}

fn refusal_for(role: ProcessRole) -> &'static str {
    match role {
        ProcessRole::Client => ATTEST_REQUIRED,
        ProcessRole::Backend | ProcessRole::Fixture => EXTERNAL_GCU_REFUSED,
    }
}

/// The only `std::process::Command::new` for an admitted process.
pub(crate) fn std_command(launch: AdmittedLaunch) -> std::process::Command {
    std::process::Command::new(program_of(launch))
}

/// The only `tokio::process::Command::new` for an admitted process.
pub(crate) fn tokio_command(launch: AdmittedLaunch) -> tokio::process::Command {
    tokio::process::Command::new(program_of(launch))
}

fn program_of(launch: AdmittedLaunch) -> std::path::PathBuf {
    let AdmittedLaunch { program, role } = launch;
    // Every role is a process. The program was bound by admission or by the
    // test-only mint, not by the caller of `Command` construction.
    match role {
        ProcessRole::Client | ProcessRole::Backend | ProcessRole::Fixture => program,
    }
}

/// Harness-free S3 lifecycle only. Not an admission decision and not a provider.
///
/// `tests/scope_supervisor.rs` is outside this crate, so the capability it
/// needs to exercise ACK, renew, and teardown cannot be `cfg(test)`. It exists
/// only when the `test-support` feature is on, so no production build has a way
/// to mint a launch for an arbitrary executable. The source test rejects every
/// other caller. Production paths use [`admit_scoped_backend`].
#[cfg(feature = "test-support")]
#[doc(hidden)]
#[must_use]
pub fn mint_scope_lifecycle_test(program: std::path::PathBuf) -> AdmittedLaunch {
    AdmittedLaunch {
        program,
        role: ProcessRole::Backend,
    }
}

#[cfg(all(test, unix))]
pub(crate) fn mint_test_fixture(program: std::path::PathBuf) -> AdmittedLaunch {
    AdmittedLaunch {
        program,
        role: ProcessRole::Fixture,
    }
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

    #[test]
    fn process_capabilities_stay_refused_without_a_provider() {
        for path in ["eval-fake-agent", "eval-fake-gcu", "gcu-eval", ""] {
            let path = Path::new(path);
            assert!(admit_client(path)
                .unwrap_err()
                .contains("provider_unavailable"));
            assert!(admit_scoped_backend(path)
                .unwrap_err()
                .contains("provider_unavailable"));
            assert!(admit_fixture(path)
                .unwrap_err()
                .contains("provider_unavailable"));
        }
        assert!(!provider_gate_allows_process(false));
        assert!(!provider_gate_allows_process(true));
    }

    #[test]
    fn command_new_is_only_in_the_admission_module() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut command_hits = Vec::new();
        let mut lifecycle_hits = Vec::new();
        let mut fixture_hits = Vec::new();
        let mut tart_files = 0u32;
        let mut visit = |path: &Path, text: &str| {
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(path)
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let name = path
                .file_name()
                .and_then(|item| item.to_str())
                .unwrap_or("");
            let test_module = name == "tests.rs" || name.ends_with("_tests.rs");
            let admission = rel.ends_with("src/boundary.rs");
            // S11a lives under src/ and must stay inside this scan, including
            // any later test module. It cannot mint a launch or build a process.
            let tart = rel.starts_with("src/disposable/tart/");
            if tart {
                tart_files += 1;
                if text.contains("Command::new")
                    || text.contains("std::process::Command")
                    || text.contains("tokio::process::Command")
                    || text.contains("AdmittedLaunch")
                {
                    command_hits.push(rel.clone());
                }
            } else if !test_module
                && !admission
                && !rel.starts_with("tests/")
                && text.contains("Command::new")
            {
                command_hits.push(rel.clone());
            }
            let lifecycle = "mint_scope_lifecycle_test";
            if text.contains(lifecycle) && !admission && !rel.ends_with("tests/scope_supervisor.rs")
            {
                // `lib.rs` may re-export the name. A call there is still a hit.
                let reexport =
                    rel.ends_with("src/lib.rs") && !text.contains(&format!("{lifecycle}("));
                if !reexport {
                    lifecycle_hits.push(rel.clone());
                }
            }
            if text.contains("mint_test_fixture")
                && !admission
                && !rel.ends_with("src/prep_tests.rs")
            {
                fixture_hits.push(rel);
            }
        };
        walk(&root.join("src"), &mut visit);
        walk(&root.join("tests"), &mut visit);
        assert!(
            command_hits.is_empty(),
            "process launch outside the admission module: {command_hits:?}"
        );
        assert!(tart_files > 0, "tart interface module was not scanned");
        assert!(
            lifecycle_hits.is_empty(),
            "lifecycle mint escaped its test: {lifecycle_hits:?}"
        );
        // The mint and its re-export must not exist without the feature.
        let gated = |file: &str, needle: &str| {
            let text = std::fs::read_to_string(root.join(file)).unwrap();
            let at = text
                .find(needle)
                .unwrap_or_else(|| panic!("{file}: {needle}"));
            let mut from = at.saturating_sub(240);
            while !text.is_char_boundary(from) {
                from += 1;
            }
            text[from..at].contains("#[cfg(feature = \"test-support\")]")
        };
        assert!(
            gated("src/boundary.rs", "pub fn mint_scope_lifecycle_test"),
            "the lifecycle mint must be behind the test-support feature"
        );
        assert!(
            gated("src/lib.rs", "pub use boundary::mint_scope_lifecycle_test"),
            "the lifecycle mint re-export must be behind the test-support feature"
        );
        assert!(
            fixture_hits.is_empty(),
            "fixture mint escaped its test: {fixture_hits:?}"
        );
    }

    fn walk(dir: &Path, visit: &mut dyn FnMut(&Path, &str)) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, visit);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            visit(&path, &text);
        }
    }
}
