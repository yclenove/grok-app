//! macOS engine signature checks that run before any `codesign -dvvv` field is trusted.
//!
//! Display text is not evidence. A tampered file, a broken signature, or a
//! self-signed certificate with the same team string can still print
//! `Identifier` / `TeamIdentifier` / `CDHash`. Those fields are parsed only
//! after `codesign --verify --strict` succeeds. In release mode a Developer ID
//! identity must also satisfy an Apple-anchored team requirement built from
//! `ReleaseIdentity::MacDeveloperId`. Developer mode does not add that
//! requirement: ad-hoc and identifier-only engines keep the existing dev grant.
//! Validity failure still drops the display text there, so a forged team is
//! not treated as the release identity.
//!
//! `--deep` walks nested helpers and frameworks. It applies only when the
//! candidate path is a directory (an `.app` bundle). The bundled engine is a
//! single Mach-O with no nested code; `--deep` is defined for bundles, so a
//! file is checked with `--strict` alone. That is the same validity check for
//! the bytes that will be executed, not a weaker one.
//!
//! The requirement is one argv element, `-R=anchor apple generic and ...`.
//! The attached `=` form makes codesign read the rest as a requirement
//! expression. `-R` followed by a separate bare expression is read as a
//! filename, and `-R==expr` is a requirement syntax error (checked on macOS
//! 15.8.1 with an ad-hoc signed file; see the `requirement_syntax_*` test).
//! No shell is involved, and the path is its own argument.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use gcu_host_protocol::supervisor::{
    parse_codesign_display, CodesignFacts, HostBuild, ReleaseIdentity,
};

/// Bound for one `codesign` child. Three checks (verify, requirement, display)
/// can therefore take at most 15 seconds. Each overrun kills that child.
pub(super) const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Retained combined stdout and stderr. Extra bytes are discarded so a full
/// pipe cannot stall the child past the deadline, and a truncated display is
/// not parsed.
const OUTPUT_CAP: usize = 64 * 1024;

/// Metadata categories only. Paths and command output are not part of this error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProbeError {
    /// The child exited non-zero, or its output was truncated.
    Failed,
    /// The child was still running at the deadline and was killed.
    TimedOut,
    /// The program could not be spawned.
    Unavailable,
}

/// One signature question. Callers must not infer identity from a failure.
///
/// Blocking: each method may wait up to its own timeout. Cancellation: none
/// beyond killing a timed-out child. Display text is untrusted until verify
/// (and, in release, the team requirement) has succeeded.
pub(super) trait SignatureProbe {
    /// `codesign --verify --strict`, plus `--deep` for a bundle directory.
    fn verify(&self, path: &Path) -> Result<(), ProbeError>;

    /// Release Developer ID chain. `requirement` is one expression, not a shell line.
    fn test_requirement(&self, path: &Path, requirement: &str) -> Result<(), ProbeError>;

    /// Raw `codesign -dvvv` text, including stderr. Ignored unless verification succeeded.
    fn display(&self, path: &Path) -> Result<String, ProbeError>;
}

/// Apple Developer ID chain for `team_id`. `None` when the team id could change
/// the requirement syntax; the caller then fails closed and does not spawn.
pub(super) fn developer_id_requirement(team_id: &str) -> Option<String> {
    if team_id.is_empty()
        || team_id.len() > 32
        || !team_id.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(format!(
        "anchor apple generic and certificate leaf[subject.OU] = \"{team_id}\""
    ))
}

/// Args for a validity check. The path is always the last element.
pub(super) fn verify_argv(path: &Path) -> Vec<OsString> {
    let mut args = verify_body(path);
    args.push(path.as_os_str().to_os_string());
    args
}

/// Args for a release team check. `-R=...` is a single element; the path follows it.
pub(super) fn requirement_argv(path: &Path, requirement: &str) -> Vec<OsString> {
    let mut args = verify_body(path);
    args.push(OsString::from(format!("-R={requirement}")));
    args.push(path.as_os_str().to_os_string());
    args
}

/// Args for display. Parsed only after verification has succeeded.
pub(super) fn display_argv(path: &Path) -> Vec<OsString> {
    vec![OsString::from("-dvvv"), path.as_os_str().to_os_string()]
}

fn verify_body(path: &Path) -> Vec<OsString> {
    let mut args = vec![OsString::from("--verify"), OsString::from("--strict")];
    if path.is_dir() {
        args.push(OsString::from("--deep"));
    }
    args
}

/// Facts that may be copied into admission. Any failed check yields empty facts.
///
/// Blocking and cancellation follow `SignatureProbe`. Developer mode skips the
/// Apple team requirement so an identifier-only dev engine is unchanged. It
/// still returns empty facts when validity itself fails.
pub(super) fn trusted_facts<P: SignatureProbe>(
    path: &Path,
    identity: &ReleaseIdentity,
    mode: HostBuild,
    probe: &P,
) -> CodesignFacts {
    if probe.verify(path).is_err() {
        return empty_facts();
    }
    if mode == HostBuild::Release {
        if let ReleaseIdentity::MacDeveloperId { team_id, .. } = identity {
            let Some(requirement) = developer_id_requirement(team_id) else {
                return empty_facts();
            };
            if probe.test_requirement(path, &requirement).is_err() {
                return empty_facts();
            }
        }
    }
    match probe.display(path) {
        Ok(text) => parse_codesign_display(&text),
        Err(_) => empty_facts(),
    }
}

pub(super) fn empty_facts() -> CodesignFacts {
    CodesignFacts {
        signing_id: None,
        team_id: None,
        cdhash: None,
    }
}

/// Non-macOS hosts have no `codesign`. Every question fails closed.
#[cfg(not(target_os = "macos"))]
pub(super) struct AbsentProbe;

#[cfg(not(target_os = "macos"))]
impl SignatureProbe for AbsentProbe {
    fn verify(&self, _path: &Path) -> Result<(), ProbeError> {
        Err(ProbeError::Unavailable)
    }

    fn test_requirement(&self, _path: &Path, _requirement: &str) -> Result<(), ProbeError> {
        Err(ProbeError::Unavailable)
    }

    fn display(&self, _path: &Path) -> Result<String, ProbeError> {
        Err(ProbeError::Unavailable)
    }
}

/// Live `/usr/bin/codesign`. The absolute path is intentional: `PATH` is not searched.
#[cfg(target_os = "macos")]
pub(super) struct OsProbe;

#[cfg(target_os = "macos")]
impl SignatureProbe for OsProbe {
    fn verify(&self, path: &Path) -> Result<(), ProbeError> {
        run_bounded(
            Path::new("/usr/bin/codesign"),
            &verify_argv(path),
            CHECK_TIMEOUT,
            false,
        )
        .map(|_| ())
    }

    fn test_requirement(&self, path: &Path, requirement: &str) -> Result<(), ProbeError> {
        run_bounded(
            Path::new("/usr/bin/codesign"),
            &requirement_argv(path, requirement),
            CHECK_TIMEOUT,
            false,
        )
        .map(|_| ())
    }

    fn display(&self, path: &Path) -> Result<String, ProbeError> {
        run_bounded(
            Path::new("/usr/bin/codesign"),
            &display_argv(path),
            CHECK_TIMEOUT,
            true,
        )
    }
}

/// Run `program` with `args` and no shell. Kills the child at `timeout`.
///
/// When `keep_text` is false, captured bytes are dropped and only the exit
/// status matters. Output past the cap is discarded; if text was requested,
/// truncation is `ProbeError::Failed` so a partial display cannot be parsed.
/// The error is only a category: spawn failures and command text are not returned.
#[cfg(target_os = "macos")]
pub(super) fn run_bounded(
    program: &Path,
    args: &[OsString],
    timeout: Duration,
    keep_text: bool,
) -> Result<String, ProbeError> {
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::Instant;

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return Err(ProbeError::Unavailable),
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_handle = thread::spawn(move || read_capped(stdout));
    let stderr_handle = thread::spawn(move || read_capped(stderr));
    let deadline = Instant::now() + timeout;
    let finished = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(ProbeError::TimedOut);
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(ProbeError::Failed);
            }
        }
    };
    let stdout = stdout_handle.join();
    let stderr = stderr_handle.join();
    let status = finished?;
    if !status.success() {
        return Err(ProbeError::Failed);
    }
    if !keep_text {
        return Ok(String::new());
    }
    match (stdout, stderr) {
        (Ok(out), Ok(err)) if !out.truncated && !err.truncated => {
            let mut text = out.text;
            text.push_str(&err.text);
            Ok(text)
        }
        _ => Err(ProbeError::Failed),
    }
}

#[cfg(target_os = "macos")]
struct ReadOutcome {
    text: String,
    truncated: bool,
}

#[cfg(target_os = "macos")]
fn read_capped(pipe: Option<impl std::io::Read>) -> ReadOutcome {
    let mut pipe = match pipe {
        Some(pipe) => pipe,
        None => {
            return ReadOutcome {
                text: String::new(),
                truncated: false,
            };
        }
    };
    let mut kept = Vec::new();
    let mut buf = [0_u8; 4096];
    let mut truncated = false;
    loop {
        match std::io::Read::read(&mut pipe, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if kept.len() >= OUTPUT_CAP {
                    truncated = true;
                    continue;
                }
                let room = OUTPUT_CAP - kept.len();
                if n > room {
                    kept.extend_from_slice(&buf[..room]);
                    truncated = true;
                } else {
                    kept.extend_from_slice(&buf[..n]);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    ReadOutcome {
        text: String::from_utf8_lossy(&kept).into_owned(),
        truncated,
    }
}
