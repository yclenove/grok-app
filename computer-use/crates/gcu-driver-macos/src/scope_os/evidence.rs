//! Driver-held evidence. Reads go back to [`super::OsFacts`] at the call.
//!
//! [`Evidence::closed`] does not enforce delivery, so portable event tests
//! keep their current posts. [`Evidence::install`] and the live constructor
//! enforce: a post with no fresh proof is not sent.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use gcu_core::driver::{AppRef, DriverError, WindowRef};
use gcu_core::model::NativeHandle;
use gcu_core::scope::{
    ElementBinding, ExecutableProof, InstanceEvidence, ProcessInstance, WindowBinding,
};

use super::deliver::{self, DeliveryFault, PostView};
use super::facts::{ClosedFacts, ElementFact, OsFacts};
use super::table::{self, IdentityState};
use crate::ax::AxHandle;
use crate::driver::MacosDriver;

/// Process, window, and element evidence for one macOS driver.
pub struct Evidence {
    facts: RwLock<Arc<dyn OsFacts>>,
    state: Mutex<IdentityState>,
    enforcing: AtomicBool,
}

impl Evidence {
    /// Proves nothing and does not block delivery.
    pub(crate) fn closed() -> Arc<Self> {
        Arc::new(Self {
            facts: RwLock::new(Arc::new(ClosedFacts)),
            state: Mutex::new(IdentityState::default()),
            enforcing: AtomicBool::new(false),
        })
    }

    /// Enforcing evidence. The previous witness table is empty.
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn from_facts(facts: Arc<dyn OsFacts>) -> Arc<Self> {
        Arc::new(Self {
            facts: RwLock::new(facts),
            state: Mutex::new(IdentityState::default()),
            enforcing: AtomicBool::new(true),
        })
    }

    /// Replace facts, forget witnesses, and enforce delivery.
    #[cfg(test)]
    pub(crate) fn install(&self, facts: Arc<dyn OsFacts>) {
        *self.facts.write().unwrap_or_else(|err| err.into_inner()) = facts;
        *self.lock_state() = IdentityState::default();
        self.enforcing.store(true, Ordering::Release);
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
        facts.prepare();
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
        facts.prepare();
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
    #[cfg(any(test, target_os = "macos"))]
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
        facts.prepare();
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
