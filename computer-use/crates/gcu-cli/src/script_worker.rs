//! Hidden `script-worker` child and the MCP runner that supervises it.
//!
//! The child speaks only the narrow frame codec. The parent dispatches each
//! `cu.*` call through [`gcu_script::dispatch`] while it already holds the
//! session queue. The watchdog calls [`std::process::Child::kill`], which is
//! SIGKILL on Unix, and does not enter QuickJS.

use crate::exit::Failure;
#[cfg(all(unix, feature = "script"))]
use std::os::unix::process::CommandExt;
use std::sync::Arc;
#[cfg(any(feature = "script", all(test, unix)))]
use std::{
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};

/// Installed only when this binary was built with the script feature.
pub(crate) fn runner() -> Option<Arc<dyn gcu_mcp::ScriptRunner>> {
    #[cfg(feature = "script")]
    {
        Some(Arc::new(ProcessRunner))
    }
    #[cfg(not(feature = "script"))]
    {
        None
    }
}

/// Child entry. With the feature off, this is an arguments error.
///
/// Blocking: the process, until the parent closes the pipes or the watchdog
/// kills it. Timeout: the start frame, capped at 30s. Cancellation: pipe close
/// or kill. No environment, path, token, or native handle is read.
pub(crate) async fn run() -> Result<u8, Failure> {
    #[cfg(feature = "script")]
    {
        let code = gcu_script::worker_main(std::io::stdin(), std::io::stdout());
        Ok(u8::try_from(code.max(0)).unwrap_or(1))
    }
    #[cfg(not(feature = "script"))]
    Err(Failure::arguments("script feature is not enabled"))
}

/// Kill `child` within one 10ms poll of `deadline`, unless `done` is set.
///
/// Blocking: this thread sleeps in 10ms slices. Timeout: `deadline`.
/// Cancellation: `done` returns without killing. Does not signal pid 0 or a
/// process group. On Unix, [`Child::kill`] is SIGKILL.
#[cfg(any(feature = "script", all(test, unix)))]
pub(crate) fn watch_child(child: Arc<Mutex<Child>>, deadline: Instant, done: &AtomicBool) {
    loop {
        if done.load(Ordering::Acquire) {
            return;
        }
        if Instant::now() >= deadline {
            kill_locked(&child);
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(any(feature = "script", all(test, unix)))]
fn kill_locked(child: &Mutex<Child>) {
    let mut guard = child.lock().unwrap_or_else(|err| err.into_inner());
    let _ = guard.kill();
}

#[cfg(feature = "script")]
struct ProcessRunner;

#[cfg(feature = "script")]
impl gcu_mcp::ScriptRunner for ProcessRunner {
    fn run<'a>(
        &'a self,
        router: &'a gcu_core::action::ActionRouter,
        session: &'a gcu_core::session::Session,
        call_id: &'a str,
        params: gcu_protocol::ComputerScriptParams,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = gcu_mcp::ScriptBody> + Send + 'a>> {
        Box::pin(async move {
            match session
                .enqueue(drive(router, session, call_id, params))
                .await
            {
                Ok(body) => body,
                Err(_) => tool_body(gcu_protocol::ErrorCode::StoppedByUser, "session closed"),
            }
        })
    }
}

#[cfg(feature = "script")]
enum ChildMsg {
    Request(gcu_script::Op),
    Finish(gcu_script::Frame),
    Dead,
}

#[cfg(feature = "script")]
async fn drive(
    router: &gcu_core::action::ActionRouter,
    session: &gcu_core::session::Session,
    call_id: &str,
    params: gcu_protocol::ComputerScriptParams,
) -> gcu_mcp::ScriptBody {
    if let Err(error) = params.validate() {
        return tool_body(gcu_protocol::ErrorCode::CapabilityUnavailable, error.0);
    }
    let timeout = Duration::from_millis(u64::from(params.timeout_ms).min(30_000));
    let deadline = Instant::now() + timeout;
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => return tool_body(gcu_protocol::ErrorCode::Internal, "worker executable"),
    };
    let mut command = Command::new(exe);
    command
        .arg("script-worker")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    limit_address_space(&mut command);
    let mut spawned = match command.spawn() {
        Ok(child) => child,
        Err(_) => return tool_body(gcu_protocol::ErrorCode::Internal, "worker start"),
    };
    let Some(input) = spawned.stdin.take() else {
        let _ = spawned.kill();
        let _ = spawned.wait();
        return tool_body(gcu_protocol::ErrorCode::Internal, "worker stdin");
    };
    let Some(output) = spawned.stdout.take() else {
        let _ = spawned.kill();
        let _ = spawned.wait();
        return tool_body(gcu_protocol::ErrorCode::Internal, "worker stdout");
    };
    let child = Arc::new(Mutex::new(spawned));
    let done = Arc::new(AtomicBool::new(false));
    let watch = {
        let child = Arc::clone(&child);
        let done = Arc::clone(&done);
        thread::Builder::new()
            .name("gcu-script-watchdog".to_owned())
            .spawn(move || watch_child(child, deadline, &done))
            .ok()
    };
    let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(8);
    let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
    let timeout_ms = params.timeout_ms.min(30_000);
    let code = params.code;
    let io = thread::Builder::new()
        .name("gcu-script-io".to_owned())
        .spawn(move || io_loop(input, output, events_tx, reply_rx, timeout_ms, code));
    let io = match io {
        Ok(handle) => handle,
        Err(_) => {
            done.store(true, Ordering::Release);
            kill_locked(&child);
            if let Some(handle) = watch {
                let _ = handle.join();
            }
            reap(&child);
            return tool_body(gcu_protocol::ErrorCode::Internal, "worker thread");
        }
    };
    let mut seq = 0u32;
    let body = loop {
        if Instant::now() >= deadline {
            break tool_body(gcu_protocol::ErrorCode::Timeout, "timeout");
        }
        if session.is_stopped() || session.is_closed() {
            break tool_body(gcu_protocol::ErrorCode::StoppedByUser, "stopped");
        }
        let wait = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(wait, events_rx.recv()).await {
            Ok(Some(ChildMsg::Request(op))) => {
                if session.is_stopped() || session.is_closed() || Instant::now() >= deadline {
                    let _ = reply_tx.send(gcu_script::Reply::stopped());
                    break tool_body(gcu_protocol::ErrorCode::StoppedByUser, "stopped");
                }
                let reply =
                    gcu_script::dispatch(router, session, &format!("{call_id}:{seq}"), op).await;
                seq = seq.saturating_add(1);
                if reply_tx.send(reply).is_err() {
                    break tool_body(gcu_protocol::ErrorCode::Internal, "worker pipe");
                }
            }
            Ok(Some(ChildMsg::Finish(frame))) => break body_from_finish(frame),
            Ok(Some(ChildMsg::Dead)) | Ok(None) => {
                break tool_body(gcu_protocol::ErrorCode::Internal, "worker exited");
            }
            Err(_) => break tool_body(gcu_protocol::ErrorCode::Timeout, "timeout"),
        }
    };
    done.store(true, Ordering::Release);
    kill_locked(&child);
    drop(reply_tx);
    let _ = io.join();
    if let Some(handle) = watch {
        let _ = handle.join();
    }
    reap(&child);
    body
}

#[cfg(feature = "script")]
fn io_loop(
    mut input: std::process::ChildStdin,
    mut output: std::process::ChildStdout,
    events: tokio::sync::mpsc::Sender<ChildMsg>,
    replies: std::sync::mpsc::Receiver<gcu_script::Reply>,
    timeout_ms: u32,
    code: String,
) {
    let start = gcu_script::Frame::Start { timeout_ms, code };
    if gcu_script::write_frame(&mut input, &start).is_err() {
        let _ = events.blocking_send(ChildMsg::Dead);
        return;
    }
    loop {
        match gcu_script::read_frame(&mut output) {
            Ok(gcu_script::Frame::Request(op)) => {
                if events.blocking_send(ChildMsg::Request(op)).is_err() {
                    return;
                }
                let Ok(reply) = replies.recv() else {
                    return;
                };
                if gcu_script::write_frame(&mut input, &gcu_script::Frame::Reply(reply)).is_err() {
                    let _ = events.blocking_send(ChildMsg::Dead);
                    return;
                }
            }
            Ok(frame @ gcu_script::Frame::Finish { .. }) => {
                let _ = events.blocking_send(ChildMsg::Finish(frame));
                return;
            }
            _ => {
                let _ = events.blocking_send(ChildMsg::Dead);
                return;
            }
        }
    }
}

#[cfg(feature = "script")]
fn body_from_finish(frame: gcu_script::Frame) -> gcu_mcp::ScriptBody {
    let gcu_script::Frame::Finish {
        status,
        output,
        error_code,
        error_message,
        ..
    } = frame
    else {
        return tool_body(gcu_protocol::ErrorCode::Internal, "worker frame");
    };
    if gcu_script::status_from_byte(status) == gcu_script::ScriptStatus::Done {
        return gcu_mcp::ScriptBody {
            output,
            error: None,
        };
    }
    let code = gcu_protocol::ErrorCode::ALL
        .into_iter()
        .find(|item| item.as_str() == error_code)
        .unwrap_or(gcu_protocol::ErrorCode::Internal);
    let message = if error_message.is_empty() {
        code.as_str().to_owned()
    } else {
        error_message
    };
    gcu_mcp::ScriptBody {
        output,
        error: Some(gcu_protocol::ToolError {
            code,
            message,
            hint: "Use fresh state. Do not replay an unknown outcome.".into(),
            retryable: false,
            state: None,
        }),
    }
}

#[cfg(feature = "script")]
fn tool_body(code: gcu_protocol::ErrorCode, message: &str) -> gcu_mcp::ScriptBody {
    gcu_mcp::ScriptBody {
        output: String::new(),
        error: Some(gcu_protocol::ToolError {
            code,
            message: message.to_owned(),
            hint: "Use fresh state. Do not replay an unknown outcome.".into(),
            retryable: false,
            state: None,
        }),
    }
}

#[cfg(feature = "script")]
fn reap(child: &Mutex<Child>) {
    let mut guard = child.lock().unwrap_or_else(|err| err.into_inner());
    let _ = guard.wait();
}

#[cfg(all(unix, feature = "script"))]
fn limit_address_space(command: &mut Command) {
    // SAFETY: `pre_exec` runs only in the forked child, before exec. The
    // closure calls `setrlimit`, which is async-signal-safe, and only changes
    // this process's address-space cap. It does not signal pid 0.
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: gcu_script::WORKER_AS_BYTES as libc::rlim_t,
                rlim_max: gcu_script::WORKER_AS_BYTES as libc::rlim_t,
            };
            // `setrlimit` is async-signal-safe. No allocation or lock.
            let rc = libc::setrlimit(libc::RLIMIT_AS, &limit);
            if rc == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn watchdog_kills_sleep_within_500ms_of_deadline() {
        let child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let child = Arc::new(Mutex::new(child));
        let done = AtomicBool::new(false);
        let start = Instant::now();
        watch_child(Arc::clone(&child), start + Duration::from_millis(40), &done);
        let elapsed = start.elapsed();
        let status = child
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .wait()
            .unwrap();
        assert!(elapsed >= Duration::from_millis(40), "{elapsed:?}");
        assert!(elapsed < Duration::from_millis(540), "{elapsed:?}");
        assert!(!status.success());
    }
}
