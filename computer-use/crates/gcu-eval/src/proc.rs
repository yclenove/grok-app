//! Client process lifetime.
//!
//! On Unix the child is placed in its own process group. Timeout and drop
//! signal that group **before** the leader is reaped, then wait. After `wait`
//! the numeric pgid is forgotten and is not signaled again: a recycled id must
//! not be killed. A voluntary leader exit does not signal the group afterwards.
//! Descendants that leave the group (`setsid` / `setpgid`) are not owned; that
//! lifetime barrier is not implemented (ADR 0017).
//!
//! Stdout and stderr are read while the child runs so a full pipe cannot turn
//! into a false timeout. On timeout or cancellation the readers are aborted
//! and joined. A read error is incomplete capture, not EOF.

use std::time::Duration;

/// Bytes kept from one pipe.
pub(crate) struct PipeCapture {
    /// Prefix that fit in the cap.
    pub bytes: Vec<u8>,
    /// The pipe produced more than `bytes` could hold.
    pub overflow: bool,
    /// The read failed, or the reader was aborted before EOF.
    pub incomplete: bool,
}

impl PipeCapture {
    fn empty() -> Self {
        Self {
            bytes: Vec::new(),
            overflow: false,
            incomplete: false,
        }
    }

    fn abandoned() -> Self {
        Self {
            bytes: Vec::new(),
            overflow: false,
            incomplete: true,
        }
    }
}

pub(crate) struct Finished {
    pub timed_out: bool,
    pub exit_code: Option<i32>,
    pub wait_failed: bool,
    /// In-process worker was cancelled and did not stop within the join budget.
    ///
    /// The handle stays owned by the process. This attempt is not evidence.
    /// Real process waits leave this false.
    pub worker_unconfirmed: bool,
    pub stdout: PipeCapture,
    /// Stderr is discarded after the drain. Incomplete means the drain did not
    /// reach EOF, so the capture is not a full record of the child.
    pub capture_incomplete: bool,
}

struct GroupChild {
    child: tokio::process::Child,
    /// Leader pid while it has not been reaped. `None` after wait or after the
    /// one allowed pre-reap signal.
    pid: Option<u32>,
    reaped: bool,
}

impl GroupChild {
    fn spawn(command: &mut tokio::process::Command) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            let _ = command.process_group(0);
        }
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let child = command.spawn()?;
        let pid = child.id();
        Ok(Self {
            child,
            pid,
            reaped: false,
        })
    }

    /// Signal the group only while the leader id is still ours.
    fn kill_group_before_reap(&mut self) {
        let Some(pid) = self.pid.take() else {
            return;
        };
        if self.reaped {
            return;
        }
        #[cfg(unix)]
        crate::ffi::kill_process_group(pid);
        #[cfg(not(unix))]
        {
            let _ = pid;
        }
    }

    fn mark_reaped(&mut self) {
        self.reaped = true;
        self.pid = None;
    }
}

impl Drop for GroupChild {
    fn drop(&mut self) {
        // Cancellation: signal before anything reaps the leader, then forget
        // the id. Do not signal again from a later wait.
        self.kill_group_before_reap();
        let _ = self.child.start_kill();
        self.mark_reaped();
    }
}

struct AbortOnDrop(Option<tokio::task::JoinHandle<PipeCapture>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        let Some(handle) = self.0.take() else {
            return;
        };
        handle.abort();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = handle.await;
            });
        }
    }
}

/// Run `command` until it exits or `timeout` elapses.
///
/// Blocking: until exit, timeout, or cancellation. On timeout the Unix process
/// group is killed and then the leader is waited. Both pipes are drained
/// concurrently. Drop aborts those readers. A read error sets
/// [`Finished::capture_incomplete`].
pub(crate) async fn run_command(
    command: &mut tokio::process::Command,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<Finished, std::io::Error> {
    let mut group = GroupChild::spawn(command)?;
    let stdout_task = group
        .child
        .stdout
        .take()
        .map(|pipe| AbortOnDrop(Some(tokio::spawn(read_capped(pipe, stdout_limit)))));
    let stderr_task = group
        .child
        .stderr
        .take()
        .map(|pipe| AbortOnDrop(Some(tokio::spawn(read_capped(pipe, stderr_limit)))));
    let waited = tokio::time::timeout(timeout, group.child.wait()).await;
    let (timed_out, exit_code, wait_failed) = match waited {
        Ok(Ok(status)) => {
            // The leader is reaped. Do not signal its former pgid.
            group.mark_reaped();
            (false, status.code(), false)
        }
        Ok(Err(_)) => {
            group.kill_group_before_reap();
            let _ = group.child.start_kill();
            let _ = group.child.wait().await;
            group.mark_reaped();
            (false, None, true)
        }
        Err(_elapsed) => {
            group.kill_group_before_reap();
            let _ = group.child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(2), group.child.wait()).await;
            group.mark_reaped();
            (true, None, false)
        }
    };
    let stdout = finish_pipe(stdout_task).await;
    let stderr = finish_pipe(stderr_task).await;
    let capture_incomplete = stdout.incomplete || stderr.incomplete;
    Ok(Finished {
        timed_out,
        exit_code,
        wait_failed,
        worker_unconfirmed: false,
        stdout,
        capture_incomplete,
    })
}

async fn finish_pipe(handle: Option<AbortOnDrop>) -> PipeCapture {
    let Some(mut wrapper) = handle else {
        return PipeCapture::empty();
    };
    let Some(task) = wrapper.0.take() else {
        return PipeCapture::abandoned();
    };
    // Hold the join handle. Moving it into `timeout` detaches the task on
    // expiry, so a later abort would not run and the reader could outlive us.
    let mut task = task;
    match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
        Ok(Ok(capture)) => capture,
        Ok(Err(_)) => PipeCapture::abandoned(),
        Err(_elapsed) => {
            task.abort();
            match task.await {
                Ok(capture) => {
                    let mut capture = capture;
                    capture.incomplete = true;
                    capture
                }
                Err(_) => PipeCapture::abandoned(),
            }
        }
    }
}

async fn read_capped(mut pipe: impl tokio::io::AsyncRead + Unpin, limit: usize) -> PipeCapture {
    use tokio::io::AsyncReadExt;
    let mut kept = Vec::new();
    let mut overflow = false;
    let mut incomplete = false;
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) => break,
            Err(_) => {
                incomplete = true;
                break;
            }
            Ok(n) => {
                if kept.len() < limit {
                    let room = limit - kept.len();
                    let take = n.min(room);
                    kept.extend_from_slice(&buf[..take]);
                    if take < n {
                        overflow = true;
                    }
                } else {
                    overflow = true;
                }
            }
        }
    }
    PipeCapture {
        bytes: kept,
        overflow,
        incomplete,
    }
}

#[cfg(all(test, unix))]
#[path = "proc_tests.rs"]
mod tests;
