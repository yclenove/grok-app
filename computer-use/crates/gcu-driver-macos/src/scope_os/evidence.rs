//! Driver-held evidence. Reads go back to [`super::OsFacts`] at the call.
//!
//! [`Evidence::closed`] does not enforce delivery, so portable event tests
//! keep their current posts. [`Evidence::install`] enforces. The live adapter
//! does not enforce until a scope pins the admitted binding. A pinned binding
//! is that admitted value. A later lookup does not replace it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use gcu_core::driver::{AppRef, DriverError, Result, WindowRef};
use gcu_core::model::NativeHandle;
use gcu_core::scope::{
    ElementBinding, ExecutableProof, InstanceEvidence, ProcessInstance, WindowBinding,
};
use gcu_protocol::capabilities::ForegroundCapability;
use gcu_protocol::Capabilities;

use super::deliver::{self, DeliveryFault, PostView};
#[cfg(any(test, target_os = "macos"))]
use super::facts::Snapshot;
use super::facts::{ClosedFacts, ElementFact, OsFacts};
use super::table::{self, IdentityState};
use crate::ax::AxHandle;
use crate::driver::MacosDriver;

/// Doctor sentence for scoped macOS routes. It names no target.
pub(crate) const SCOPED_ROUTE_NOTE: &str = "scoped mode: global HID delivery is unavailable; process-wide activation is unavailable; targeted events and accessibility actions remain when they can be bound.";

/// Process, window, and element evidence for one macOS driver.
pub struct Evidence {
    facts: RwLock<Arc<dyn OsFacts>>,
    state: Mutex<IdentityState>,
    enforcing: AtomicBool,
    pinned: Mutex<Option<WindowBinding>>,
    barrier: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl Evidence {
    /// Proves nothing and does not block delivery.
    pub(crate) fn closed() -> Arc<Self> {
        Arc::new(Self::new(Arc::new(ClosedFacts), false))
    }

    /// Enforcing evidence. The previous witness table is empty.
    #[cfg(test)]
    pub(crate) fn from_facts(facts: Arc<dyn OsFacts>) -> Arc<Self> {
        Arc::new(Self::new(facts, true))
    }

    /// Live adapter with no scope pin. Missing private SPI does not block input.
    ///
    /// A later [`Evidence::pin`] is what turns delivery checks on. Memory only
    /// aside from the fact source the caller already holds. No timeout or
    /// cancellation is added here.
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn live_adapter(facts: Arc<dyn OsFacts>) -> Arc<Self> {
        Arc::new(Self::new(facts, false))
    }

    fn new(facts: Arc<dyn OsFacts>, enforcing: bool) -> Self {
        Self {
            facts: RwLock::new(facts),
            state: Mutex::new(IdentityState::default()),
            enforcing: AtomicBool::new(enforcing),
            pinned: Mutex::new(None),
            barrier: Mutex::new(None),
        }
    }

    /// Replace facts, forget witnesses, and enforce delivery.
    #[cfg(test)]
    pub(crate) fn install(&self, facts: Arc<dyn OsFacts>) {
        self.replace_facts(facts, true);
    }

    #[cfg(test)]
    fn replace_facts(&self, facts: Arc<dyn OsFacts>, enforcing: bool) {
        *self.facts.write().unwrap_or_else(|err| err.into_inner()) = facts;
        *self.lock_state() = IdentityState::default();
        self.enforcing.store(enforcing, Ordering::Release);
        self.pin(None);
        *self.barrier.lock().unwrap_or_else(|err| err.into_inner()) = None;
    }

    /// Store the admitted binding, or clear it. This does not look up a window.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub(crate) fn pin(&self, binding: Option<WindowBinding>) {
        *self.pinned.lock().unwrap_or_else(|err| err.into_inner()) = binding;
    }

    /// Admitted binding, when a scope pinned one.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub(crate) fn pinned(&self) -> Option<WindowBinding> {
        self.pinned
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    /// True when delivery must use the pinned binding.
    pub(crate) fn is_scoped(&self) -> bool {
        self.pinned().is_some()
    }

    /// Run once, immediately before the next sink check. Tests use this to
    /// change ownership after an earlier proof.
    ///
    /// Memory only until the hook runs. The hook's own blocking is the caller's.
    #[cfg(test)]
    pub(crate) fn set_pre_sink_barrier(&self, barrier: impl Fn() + Send + Sync + 'static) {
        *self.barrier.lock().unwrap_or_else(|err| err.into_inner()) = Some(Arc::new(barrier));
    }

    /// Take and run the one-shot pre-sink hook, if a test installed one.
    ///
    /// The hook runs after the lock is released. No timeout or cancellation.
    pub(crate) fn run_barrier(&self) {
        let hook = {
            let mut slot = self.barrier.lock().unwrap_or_else(|err| err.into_inner());
            slot.take()
        };
        if let Some(hook) = hook {
            hook();
        }
    }

    /// True when a post without a fresh proof is refused.
    pub(crate) fn enforcing(&self) -> bool {
        self.enforcing.load(Ordering::Acquire)
    }

    /// Shared token book, when the fact source has one.
    #[cfg(target_os = "macos")]
    pub(crate) fn token_book(&self) -> Option<Arc<super::facts::TokenBook>> {
        self.facts().token_book()
    }

    /// Process identity. A reused pid is not adopted.
    pub(crate) fn app_pid(&self, pid: u32) -> Option<ProcessInstance> {
        let facts = self.facts();
        let mut state = self.lock_state();
        table::prove_pid(&mut state, facts.as_ref(), pid)
    }

    /// Path evidence. This does not witness the pid and does not authorize it.
    ///
    /// Scope admission does not call this. Callers that record an
    /// [`ExecutableProof`] use it as evidence beside pid and start time.
    #[allow(dead_code)]
    pub(crate) fn executable_proof(&self, pid: u32) -> Option<ExecutableProof> {
        ExecutableProof::new(self.facts().executable_path(pid)?).ok()
    }

    /// Window identity for a `CGWindowID`. The embedded pid in a discovery
    /// string is not an input; callers parse it with [`parse_cg`] first.
    pub(crate) fn window_binding_cg(&self, cg_id: u32) -> Option<WindowBinding> {
        if cg_id == 0 {
            return None;
        }
        let facts = self.facts();
        let mut state = self.lock_state();
        table::sync(&mut state, facts.as_ref());
        table::binding_of(&state, cg_id)
    }

    /// Element owner. A handle that is not [`AxHandle`] is forged.
    pub(crate) fn element_handle(&self, handle: &NativeHandle) -> Option<ElementBinding> {
        let ax = handle.downcast_ref::<AxHandle>()?;
        self.element_ids(ax.pid, ax.id)
    }

    /// Element owner from a process id and a source element id.
    pub(crate) fn element_ids(&self, pid: u32, id: u64) -> Option<ElementBinding> {
        let facts = self.facts();
        let fact = facts.element(pid, id)?;
        if fact.pid != pid {
            return None;
        }
        self.bind_fact(&facts, &fact)
    }

    /// Last check on the accessibility thread. Does not call `prepare`.
    ///
    /// The fresh window must equal `expected`, so a descendant of that window
    /// passes and a jump to another window fails.
    #[cfg(test)]
    pub(crate) fn accept_gathered(&self, fact: &ElementFact, expected: &WindowBinding) -> bool {
        let facts = self.facts();
        let Some(window) = self.bind_fact(&facts, fact) else {
            return false;
        };
        window.window() == expected
    }

    /// Re-check `admitted` and the post against a fresh window list.
    pub(crate) fn prove_delivery(
        &self,
        admitted: &WindowBinding,
        post: &PostView,
    ) -> Result<(), DeliveryFault> {
        let facts = self.facts();
        let mut state = self.lock_state();
        table::sync(&mut state, facts.as_ref());
        let fresh = table::binding_of(&state, post.window_id);
        let rows = state.rows.clone();
        drop(state);
        deliver::prove_post(admitted, &rows, fresh.as_ref(), post)
    }

    fn bind_fact(&self, facts: &Arc<dyn OsFacts>, fact: &ElementFact) -> Option<ElementBinding> {
        if fact.pid == 0 {
            return None;
        }
        let cg = fact.cg_id?;
        let mut state = self.lock_state();
        table::sync(&mut state, facts.as_ref());
        let window = table::binding_of(&state, cg)?;
        if window.owner().pid() != fact.pid {
            return None;
        }
        Some(ElementBinding::new(window))
    }

    fn facts(&self) -> Arc<dyn OsFacts> {
        self.facts
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, IdentityState> {
        self.state.lock().unwrap_or_else(|err| err.into_inner())
    }

    /// Re-read the pinned window after the one-shot barrier. A changed
    /// incarnation, owner, or boot is a refusal.
    ///
    /// May block inside the fact source for the accessibility call budget.
    /// No extra timeout or cancellation is added here.
    pub(crate) fn recheck_pinned(&self) -> Result<WindowBinding> {
        self.run_barrier();
        let pin = self.pinned().ok_or_else(not_proved)?;
        let cg = parse_cg(pin.native_id()).ok_or_else(not_proved)?;
        let facts = self.facts();
        let mut state = self.lock_state();
        table::sync(&mut state, facts.as_ref());
        let fresh = table::binding_of(&state, cg).ok_or_else(not_proved)?;
        if fresh != pin {
            return Err(not_proved());
        }
        Ok(pin)
    }

    /// Compare `fact` with `admitted` using `snap` only. Geometry is not read.
    ///
    /// Memory only. No blocking, timeout, or cancellation. `snap` must already
    /// be the observation that belongs with `fact`.
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn accept_facts(
        &self,
        admitted: &WindowBinding,
        facts: &[ElementFact],
        snap: &Snapshot,
    ) -> Result<()> {
        let mut state = self.lock_state();
        table::apply(&mut state, snap);
        for fact in facts {
            let Some(cg) = fact.cg_id.filter(|id| *id > 0) else {
                return Err(not_proved());
            };
            if cg.to_string() != admitted.native_id() || fact.pid != admitted.owner().pid() {
                return Err(not_proved());
            }
            if table::binding_of(&state, cg).as_ref() != Some(admitted) {
                return Err(not_proved());
            }
        }
        Ok(())
    }

    /// Re-read the element after the barrier, then run `write` only if it is
    /// still `admitted`. Geometry is not an input.
    ///
    /// May block inside the fact source. No extra timeout or cancellation.
    #[cfg(test)]
    pub(crate) fn commit_ax_write<R>(
        &self,
        admitted: &WindowBinding,
        pid: u32,
        id: u64,
        write: impl FnOnce() -> R,
    ) -> Result<R> {
        self.run_barrier();
        let facts = self.facts();
        let fact = facts.element(pid, id).ok_or_else(not_proved)?;
        let snap = facts.snapshot().ok_or_else(not_proved)?;
        self.accept_facts(admitted, &[fact], &snap)?;
        Ok(write())
    }

    /// Pinned window for an element, or an error when the element moved.
    ///
    /// Never returns the element's new window. Memory or one fact read. No
    /// extra timeout or cancellation.
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn mutation_window(&self, handle: &NativeHandle) -> Result<WindowBinding> {
        let pin = self.pinned().ok_or_else(not_proved)?;
        let binding = self.element_handle(handle).ok_or_else(not_proved)?;
        if binding.window() != &pin {
            return Err(not_proved());
        }
        Ok(pin)
    }
}

impl InstanceEvidence for MacosDriver {
    fn app_instance(&self, app: &AppRef) -> Option<ProcessInstance> {
        self.evidence.app_pid(app.info.pid?)
    }

    fn window_binding(&self, window: &WindowRef) -> Option<WindowBinding> {
        self.evidence.window_binding_cg(parse_cg(&window.id)?)
    }

    fn element_binding(&self, handle: &NativeHandle) -> Option<ElementBinding> {
        self.evidence.element_handle(handle)
    }

    fn pin_admitted(&self, binding: Option<&WindowBinding>) {
        self.evidence.pin(binding.cloned());
    }

    fn scope_capabilities(&self, current: &Capabilities) -> Capabilities {
        let mut caps = current.clone();
        caps.delivery.foreground = ForegroundCapability::Unavailable;
        caps
    }

    fn scoped_doctor_note(&self) -> Option<&'static str> {
        Some(SCOPED_ROUTE_NOTE)
    }
}

impl MacosDriver {
    /// Install enforcing facts and forget every witness.
    #[cfg(test)]
    pub(crate) fn install_os_facts(&self, facts: Arc<dyn OsFacts>) {
        self.evidence.install(facts);
    }
}

/// `CGWindowID` from `cg:{id}:{pid}` or a decimal id.
///
/// The pid embedded in the discovery string is ignored. Zero is not an id.
pub(crate) fn parse_cg(id: &str) -> Option<u32> {
    let raw = id.strip_prefix("cg:").unwrap_or(id);
    let head = raw.split(':').next().unwrap_or(raw);
    let cg = head.parse::<u32>().ok()?;
    if cg == 0 {
        None
    } else {
        Some(cg)
    }
}

/// Refusal used before any OS post or accessibility mutation.
pub(crate) fn not_proved() -> DriverError {
    DriverError::Native {
        message: "window identity was not proved before delivery".into(),
        maybe_executed: false,
    }
}
