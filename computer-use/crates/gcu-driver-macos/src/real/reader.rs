//! Live [`AxSource`](crate::ax::AxSource). One batch attribute read per node.
//!
//! `AXUIElementCopyMultipleAttributeValues` uses option 0, so one missing
//! attribute does not fail the node. Text is read with `AXStringForRange`
//! when `AXNumberOfCharacters` is above the fetch limit; otherwise `AXValue`
//! is copied and the tree truncates it. Password fields release the value
//! without converting it to a Rust string.
//!
//! Element pointers stay in [`super::world::World`]. This type only stores ids.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use accessibility_sys::{kAXErrorAPIDisabled, kAXErrorSuccess, AXUIElementCopyAttributeValue};
use async_trait::async_trait;
use core_foundation::base::{CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::CFString;
use gcu_core::{
    driver::DriverError,
    model::{NativeHandle, NodeAction, NodeValue, Rect, TextRange},
    scope::WindowBinding,
};

use super::act;
use super::parse::{self, Facts};
use super::thread::{AxThread, AX_CALL_TIMEOUT};
use super::world::{self, World};
use crate::ax::source::{AxCall, AxHandle, AxIdentity, AxSource, AxStateBits};
use crate::ax::{ToggleName, ToggleStore};
use gcu_core::driver::Result;

/// AX source. Nodes are ids into the worker's element map.
pub struct RealAxSource {
    thread: Arc<AxThread>,
    cache: Mutex<HashMap<u64, Facts>>,
    root: u64,
    pid: u32,
    text_limit: Mutex<Option<usize>>,
    evidence: Option<Arc<crate::scope_os::Evidence>>,
    expected: Mutex<Option<WindowBinding>>,
}

impl RealAxSource {
    /// Resolve `AXWindows[index]` and keep that element for the walk.
    ///
    /// The snapshot element map is cleared first. Application elements used by
    /// toggles are not cleared.
    pub async fn open(thread: Arc<AxThread>, pid: u32, index: u32) -> Result<Self> {
        let root = thread
            .call(AX_CALL_TIMEOUT, false, move |world| {
                parse::open_root(world, pid, index)
            })
            .await?;
        Ok(Self {
            thread,
            cache: Mutex::new(HashMap::new()),
            root,
            pid,
            text_limit: Mutex::new(None),
            evidence: None,
            expected: Mutex::new(None),
        })
    }

    /// Root id returned by [`RealAxSource::open`].
    pub fn root(&self) -> u64 {
        self.root
    }

    /// Character budget for the next reads. `None` still honors the byte cap.
    pub fn set_text_limit(&self, limit: Option<usize>) {
        if let Ok(mut slot) = self.text_limit.lock() {
            *slot = limit;
        }
    }

    /// Action source for elements a snapshot already stored.
    ///
    /// Does not reset the element map. The next [`RealAxSource::open`] does.
    /// `evidence` is checked again inside each mutation, on this thread.
    pub fn attach(thread: Arc<AxThread>, evidence: Option<Arc<crate::scope_os::Evidence>>) -> Self {
        Self {
            thread,
            cache: Mutex::new(HashMap::new()),
            root: 0,
            pid: 0,
            text_limit: Mutex::new(None),
            evidence,
            expected: Mutex::new(None),
        }
    }

    /// Window the mutation must still belong to. Set before `AX` perform.
    pub fn set_expected(&self, window: WindowBinding) {
        *self.expected.lock().unwrap_or_else(|err| err.into_inner()) = Some(window);
    }

    async fn mutate<T: Send + 'static>(
        &self,
        ids: Vec<u64>,
        job: impl FnOnce(&mut World) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let evidence = self.evidence.clone();
        let expected = self
            .expected
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        self.thread
            .call(AX_CALL_TIMEOUT, true, move |world| {
                super::prove_mutation(evidence.as_deref(), expected.as_ref(), &ids, world)?;
                job(world)
            })
            .await
    }

    async fn ensure(&self, id: u64) -> Result<Facts> {
        if let Some(found) = self
            .cache
            .lock()
            .ok()
            .and_then(|guard| guard.get(&id).cloned())
        {
            return Ok(found);
        }
        let limit = self.text_limit.lock().ok().and_then(|guard| *guard);
        let facts = self
            .thread
            .call(AX_CALL_TIMEOUT, false, move |world| {
                parse::read_facts(world, id, limit)
            })
            .await?;
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(id, facts.clone());
        }
        Ok(facts)
    }
}

#[async_trait]
impl AxSource for RealAxSource {
    type Node = u64;

    async fn children(&self, node: &Self::Node) -> Result<Vec<Self::Node>> {
        Ok(self.ensure(*node).await?.children)
    }
    async fn attributes(&self, node: &Self::Node) -> Result<HashMap<String, String>> {
        Ok(self.ensure(*node).await?.attributes)
    }
    async fn states(&self, node: &Self::Node) -> Result<AxStateBits> {
        Ok(self.ensure(*node).await?.states)
    }
    async fn role(&self, node: &Self::Node) -> Result<String> {
        Ok(self.ensure(*node).await?.role)
    }
    async fn subrole(&self, node: &Self::Node) -> Result<Option<String>> {
        Ok(self.ensure(*node).await?.subrole)
    }
    async fn title(&self, node: &Self::Node) -> Result<Option<String>> {
        Ok(self.ensure(*node).await?.title)
    }
    async fn value(&self, node: &Self::Node) -> Result<Option<NodeValue>> {
        Ok(self.ensure(*node).await?.value)
    }
    async fn fresh_value(&self, node: &Self::Node) -> Result<Option<NodeValue>> {
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(node);
        }
        self.value(node).await
    }
    async fn description(&self, node: &Self::Node) -> Result<Option<String>> {
        Ok(self.ensure(*node).await?.description)
    }
    async fn placeholder(&self, node: &Self::Node) -> Result<Option<String>> {
        Ok(self.ensure(*node).await?.placeholder)
    }
    async fn help(&self, node: &Self::Node) -> Result<Option<String>> {
        Ok(self.ensure(*node).await?.help)
    }
    async fn identifier(&self, node: &Self::Node) -> Result<Option<String>> {
        Ok(self.ensure(*node).await?.identifier)
    }
    async fn frame(&self, node: &Self::Node) -> Result<Option<Rect>> {
        Ok(self.ensure(*node).await?.frame)
    }
    async fn actions(&self, node: &Self::Node) -> Result<Vec<NodeAction>> {
        Ok(self.ensure(*node).await?.actions)
    }
    async fn selected_text_range(&self, node: &Self::Node) -> Result<Option<TextRange>> {
        Ok(self.ensure(*node).await?.selected)
    }
    async fn visible_text_range(&self, node: &Self::Node) -> Result<Option<TextRange>> {
        Ok(self.ensure(*node).await?.visible)
    }
    async fn number_of_characters(&self, node: &Self::Node) -> Result<Option<usize>> {
        Ok(self.ensure(*node).await?.chars)
    }
    async fn url(&self, node: &Self::Node) -> Result<Option<String>> {
        Ok(self.ensure(*node).await?.url)
    }
    fn handle(&self, node: &Self::Node) -> NativeHandle {
        let identity = self
            .cache
            .lock()
            .ok()
            .and_then(|guard| guard.get(node).cloned())
            .map(|facts| AxIdentity {
                role: facts.role,
                title: facts.title,
                frame: facts.frame,
            })
            .unwrap_or_default();
        NativeHandle::new(AxHandle {
            pid: self.pid,
            id: *node,
            identity,
        })
    }

    async fn action_names(&self, node: &Self::Node) -> Result<Vec<String>> {
        let id = *node;
        self.thread
            .call(AX_CALL_TIMEOUT, false, move |world| {
                act::action_names(world, id)
            })
            .await
    }

    async fn invoke(&self, node: &Self::Node, name: &str) -> Result<AxCall> {
        let id = *node;
        let name = name.to_string();
        self.mutate(vec![id], move |world| act::invoke(world, id, &name))
            .await
    }

    async fn attribute_settable(&self, node: &Self::Node, attribute: &str) -> Result<Option<bool>> {
        let id = *node;
        let attribute = attribute.to_string();
        self.thread
            .call(AX_CALL_TIMEOUT, false, move |world| {
                act::attribute_settable(world, id, &attribute)
            })
            .await
    }

    async fn set_string(&self, node: &Self::Node, attribute: &str, value: &str) -> Result<AxCall> {
        let id = *node;
        let attribute = attribute.to_string();
        let value = value.to_string();
        self.mutate(vec![id], move |world| {
            act::set_string(world, id, &attribute, &value)
        })
        .await
    }

    async fn set_number(&self, node: &Self::Node, attribute: &str, value: f64) -> Result<AxCall> {
        let id = *node;
        let attribute = attribute.to_string();
        self.mutate(vec![id], move |world| {
            act::set_number(world, id, &attribute, value)
        })
        .await
    }

    async fn set_text_range(&self, node: &Self::Node, range: TextRange) -> Result<AxCall> {
        let id = *node;
        self.mutate(vec![id], move |world| act::set_text_range(world, id, range))
            .await
    }

    async fn select_child(&self, parent: &Self::Node, child: &Self::Node) -> Result<AxCall> {
        let parent = *parent;
        let child = *child;
        self.mutate(vec![parent, child], move |world| {
            act::select_child(world, parent, child)
        })
        .await
    }

    async fn parent(&self, node: &Self::Node) -> Result<Option<Self::Node>> {
        let id = *node;
        self.thread
            .call(AX_CALL_TIMEOUT, false, move |world| act::parent(world, id))
            .await
    }

    async fn fresh_identity(&self, node: &Self::Node) -> Result<AxIdentity> {
        let id = *node;
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(&id);
        }
        self.thread
            .call(AX_CALL_TIMEOUT, false, move |world| {
                act::fresh_identity(world, id)
            })
            .await
    }

    async fn resolve(&self, id: u64) -> Result<Option<Self::Node>> {
        self.thread
            .call(AX_CALL_TIMEOUT, false, move |world| {
                Ok(world.element(id).map(|_| id))
            })
            .await
    }

    fn note(&self, entry: &str) {
        crate::diagnostics::note("AX_FALLBACK", entry);
    }
}

/// Writes `AXEnhancedUserInterface` and `AXManualAccessibility` on the AX thread.
pub struct AxToggleStore {
    thread: Arc<AxThread>,
}

impl AxToggleStore {
    /// Store that shares the driver's AX thread.
    pub fn new(thread: Arc<AxThread>) -> Self {
        Self { thread }
    }
}

impl ToggleStore for AxToggleStore {
    fn get(&self, pid: u32, name: ToggleName) -> Result<Option<bool>> {
        self.thread
            .call_blocking(AX_CALL_TIMEOUT, false, move |world| {
                read_toggle(world, pid, name)
            })
    }

    fn set(&self, pid: u32, name: ToggleName, value: bool) -> Result<bool> {
        self.thread
            .call_blocking(AX_CALL_TIMEOUT, true, move |world| {
                write_toggle(world, pid, name, value)
            })
    }
}

fn read_toggle(world: &mut World, pid: u32, name: ToggleName) -> Result<Option<bool>> {
    let pid = i32::try_from(pid).map_err(|_| DriverError::InvalidRequest)?;
    let app = world.application(pid)?;
    world::set_messaging_timeout(app)?;
    let mut result: CFTypeRef = std::ptr::null();
    let cf = CFString::new(name.as_str());
    // SAFETY: app is the cached +1 application element. result is +1 on success.
    let status = unsafe {
        crate::diagnostics::ax_call(
            "AXUIElementCopyAttributeValue",
            (app) as usize,
            name.as_str(),
            || AXUIElementCopyAttributeValue(app, cf.as_concrete_TypeRef(), &mut result),
        )
    };
    if status == kAXErrorAPIDisabled {
        return Err(DriverError::PermissionMissing {
            permission: "accessibility".into(),
        });
    }
    if status != kAXErrorSuccess || result.is_null() {
        if !result.is_null() {
            world::release(result);
        }
        return Ok(None);
    }
    let value = parse::bool_value(result);
    world::release(result);
    Ok(value)
}

fn write_toggle(world: &mut World, pid: u32, name: ToggleName, value: bool) -> Result<bool> {
    let pid = i32::try_from(pid).map_err(|_| DriverError::InvalidRequest)?;
    let app = world.application(pid)?;
    world::set_messaging_timeout(app)?;
    let cf = CFString::new(name.as_str());
    let bit = if value {
        CFBoolean::true_value()
    } else {
        CFBoolean::false_value()
    };
    // SAFETY: app is live. bit is a process-lifetime boolean singleton. The
    // call copies it and does not take ownership.
    let status = unsafe {
        crate::diagnostics::ax_call(
            "AXUIElementSetAttributeValue",
            (app) as usize,
            name.as_str(),
            || {
                accessibility_sys::AXUIElementSetAttributeValue(
                    app,
                    cf.as_concrete_TypeRef(),
                    bit.as_CFTypeRef(),
                )
            },
        )
    };
    if status == kAXErrorAPIDisabled {
        return Err(DriverError::PermissionMissing {
            permission: "accessibility".into(),
        });
    }
    Ok(status == kAXErrorSuccess)
}
