//! Parent of the harness-free tart offline binary.
//!
//! The child is this same executable. The parent polls `try_wait` on a
//! monotonic clock and, at the deadline, kills and reaps the child. No extra
//! thread is started. The hang mode is only spawned by the parent `main`.

use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Set only on the re-exec'd child. Not a product switch.
pub(super) const MODE_ENV: &str = "GCU_TART_OFFLINE_CHILD";

pub(super) const CHECKS: &str = "checks";
pub(super) const HANG: &str = "hang";

/// ADR 0018 §8 ceiling for the check child.
pub(super) const CHECK_LIMIT: Duration = Duration::from_secs(30);

/// Short bound used only to prove this wrapper kills a child that does not
/// return. It stays under [`CHECK_LIMIT`].
pub(super) const HANG_LIMIT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(super) struct Supervised {
    pub(super) code: i32,
    pub(super) killed: bool,
}

pub(super) fn child_mode() -> Option<String> {
    std::env::var(MODE_ENV).ok()
}

pub(super) fn hang() -> ! {
    loop {
        std::hint::spin_loop();
    }
}

pub(super) fn supervise(mode: &str, limit: Duration) -> Supervised {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => panic!("current_exe: {err}"),
    };
    let mut child = match Command::new(exe)
        .env(MODE_ENV, mode)
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => panic!("spawn {mode} child: {err}"),
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Supervised {
                    code: exit_code(status),
                    killed: false,
                };
            }
            Ok(None) if started.elapsed() >= limit => return kill_and_reap(&mut child),
            Ok(None) => std::hint::spin_loop(),
            Err(err) => panic!("try_wait: {err}"),
        }
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

fn kill_and_reap(child: &mut Child) -> Supervised {
    match child.try_wait() {
        Ok(Some(status)) => {
            return Supervised {
                code: exit_code(status),
                killed: false,
            };
        }
        Ok(None) => {}
        Err(err) => panic!("try_wait before kill: {err}"),
    }
    let _ = child.kill();
    match child.wait() {
        Ok(status) => Supervised {
            code: status.code().unwrap_or(128),
            killed: true,
        },
        Err(err) => panic!("reap: {err}"),
    }
}
