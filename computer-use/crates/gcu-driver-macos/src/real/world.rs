//! AX element map owned by the `gcu-macos-ax` thread.
//!
//! Application elements stay for the life of the thread so accessibility
//! toggles survive a snapshot reset. Snapshot elements are +1 (`CFRetain` when
//! taken out of a copied array, or the create rule) and [`World::reset_snapshot`]
//! releases each one. Drop releases both maps.

use std::collections::HashMap;

use accessibility_sys::{
    kAXErrorAPIDisabled, kAXErrorSuccess, kAXWindowsAttribute, AXUIElementCreateApplication,
    AXUIElementRef, AXUIElementSetMessagingTimeout,
};
use core_foundation::base::{CFRelease, CFRetain, CFTypeRef};
use gcu_core::driver::DriverError;

use gcu_core::driver::Result;

/// One second, so a wedged app cannot hold the walk for its whole budget.
pub const MESSAGING_TIMEOUT_SECS: f32 = 1.0;

/// Element tables. Not `Send` on purpose: pointers never leave this thread.
pub struct World {
    next: u64,
    elements: HashMap<u64, AXUIElementRef>,
    apps: HashMap<i32, AXUIElementRef>,
}

impl World {
    pub(super) fn new() -> Self {
        Self {
            next: 1,
            elements: HashMap::new(),
            apps: HashMap::new(),
        }
    }

    pub(super) fn element(&self, id: u64) -> Option<AXUIElementRef> {
        self.elements.get(&id).copied()
    }

    /// Store a +1 element. The map releases it later.
    pub(super) fn insert(&mut self, element: AXUIElementRef) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        self.elements.insert(id, element);
        id
    }

    /// Consume +1, reusing the id of an equal AX object already in this walk.
    pub(super) fn intern(&mut self, element: AXUIElementRef) -> u64 {
        for (id, existing) in &self.elements {
            // SAFETY: both are live AX objects owned by this world/caller.
            if unsafe { core_foundation::base::CFEqual(*existing as _, element as _) } != 0 {
                release(element as _);
                return *id;
            }
        }
        self.insert(element)
    }

    /// Release snapshot elements. Application elements stay.
    pub(super) fn reset_snapshot(&mut self) {
        for element in self.elements.drain() {
            release(element.1 as CFTypeRef);
        }
    }

    /// Cached application element, created +1 on first use.
    pub(super) fn application(&mut self, pid: i32) -> Result<AXUIElementRef> {
        if let Some(app) = self.apps.get(&pid).copied() {
            return Ok(app);
        }
        // SAFETY: pid names a process. Create returns +1, stored in `apps` and
        // released from Drop.
        let app = unsafe { AXUIElementCreateApplication(pid) };
        if app.is_null() {
            return Err(DriverError::Native {
                message: "accessibility application element was null".into(),
                maybe_executed: false,
            });
        }
        let _ = set_messaging_timeout(app);
        self.apps.insert(pid, app);
        Ok(app)
    }

    pub(super) fn windows_attribute() -> &'static str {
        kAXWindowsAttribute
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.reset_snapshot();
        for app in self.apps.drain() {
            release(app.1 as CFTypeRef);
        }
    }
}

/// Set the per-element AX messaging timeout. `APIDisabled` is a missing permission.
pub(super) fn set_messaging_timeout(element: AXUIElementRef) -> Result<()> {
    // SAFETY: element is a live AX object. The timeout is a float in seconds.
    let status = unsafe { AXUIElementSetMessagingTimeout(element, MESSAGING_TIMEOUT_SECS) };
    ax_status(status)
}

pub(super) fn ax_status(status: i32) -> Result<()> {
    if status == kAXErrorAPIDisabled {
        Err(DriverError::PermissionMissing {
            permission: "accessibility".into(),
        })
    } else if status == kAXErrorSuccess {
        Ok(())
    } else {
        Err(DriverError::Native {
            message: "accessibility call failed".into(),
            maybe_executed: false,
        })
    }
}

/// A +1 element. Drop releases it unless [`OwnedElement::into_inner`] ran.
pub(super) struct OwnedElement(AXUIElementRef);

impl OwnedElement {
    /// Take ownership of an existing +1 reference.
    pub(super) fn from_retained(element: AXUIElementRef) -> Self {
        Self(element)
    }

    /// Transfer the +1 without releasing it.
    pub(super) fn into_inner(self) -> AXUIElementRef {
        let element = self.0;
        std::mem::forget(self);
        element
    }
}

impl Drop for OwnedElement {
    fn drop(&mut self) {
        release(self.0 as CFTypeRef);
    }
}

/// Retain a live object. The caller owns the returned +1.
pub(super) fn retain(value: CFTypeRef) -> CFTypeRef {
    if value.is_null() {
        return value;
    }
    // SAFETY: value is a live CoreFoundation object. The caller releases the +1.
    unsafe { CFRetain(value) }
}

/// Release a +1 object. Null is ignored.
pub(super) fn release(value: CFTypeRef) {
    if !value.is_null() {
        // SAFETY: value is a +1 CoreFoundation object owned by the caller.
        unsafe { CFRelease(value) };
    }
}
