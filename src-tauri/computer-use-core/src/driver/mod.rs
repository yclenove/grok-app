//! App-owned Cua private worker, pinned to one reviewed protocol revision.
//! The model sees Broker tools only; this channel has no socket or reconnect.
mod process;
pub mod wire;

#[cfg(feature = "test-support")]
pub use process::pid_is_running;

use crate::execution::ActionCancellation;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wire::{
    Completion, Handshake, Request, RequestCancellation, Response, WorkerError, BUILD_IDENTITY,
    MAX_REQUEST, MAX_RESPONSE, WIRE_VERSION,
};

const INITIALIZE_RUN_ID: &str = "worker-initialize";
const BIND_RUN_ID: &str = "worker-bind";

struct Exchange {
    line: Vec<u8>,
    reply: mpsc::SyncSender<Result<Vec<u8>, String>>,
}

struct Outbound<'a> {
    operation: &'a str,
    name: Option<&'a str>,
    arguments: Value,
    run_id: &'a str,
    session: Option<&'a str>,
    cancellation: &'a ActionCancellation,
    timeout: Duration,
}

pub struct WorkerOptions {
    /// Must be resolved and integrity-checked by the App runtime installer.
    pub binary: PathBuf,
    /// Immutable manifest written by the Host after a concrete scope grant.
    pub manifest: PathBuf,
    pub host_bundle_id: String,
    pub startup_timeout: Duration,
}

pub struct PrivateWorker {
    process: Arc<process::Process>,
    generation: String,
    handshake: Handshake,
    requests: mpsc::SyncSender<Exchange>,
    admission: Mutex<()>,
    next_id: AtomicU64,
    revoked: AtomicBool,
}

impl PrivateWorker {
    pub fn start(options: WorkerOptions) -> Result<Self, WorkerError> {
        let bad =
            |message: String| WorkerError::new(Completion::NotStarted, "worker_start", message);
        if !options.binary.is_absolute()
            || !options.manifest.is_absolute()
            || options.host_bundle_id.trim().is_empty()
        {
            return Err(bad(
                "absolute binary/manifest paths and Host identity required".into(),
            ));
        }
        std::fs::metadata(&options.manifest).map_err(|e| bad(e.to_string()))?;
        let generation = uuid::Uuid::new_v4().to_string();
        let mut child = process::command(&options.binary, &generation)
            .spawn()
            .map_err(|e| bad(e.to_string()))?;
        let pid = child.id();
        let mut stdin = child.stdin.take().expect("piped stdin");
        let mut stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        let process = Arc::new(process::Process::own(child).map_err(bad)?);
        let (requests, rx) = mpsc::sync_channel::<Exchange>(1);
        let reader_process = process.clone();
        std::thread::Builder::new()
            .name("computer-use-driver-io".into())
            .spawn(move || {
                while let Ok(exchange) = rx.recv() {
                    let result = (|| {
                        stdin.write_all(&exchange.line).map_err(|e| e.to_string())?;
                        stdin.flush().map_err(|e| e.to_string())?;
                        read_frame(&mut stdout)
                    })();
                    let failed = result.is_err();
                    let _ = exchange.reply.send(result);
                    if failed {
                        break;
                    }
                }
                let _ = reader_process.request_stop();
            })
            .map_err(|e| bad(e.to_string()))?;
        let mut worker = Self {
            process,
            generation: generation.clone(),
            handshake: Handshake {
                protocol_version: WIRE_VERSION,
                build_identity: String::new(),
                capabilities: Value::Null,
                generation: generation.clone(),
                max_response_size: MAX_RESPONSE,
            },
            requests,
            admission: Mutex::new(()),
            next_id: AtomicU64::new(0),
            revoked: AtomicBool::new(false),
        };
        let ready = worker.request(Outbound {
            operation: "initialize",
            name: None,
            arguments: initialize_args(&options),
            run_id: INITIALIZE_RUN_ID,
            session: None,
            cancellation: &ActionCancellation::default(),
            timeout: options.startup_timeout,
        })?;
        match wire::validate_ready(&ready, pid, &options.host_bundle_id, &worker.generation) {
            Ok(handshake) => {
                worker.handshake = handshake;
                Ok(worker)
            }
            Err(error) => {
                let _ = worker.request_stop();
                Err(error)
            }
        }
    }

    pub fn handshake(&self) -> &Handshake {
        &self.handshake
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }

    pub fn stderr_log(&self) -> String {
        self.process.stderr_preview()
    }

    /// Only trusted adapters can name driver operations. MCP never exposes this API.
    pub fn call(
        &self,
        name: &str,
        arguments: Value,
        run_id: &str,
        session: Option<&str>,
        cancellation: &ActionCancellation,
        timeout: Duration,
    ) -> Result<Value, WorkerError> {
        if run_id.trim().is_empty() {
            return Err(WorkerError::new(
                Completion::NotStarted,
                "invalid_request",
                "run_id is required",
            ));
        }
        self.request(Outbound {
            operation: "call",
            name: Some(name),
            arguments,
            run_id,
            session,
            cancellation,
            timeout,
        })
    }

    pub fn bind(&self, label: &str, manifest: &Path) -> Result<String, WorkerError> {
        let value = self.request(Outbound {
            operation: "bind_session",
            name: None,
            arguments: json!({
                "public_session": label, "mode": "bounded", "ttl_seconds": 3600,
                "idle_ttl_seconds": 900, "capability_manifest_path": manifest,
                "bounded_manifest_path": null,
            }),
            run_id: BIND_RUN_ID,
            session: None,
            cancellation: &ActionCancellation::default(),
            timeout: Duration::from_secs(5),
        })?;
        value["session_handle"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                WorkerError::new(
                    Completion::NotStarted,
                    "invalid_session",
                    "driver did not return a session handle",
                )
            })
    }

    fn request(&self, spec: Outbound<'_>) -> Result<Value, WorkerError> {
        let Outbound {
            operation,
            name,
            arguments,
            run_id,
            session,
            cancellation,
            timeout,
        } = spec;
        if run_id.trim().is_empty() {
            return Err(WorkerError::new(
                Completion::NotStarted,
                "invalid_request",
                "run_id is required",
            ));
        }
        let start = Instant::now();
        let check = || {
            if self.revoked.load(Ordering::Acquire) || cancellation.check().is_err() {
                Err(WorkerError::new(
                    Completion::NotStarted,
                    "worker_revoked",
                    "dispatch cancelled",
                ))
            } else {
                Ok(())
            }
        };
        let _admission = loop {
            check()?;
            if start.elapsed() >= timeout {
                return Err(WorkerError::new(
                    Completion::NotStarted,
                    "worker_busy",
                    "admission timed out",
                ));
            }
            if let Some(guard) = self.admission.try_lock_for(Duration::from_millis(10)) {
                break guard;
            }
        };
        check()?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut line = serde_json::to_vec(&Request {
            protocol_version: WIRE_VERSION,
            request_id: id,
            generation: &self.generation,
            run_id,
            deadline_ms: deadline_unix_ms(timeout),
            cancellation: RequestCancellation { armed: true },
            operation,
            name,
            arguments,
            session_handle: session,
        })
        .map_err(|e| WorkerError::new(Completion::NotStarted, "invalid_request", e))?;
        if line.len() >= MAX_REQUEST {
            return Err(WorkerError::new(
                Completion::NotStarted,
                "request_too_large",
                "driver request exceeds limit",
            ));
        }
        line.push(b'\n');
        let (tx, rx) = mpsc::sync_channel(1);
        self.requests
            .try_send(Exchange { line, reply: tx })
            .map_err(|e| WorkerError::new(Completion::NotStarted, "worker_unavailable", e))?;
        loop {
            if check().is_err() || start.elapsed() >= timeout {
                let _ = self.request_stop();
                return Err(WorkerError::new(Completion::Unknown, "worker_interrupted",
                    "request may have executed; wait for process exit and observe before continuing"));
            }
            match rx.recv_timeout(Duration::from_millis(10)) {
                Ok(Ok(frame)) => return self.response(id, &frame),
                Ok(Err(error)) => {
                    let _ = self.request_stop();
                    return Err(WorkerError::new(Completion::Unknown, "worker_io", error));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = self.request_stop();
                    return Err(WorkerError::new(
                        Completion::Unknown,
                        "worker_disconnected",
                        "driver pipe closed",
                    ));
                }
            }
        }
    }

    fn response(&self, id: u64, frame: &[u8]) -> Result<Value, WorkerError> {
        let response = serde_json::from_slice::<Response>(frame);
        let response = match response {
            Ok(r)
                if r.protocol_version == WIRE_VERSION
                    && r.request_id == id
                    && r.generation == self.generation =>
            {
                r
            }
            _ => {
                let _ = self.request_stop();
                return Err(WorkerError::new(
                    Completion::Unknown,
                    "invalid_response",
                    "driver returned malformed or mismatched response",
                ));
            }
        };
        if !response.ok {
            return Err(WorkerError::new(
                response.completion,
                response.error_code.as_deref().unwrap_or("driver_error"),
                response
                    .error
                    .unwrap_or_else(|| "driver refused operation".into()),
            ));
        }
        response.result.ok_or_else(|| {
            WorkerError::new(
                Completion::Unknown,
                "invalid_response",
                "driver omitted result",
            )
        })
    }

    pub fn request_stop(&self) -> Result<(), String> {
        self.revoked.store(true, Ordering::Release);
        self.process.request_stop()
    }

    /// Only process-tree quiescence counts as stopped, never Promise cancellation.
    pub fn is_stopped(&self) -> bool {
        self.process.stopped()
    }
}

impl Drop for PrivateWorker {
    fn drop(&mut self) {
        let _ = self.request_stop();
    }
}

fn initialize_args(options: &WorkerOptions) -> Value {
    json!({
        "host_bundle_id": options.host_bundle_id,
        "build_identity": BUILD_IDENTITY,
        "max_response_size": MAX_RESPONSE,
        "protocol_version": WIRE_VERSION,
        "configured_driver": {
            "claude_code_compatibility": false,
            "authorization": {
                "allowed_modes": ["bounded"],
                "compatibility_mode": "bounded",
                "compatibility_capability_manifest_path": options.manifest,
                "compatibility_bounded_manifest_path": null,
                "unrestricted_acknowledged": false,
                "max_session_ttl_seconds": 3600,
                "max_idle_ttl_seconds": 900,
            }
        }
    })
}

fn deadline_unix_ms(timeout: Duration) -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .saturating_add(timeout)
        .as_millis() as u64
}

fn read_frame(reader: &mut impl BufRead) -> Result<Vec<u8>, String> {
    let mut frame = Vec::new();
    reader
        .take((MAX_RESPONSE + 1) as u64)
        .read_until(b'\n', &mut frame)
        .map_err(|e| e.to_string())?;
    if frame.len() > MAX_RESPONSE || !frame.ends_with(b"\n") {
        return Err("driver response truncated or exceeds limit".into());
    }
    Ok(frame)
}
