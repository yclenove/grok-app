//! Sandboxed `computer_script` runtime bound to the core action router.
//!
//! The JS engine lives here. `gcu-mcp` does not depend on this crate. Every
//! `cu.*` driver call goes through [`dispatch`], which uses the same
//! `execute_enqueued` / `observe_enqueued` entry as a single tool and as batch.

#![deny(missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod bindings;
mod limits;
mod report;
mod route;
mod rpc;
mod runtime;
mod watchdog;
mod worker;

pub use limits::{
    Admit, CallGate, QueueFull, HOST_BUFFER_BYTES, OUTPUT_MAX_BYTES, WATCHDOG_KILL_MS,
    WORKER_AS_BYTES,
};
pub use report::status_from_byte;
pub use route::{dispatch, outcome_to_response};
pub use rpc::{
    decode_payload, encode_payload, read_frame, write_frame, CodecError, ElementView, Frame, Op,
    Reply, StateView,
};
pub use worker::{run, worker_main};

use std::time::Instant;

/// Poll until `deadline`, then run `on_deadline`.
///
/// This is the worker watchdog. It does not enter QuickJS. Production process
/// supervision signals the child from the same kind of deadline callback.
///
/// Blocking: this thread sleeps in 10ms slices until `deadline`. Timeout: the
/// deadline. Cancellation: none. `on_deadline` runs at most 500ms after the
/// deadline on a responsive scheduler.
pub fn run_watchdog_until(deadline: Instant, on_deadline: impl FnOnce()) {
    let done = AtomicBool::new(false);
    let cancel = AtomicBool::new(false);
    let wake = runtime::Wake::default();
    watchdog::supervise(deadline, &done, &cancel, &wake, on_deadline);
}

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use gcu_core::{action::ActionRouter, session::Session};
use gcu_protocol::ComputerScriptParams;

/// How a script stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptStatus {
    /// The script returned or emitted a value.
    Done,
    /// A byte, heap, stack, call, queue, or output ceiling was hit.
    Limit,
    /// The wall clock fired.
    Timeout,
    /// Stop, close, or a hard router error. Later driver calls were not sent.
    Stopped,
    /// The source or engine failed before a normal return.
    Error,
}

/// Result of one script. Unknown router outcomes are not replayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptReport {
    /// Why the run ended.
    pub status: ScriptStatus,
    /// Emits, or the return value, or the last state text when nothing was emitted.
    pub output: String,
    /// Driver calls that were admitted. `emit` and `find` are not included.
    pub driver_calls: u32,
    /// Wire error code, when the run did not finish cleanly.
    pub error_code: Option<String>,
    /// Short error text. Never a handle, path, or token.
    pub error_message: Option<String>,
}

/// Run a script against a live router.
///
/// The whole script holds [`Session::enqueue`]. Observations use
/// [`ActionRouter::observe_enqueued`] and actions use
/// [`ActionRouter::execute_enqueued`]. A second `enqueue` is not taken.
/// Native calls keep the 5 second ceiling. The script wall clock is at most
/// 30 seconds. Stop, session close, and `cancel` abort the worker. After an
/// unknown outcome or a hard stop, further `cu.*` calls do not reach the router.
///
/// Blocking: the session queue, then the script. Timeout: `params.timeout_ms`
/// capped at 30s; each native call still caps at 5s. Cancellation: Stop,
/// session close, `cancel`, or dropping this future.
pub async fn run_router(
    router: &ActionRouter,
    session: &Session,
    call_id: &str,
    params: ComputerScriptParams,
    cancel: Arc<AtomicBool>,
) -> ScriptReport {
    if let Err(error) = params.validate() {
        return ScriptReport {
            status: ScriptStatus::Limit,
            output: String::new(),
            driver_calls: 0,
            error_code: Some("capability_unavailable".to_owned()),
            error_message: Some(error.0.to_owned()),
        };
    }
    let timeout = Duration::from_millis(u64::from(params.timeout_ms));
    let code = params.code;
    let call_id = call_id.to_owned();
    let seq = Arc::new(AtomicU32::new(0));
    let cancel_for_host = cancel.clone();
    let joined = session
        .enqueue(async {
            run(&code, timeout, cancel.clone(), move |op| {
                let call_id = call_id.clone();
                let seq = seq.clone();
                let cancel = cancel_for_host.clone();
                async move {
                    if session.is_closed() || session.is_stopped() || cancel.load(Ordering::Acquire)
                    {
                        cancel.store(true, Ordering::Release);
                        return Reply::stopped();
                    }
                    let n = seq.fetch_add(1, Ordering::Relaxed);
                    dispatch(router, session, &format!("{call_id}:{n}"), op).await
                }
            })
            .await
        })
        .await;
    match joined {
        Ok(report) => report,
        Err(_) => ScriptReport {
            status: ScriptStatus::Stopped,
            output: String::new(),
            driver_calls: 0,
            error_code: Some("stopped_by_user".to_owned()),
            error_message: Some("session closed".to_owned()),
        },
    }
}
