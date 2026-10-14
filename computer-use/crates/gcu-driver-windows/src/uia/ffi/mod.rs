//! COM bindings. Every interface is created, used, and destroyed on the MTA.
mod events;
mod properties;
use super::{
    cache::{Properties, BATCH_SIZE, PROPERTY_IDS},
    events::{EventRegistration, EventSink, TreeVersion, UiaEventSource},
    source::{ChildPage, NodeId, UiaSource},
    worker::{Request, Worker},
};
use crate::ffi::Com;
use async_trait::async_trait;
use gcu_core::driver::{DriverError, Result};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use windows::{
    core::Interface,
    Win32::{
        Foundation::HWND,
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        UI::Accessibility::*,
    },
};

/// Production UIA source. One persistent MTA; callers never hold COM pointers.
/// Every job has a DRIVER_DEADLINE ceiling. A tree future's drop abandons work
/// between COM calls; hung providers cannot block cancellation, Drop, or Stop.
pub struct RealUia {
    worker: Worker<State>,
    version: TreeVersion,
}
impl RealUia {
    /// Initialize COM/UIA on its native worker, bounded by DRIVER_DEADLINE.
    /// No applications are launched and Chromium accessibility settings are untouched.
    pub async fn start() -> Result<Self> {
        Ok(Self {
            worker: Worker::start(State::new, State::maintain).await?,
            version: TreeVersion::default(),
        })
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
    // Last field: interfaces must release before CoUninitialize.
    _com: Com,
}
impl State {
    fn new() -> Result<Self> {
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
        Ok(Self {
            automation,
            walker,
            cache,
            elements: BTreeMap::new(),
            generation: 0,
            slot: 0,
            registrations: Vec::new(),
            _com: com,
        })
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
    fn root(&mut self, hwnd: u64, request: &Request) -> Result<Option<NodeId>> {
        self.elements.clear();
        self.generation += 1;
        self.slot = 0;
        request.check()?;
        // SAFETY: exact validated HWND, cache/UIA belong to this MTA.
        let element = unsafe {
            self.automation
                .ElementFromHandleBuildCache(HWND(hwnd as usize as *mut _), &self.cache)
        }
        .map_err(native)?;
        Ok(Some(self.put(element)))
    }
    fn children(
        &mut self,
        node: NodeId,
        after: Option<NodeId>,
        limit: usize,
        request: &Request,
    ) -> Result<ChildPage> {
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
        status.ok().map_err(native)?;
        if result.is_null() {
            return Ok(None);
        }
        // SAFETY: success returned a non-null owned +1 element; from_raw takes ownership.
        Ok(Some(unsafe { IUIAutomationElement::from_raw(result) }))
    }
}
#[async_trait]
impl UiaSource for RealUia {
    async fn root(&self, hwnd: u64) -> Result<Option<NodeId>> {
        self.worker.call(move |s, r| s.root(hwnd, r)).await
    }
    async fn properties(&self, node: NodeId) -> Result<Properties> {
        self.worker
            .call(move |s, r| {
                r.check()?;
                Ok(properties::read(&s.element(node)?))
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
fn native(error: windows::core::Error) -> DriverError {
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
