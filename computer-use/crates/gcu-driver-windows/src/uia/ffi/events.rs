//! Native subscription RAII. Callbacks copy no element, property value or runtime ID.
use super::super::{
    cache::PROPERTY_IDS,
    events::{EventKind, EventSink},
};
use super::{native, Request};
use gcu_core::driver::Result;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use windows::{
    core::{implement, Ref},
    Win32::{
        Foundation::HWND,
        System::{Com::SAFEARRAY, Variant::VARIANT},
        UI::Accessibility::*,
    },
};

#[implement(
    IUIAutomationStructureChangedEventHandler,
    IUIAutomationPropertyChangedEventHandler,
    IUIAutomationFocusChangedEventHandler
)]
struct Handler {
    sink: EventSink,
    closed: Arc<AtomicBool>,
}
impl Handler {
    fn emit(&self, kind: EventKind) {
        // No panic may cross a COM callback boundary, including a poisoned sink.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !self.closed.load(Ordering::Acquire) {
                self.sink.emit(kind);
            }
        }));
    }
}
impl IUIAutomationStructureChangedEventHandler_Impl for Handler_Impl {
    fn HandleStructureChangedEvent(
        &self,
        _: Ref<IUIAutomationElement>,
        _: StructureChangeType,
        _: *const SAFEARRAY,
    ) -> windows::core::Result<()> {
        self.emit(EventKind::Structure);
        Ok(())
    }
}
impl IUIAutomationPropertyChangedEventHandler_Impl for Handler_Impl {
    fn HandlePropertyChangedEvent(
        &self,
        _: Ref<IUIAutomationElement>,
        _: UIA_PROPERTY_ID,
        _: &VARIANT,
    ) -> windows::core::Result<()> {
        self.emit(EventKind::Property);
        Ok(())
    }
}
impl IUIAutomationFocusChangedEventHandler_Impl for Handler_Impl {
    fn HandleFocusChangedEvent(&self, _: Ref<IUIAutomationElement>) -> windows::core::Result<()> {
        self.emit(EventKind::Focus);
        Ok(())
    }
}

pub(super) struct Registration {
    automation: IUIAutomation,
    root: IUIAutomationElement,
    structure: IUIAutomationStructureChangedEventHandler,
    property: IUIAutomationPropertyChangedEventHandler,
    focus: IUIAutomationFocusChangedEventHandler,
    added: [bool; 3],
    sink: EventSink,
    closed: Arc<AtomicBool>,
}
impl Registration {
    pub(super) fn new(
        automation: &IUIAutomation,
        hwnd: u64,
        sink: EventSink,
        closed: Arc<AtomicBool>,
        request: &Request,
    ) -> Result<Self> {
        use windows::core::Interface;
        request.check()?;
        // SAFETY: exact HWND, live MTA-local UIA; no activation or input.
        let root = unsafe { automation.ElementFromHandle(HWND(hwnd as usize as *mut _)) }
            .map_err(native)?;
        let structure: IUIAutomationStructureChangedEventHandler = Handler {
            sink: sink.clone(),
            closed: closed.clone(),
        }
        .into();
        let property = structure.cast().map_err(native)?;
        let focus = structure.cast().map_err(native)?;
        let mut reg = Self {
            automation: automation.clone(),
            root,
            structure,
            property,
            focus,
            added: [false; 3],
            sink,
            closed,
        };
        request.check()?;
        // SAFETY: interfaces stay owned by reg until each subscription is removed.
        unsafe {
            automation.AddStructureChangedEventHandler(
                &reg.root,
                TreeScope_Subtree,
                None,
                &reg.structure,
            )
        }
        .map_err(native)?;
        reg.added[0] = true;
        request.check()?;
        let ids: Vec<_> = PROPERTY_IDS.iter().map(|id| UIA_PROPERTY_ID(*id)).collect();
        // SAFETY: IDs slice stays live during registration, COM retains the handler.
        unsafe {
            automation.AddPropertyChangedEventHandlerNativeArray(
                &reg.root,
                TreeScope_Subtree,
                None,
                &reg.property,
                &ids,
            )
        }
        .map_err(native)?;
        reg.added[1] = true;
        request.check()?;
        // SAFETY: focus handler lives until reg's balanced unregistration.
        unsafe { automation.AddFocusChangedEventHandler(None, &reg.focus) }.map_err(native)?;
        reg.added[2] = true;
        Ok(reg)
    }
    pub(super) fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire) || self.sink.is_closed()
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        // SAFETY: every successful Add is tracked individually, including partial
        // registration rollback. All interfaces are live and on their original MTA.
        unsafe {
            if self.added[2] {
                let _ = self.automation.RemoveFocusChangedEventHandler(&self.focus);
            }
            if self.added[1] {
                let _ = self
                    .automation
                    .RemovePropertyChangedEventHandler(&self.root, &self.property);
            }
            if self.added[0] {
                let _ = self
                    .automation
                    .RemoveStructureChangedEventHandler(&self.root, &self.structure);
            }
        }
    }
}
