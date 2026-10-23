//! Scope records and the in-memory lease.
//!
//! Identity is the ADR 0017 abstraction: process id, OS start time, boot
//! identity, window id, and incarnation. This module does not read a platform.
//! A later card supplies those values. Missing or zero evidence cannot be
//! stored, so a caller cannot register a target it cannot prove.

use std::sync::{Arc, Mutex, RwLock};
use std::task::Waker;

use tokio::sync::watch;

use crate::driver::{AppRef, WindowRef};
use crate::model::NativeHandle;

/// Why a scope record was rejected or a lease update did not apply.
///
/// Display text is a fixed sentence. It never includes a path, app, or window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScopeError {
    /// A required identity, proof, or lease field is missing or inconsistent.
    #[error("Evaluation scope is incomplete.")]
    Incomplete,
    /// The replacement generation is not strictly newer.
    #[error("Evaluation scope generation is stale.")]
    StaleGeneration,
    /// The lease was revoked and cannot be rebound.
    #[error("Evaluation scope is revoked.")]
    Revoked,
    /// The replacement belongs to a different engine run.
    #[error("Evaluation scope run does not match.")]
    RunMismatch,
}

/// OS boot identity. An empty value is not evidence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BootIdentity(String);

impl BootIdentity {
    /// Store a non-empty boot identity.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn new(value: impl Into<String>) -> Result<Self, ScopeError> {
        let value = value.into();
        if value.is_empty() || value.len() > 256 {
            return Err(ScopeError::Incomplete);
        }
        Ok(Self(value))
    }

    /// Borrow the boot identity. Memory only; no timeout or cancellation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// OS process start time. Zero is not evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OsStartTime(u64);

impl OsStartTime {
    /// Store a non-zero start timestamp.
    ///
    /// The units are the platform's. This constructor does not read a clock.
    /// No blocking, timeout, or cancellation.
    pub fn new(ticks: u64) -> Result<Self, ScopeError> {
        if ticks == 0 {
            return Err(ScopeError::Incomplete);
        }
        Ok(Self(ticks))
    }

    /// Return the stored timestamp. Memory only; no timeout or cancellation.
    pub fn ticks(self) -> u64 {
        self.0
    }
}

/// One running process instance. Pid alone is not an identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProcessInstance {
    pid: u32,
    start: OsStartTime,
    boot: BootIdentity,
}

impl ProcessInstance {
    /// Require a non-zero pid and the already validated start and boot values.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn new(pid: u32, start: OsStartTime, boot: BootIdentity) -> Result<Self, ScopeError> {
        if pid == 0 {
            return Err(ScopeError::Incomplete);
        }
        Ok(Self { pid, start, boot })
    }

    /// Process id. Memory only; no timeout or cancellation.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// OS start time. Memory only; no timeout or cancellation.
    pub fn start(&self) -> OsStartTime {
        self.start
    }

    /// Boot identity. Memory only; no timeout or cancellation.
    pub fn boot(&self) -> &BootIdentity {
        &self.boot
    }
}

/// One native window bound to the process that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowBinding {
    native_id: String,
    incarnation: u64,
    owner: ProcessInstance,
}

impl WindowBinding {
    /// Require a non-empty window id and a non-zero incarnation.
    ///
    /// Memory only. No blocking, timeout, or cancellation. The owner is stored
    /// and compared later; this function does not ask the platform.
    pub fn new(
        native_id: impl Into<String>,
        incarnation: u64,
        owner: ProcessInstance,
    ) -> Result<Self, ScopeError> {
        let native_id = native_id.into();
        if native_id.is_empty() || native_id.len() > 1024 || incarnation == 0 {
            return Err(ScopeError::Incomplete);
        }
        Ok(Self {
            native_id,
            incarnation,
            owner,
        })
    }

    /// Native window id. Memory only; no timeout or cancellation.
    pub fn native_id(&self) -> &str {
        &self.native_id
    }

    /// Window incarnation. Memory only; no timeout or cancellation.
    pub fn incarnation(&self) -> u64 {
        self.incarnation
    }

    /// Owning process. Memory only; no timeout or cancellation.
    pub fn owner(&self) -> &ProcessInstance {
        &self.owner
    }
}

/// Element evidence. It points at the window that owns the handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementBinding {
    window: WindowBinding,
}

impl ElementBinding {
    /// Wrap a window binding. Memory only; no blocking, timeout, or cancellation.
    pub fn new(window: WindowBinding) -> Self {
        Self { window }
    }

    /// Window that owns the element. Memory only; no timeout or cancellation.
    pub fn window(&self) -> &WindowBinding {
        &self.window
    }
}

/// Metadata-only evidence. Implementations must not read pixels or deliver input.
///
/// `None` means the platform cannot prove the identity. Callers refuse.
/// The trait is separate from [`crate::driver::Driver`] so adding a platform
/// method is not required and a default cannot silently forward.
pub trait InstanceEvidence: Send + Sync {
    /// Process identity for a discovered app, or `None` when it is unproved.
    ///
    /// Memory or a metadata query the implementor bounds itself. This trait
    /// does not add a timeout. Cancellation is the implementor's.
    fn app_instance(&self, app: &AppRef) -> Option<ProcessInstance>;

    /// Window identity, or `None` when the id cannot be proved.
    ///
    /// Memory or a metadata query. No content read. No timeout is added here.
    fn window_binding(&self, window: &WindowRef) -> Option<WindowBinding>;

    /// Element owner, or `None` when the handle is unknown or forged.
    ///
    /// Compares handle identity only. No content read, timeout, or cancellation
    /// is added by this trait.
    fn element_binding(&self, handle: &NativeHandle) -> Option<ElementBinding>;

    /// Pause before an attempt check. The default is ready immediately.
    ///
    /// Blocking: none in the default. Timeout: none. Cancellation: none.
    /// A test double may wait and must bound that wait itself. Production
    /// evidence uses this default and adds no latency.
    fn attempt_fence<'a>(
        &'a self,
        _phase: AttemptPhase,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(std::future::ready(()))
    }
}

/// Where an attempt waits so a test can revoke before the next check.
///
/// Memory only when the default fence is used. No blocking, timeout, or
/// cancellation is added by the enum itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptPhase {
    /// Before the attempt reads the lease.
    BeforeOpen,
    /// Before the synchronous admission query.
    Admit,
    /// After the receiver is armed and before that generation is marked seen.
    MarkSeen,
}

/// Proof the assembler recorded an executable. The path is not an auth key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableProof {
    path: String,
}

impl ExecutableProof {
    /// Store a non-empty path. Memory only; no blocking, timeout, or cancellation.
    ///
    /// The wrapper never authorizes a target by comparing this path.
    pub fn new(path: impl Into<String>) -> Result<Self, ScopeError> {
        let path = path.into();
        if path.is_empty() || path.len() > 4096 {
            return Err(ScopeError::Incomplete);
        }
        Ok(Self { path })
    }

    /// Borrow the recorded path. Memory only; no timeout or cancellation.
    ///
    /// Callers must not treat this string as authorization.
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// A window the assembler explicitly registered on one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredWindow {
    binding: WindowBinding,
}

impl RegisteredWindow {
    /// Register one proved window. Memory only; no blocking, timeout, or cancellation.
    pub fn new(binding: WindowBinding) -> Self {
        Self { binding }
    }

    /// Borrow the binding. Memory only; no timeout or cancellation.
    pub fn binding(&self) -> &WindowBinding {
        &self.binding
    }
}

/// One allowed process and the windows that were explicitly registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureTarget {
    instance: ProcessInstance,
    role: String,
    proof: ExecutableProof,
    windows: Vec<RegisteredWindow>,
}

impl FixtureTarget {
    /// Reject a window whose owner is not this instance.
    ///
    /// `role` may be empty. It is not an authorization key. Memory only.
    /// No blocking, timeout, or cancellation.
    pub fn new(
        instance: ProcessInstance,
        role: impl Into<String>,
        proof: ExecutableProof,
        windows: Vec<RegisteredWindow>,
    ) -> Result<Self, ScopeError> {
        let role = role.into();
        if role.len() > 128 || windows.is_empty() {
            return Err(ScopeError::Incomplete);
        }
        if windows
            .iter()
            .any(|window| window.binding.owner != instance)
        {
            return Err(ScopeError::Incomplete);
        }
        Ok(Self {
            instance,
            role,
            proof,
            windows,
        })
    }

    /// Role label. It is not an authorization key.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Recorded executable proof. It does not authorize a live process.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn proof(&self) -> &ExecutableProof {
        &self.proof
    }

    /// Process this target admits. Memory only; no timeout or cancellation.
    pub fn instance(&self) -> &ProcessInstance {
        &self.instance
    }
}

/// The allowed set for one engine run and one control-lease generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureScope {
    run_id: String,
    generation: u64,
    engine_instance: String,
    desktop_session: String,
    control_lease: String,
    targets: Vec<FixtureTarget>,
}

impl FixtureScope {
    /// Validate a scope record. Memory only; no blocking, timeout, or cancellation.
    ///
    /// Empty run, engine, desktop, or control-lease strings are rejected.
    /// Generation zero is rejected. Duplicate instances or window ids are rejected.
    /// An empty target list is valid and admits nothing.
    pub fn try_new(
        run_id: impl Into<String>,
        generation: u64,
        engine_instance: impl Into<String>,
        desktop_session: impl Into<String>,
        control_lease: impl Into<String>,
        targets: Vec<FixtureTarget>,
    ) -> Result<Self, ScopeError> {
        let run_id = run_id.into();
        let engine_instance = engine_instance.into();
        let desktop_session = desktop_session.into();
        let control_lease = control_lease.into();
        if !token_ok(&run_id)
            || !token_ok(&engine_instance)
            || !token_ok(&desktop_session)
            || !token_ok(&control_lease)
            || generation == 0
        {
            return Err(ScopeError::Incomplete);
        }
        let mut instances = Vec::new();
        let mut windows = Vec::new();
        for target in &targets {
            if instances.iter().any(|seen| seen == &target.instance) {
                return Err(ScopeError::Incomplete);
            }
            instances.push(target.instance.clone());
            for window in &target.windows {
                if windows.iter().any(|seen| seen == &window.binding.native_id) {
                    return Err(ScopeError::Incomplete);
                }
                windows.push(window.binding.native_id.clone());
            }
        }
        Ok(Self {
            run_id,
            generation,
            engine_instance,
            desktop_session,
            control_lease,
            targets,
        })
    }

    /// Monotonic generation. Memory only; no timeout or cancellation.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Engine run id. Memory only; no timeout or cancellation.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Assembler engine instance. Not an authorization key.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn engine_instance(&self) -> &str {
        &self.engine_instance
    }

    /// Desktop session id recorded by the assembler.
    ///
    /// Memory only. No blocking, timeout, or cancellation. Not an authorization key.
    pub fn desktop_session(&self) -> &str {
        &self.desktop_session
    }

    /// Control-lease id recorded by the assembler.
    ///
    /// Memory only. No blocking, timeout, or cancellation. The private transport
    /// that carries this id is a later card.
    pub fn control_lease(&self) -> &str {
        &self.control_lease
    }

    pub(crate) fn admits_instance(&self, instance: &ProcessInstance) -> bool {
        self.targets
            .iter()
            .any(|target| &target.instance == instance)
    }

    pub(crate) fn admits_window(&self, binding: &WindowBinding) -> bool {
        self.targets.iter().any(|target| {
            target.instance == binding.owner
                && target
                    .windows
                    .iter()
                    .any(|window| &window.binding == binding)
        })
    }
}

fn token_ok(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256
}

#[derive(Debug)]
pub(super) enum LeaseState {
    Active(FixtureScope),
    Revoked,
}

#[derive(Debug)]
pub(super) struct LeaseShared {
    pub(super) state: RwLock<LeaseState>,
    /// Active generation, or `0` after revoke. A publish is stored even when
    /// nobody is subscribed.
    pub(super) epoch: watch::Sender<u64>,
    pub(super) waiters: Mutex<Vec<Waker>>,
    pub(super) commit_before: std::sync::Arc<crate::action::PublishGate>,
    pub(super) commit_during: std::sync::Arc<crate::action::PublishGate>,
    pub(super) commit_after: std::sync::Arc<crate::action::PublishGate>,
}

/// Shared scope generation. Clones share the record. The lock is never held
/// across an await. Poison keeps the last value instead of panicking.
///
/// [`Self::rebind`] and [`Self::revoke`] publish on an in-memory watch. A caller
/// that already copied a scope must compare it again before handing out a result.
#[derive(Debug, Clone)]
pub struct ScopeLease {
    pub(super) shared: Arc<LeaseShared>,
}

impl ScopeLease {
    /// Install a validated scope. Memory only; no blocking, timeout, or cancellation.
    pub fn install(scope: FixtureScope) -> Self {
        let (epoch, _) = watch::channel(scope.generation());
        Self {
            shared: Arc::new(LeaseShared {
                state: RwLock::new(LeaseState::Active(scope)),
                epoch,
                waiters: Mutex::new(Vec::new()),
                commit_before: crate::action::PublishGate::new(),
                commit_during: crate::action::PublishGate::new(),
                commit_after: crate::action::PublishGate::new(),
            }),
        }
    }

    /// Clone the active scope, or `None` when it was revoked.
    ///
    /// The lock is dropped before this returns. Memory only. No timeout or
    /// cancellation. A poisoned lock yields the inner value.
    pub fn current(&self) -> Option<FixtureScope> {
        let guard = self
            .shared
            .state
            .read()
            .unwrap_or_else(|err| err.into_inner());
        match &*guard {
            LeaseState::Active(scope) => Some(scope.clone()),
            LeaseState::Revoked => None,
        }
    }

    /// Whether `opened` is still the active record, including its generation.
    ///
    /// Memory only. No blocking, timeout, or cancellation. A poisoned lock
    /// yields the inner value.
    pub fn matches(&self, opened: &FixtureScope) -> bool {
        self.current().as_ref() == Some(opened)
    }

    /// Whether the active scope still has this run and generation.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub fn same_epoch(&self, run_id: &str, generation: u64) -> bool {
        self.current()
            .is_some_and(|scope| scope.run_id() == run_id && scope.generation() == generation)
    }

    /// Watch that publishes the active generation, or `0` after revoke.
    ///
    /// Memory only. This does not block. Waiting is the caller's cancellation.
    ///
    /// A publish is stored when no receiver exists. The next [`Self::subscribe`]
    /// observes that generation. Do not mark an unseen update seen unless it is
    /// still the generation the caller compared.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.shared.epoch.subscribe()
    }

    pub(super) fn register_waker(&self, waker: &Waker) {
        let mut waiters = self
            .shared
            .waiters
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if waiters.iter().any(|existing| existing.will_wake(waker)) {
            return;
        }
        waiters.push(waker.clone());
    }

    /// Replace the scope when the run matches and the generation is newer.
    ///
    /// Memory only. No blocking, timeout, or cancellation. Replay of an older
    /// or equal generation fails. Rebind after revoke fails. A successful
    /// replacement wakes [`Self::subscribe`] after the lock is dropped.
    pub fn rebind(&self, next: FixtureScope) -> Result<(), ScopeError> {
        let published = {
            let mut guard = self
                .shared
                .state
                .write()
                .unwrap_or_else(|err| err.into_inner());
            match &mut *guard {
                LeaseState::Revoked => return Err(ScopeError::Revoked),
                LeaseState::Active(current) => {
                    if current.run_id != next.run_id {
                        return Err(ScopeError::RunMismatch);
                    }
                    if next.generation <= current.generation {
                        return Err(ScopeError::StaleGeneration);
                    }
                    let generation = next.generation();
                    *current = next;
                    generation
                }
            }
        };
        publish(&self.shared, published);
        Ok(())
    }

    /// Drop the active scope. Later calls refuse. Memory only.
    ///
    /// No blocking, timeout, or cancellation. Idempotent. Wakes
    /// [`Self::subscribe`] with generation `0` after the lock is dropped.
    pub fn revoke(&self) {
        {
            let mut guard = self
                .shared
                .state
                .write()
                .unwrap_or_else(|err| err.into_inner());
            *guard = LeaseState::Revoked;
        }
        publish(&self.shared, 0);
    }
}

fn publish(shared: &LeaseShared, generation: u64) {
    shared.epoch.send_modify(|slot| *slot = generation);
    let waiters = {
        let mut guard = shared.waiters.lock().unwrap_or_else(|err| err.into_inner());
        std::mem::take(&mut *guard)
    };
    for waker in waiters {
        waker.wake();
    }
}
