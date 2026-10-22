//! Client process lifetime.
//!
//! On Unix the child is placed in its own process group. Timeout, failure,
//! and drop signal that group. Stdout and stderr are read while the child
//! runs so a full pipe cannot turn into a false timeout.

use std::time::Duration;

pub(crate) struct PipeCapture {
    pub bytes: Vec<u8>,
    pub overflow: bool,
}

pub(crate) struct Finished {
    pub timed_out: bool,
    pub exit_code: Option<i32>,
    pub wait_failed: bool,
    pub stdout: PipeCapture,
}

struct GroupChild {
    child: tokio::process::Child,
    pid: Option<u32>,
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
        Ok(Self { child, pid })
    }

    fn kill_group(&self) {
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            crate::ffi::kill_process_group(pid);
        }
        #[cfg(not(unix))]
        {
            let _ = self.pid;
        }
    }
}

impl Drop for GroupChild {
    fn drop(&mut self) {
        self.kill_group();
        let _ = self.child.start_kill();
    }
}

/// Run `command` until it exits or `timeout` elapses.
///
/// Blocking: until exit, timeout, or cancellation. On timeout and on drop the
/// Unix process group is killed. Both pipes are drained concurrently.
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
        .map(|pipe| tokio::spawn(read_capped(pipe, stdout_limit)));
    let stderr_task = group
        .child
        .stderr
        .take()
        .map(|pipe| tokio::spawn(read_capped(pipe, stderr_limit)));
    let waited = tokio::time::timeout(timeout, group.child.wait()).await;
    let (timed_out, exit_code, wait_failed) = match waited {
        Ok(Ok(status)) => (false, status.code(), false),
        Ok(Err(_)) => {
            group.kill_group();
            let _ = group.child.start_kill();
            (false, None, true)
        }
        Err(_elapsed) => {
            group.kill_group();
            let _ = group.child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(2), group.child.wait()).await;
            (true, None, false)
        }
    };
    group.kill_group();
    let stdout = take_pipe(stdout_task).await;
    // Stderr is drained so a full pipe cannot stall the child. Its bytes are not kept.
    let _stderr = take_pipe(stderr_task).await;
    Ok(Finished {
        timed_out,
        exit_code,
        wait_failed,
        stdout,
    })
}

async fn take_pipe(handle: Option<tokio::task::JoinHandle<PipeCapture>>) -> PipeCapture {
    let Some(handle) = handle else {
        return PipeCapture {
            bytes: Vec::new(),
            overflow: false,
        };
    };
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or(PipeCapture {
            bytes: Vec::new(),
            overflow: true,
        })
}

async fn read_capped(mut pipe: impl tokio::io::AsyncRead + Unpin, limit: usize) -> PipeCapture {
    use tokio::io::AsyncReadExt;
    let mut kept = Vec::new();
    let mut overflow = false;
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => break,
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
    }
}
