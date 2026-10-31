//! COM bindings. Every interface is created, used, and destroyed on the MTA.
mod actions;
mod events;
mod identity;
mod input;
mod properties;
use super::{
    cache::{Properties, BATCH_SIZE, PROPERTY_IDS},
    events::{EventRegistration, EventSink, TreeVersion, UiaEventSource},
    source::{AdmittedWindow, ChildPage, NodeId, UiaSource},
    worker::{Request, Worker},
};
use crate::ffi::Com;
use async_trait::async_trait;
use gcu_core::driver::{DriverError, Result};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use windows::{
    core::Interface,
    Win32::{
        Foundation::HWND,
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        UI::{
            Accessibility::*,
            WindowsAndMessaging::{GetAncestor, GA_ROOT},
        },
    },
};

/// Production UIA source. One persistent MTA; callers never hold COM pointers.
/// Every job has a DRIVER_DEADLINE ceiling. A tree future's drop abandons work
/// between COM calls; hung providers cannot block cancellation, Drop, or Stop.
pub struct RealUia {
    worker: Worker<State>,
    version: TreeVersion,
    /// `(owner pid, top-level HWND)` recorded when a fragment was proved.
    facts: Arc<Mutex<BTreeMap<NodeId, (u32, u64)>>>,
}
impl RealUia {
    /// Initialize COM/UIA on its native worker, bounded by DRIVER_DEADLINE.
    /// No applications are launched and Chromium accessibility settings are untouched.
    pub async fn start() -> Result<Self> {
        let facts = Arc::new(Mutex::new(BTreeMap::new()));
        let shared = Arc::clone(&facts);
        Ok(Self {
            worker: Worker::start(move || State::new(shared), State::maintain).await?,
            version: TreeVersion::default(),
            facts,
        })
    }
    /// Owner pid and top-level HWND of `node`, without its name or value.
    ///
    /// Unknown nodes are [`DriverError::StaleElement`]. An unproved owner is
    /// [`DriverError::ScopeDenied`]. This does not read the property cache.
    pub(crate) async fn prove_fragment(&self, node: NodeId) -> Result<(u32, u64)> {
        self.worker
            .call(move |state, request| {
                request.check()?;
                let element = state.element(node)?;
                let fact = state.fragment_owner(&element, request)?;
                state.record(node, fact.0, fact.1)?;
                Ok(fact)
            })
            .await
    }
}
struct State {
    automation: IUIAutomation,
    walker: IUIAutomationTreeWalker,
    cache: IUIAutomationCacheRequest,
    elements: BTreeMap<NodeId, IUIAutomationElement>,
    generation: u64,
    slot: u64,
    registrations: Vec<events::Registration>,
    observed: BTreeMap<NodeId, identity::Observed>,
    window: Option<identity::WindowIdentity>,
    api: crate::RealWinApi,
    /// Claim copied by `open_root`. `None` keeps the unscoped child route.
    admitted: Option<AdmittedWindow>,
    facts: Arc<Mutex<BTreeMap<NodeId, (u32, u64)>>>,
    // Last field: interfaces must release before CoUninitialize.
    _com: Com,
}
impl State {
    fn new(facts: Arc<Mutex<BTreeMap<NodeId, (u32, u64)>>>) -> Result<Self> {
        let _timing = crate::timing::Timing::start("uia_initialize");
        let com = Com::new()?;
        // SAFETY: this dedicated thread has initialized MTA; interfaces stay here.
        let (automation, walker, cache) = unsafe {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER).map_err(native)?;
            let settings: IUIAutomation2 = automation.cast().map_err(native)?;
            // Native transaction limits complement the caller's independent deadline.
            settings.SetConnectionTimeout(500).map_err(native)?;
            settings.SetTransactionTimeout(500).map_err(native)?;
            let cache = automation.CreateCacheRequest().map_err(native)?;
            cache.SetTreeScope(TreeScope_Subtree).map_err(native)?;
            // Cache singleton subtree fragments. A false descendant filter prevents
            // providers materializing an unbounded subtree inside BuildCache. The
            // traversal root is always cached; raw-view navigation fetches the next
            // fragment. Portable child pages group up to BATCH_SIZE fragments.
            cache
                .SetTreeFilter(&automation.CreateFalseCondition().map_err(native)?)
                .map_err(native)?;
            for id in PROPERTY_IDS {
                cache.AddProperty(UIA_PROPERTY_ID(*id)).map_err(native)?;
            }
            // Deliberately no AddPattern: availability/state properties suffice.
            let walker = automation.RawViewWalker().map_err(native)?;
            (automation, walker, cache)
        };
        tracing::info!(target: "gcu_windows_uia", tree_scope = "Subtree (singleton false-filter fragments)",
            batch_size = BATCH_SIZE, property_count = PROPERTY_IDS.len(), connection_timeout_ms = 500,
            transaction_timeout_ms = 500, "UIA_CACHE_CONFIGURATION");
        Ok(Self {
            automation,
            walker,
            cache,
            elements: BTreeMap::new(),
            generation: 0,
            slot: 0,
            registrations: Vec::new(),
            observed: BTreeMap::new(),
            window: None,
            api: crate::RealWinApi::new()?,
            admitted: None,
            facts,
            _com: com,
        })
    }
    fn record(&self, id: NodeId, pid: u32, top: u64) -> Result<()> {
        let mut facts = self.facts.lock().map_err(|_| DriverError::ScopeDenied)?;
        facts.insert(id, (pid, top));
        Ok(())
    }
    fn clear_facts(&self) {
        if let Ok(mut facts) = self.facts.lock() {
            facts.clear();
        }
    }
    fn maintain(&mut self) {
        self.registrations.retain(|reg| !reg.closed());
    }
    fn put(&mut self, element: IUIAutomationElement) -> NodeId {
        self.slot += 1;
        let id = NodeId {
            generation: self.generation,
            slot: self.slot,
        };
        self.elements.insert(id, element);
        id
    }
    fn element(&self, id: NodeId) -> Result<IUIAutomationElement> {
        self.elements
            .get(&id)
            .cloned()
            .ok_or(DriverError::StaleElement)
    }
    fn owner_pid(&self, id: NodeId) -> Result<u32> {
        let element = self.element(id)?;
        // SAFETY: MTA-local element. CurrentProcessId reads one property and does
        // not walk the tree or add UIA_ProcessId to the cache request.
        // The COM property is a signed LONG. Zero and negative values are not a pid.
        let pid = unsafe { element.CurrentProcessId() }.map_err(|_| DriverError::ScopeDenied)?;
        let pid = u32::try_from(pid).map_err(|_| DriverError::ScopeDenied)?;
        if pid == 0 {
            return Err(DriverError::ScopeDenied);
        }
        Ok(pid)
    }
    fn root(&mut self, hwnd: u64, request: &Request) -> Result<Option<NodeId>> {
        self.admitted = None;
        self.clear_facts();
        {
            let _timing = crate::timing::Timing::provider("uia_release_previous_elements");
            self.elements.clear();
        }
        self.observed.clear();
        self.window = Some(identity::window(&self.api, hwnd, request)?);
        self.generation += 1;
        self.slot = 0;
        request.check()?;
        let started = tokio::time::Instant::now();
        let _timing = crate::timing::Timing::provider("ElementFromHandleBuildCache");
        // SAFETY: exact validated HWND and MTA-local automation/cache interfaces.
        let element = unsafe {
            self.automation
                .ElementFromHandleBuildCache(HWND(hwnd as usize as *mut _), &self.cache)
        }
        .map_err(|e| {
            super::diagnostics::native_call(
                "ElementFromHandleBuildCache",
                started,
                e,
                &Properties::default(),
            )
        })?;
        Ok(Some(self.put(element)))
    }
    fn children(
        &mut self,
        node: NodeId,
        after: Option<NodeId>,
        limit: usize,
        request: &Request,
    ) -> Result<ChildPage> {
        if self.admitted.is_some() {
            return self.children_scoped(node, after, limit, request);
        }
        if limit == 0 || limit > BATCH_SIZE {
            return Err(DriverError::InvalidRequest);
        }
        let mut next = if let Some(after) = after {
            self.next(&self.element(after)?, false, request)?
        } else {
            self.next(&self.element(node)?, true, request)?
        };
        let mut nodes = Vec::new();
        while let Some(element) = next {
            request.check()?;
            if nodes.len() == limit {
                // Lookahead was cached but is not retained. Next page re-reads it
                // from the preceding sibling, so no unbounded cursor map exists.
                return Ok(ChildPage { nodes, more: true });
            }
            next = self.next(&element, false, request)?;
            nodes.push(self.put(element));
        }
        Ok(ChildPage { nodes, more: false })
    }
    fn next(
        &self,
        element: &IUIAutomationElement,
        first: bool,
        request: &Request,
    ) -> Result<Option<IUIAutomationElement>> {
        request.check()?;
        let mut result = std::ptr::null_mut();
        let started = tokio::time::Instant::now();
        let _timing = crate::timing::Timing::provider(if first {
            "GetFirstChildElementBuildCache"
        } else {
            "GetNextSiblingElementBuildCache"
        });
        // SAFETY: all interfaces are MTA-local and live; out pointer is valid.
        // Calling the vtable preserves S_OK/null (documented end-of-list) versus
        // an actual provider E_POINTER failure, which must mark the tree partial.
        let status = unsafe {
            let vtable = self.walker.vtable();
            let method = if first {
                vtable.GetFirstChildElementBuildCache
            } else {
                vtable.GetNextSiblingElementBuildCache
            };
            method(
                self.walker.as_raw(),
                element.as_raw(),
                self.cache.as_raw(),
                &mut result,
            )
        };
        status.ok().map_err(|e| {
            super::diagnostics::native_call(
                if first {
                    "GetFirstChildElementBuildCache"
                } else {
                    "GetNextSiblingElementBuildCache"
                },
                started,
                e,
                &properties::read(element),
            )
        })?;
        if result.is_null() {
            return Ok(None);
        }
        // SAFETY: success returned a non-null owned +1 element; from_raw takes ownership.
        Ok(Some(unsafe { IUIAutomationElement::from_raw(result) }))
    }
    fn open_scoped(
        &mut self,
        hwnd: u64,
        admitted: AdmittedWindow,
        request: &Request,
    ) -> Result<Option<NodeId>> {
        self.admitted = None;
        self.clear_facts();
        {
            let _timing = crate::timing::Timing::provider("uia_release_previous_elements");
            self.elements.clear();
        }
        self.observed.clear();
        self.window = None;
        if hwnd != admitted.hwnd || admitted.hwnd == 0 {
            return Err(DriverError::ScopeDenied);
        }
        let identity = identity::window_matching(&self.api, &admitted, request)?;
        self.generation += 1;
        self.slot = 0;
        request.check()?;
        let started = tokio::time::Instant::now();
        // SAFETY: exact HWND. No cache, so a foreign window is not materialized.
        let element = unsafe {
            self.automation
                .ElementFromHandle(HWND(hwnd as usize as *mut _))
        }
        .map_err(|error| {
            super::diagnostics::native_call(
                "ElementFromHandle",
                started,
                error,
                &Properties::default(),
            )
        })?;
        let (pid, top) = self.fragment_owner(&element, request)?;
        crate::scope_identity::admit_delivery_element(
            admitted.hwnd,
            top,
            "",
            Some(pid),
            Some(admitted.pid),
        )?;
        crate::scope_identity::admit_fragment(
            &crate::scope_identity::WindowClaim::from(&admitted),
            Some(crate::scope_identity::FragmentFact {
                owner_pid: pid,
                top_hwnd: top,
            }),
        )?;
        let started = tokio::time::Instant::now();
        // SAFETY: the fragment above belongs to the admitted window. Cache only that element.
        let cached = unsafe { element.BuildUpdatedCache(&self.cache) }.map_err(|error| {
            super::diagnostics::native_call(
                "BuildUpdatedCache",
                started,
                error,
                &Properties::default(),
            )
        })?;
        let id = self.put(cached);
        self.record(id, pid, top)?;
        self.window = Some(identity);
        self.admitted = Some(admitted);
        Ok(Some(id))
    }
    fn children_scoped(
        &mut self,
        node: NodeId,
        after: Option<NodeId>,
        limit: usize,
        request: &Request,
    ) -> Result<ChildPage> {
        if limit == 0 || limit > BATCH_SIZE {
            return Err(DriverError::InvalidRequest);
        }
        let admitted = self.admitted.clone().ok_or(DriverError::ScopeDenied)?;
        let mut next = if let Some(after) = after {
            self.next_raw(&self.element(after)?, false, request)?
        } else {
            self.next_raw(&self.element(node)?, true, request)?
        };
        let mut nodes = Vec::new();
        while let Some(element) = next {
            request.check()?;
            if nodes.len() == limit {
                return Ok(ChildPage { nodes, more: true });
            }
            next = self.next_raw(&element, false, request)?;
            let (pid, top) = match self.fragment_owner(&element, request) {
                Ok(fact) => fact,
                Err(DriverError::ScopeDenied) => continue,
                Err(error) => return Err(error),
            };
            let element_ok = crate::scope_identity::admit_delivery_element(
                admitted.hwnd,
                top,
                "",
                Some(pid),
                Some(admitted.pid),
            );
            let fragment_ok = crate::scope_identity::admit_fragment(
                &crate::scope_identity::WindowClaim::from(&admitted),
                Some(crate::scope_identity::FragmentFact {
                    owner_pid: pid,
                    top_hwnd: top,
                }),
            );
            if element_ok.is_err() || fragment_ok.is_err() {
                continue;
            }
            let started = tokio::time::Instant::now();
            // SAFETY: this child matched the admitted window. Cache only then.
            let cached = unsafe { element.BuildUpdatedCache(&self.cache) }.map_err(|error| {
                super::diagnostics::native_call(
                    "BuildUpdatedCache",
                    started,
                    error,
                    &Properties::default(),
                )
            })?;
            let id = self.put(cached);
            self.record(id, pid, top)?;
            nodes.push(id);
        }
        Ok(ChildPage { nodes, more: false })
    }
    fn next_raw(
        &self,
        element: &IUIAutomationElement,
        first: bool,
        request: &Request,
    ) -> Result<Option<IUIAutomationElement>> {
        request.check()?;
        let mut result = std::ptr::null_mut();
        let started = tokio::time::Instant::now();
        let name = if first {
            "GetFirstChildElement"
        } else {
            "GetNextSiblingElement"
        };
        // SAFETY: MTA-local walker. The non-cache method does not build a property bag.
        let status = unsafe {
            let vtable = self.walker.vtable();
            let method = if first {
                vtable.GetFirstChildElement
            } else {
                vtable.GetNextSiblingElement
            };
            method(self.walker.as_raw(), element.as_raw(), &mut result)
        };
        status.ok().map_err(|error| {
            super::diagnostics::native_call(name, started, error, &Properties::default())
        })?;
        if result.is_null() {
            return Ok(None);
        }
        // SAFETY: success returned a non-null owned element.
        Ok(Some(unsafe { IUIAutomationElement::from_raw(result) }))
    }
    fn fragment_owner(
        &self,
        element: &IUIAutomationElement,
        request: &Request,
    ) -> Result<(u32, u64)> {
        request.check()?;
        // SAFETY: one process-id property. Name and value are not read.
        let pid = unsafe { element.CurrentProcessId() }.map_err(|_| DriverError::ScopeDenied)?;
        let pid = u32::try_from(pid).map_err(|_| DriverError::ScopeDenied)?;
        if pid == 0 {
            return Err(DriverError::ScopeDenied);
        }
        let mut current = element.clone();
        for _ in 0..=super::tree::DEPTH_CAP {
            request.check()?;
            // SAFETY: native window handle only. No property bag.
            let native = unsafe { current.CurrentNativeWindowHandle() }
                .map_err(|_| DriverError::ScopeDenied)?;
            if !native.0.is_null() {
                // SAFETY: numeric HWND. GA_ROOT returns the top-level window or null.
                let root = unsafe { GetAncestor(native, GA_ROOT) };
                let top = root.0 as usize as u64;
                if top == 0 {
                    return Err(DriverError::ScopeDenied);
                }
                return Ok((pid, top));
            }
            match self.parent_element(&current)? {
                Some(parent) => current = parent,
                None => return Err(DriverError::ScopeDenied),
            }
        }
        Err(DriverError::ScopeDenied)
    }
    fn parent_element(
        &self,
        element: &IUIAutomationElement,
    ) -> Result<Option<IUIAutomationElement>> {
        let mut result = std::ptr::null_mut();
        // SAFETY: MTA-local walker. Failure is unproved; it does not read properties.
        let status = unsafe {
            (self.walker.vtable().GetParentElement)(
                self.walker.as_raw(),
                element.as_raw(),
                &mut result,
            )
        };
        if status.is_err() {
            return Err(DriverError::ScopeDenied);
        }
        if result.is_null() {
            return Ok(None);
        }
        // SAFETY: success returned a non-null owned element.
        Ok(Some(unsafe { IUIAutomationElement::from_raw(result) }))
    }
}
#[async_trait]
impl UiaSource for RealUia {
    async fn owner_pid(&self, node: NodeId) -> Result<Option<u32>> {
        self.worker
            .call(move |state, request| {
                request.check()?;
                state.owner_pid(node).map(Some)
            })
            .await
    }
    async fn validate(&self, node: NodeId, hwnd: u64) -> Result<Properties> {
        self.worker
            .call(move |s, r| s.validate(node, hwnd, r).map(|(_, p)| p))
            .await
    }
    async fn action(
        &self,
        node: NodeId,
        hwnd: u64,
        action: super::patterns::PatternAction,
        context: super::patterns::ActionContext,
    ) -> Result<super::patterns::PatternResult> {
        self.worker
            .call_action(context, move |s, r| {
                actions::perform(s, node, hwnd, action, r)
            })
            .await
    }

    async fn root(&self, hwnd: u64) -> Result<Option<NodeId>> {
        self.worker.call(move |s, r| s.root(hwnd, r)).await
    }
    async fn open_root(
        &self,
        hwnd: u64,
        admitted: Option<AdmittedWindow>,
    ) -> Result<Option<NodeId>> {
        match admitted {
            Some(admitted) => {
                self.worker
                    .call(move |state, request| state.open_scoped(hwnd, admitted, request))
                    .await
            }
            None => self.root(hwnd).await,
        }
    }
    async fn fragment(&self, node: NodeId) -> Result<Option<(u32, u64)>> {
        match self.prove_fragment(node).await {
            Ok(fact) => Ok(Some(fact)),
            Err(DriverError::ScopeDenied) => Ok(None),
            Err(error) => Err(error),
        }
    }
    fn fragment_now(&self, node: NodeId) -> Option<(u32, u64)> {
        self.facts.lock().ok()?.get(&node).copied()
    }
    async fn properties(&self, node: NodeId) -> Result<Properties> {
        self.worker
            .call(move |s, r| {
                r.check()?;
                s.observe(node, r)
            })
            .await
    }
    async fn children(
        &self,
        node: NodeId,
        after: Option<NodeId>,
        limit: usize,
    ) -> Result<ChildPage> {
        self.worker
            .call(move |s, r| s.children(node, after, limit, r))
            .await
    }
}
struct Lease(Arc<AtomicBool>);
impl EventRegistration for Lease {}
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
impl UiaEventSource for RealUia {
    fn register(&self, hwnd: u64, sink: EventSink) -> Result<Box<dyn EventRegistration>> {
        let closed = Arc::new(AtomicBool::new(false));
        let flag = closed.clone();
        self.worker.enqueue(move |s, r| {
            if sink.is_closed() || flag.load(Ordering::Acquire) {
                return;
            }
            // Failure closes the stream (sink drops); partial registration rolls back.
            if let Ok(registration) = events::Registration::new(&s.automation, hwnd, sink, flag, r)
            {
                s.registrations.push(registration);
            }
        })?;
        Ok(Box::new(Lease(closed)))
    }
    fn version(&self) -> TreeVersion {
        self.version.clone()
    }
}
pub(super) fn native(error: windows::core::Error) -> DriverError {
    tracing::warn!(target: "gcu_windows_uia", hresult = error.code().0 as u32, "UIA_NATIVE_HRESULT");
    if error.code().0 as u32 == UIA_E_TIMEOUT || error.code().0 as u32 == 0x800705b4 {
        DriverError::Timeout {
            maybe_executed: false,
        }
    } else {
        DriverError::Native {
            message: format!("UIA HRESULT {:08x}", error.code().0 as u32),
            maybe_executed: false,
        }
    }
}
