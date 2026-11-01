//! Parent of the harness-free tart offline binary.
//!
//! The child is this same executable. A child mode is selected only when the
//! parent passes a private re-exec argument and sets [`MODE_ENV`]. The
//! environment variable alone does not enter hang or checks.
//!
//! The parent polls `try_wait` on a monotonic clock. Each child has its own
//! [`CHECK_LIMIT`]. After kill, exit is observed with `try_wait` for
//! [`CLEANUP_LIMIT`], which is outside the 30s check budget. No extra thread
//! is started. `Child::wait` is not used: Drop does not reap, and a blocking
//! wait would make a failed kill unbounded.

use std::cell::Cell;
use std::io;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Child mode environment value. Ignored unless [`REEXEC_ARG`] is also present.
/// Not a product switch.
pub(super) const MODE_ENV: &str = "GCU_TART_OFFLINE_CHILD";

/// Private argv marker set only by the parent's re-exec.
const REEXEC_ARG: &str = "--gcu-tart-offline-reexec";

pub(super) const CHECKS: &str = "checks";
pub(super) const HANG: &str = "hang";

/// Exits 7 at once. The propagate probe must surface that status unchanged.
const EXIT_SEVEN: &str = "exit7";

/// Re-exec that supervises [`EXIT_SEVEN`] and returns its code through the
/// same `std::process::exit` as the outer parent.
const PROPAGATE: &str = "propagate";

/// 30s for each child process, not 30s for the whole tart_offline target.
///
/// ADR 0018 §8 is this per-child ceiling. The parent also runs the hang probe,
/// the nonzero-exit probe, and [`CLEANUP_LIMIT`] after a kill. Those are
/// outside this budget.
pub(super) const CHECK_LIMIT: Duration = Duration::from_secs(30);

/// Monotonic budget for observing exit after kill. Independent of
/// [`CHECK_LIMIT`]: a child may use the full 30s, and cleanup still gets this
/// extra window. It is not a second check budget.
const CLEANUP_LIMIT: Duration = Duration::from_secs(5);

/// Short bound used only to prove this wrapper kills a child that does not
/// return. It stays under [`CHECK_LIMIT`].
pub(super) const HANG_LIMIT: Duration = Duration::from_secs(2);

/// Pause between `try_wait` polls so supervision does not burn a core.
const POLL: Duration = Duration::from_millis(10);

/// Nonzero status used when exit was not observed. Not an OS exit code.
const UNCLEAN_CODE: i32 = 1;

#[derive(Debug)]
pub(super) struct Supervised {
    pub(super) code: i32,
    pub(super) killed: bool,
    /// Set when kill/reap did not observe exit. `killed` is then false.
    pub(super) unclean: bool,
}

pub(super) fn entry(checks: fn() -> i32) -> i32 {
    match child_mode().as_deref() {
        Some(HANG) => hang(),
        Some(EXIT_SEVEN) => std::process::exit(7),
        Some(PROPAGATE) => propagate_child_failure(),
        Some(CHECKS) => checks(),
        Some(other) => panic!("unknown tart offline child mode: {other}"),
        None => parent(),
    }
}

fn parent() -> i32 {
    if let Err(err) = assert_reexec_gate() {
        panic!("{err}");
    }
    if let Err(err) = assert_cleanup_stays_bounded() {
        panic!("{err}");
    }
    let hung = supervise(HANG, HANG_LIMIT);
    if hung.unclean || !hung.killed || hung.code == 0 {
        panic!(
            "watchdog did not kill the non-returning child: code {} killed {} unclean {}",
            hung.code, hung.killed, hung.unclean
        );
    }
    let propagated = supervise(PROPAGATE, HANG_LIMIT);
    if propagated.unclean || propagated.killed || propagated.code == 0 {
        panic!(
            "check child failure did not stay a nonzero parent status: code {} killed {} unclean {}",
            propagated.code, propagated.killed, propagated.unclean
        );
    }
    if propagated.code != 7 {
        panic!("expected propagated exit 7, got {}", propagated.code);
    }
    let ran = supervise(CHECKS, CHECK_LIMIT);
    if ran.unclean || ran.killed {
        panic!(
            "tart offline checks exceeded the per-child 30s limit: code {} killed {} unclean {}",
            ran.code, ran.killed, ran.unclean
        );
    }
    ran.code
}

fn propagate_child_failure() -> i32 {
    let failed = supervise(EXIT_SEVEN, HANG_LIMIT);
    if failed.unclean || failed.killed || failed.code == 0 {
        panic!(
            "exit probe was not a clean nonzero status: code {} killed {} unclean {}",
            failed.code, failed.killed, failed.unclean
        );
    }
    failed.code
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

/// `env_mode` selects a child only when `reexec` is set. The environment
/// variable alone must not bypass the wrapper.
fn select_child_mode(reexec: bool, env_mode: Option<&str>) -> Option<&str> {
    if reexec {
        env_mode
    } else {
        None
    }
}

pub(super) fn hang() -> ! {
    loop {
        std::hint::spin_loop();
    }
}

pub(super) fn supervise(mode: &str, limit: Duration) -> Supervised {
    let child = spawn(mode);
    supervise_with(OsChild(child), limit, Instant::now)
}

fn spawn(mode: &str) -> Child {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => panic!("current_exe: {err}"),
    };
    match Command::new(exe)
        .arg(REEXEC_ARG)
        .env(MODE_ENV, mode)
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => panic!("spawn {mode} child: {err}"),
    }
}

trait ChildProc {
    fn try_wait(&mut self) -> io::Result<Option<i32>>;
    fn kill(&mut self) -> io::Result<()>;
}

struct OsChild(Child);

impl ChildProc for OsChild {
    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        self.0.try_wait().map(|status| status.map(exit_code))
    }

    fn kill(&mut self) -> io::Result<()> {
        self.0.kill()
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(128)
}

fn supervise_with(
    mut child: impl ChildProc,
    limit: Duration,
    mut now: impl FnMut() -> Instant,
) -> Supervised {
    let started = now();
    loop {
        match child.try_wait() {
            Ok(Some(code)) => {
                return Supervised {
                    code,
                    killed: false,
                    unclean: false,
                };
            }
            Ok(None) if now().saturating_duration_since(started) >= limit => {
                return kill_and_reap(child, &mut now);
            }
            Ok(None) => std::thread::sleep(POLL),
            // Polling failed. Do not panic: unwinding would drop `Child`
            // without a cleanup attempt and without an unclean result.
            Err(_) => return kill_and_reap(child, &mut now),
        }
    }
}

fn kill_and_reap(mut child: impl ChildProc, now: &mut impl FnMut() -> Instant) -> Supervised {
    match child.try_wait() {
        Ok(Some(code)) => {
            return Supervised {
                code,
                killed: false,
                unclean: false,
            };
        }
        Ok(None) => {}
        Err(_) => {
            let kill_ok = child.kill().is_ok();
            return poll_until_exit(child, now, kill_ok);
        }
    }
    let kill_ok = child.kill().is_ok();
    poll_until_exit(child, now, kill_ok)
}

/// Observe exit until [`CLEANUP_LIMIT`]. `killed` is set only after an
/// observed exit whose `kill` returned `Ok`. A no-op or failed kill that never
/// shows an exit is unclean: Drop will not reap it, and this function does not
/// call a blocking wait.
fn poll_until_exit(
    mut child: impl ChildProc,
    now: &mut impl FnMut() -> Instant,
    kill_ok: bool,
) -> Supervised {
    let started = now();
    loop {
        let overdue = now().saturating_duration_since(started) >= CLEANUP_LIMIT;
        match child.try_wait() {
            Ok(Some(code)) => {
                return Supervised {
                    code,
                    killed: kill_ok,
                    unclean: false,
                };
            }
            Ok(None) | Err(_) if !overdue => std::thread::sleep(POLL),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                // Exit was not observed. Drop does not kill or reap; kill was
                // already attempted. The parent must not treat this as success.
                return Supervised {
                    code: UNCLEAN_CODE,
                    killed: false,
                    unclean: true,
                };
            }
        }
    }
}

fn assert_reexec_gate() -> Result<(), String> {
    if select_child_mode(false, Some(HANG)).is_some()
        || select_child_mode(false, Some(CHECKS)).is_some()
        || select_child_mode(false, Some(EXIT_SEVEN)).is_some()
    {
        return Err("environment variable alone bypasses the wrapper".into());
    }
    if select_child_mode(true, Some(CHECKS)) != Some(CHECKS) {
        return Err("reexec argument did not keep the child mode".into());
    }
    if select_child_mode(true, None).is_some() {
        return Err("reexec argument invented a child mode".into());
    }
    Ok(())
}

/// Synchronous stand-in for a child whose kill is a no-op or whose exit shows
/// up on a later poll. The clock jumps one second per read, so the 5s cleanup
/// budget returns without sleeping and without a thread. A loop that used the
/// 30s check budget instead would take many more steps.
fn assert_cleanup_stays_bounded() -> Result<(), String> {
    expect_unclean(true, false, "kill accepted but the child never exited")?;
    expect_unclean(false, false, "kill failed and the child never exited")?;
    expect_unclean(false, true, "try_wait failed")?;
    expect_delayed_exit()?;
    Ok(())
}

fn expect_unclean(kill_ok: bool, wait_error: bool, what: &str) -> Result<(), String> {
    let probe = Rc::new(Probe::new(kill_ok, wait_error, None));
    let clock = StepClock::new(Duration::from_secs(1));
    let result = drive(&probe, &clock);
    if !result.unclean || result.killed || result.code == 0 {
        return Err(format!(
            "{what}: expected unclean nonzero status, got code {} killed {} unclean {}",
            result.code, result.killed, result.unclean
        ));
    }
    if probe.kills.get() == 0 {
        return Err(format!("{what}: kill was not attempted"));
    }
    let steps = clock.ticks();
    let cleanup_steps = u32::try_from(CLEANUP_LIMIT.as_secs()).unwrap_or(u32::MAX);
    // A few samples before the cleanup window, plus one poll per cleanup second.
    // The 30s check budget is well above this.
    if steps > cleanup_steps + 6 {
        return Err(format!(
            "{what}: cleanup took {steps} clock steps, above the {}s cleanup budget",
            cleanup_steps
        ));
    }
    Ok(())
}

fn expect_delayed_exit() -> Result<(), String> {
    let probe = Rc::new(Probe::new(true, false, Some((2, 9))));
    let clock = StepClock::new(Duration::from_secs(1));
    let result = drive(&probe, &clock);
    if result.unclean || !result.killed || result.code != 9 {
        return Err(format!(
            "delayed exit: code {} killed {} unclean {}",
            result.code, result.killed, result.unclean
        ));
    }
    if probe.kills.get() == 0 {
        return Err("delayed exit: kill was not attempted".into());
    }
    let steps = clock.ticks();
    let cleanup_steps = u32::try_from(CLEANUP_LIMIT.as_secs()).unwrap_or(u32::MAX);
    if steps > cleanup_steps + 6 {
        return Err(format!(
            "delayed exit waited {steps} clock steps; exit should be observed inside cleanup"
        ));
    }
    Ok(())
}

fn drive(probe: &Rc<Probe>, clock: &StepClock) -> Supervised {
    let child = FakeChild {
        probe: Rc::clone(probe),
    };
    supervise_with(child, Duration::ZERO, || clock.now())
}

struct StepClock {
    origin: Instant,
    step: Duration,
    ticks: Cell<u32>,
}

impl StepClock {
    fn new(step: Duration) -> Self {
        Self {
            origin: Instant::now(),
            step,
            ticks: Cell::new(0),
        }
    }

    fn now(&self) -> Instant {
        let tick = self.ticks.get();
        self.ticks.set(tick.saturating_add(1));
        self.origin + self.step.saturating_mul(tick)
    }

    fn ticks(&self) -> u32 {
        self.ticks.get()
    }
}

struct Probe {
    kills: Cell<u32>,
    kill_ok: bool,
    wait_error: bool,
    /// Post-kill `try_wait` count (1-based) that reports this code.
    exit_at: Option<(u32, i32)>,
    armed: Cell<bool>,
    after_kill_polls: Cell<u32>,
}

impl Probe {
    fn new(kill_ok: bool, wait_error: bool, exit_at: Option<(u32, i32)>) -> Self {
        Self {
            kills: Cell::new(0),
            kill_ok,
            wait_error,
            exit_at,
            armed: Cell::new(false),
            after_kill_polls: Cell::new(0),
        }
    }
}

struct FakeChild {
    probe: Rc<Probe>,
}

impl ChildProc for FakeChild {
    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        if self.probe.wait_error {
            return Err(io::Error::other("injected try_wait failure"));
        }
        if !self.probe.armed.get() {
            return Ok(None);
        }
        let poll = self.probe.after_kill_polls.get().saturating_add(1);
        self.probe.after_kill_polls.set(poll);
        if let Some((at, code)) = self.probe.exit_at {
            if poll >= at {
                return Ok(Some(code));
            }
        }
        Ok(None)
    }

    fn kill(&mut self) -> io::Result<()> {
        self.probe
            .kills
            .set(self.probe.kills.get().saturating_add(1));
        self.probe.armed.set(true);
        if self.probe.kill_ok {
            Ok(())
        } else {
            Err(io::Error::other("injected kill failure"))
        }
    }
}
