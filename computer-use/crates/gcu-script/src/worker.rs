//! In-process worker thread and the stdio worker process entry.

use std::{
    future::Future,
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
        Arc, Mutex,
    },
    task::{Context, Poll},
    thread,
    time::{Duration, Instant},
};

use crate::{
    limits::WALL_MS,
    rpc::{self, Frame, Op, Reply},
    runtime::{self, Shared, Slot, Wake},
    watchdog, ScriptReport, ScriptStatus,
};

/// Run `code` on a fresh worker thread.
///
/// `host` runs on the caller's async task, which must already hold the session
/// queue when `host` calls the router. Each `cu.*` waits here and is delivered
/// through `host`. Native concurrency is one. The 33rd queued admission is
/// rejected inside the worker without blocking the queue.
///
/// Blocking: waits for the script, `host`, and thread join. Timeout: `timeout`,
/// capped at 30s. Cancellation: `cancel`, the deadline, or dropping this future.
/// Dropping sets `cancel` so a host wait returns within 10ms and the JS
/// interrupt aborts a loop. Unknown and hard-stop replies are not sent again.
pub async fn run<F, Fut>(
    code: &str,
    timeout: Duration,
    cancel: Arc<AtomicBool>,
    mut host: F,
) -> ScriptReport
where
    F: FnMut(Op) -> Fut,
    Fut: Future<Output = Reply>,
{
    let timeout = timeout.min(Duration::from_millis(WALL_MS));
    let deadline = Instant::now() + timeout;
    let (tx, rx) = mpsc::sync_channel(32);
    // `Receiver` is `Send` but not `Sync`. The session queue requires this
    // future to be `Send`, so the receiver lives in a mutex and is locked only
    // inside `poll_op`, never across `.await`.
    let rx = Mutex::new(rx);
    let slot = Arc::new(Slot::default());
    let wake = Arc::new(Wake::default());
    let shared = Arc::new(Shared::channel(
        tx,
        slot.clone(),
        wake.clone(),
        cancel.clone(),
        deadline,
    ));
    let done = Arc::new(AtomicBool::new(false));
    let watchdog_done = done.clone();
    let watchdog_cancel = cancel.clone();
    let watchdog_wake = wake.clone();
    let watchdog = thread::Builder::new()
        .name("gcu-script-watchdog".to_owned())
        .spawn(move || {
            watchdog::supervise(
                deadline,
                &watchdog_done,
                &watchdog_cancel,
                &watchdog_wake,
                || {},
            );
        });
    let worker_shared = shared.clone();
    let worker_code = code.to_owned();
    let worker = thread::Builder::new()
        .name("gcu-script".to_owned())
        .spawn(move || runtime::execute(worker_shared, &worker_code));
    let worker = match worker {
        Ok(worker) => worker,
        Err(_) => {
            done.store(true, Ordering::Release);
            if let Ok(handle) = watchdog {
                let _ = handle.join();
            }
            return engine_report("worker thread");
        }
    };
    loop {
        match wait_op(&rx, &wake, &cancel, deadline).await {
            Wait::Op(op) => {
                if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                    slot.deliver(Reply::stopped());
                    break;
                }
                let reply = host(op).await;
                slot.deliver(reply);
            }
            Wait::Closed => break,
            Wait::Stop => {
                slot.deliver(Reply::stopped());
                break;
            }
        }
    }
    done.store(true, Ordering::Release);
    cancel.store(true, Ordering::Release);
    wake.wake();
    slot.poke();
    let report = match worker.join() {
        Ok(report) => report,
        Err(_) => engine_report("worker panicked"),
    };
    if let Ok(handle) = watchdog {
        let _ = handle.join();
    }
    report
}

enum Wait {
    Op(Op),
    Closed,
    Stop,
}

fn wait_op<'a>(
    rx: &'a Mutex<Receiver<Op>>,
    wake: &'a Wake,
    cancel: &'a AtomicBool,
    deadline: Instant,
) -> impl Future<Output = Wait> + 'a {
    std::future::poll_fn(move |cx: &mut Context<'_>| poll_op(rx, wake, cancel, deadline, cx))
}

fn poll_op(
    rx: &Mutex<Receiver<Op>>,
    wake: &Wake,
    cancel: &AtomicBool,
    deadline: Instant,
    cx: &mut Context<'_>,
) -> Poll<Wait> {
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Poll::Ready(Wait::Stop);
    }
    let guard = rx.lock().unwrap_or_else(|err| err.into_inner());
    match guard.try_recv() {
        Ok(op) => Poll::Ready(Wait::Op(op)),
        Err(TryRecvError::Disconnected) => Poll::Ready(Wait::Closed),
        Err(TryRecvError::Empty) => {
            wake.set(cx.waker().clone());
            match guard.try_recv() {
                Ok(op) => Poll::Ready(Wait::Op(op)),
                Err(TryRecvError::Disconnected) => Poll::Ready(Wait::Closed),
                Err(TryRecvError::Empty) => {
                    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                        Poll::Ready(Wait::Stop)
                    } else {
                        Poll::Pending
                    }
                }
            }
        }
    }
}

/// Child entry. Reads one start frame, runs the engine, writes one finish frame.
///
/// Blocking: this thread for the whole script. Timeout: the start frame's
/// timeout, capped at 30s. Cancellation: the parent closes the pipe or kills
/// the process. No environment, path, token, or native handle is read.
///
/// The return value is a process status: `0` when the finish frame was written.
pub fn worker_main(
    mut input: impl Read + Send + 'static,
    output: impl Write + Send + 'static,
) -> i32 {
    let start = match rpc::read_frame(&mut input) {
        Ok(Frame::Start { timeout_ms, code }) => (timeout_ms, code),
        _ => return 1,
    };
    let timeout = Duration::from_millis(u64::from(start.0).min(WALL_MS));
    let shared = Arc::new(Shared::stdio(
        input,
        output,
        Arc::new(AtomicBool::new(false)),
        Instant::now() + timeout,
    ));
    let report = runtime::execute(shared.clone(), &start.1);
    if shared.write_finish(&report) {
        0
    } else {
        1
    }
}

fn engine_report(message: &str) -> ScriptReport {
    ScriptReport {
        status: ScriptStatus::Error,
        output: String::new(),
        driver_calls: 0,
        error_code: Some("internal".to_owned()),
        error_message: Some(message.to_owned()),
    }
}
