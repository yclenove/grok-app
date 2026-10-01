//! Synchronous Broker boundary over one explicitly granted portal run.
//! The granting runtime must outlive this adapter's native owner. App factory
//! selection is intentionally not enabled until platform permission/takeover
//! monitoring and installed native GNOME acceptance are implemented.
use crate::{session::SessionControl, PortalHostSession, SessionState, SourceKind};
use grok_computer_use_core::{
    adapter::{
        ActionSupport, AdapterActResult, Capabilities, CaptureOptions, ComputerUseAdapter,
        DispatchRequest, SurfaceKind, TargetInfo,
    },
    execution::ActionCancellation,
    protocol::{Observation, JS_MAX_SAFE_INTEGER},
};
use std::{
    sync::{mpsc, Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};
use tokio::sync::{mpsc as asynchronous, watch};

/// Host-only policy, never supplied by an agent request. Admission checks are
/// not a lock-screen detector: the Host must also abort on asynchronous loss
/// of permission, session lock or user takeover. No permissive default exists.
/// Read methods must be nonblocking snapshots. Native monitors must latch loss
/// until new Host consent; asynchronous polling cannot detect a lost pulse.
pub trait PortalInputPolicy: Send + Sync {
    fn input_available(&self) -> bool;
    fn user_input_active(&self) -> bool;
}

struct Status {
    generation: u64,
    claimed: bool,
    active: Option<ActionCancellation>,
    snapshot: Option<String>,
    stopping: bool,
    native_joined: bool,
    failure: Option<String>,
}
enum Work {
    Capture(
        u64,
        CaptureOptions,
        mpsc::Sender<Result<Observation, String>>,
    ),
    Act(
        DispatchRequest,
        mpsc::Sender<Result<AdapterActResult, String>>,
    ),
}

/// One run owns one granted monitor. Other runs cannot enumerate or claim it.
/// Busy/retiring records survive timeout and release until the exact native
/// owner and this worker have returned; neither a timer nor Closed alone is idle.
pub struct PortalAdapter {
    run: String,
    target: TargetInfo,
    control: SessionControl,
    policy: Arc<crate::policy::PolicyLease>,
    status: Arc<Mutex<Status>>,
    send: asynchronous::Sender<Work>,
    stop: watch::Sender<bool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

fn off_reactor<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match tokio::runtime::Handle::try_current() {
        // block_in_place hands a multithread runtime's reactor work back to
        // its scheduler; it is also safe in an existing blocking worker.
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        Ok(_) => Err(
            "synchronous portal call requires a blocking worker, not a current-thread reactor"
                .into(),
        ),
        Err(_) => f(),
    }
}

impl PortalAdapter {
    pub fn from_granted(
        host: PortalHostSession,
        policy: Arc<dyn PortalInputPolicy>,
    ) -> Result<Self, String> {
        Self::from_granted_slot(&mut Some(host), policy)
    }

    /// Do not consume the Host/native owner until the receiving thread has a
    /// working runtime. Spawn/runtime failures leave it available for joining.
    pub(crate) fn from_granted_slot(
        slot: &mut Option<PortalHostSession>,
        policy: Arc<dyn PortalInputPolicy>,
    ) -> Result<Self, String> {
        let host = slot.as_ref().ok_or("portal Host already handed off")?;
        let policy = crate::policy::PolicyLease::new(policy);
        if !policy.input_available() {
            return Err("Host policy denied native adapter handoff".into());
        }
        let SessionState::Granted(grant) = host.state() else {
            return Err("adapter requires an active portal grant".into());
        };
        let control = host.control();
        let run = host.run_id().to_owned();
        let target = TargetInfo {
            target_id: host.target_id().to_owned(),
            title: "Portal-selected monitor".into(),
            app_name: "Wayland portal".into(),
            kind: "monitor".into(),
            pid: None,
            backend: "wayland-portal".into(),
            execution_mode: "foreground-session".into(),
            replay_policy: "never".into(),
            lifecycle_stamp: host.generation(),
            display_id: grant.session_path,
            coordinate_space: "image-pixels".into(),
            scope_label: SourceKind::Monitor.scope_label().into(),
        };
        let status = Arc::new(Mutex::new(Status {
            generation: host.generation(),
            claimed: false,
            active: None,
            snapshot: None,
            stopping: false,
            native_joined: false,
            failure: None,
        }));
        let (send, receive) = asynchronous::channel(1);
        let (stop, stopping) = watch::channel(false);
        let shared = status.clone();
        let native = control.clone();
        let host_policy = policy.clone();
        let (ready, startup) = mpsc::channel();
        let (handoff, incoming) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("cu-wayland-host".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let runtime = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(runtime) => runtime,
                        Err(error) => {
                            let error = format!("portal adapter runtime: {error}");
                            let _ = ready.send(Err(error.clone()));
                            return Err(error);
                        }
                    };
                    if ready.send(Ok(())).is_err() {
                        return Ok(());
                    }
                    let Ok(host) = incoming.recv() else {
                        return Ok(());
                    };
                    runtime.block_on(serve(
                        host,
                        receive,
                        stopping,
                        shared.clone(),
                        host_policy,
                        native.clone(),
                    ))
                }));
                let error = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error),
                    Err(_) => {
                        Some("portal adapter owner panicked; native completion unproven".into())
                    }
                };
                if let Some(error) = error {
                    native.request_stop();
                    if let Ok(mut s) = shared.lock() {
                        s.stopping = true;
                        s.failure = Some(error);
                        s.native_joined = false;
                    }
                }
            })
            .map_err(|e| format!("portal adapter thread: {e}"))?;
        if let Err(error) = startup.recv().map_err(|e| e.to_string()).and_then(|r| r) {
            let _ = worker.join();
            return Err(error);
        }
        let host = slot.take().ok_or("portal Host already handed off")?;
        if let Err(error) = handoff.send(host) {
            *slot = Some(error.0);
            let _ = worker.join();
            return Err("portal adapter terminated before owner handoff".into());
        }
        Ok(Self {
            run,
            target,
            control,
            policy,
            status,
            send,
            stop,
            worker: Mutex::new(Some(worker)),
        })
    }

    pub fn target_id(&self) -> &str {
        &self.target.target_id
    }

    pub fn cleanup_error(&self) -> Option<String> {
        self.status.lock().ok().and_then(|s| s.failure.clone())
    }

    fn identity(&self, run: &str, target: &str) -> Result<(), String> {
        if run != self.run || target != self.target.target_id {
            return Err("run does not own this portal target".into());
        }
        Ok(())
    }

    pub(crate) fn fence(&self, fence: u64) -> bool {
        let revoke = match self.status.lock() {
            Ok(mut s) if fence > s.generation => {
                s.stopping = true;
                s.claimed = false;
                s.snapshot = None;
                if let Some(token) = &s.active {
                    token.cancel();
                }
                true
            }
            Ok(_) => false,
            Err(_) => true,
        };
        if revoke {
            // Never hold the adapter status mutex while native locks/cleanup run.
            self.control.request_stop();
            let _ = self.stop.send(true);
        }
        revoke
    }

    fn request<T>(
        &self,
        run: &str,
        target: &str,
        generation: u64,
        rebind: bool,
        cancellation: ActionCancellation,
        command: impl FnOnce(mpsc::Sender<Result<T, String>>) -> Work,
    ) -> Result<T, String> {
        off_reactor(|| {
            self.identity(run, target)?;
            let allowed = self.policy.input_available() && !self.policy.user_input_active();
            if !allowed {
                self.fence(u64::MAX);
                return Err("Host input policy revoked portal operation".into());
            }
            let (reply, response) = mpsc::channel();
            let work = command(reply);
            {
                let mut s = self
                    .status
                    .lock()
                    .map_err(|_| "portal adapter state poisoned")?;
                if s.stopping
                    || !s.claimed
                    || !matches!(self.control.state(), SessionState::Granted(_))
                {
                    return Err("portal target is unclaimed or retired".into());
                }
                if !(1..=JS_MAX_SAFE_INTEGER).contains(&generation)
                    || generation < s.generation
                    || (!rebind && generation != s.generation)
                {
                    return Err("portal adapter generation mismatch".into());
                }
                if s.active.is_some() {
                    return Err("portal owner is busy".into());
                }
                match &work {
                    Work::Capture(_, options, _) if options.for_model => {
                        // Retire authority at attempt admission, even when the
                        // attempt is cancelled/denied before reaching the owner.
                        s.snapshot = None;
                    }
                    Work::Act(request, _)
                        if s.snapshot.as_deref() != Some(&request.snapshot_id) =>
                    {
                        return Err("model snapshot retired or not issued by this adapter".into());
                    }
                    _ => {}
                }
                cancellation.check()?;
                s.active = Some(cancellation.clone());
                if self.send.try_send(work).is_err() {
                    s.active = None;
                    return Err("portal owner command unavailable".into());
                }
                s.generation = generation;
            }
            match response.recv_timeout(Duration::from_secs(5)) {
                Ok(result) => result,
                Err(error) => {
                    cancellation.cancel();
                    self.fence(u64::MAX);
                    Err(format!("portal operation completion unknown: {error}"))
                }
            }
        })
    }
}

async fn serve(
    mut host: PortalHostSession,
    mut receive: asynchronous::Receiver<Work>,
    mut stop: watch::Receiver<bool>,
    status: Arc<Mutex<Status>>,
    policy: Arc<dyn PortalInputPolicy>,
    control: SessionControl,
) -> Result<(), String> {
    let mut native_state = control.subscribe();
    loop {
        if *stop.borrow() || !matches!(control.state(), SessionState::Granted(_)) {
            break;
        }
        let work = tokio::select! {
            biased;
            _ = stop.changed() => break,
            _ = crate::policy::revoked(policy.as_ref()) => break,
            changed = native_state.changed() => {
                if changed.is_err() { break; }
                continue;
            }
            work = receive.recv() => match work { Some(work) => work, None => break },
        };
        let running = status.lock().map(|s| !s.stopping).unwrap_or(false);
        let admitted = running && policy.input_available() && !policy.user_input_active();
        match work {
            Work::Capture(generation, options, reply) => {
                let mut result = if admitted {
                    host.bind_generation(generation).and_then(|()| {
                        let run = host.run_id().to_owned();
                        let target = host.target_id().to_owned();
                        host.capture(&run, &target, &options, 1600, 1000)
                    })
                } else {
                    Err("portal operation revoked before execution".into())
                };
                // Capture may be synchronous. Recheck before publishing pixels
                // or snapshot authority, not just at queue admission.
                if !policy.input_available() || policy.user_input_active() {
                    result = Err("Host policy revoked during capture".into());
                    control.request_stop();
                }
                // Return occupancy only after real work, not response timeout.
                {
                    let mut s = status.lock().map_err(|_| "portal adapter state poisoned")?;
                    s.active = None;
                    if s.stopping {
                        result = Err("portal capture retired before publication".into());
                    }
                    if options.for_model && !s.stopping {
                        s.snapshot = result.as_ref().ok().map(|image| image.snapshot_id.clone());
                    }
                }
                let _ = reply.send(result);
            }
            Work::Act(request, reply) => {
                let result = if admitted {
                    let operation = host.dispatch(&request);
                    tokio::pin!(operation);
                    tokio::select! {
                        biased;
                        _ = stop.changed() => {
                            request.cancellation.cancel();
                            control.request_stop();
                            // Accepted native work remains owned until its
                            // exact completion. Never drop or replay it.
                            let _ = operation.await;
                            Err("portal action cancelled; effect unverified".into())
                        }
                        _ = crate::policy::revoked(policy.as_ref()) => {
                            request.cancellation.cancel();
                            control.request_stop();
                            let _ = operation.await;
                            Err("Host policy revoked during action; effect unverified".into())
                        }
                        result = &mut operation => {
                            if !policy.input_available() || policy.user_input_active() {
                                request.cancellation.cancel();
                                control.request_stop();
                                Err("Host policy revoked at action completion; effect unverified".into())
                            } else {
                                result
                            }
                        }
                    }
                } else {
                    Err("portal operation revoked before execution".into())
                };
                if result.is_err() {
                    // After enqueue, a transport error is not native completion.
                    // Keep occupancy until retiring the exact owner below.
                    status
                        .lock()
                        .map_err(|_| "portal adapter state poisoned")?
                        .stopping = true;
                    control.request_stop();
                    let _ = reply.send(result);
                    break;
                }
                status
                    .lock()
                    .map_err(|_| "portal adapter state poisoned")?
                    .active = None;
                let _ = reply.send(result);
            }
        }
    }
    {
        let mut s = status.lock().map_err(|_| "portal adapter state poisoned")?;
        s.stopping = true;
        s.claimed = false;
        s.snapshot = None;
        if let Some(token) = &s.active {
            token.cancel();
        }
    }
    host.stop().await?;
    let mut s = status.lock().map_err(|_| "portal adapter state poisoned")?;
    s.active = None;
    s.native_joined = true;
    Ok(())
}

impl ComputerUseAdapter for PortalAdapter {
    fn backend_id(&self) -> &'static str {
        "wayland-portal"
    }
    fn capabilities(&self) -> Capabilities {
        portal_capabilities()
    }
    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        Err("portal target discovery requires its owning run".into())
    }
    fn list_targets_for_run(&self, run: &str) -> Result<Vec<TargetInfo>, String> {
        Ok(if run == self.run && self.target_alive(self.target_id()) {
            vec![self.target.clone()]
        } else {
            Vec::new()
        })
    }
    fn target_alive(&self, target: &str) -> bool {
        target == self.target_id()
            && !self.policy.is_revoked()
            && matches!(self.control.state(), SessionState::Granted(_))
            && self.status.lock().map(|s| !s.stopping).unwrap_or(false)
    }
    fn observe(&self, _: &str) -> Result<Observation, String> {
        Err("portal capture requires run and Broker generation".into())
    }
    fn capture_for_run_at_generation(
        &self,
        run: &str,
        target: &str,
        generation: u64,
        options: CaptureOptions,
    ) -> Result<Observation, String> {
        self.request(
            run,
            target,
            generation,
            true,
            options.cancellation.clone(),
            |reply| Work::Capture(generation, options, reply),
        )
    }
    fn act(&self, request: &DispatchRequest) -> Result<AdapterActResult, String> {
        self.request(
            &request.run_id,
            &request.target_id,
            request.generation,
            false,
            request.cancellation.clone(),
            |reply| Work::Act(request.clone(), reply),
        )
    }
    fn claim_target(&self, _: &str) -> Result<(), String> {
        Err("portal claim requires its owning run".into())
    }
    fn claim_target_for_run(&self, run: &str, target: &str) -> Result<(), String> {
        self.identity(run, target)?;
        if !self.policy.input_available() {
            self.fence(u64::MAX);
            return Err("Host policy revoked before target claim".into());
        }
        let mut s = self
            .status
            .lock()
            .map_err(|_| "portal adapter state poisoned")?;
        if s.stopping || !matches!(self.control.state(), SessionState::Granted(_)) {
            return Err("portal target retired".into());
        }
        s.claimed = true;
        Ok(())
    }
    fn release_target_for_run(&self, run: &str, target: &str) {
        if self.identity(run, target).is_ok() {
            self.fence(u64::MAX);
        }
    }
    fn abort(&self, run: &str, generation: u64) -> Result<(), String> {
        if run == self.run {
            self.fence(generation);
        }
        Ok(())
    }
    fn is_idle(&self, _: &str) -> bool {
        let Ok(s) = self.status.lock() else {
            return false;
        };
        if s.failure.is_some() || s.active.is_some() {
            return false;
        }
        if !s.stopping {
            // A query can latch policy loss before the owner receives its next
            // monitor tick. That interval is retirement, not certified idle.
            return !self.policy.is_revoked()
                && matches!(self.control.state(), SessionState::Granted(_));
        }
        if !s.native_joined {
            return false;
        }
        drop(s);
        let Ok(mut worker) = self.worker.lock() else {
            return false;
        };
        if let Some(handle) = worker.as_ref() {
            if !handle.is_finished() {
                return false;
            }
        }
        if worker.take().is_some_and(|handle| handle.join().is_err()) {
            if let Ok(mut s) = self.status.lock() {
                s.failure = Some("portal adapter thread join failed".into());
                s.native_joined = false;
            }
            return false;
        }
        true
    }
    fn input_available(&self) -> bool {
        self.policy.input_available() && self.target_alive(self.target_id())
    }
    fn foreground_input_available(&self, target: &str) -> bool {
        self.target_alive(target) && self.policy.input_available()
    }
    fn user_input_active(&self) -> bool {
        self.policy.user_input_active()
    }
    fn current_geometry_revision_for(&self, target: &str) -> u64 {
        if target == self.target_id() {
            self.control.geometry_revision()
        } else {
            0
        }
    }
    fn start_periodic_preview(&self, _: &str) {}
    fn stop_periodic_preview(&self) {}
    fn periodic_preview_active(&self) -> bool {
        false
    }
}

pub(crate) fn portal_capabilities() -> Capabilities {
    let mut c = Capabilities::for_surface(SurfaceKind::WebView, "wayland-portal");
    c.observe_screenshot = true;
    c.coordinate_click = true;
    c.click = ActionSupport::COORDINATE;
    c.key = ActionSupport::COORDINATE;
    c.scroll = ActionSupport::COORDINATE;
    c.notes = vec![
        "Granted-monitor adapter only; App Wayland enablement and native GNOME acceptance pending"
            .into(),
    ];
    // native_wayland/IME/text/AX/drag/wait remain false, not implied by IPC.
    c
}
impl Drop for PortalAdapter {
    fn drop(&mut self) {
        self.fence(u64::MAX);
    }
}
