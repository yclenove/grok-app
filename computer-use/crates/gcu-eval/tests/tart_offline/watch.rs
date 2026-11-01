//! Parent of the harness-free tart offline binary.
//!
//! The child is this same executable, selected only when the parent passes the
//! private re-exec argument and sets [`MODE_ENV`]; the variable alone does not
//! enter a child mode. The parent polls `try_wait` with a short sleep on a
//! monotonic clock, starts no thread, and never calls the blocking
//! `Child::wait` (a failed kill would make it unbounded). A timed-out child is
//! a failure whatever status it shows afterwards: cleanup only tries to reap it.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Child mode environment value. Ignored unless [`REEXEC_ARG`] is also present.
/// Not a product switch.
pub(super) const MODE_ENV: &str = "GCU_TART_OFFLINE_CHILD";

/// Private argv marker set only by the parent's re-exec.
const REEXEC_ARG: &str = "--gcu-tart-offline-reexec";

const CHECKS: &str = "checks";
const HANG: &str = "hang";
const EXIT_SEVEN: &str = "exit7";

/// 30s for the checks child, not for the whole target (the hang and exit
/// probes and [`CLEANUP_LIMIT`] come on top).
const CHECK_LIMIT: Duration = Duration::from_secs(30);

/// Limit for the deliberately non-returning probe child.
const HANG_LIMIT: Duration = Duration::from_secs(2);

/// Extra monotonic budget to observe exit after kill, outside the limits above.
const CLEANUP_LIMIT: Duration = Duration::from_secs(5);

/// Pause between `try_wait` polls so supervision does not burn a core.
const POLL: Duration = Duration::from_millis(10);

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// The child exited by itself within its limit.
    Exited(i32),
    /// The limit passed. Always a failure; `reaped` says whether exit was seen.
    TimedOut { reaped: bool },
}

pub(super) fn entry(checks: fn() -> i32) -> i32 {
    match child_mode().as_deref() {
        Some(HANG) => loop {
            std::hint::spin_loop();
        },
        Some(EXIT_SEVEN) => std::process::exit(7),
        Some(CHECKS) => checks(),
        Some(other) => panic!("unknown tart offline child mode: {other}"),
        None => parent(),
    }
}

fn parent() -> i32 {
    assert!(
        select_child_mode(false, Some(CHECKS)).is_none()
            && select_child_mode(false, Some(HANG)).is_none()
            && select_child_mode(true, Some(CHECKS)) == Some(CHECKS)
            && select_child_mode(true, None).is_none(),
        "the child mode must need both the re-exec argument and the variable"
    );
    // The wrapper must kill and reap a child that never returns.
    assert_eq!(
        supervise(HANG, HANG_LIMIT),
        Outcome::TimedOut { reaped: true },
        "watchdog did not kill and reap the non-returning child"
    );
    // A failing child's status must reach this process unchanged.
    assert_eq!(supervise(EXIT_SEVEN, HANG_LIMIT), Outcome::Exited(7));
    match supervise(CHECKS, CHECK_LIMIT) {
        Outcome::Exited(code) => code,
        timed_out => panic!("tart offline checks exceeded their per-child limit: {timed_out:?}"),
    }
}

fn child_mode() -> Option<String> {
    let reexec = std::env::args_os().skip(1).any(|arg| arg == REEXEC_ARG);
    let mode = std::env::var(MODE_ENV).ok();
    match select_child_mode(reexec, mode.as_deref()) {
        Some(mode) => Some(mode.to_string()),
        None if reexec => panic!("tart offline reexec argument without {MODE_ENV}"),
        None => None,
    }
}

/// The variable selects a child only together with the re-exec argument.
fn select_child_mode(reexec: bool, env_mode: Option<&str>) -> Option<&str> {
    if reexec {
        env_mode
    } else {
        None
    }
}

fn supervise(mode: &str, limit: Duration) -> Outcome {
    let exe = std::env::current_exe().unwrap_or_else(|err| panic!("current_exe: {err}"));
    let mut child = Command::new(exe)
        .arg(REEXEC_ARG)
        .env(MODE_ENV, mode)
        .stdin(Stdio::null())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {mode} child: {err}"));
    if let Some(code) = poll(&mut child, limit) {
        return Outcome::Exited(code);
    }
    // Timed out, or polling failed. Kill, then only observe: this result is a
    // failure either way, so a late exit 0 cannot turn it into success.
    let _ = child.kill();
    Outcome::TimedOut {
        reaped: poll(&mut child, CLEANUP_LIMIT).is_some(),
    }
}

/// Exit code if the child exits within `limit`; `None` on timeout or error.
fn poll(child: &mut Child, limit: Duration) -> Option<i32> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status.code().unwrap_or(128)),
            Ok(None) if started.elapsed() < limit => std::thread::sleep(POLL),
            Ok(None) | Err(_) => return None,
        }
    }
}
