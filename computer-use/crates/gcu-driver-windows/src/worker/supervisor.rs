//! Portable process watchdog. Native work never runs in this actor.
use super::{
    codec,
    wire::{Call, Failure, Reply, Request, Response, VERSION},
};
use gcu_core::driver::{Cancel, DriverError, Result};
use std::{
    ffi::OsString,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{mpsc, oneshot, Notify},
};

static NEXT_NAMESPACE: AtomicU64 = AtomicU64::new(1);

/// Maximum per-attempt worker watchdog budget, including pipe backpressure.
pub const WORKER_TIMEOUT: Duration = Duration::from_secs(10);
/// Trusted launcher settings. No shell, sockets, token files or inherited secrets.
#[derive(Clone)]
pub struct WorkerConfig {
    /// Trusted executable path; production uses the current `gcu` executable.
    pub executable: PathBuf,
    /// Arguments; defaults to the internal `worker` command.
    pub args: Vec<OsString>,
    /// Explicit extra environment, for trusted test/embedding launchers only.
    pub environment: Vec<(OsString, OsString)>,
    /// Per-attempt watchdog, capped at ten seconds; shorter budgets aid tests.
    pub timeout: Duration,
    namespace: String,
}
impl WorkerConfig {
    /// Create an isolated launcher with no inherited nonessential environment.
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            args: vec!["worker".into()],
            environment: Vec::new(),
            timeout: WORKER_TIMEOUT,
            namespace: String::new(),
        }
    }
}
#[derive(Default)]
struct StopState {
    generation: AtomicU64,
    notify: Notify,
    completed: AtomicU64,
    cleaned: Notify,
}
/// Independent Stop signal. It never takes the RPC mutex or enters its queue.
/// Host owns the persistent Stop latch; new calls require Host authorization.
#[derive(Clone, Default)]
pub struct StopHandle(Arc<StopState>);
impl StopHandle {
    /// Invalidate active/queued calls and wake the watchdog immediately. Nonblocking.
    pub fn stop(&self) {
        self.0.generation.fetch_add(1, Ordering::SeqCst);
        self.0.notify.notify_waiters();
    }
    fn generation(&self) -> u64 {
        self.0.generation.load(Ordering::SeqCst)
    }
    fn cleaned(&self) {
        self.0.completed.store(self.generation(), Ordering::SeqCst);
        self.0.cleaned.notify_waiters();
    }
    async fn wait_cleaned(&self, generation: u64) {
        loop {
            let wake = self.0.cleaned.notified();
            tokio::pin!(wake);
            wake.as_mut().enable();
            if self.0.completed.load(Ordering::SeqCst) >= generation {
                return;
            }
            wake.await;
        }
    }
    async fn changed(&self, previous: u64) {
        loop {
            let wake = self.0.notify.notified();
            tokio::pin!(wake);
            wake.as_mut().enable();
            if self.generation() != previous {
                return;
            }
            wake.await;
        }
    }
}
struct Work {
    request: Request,
    stop_generation: u64,
    cancel: Cancel,
    dispatched: Arc<AtomicBool>,
    reply: oneshot::Sender<Result<Reply>>,
    deadline: Option<tokio::time::Instant>,
}
/// Serial persistent worker with one read-only retry, no mutation retries, and
/// cancellation/Stop independent of the queue. Drop kills the child; shutdown
/// additionally awaits reaping. Native errors preserve maybe_executed.
pub struct Supervisor {
    queue: mpsc::Sender<Work>,
    stop: StopHandle,
    shutdown: Cancel,
    done: tokio::sync::Mutex<Option<oneshot::Receiver<()>>>,
    pub(crate) capabilities: gcu_protocol::Capabilities,
    safety: Arc<std::sync::Mutex<Option<Arc<crate::input::hotkey::SafetyGuard>>>>,
}
impl Supervisor {
    /// Launch, attach lifetime containment, then probe readiness over private pipes.
    /// Startup is bounded and cancellable by dropping this future.
    pub async fn start(config: WorkerConfig) -> Result<Self> {
        Self::start_with_releaser(config, super::input::default_releaser()).await
    }
    /// Attach independent parent cleanup (fake on portable tests). No native
    /// UIA/discovery work runs here; cleanup emits only recorded up events.
    pub async fn start_with_releaser(
        mut config: WorkerConfig,
        release: Arc<dyn crate::input::api::InputReleaser>,
    ) -> Result<Self> {
        config.timeout = config.timeout.min(WORKER_TIMEOUT);
        #[cfg(windows)]
        let owner = super::unsafe_job::owner_instance()?;
        #[cfg(not(windows))]
        let owner = std::process::id().to_string();
        config.namespace = format!("{owner}.{}", NEXT_NAMESPACE.fetch_add(1, Ordering::SeqCst));
        if config.timeout.is_zero() {
            return Err(DriverError::InvalidRequest);
        }
        let (process, probe) = Process::ready(&config, 1).await?;
        let Reply::Doctor {
            mut capabilities,
            ready,
            ..
        } = probe
        else {
            return Err(crashed(false));
        };
        if !ready {
            return Err(crashed(false));
        }
        capabilities.observe.events = false;
        let (tx, rx) = mpsc::channel(32);
        let stop = StopHandle::default();
        let shutdown = Cancel::default();
        let (done_tx, done_rx) = oneshot::channel();
        let safety = Arc::new(std::sync::Mutex::new(None));
        let recovery = super::input::Recovery::new(release, safety.clone());
        recovery.arm_publication(&config.namespace);
        tokio::spawn(run(
            config,
            rx,
            stop.clone(),
            shutdown.clone(),
            Some(process),
            done_tx,
            recovery,
        ));
        Ok(Self {
            queue: tx,
            stop,
            shutdown,
            done: tokio::sync::Mutex::new(Some(done_rx)),
            capabilities,
            safety,
        })
    }
    /// Clone the independent watchdog signal. No IO or lock.
    pub fn stop_handle(&self) -> StopHandle {
        self.stop.clone()
    }
    pub(crate) fn attach_safety(&self, guard: Arc<crate::input::hotkey::SafetyGuard>) {
        let stop = self.stop.clone();
        guard.bind_stop(Arc::new(move || stop.stop()));
        *self.safety.lock().unwrap_or_else(|p| p.into_inner()) = Some(guard);
    }
    /// Kill current input dispatch independently of the RPC queue, then await
    /// the parent's up-only cleanup. Host retains its persistent Stop latch.
    pub async fn release_all_input(&self) {
        self.stop.stop();
        let generation = self.stop.generation();
        if tokio::time::timeout(
            Duration::from_millis(400),
            self.stop.wait_cleaned(generation),
        )
        .await
        .is_err()
        {
            tracing::warn!("worker stop cleanup deadline exceeded");
        }
    }
    /// Execute one closed-set request. Reads retry once after transport/native
    /// timeout/crash. Mutations return uncertain errors after any dispatch loss.
    /// Dropping the future kills an in-flight worker before another call runs.
    pub async fn execute(&self, request: Request, cancel: &Cancel) -> Result<Reply> {
        self.execute_inner(request, cancel, None).await
    }
    pub(crate) async fn execute_bounded(
        &self,
        request: Request,
        cancel: &Cancel,
        budget: Duration,
    ) -> Result<Reply> {
        self.execute_inner(request, cancel, Some(tokio::time::Instant::now() + budget))
            .await
    }
    async fn execute_inner(
        &self,
        request: Request,
        cancel: &Cancel,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<Reply> {
        if cancel.is_cancelled() || self.shutdown.is_cancelled() {
            return Err(cancelled(false));
        }
        let mutation = !request.is_read_only();
        if mutation {
            if let Some(guard) = self
                .safety
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
            {
                guard.check(super::input::foreground_request(&request))?;
            }
        }
        let dispatched = Arc::new(AtomicBool::new(false));
        let (reply, wait) = oneshot::channel();
        let work = Work {
            request,
            stop_generation: self.stop.generation(),
            cancel: cancel.clone(),
            dispatched: dispatched.clone(),
            reply,
            deadline,
        };
        self.queue.try_send(work).map_err(|_| DriverError::Native {
            message: "worker queue unavailable".into(),
            maybe_executed: false,
        })?;
        let maybe = || mutation && dispatched.load(Ordering::SeqCst);
        tokio::select! { biased;
            _ = cancel.cancelled() => Err(cancelled(maybe())),
            _ = self.shutdown.cancelled() => Err(cancelled(maybe())),
            _ = async {
                match deadline { Some(at) => tokio::time::sleep_until(at).await, None => std::future::pending::<()>().await }
            } => Err(DriverError::Timeout { maybe_executed: maybe() }),
            result = wait => result.map_err(|_| crashed(maybe()))?,
        }
    }
    /// Kill independently of the RPC queue and wait until the child is reaped.
    pub async fn shutdown(&self) -> Result<()> {
        self.shutdown.cancel();
        if let Some(done) = self.done.lock().await.take() {
            done.await.map_err(|_| crashed(false))?;
        }
        Ok(())
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}
#[path = "process.rs"]
mod process;
use process::Process;
fn matches_reply(request: &Request, reply: &Reply, epoch: u64) -> bool {
    if let (Request::Snapshot(window, _), Reply::Tree(tree)) = (request, reply) {
        return tree.valid(epoch, window);
    }
    matches!(
        (request, reply),
        (Request::Doctor, Reply::Doctor { .. })
            | (Request::ListApps, Reply::Apps { .. })
            | (
                Request::ResolveApp(_) | Request::LaunchApp(_),
                Reply::App(_)
            )
            | (Request::ListWindows(_), Reply::Windows(_))
            | (Request::KeyWindow(_), Reply::Window(_))
            | (Request::Snapshot(..), Reply::Tree(_))
            | (Request::Capture(..), Reply::Capture { .. })
            | (Request::Geometry(_), Reply::Geometry(_))
            | (Request::Focused(_), Reply::Focused(_))
            | (Request::Perform(_), Reply::Delivery(_))
    )
}
fn crashed(maybe_executed: bool) -> DriverError {
    DriverError::Crashed { maybe_executed }
}
fn cancelled(maybe_executed: bool) -> DriverError {
    DriverError::Cancelled { maybe_executed }
}
async fn kill(process: &mut Option<Process>) {
    if let Some(mut p) = process.take() {
        p.kill().await;
    }
}
async fn run(
    config: WorkerConfig,
    mut queue: mpsc::Receiver<Work>,
    stop: StopHandle,
    shutdown: Cancel,
    mut process: Option<Process>,
    done: oneshot::Sender<()>,
    recovery: super::input::Recovery,
) {
    let mut epoch = 1;
    let mut sequence = 0u64;
    let mut generation = stop.generation();
    let mut shutdown_cleaned = false;
    loop {
        let mut work = tokio::select! { biased;
            _ = shutdown.cancelled() => break,
            _ = stop.changed(generation) => {
                kill(&mut process).await;
                recovery.cleanup().await;
                generation = stop.generation();
                stop.cleaned();
                continue;
            },
            work = queue.recv() => match work { Some(w) => w, None => break },
        };
        if work.reply.is_closed() {
            continue;
        }
        if work.cancel.is_cancelled() || work.stop_generation != stop.generation() {
            let _ = work.reply.send(Err(cancelled(false)));
            continue;
        }
        let mutation = !work.request.is_read_only();
        if let Err(error) = recovery.check_request(&work.request) {
            let _ = work.reply.send(Err(error));
            continue;
        }
        if mutation && !recovery.ready() {
            let _ = work.reply.send(Err(DriverError::CapabilityUnavailable));
            continue;
        }
        let operation = async {
            for attempt in 0..=1 {
                if process.is_none() {
                    epoch += 1;
                    let (p, _) = Process::ready(&config, epoch).await?;
                    process = Some(p);
                }
                let Some(p) = process.as_mut() else {
                    return Err(crashed(false));
                };
                sequence += 1;
                let result = p
                    .exchange(
                        work.request.clone(),
                        sequence,
                        config.timeout,
                        Some(work.dispatched.as_ref()),
                        Some(&recovery),
                    )
                    .await;
                if matches!(
                    result,
                    Err(DriverError::Timeout { .. } | DriverError::Crashed { .. })
                ) {
                    tracing::warn!(target: "gcu_windows_timing", stage = "worker_fault", phase = "detected",
                        epoch, sequence, mutation, attempt, error = ?result.as_ref().err(), "worker recovery");
                    kill(&mut process).await;
                    if !mutation && attempt == 0 {
                        continue;
                    }
                }
                return result;
            }
            Err(crashed(mutation))
        };
        let result = tokio::select! { biased;
            _ = shutdown.cancelled() => None,
            _ = stop.changed(work.stop_generation) => None,
            _ = work.cancel.cancelled() => None,
            _ = async {
                match work.deadline { Some(at) => tokio::time::sleep_until(at).await, None => std::future::pending::<()>().await }
            } => Some(Err(DriverError::Timeout { maybe_executed: mutation && work.dispatched.load(Ordering::SeqCst) })),
            _ = work.reply.closed() => None,
            result = operation => Some(result),
        };
        if matches!(
            result,
            Some(Err(
                DriverError::Timeout { .. } | DriverError::Crashed { .. }
            ))
        ) {
            kill(&mut process).await;
        }
        let result = match result {
            Some(result) => result,
            None => {
                kill(&mut process).await;
                Err(cancelled(
                    mutation && work.dispatched.load(Ordering::SeqCst),
                ))
            }
        };
        // A failed input action may have abandoned a native send. Terminate the
        // child before releasing the authoritative parent ledger.
        if mutation || work.stop_generation != stop.generation() || shutdown.is_cancelled() {
            if !recovery.ready() && result.is_err() {
                kill(&mut process).await;
            }
            recovery.cleanup().await;
            shutdown_cleaned = shutdown.is_cancelled();
        }
        if work.stop_generation != stop.generation() {
            kill(&mut process).await;
            generation = stop.generation();
            stop.cleaned();
        }
        let restart = process.is_none()
            && stop.generation() == work.stop_generation
            && matches!(
                &result,
                Err(DriverError::Timeout { .. } | DriverError::Crashed { .. })
            );
        let _ = work.reply.send(result);
        if restart {
            let _restart = crate::timing::Timing::start("worker_restart");
            // Return mutation uncertainty immediately, then warm a fresh worker
            // without replaying the operation. Stop can interrupt this bootstrap.
            epoch += 1;
            let stopped_at = stop.generation();
            let bootstrap = async {
                let (p, _) = Process::ready(&config, epoch).await?;
                Ok::<_, DriverError>(p)
            };
            process = tokio::select! { biased;
                _ = shutdown.cancelled() => None,
                _ = stop.changed(stopped_at) => None,
                result = bootstrap => match result {
                    Ok(p) => Some(p),
                    Err(_) => { tracing::warn!("worker restart failed"); None },
                },
            };
        }
    }
    kill(&mut process).await;
    if !shutdown_cleaned {
        recovery.cleanup().await;
    }
    stop.cleaned();
    let _ = done.send(());
}
