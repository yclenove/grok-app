//! Window continuity and element ownership on the accessibility thread.
//!
//! A retained window element compared with `CFEqual`, plus
//! `_AXUIElementGetWindow`, is the continuity token. Polling a `CGWindowID`
//! is not. A missing symbol, a missing element, or a pid mismatch produces
//! no binding. Geometry and title are not consulted.

use std::collections::{HashMap, HashSet};

use accessibility_sys::{kAXErrorSuccess, AXUIElementGetPid, AXUIElementGetTypeID, AXUIElementRef};
use core_foundation::array::CFArray;
use core_foundation::base::{CFEqual, CFGetTypeID, CFType, CFTypeRef, TCFType};
use core_foundation::string::CFString;
use gcu_core::driver::Result;

use super::act;
use super::world::{self, World};
use crate::api::CgWindow;
use crate::scope_os::{
    cg_id_from_ax, not_proved, ElementFact, Evidence, JobScope, OsWindow, Snapshot, TokenBook,
};

/// Refresh tokens for the current on-screen windows.
///
/// Failure clears the published map. Callers then see `None` and refuse.
pub(crate) fn refresh_tokens(world: &mut World, book: &TokenBook) {
    if crate::scope_os::ax_window_status()
        != gcu_protocol::capabilities::PrivateApiStatus::Available
    {
        release_book(book);
        return;
    }
    let Some(rows) = super::windows::copy_windows() else {
        release_book(book);
        return;
    };
    let old_tokens = book.published();
    let previous = book.take_held();
    let mut held = HashMap::new();
    let mut tokens = HashMap::new();
    let pids: HashSet<u32> = rows.iter().map(|row| row.pid).collect();
    for pid in pids {
        for element in window_elements(world, pid) {
            let Some(cg) = cg_id_from_ax(element as usize) else {
                world::release(element as CFTypeRef);
                continue;
            };
            let token = match previous.get(&cg) {
                Some(old) if cf_equal(*old, element as usize) => old_tokens.get(&cg).copied(),
                _ => None,
            };
            let Some(token) = token.or_else(|| book.mint()) else {
                world::release(element as CFTypeRef);
                continue;
            };
            if let Some(displaced) = held.insert(cg, element as usize) {
                world::release(displaced as CFTypeRef);
            }
            tokens.insert(cg, token);
        }
    }
    for pointer in previous.into_values() {
        world::release(pointer as CFTypeRef);
    }
    book.store(held, tokens);
}

/// Release every held element. Must run on the accessibility thread.
pub(crate) fn release_book(book: &TokenBook) {
    for pointer in book.take_held().into_values() {
        world::release(pointer as CFTypeRef);
    }
    book.clear_published();
}

/// Owner pid and proved `CGWindowID` for a stored element. `None` is unproved.
pub(crate) fn element_fact(world: &mut World, id: u64) -> Option<ElementFact> {
    let element = world.element(id)?;
    world::set_messaging_timeout(element).ok()?;
    let pid = read_pid(element)?;
    let (window, owned) = window_element(element)?;
    if owned {
        let _ = world::set_messaging_timeout(window);
    }
    let cg = cg_id_from_ax(window as usize);
    if owned {
        world::release(window as CFTypeRef);
    }
    Some(ElementFact { pid, cg_id: cg })
}

/// Two token refreshes and two window-list reads. A mismatch is `None`.
///
/// Tokens are attached only to ids present in both reads. This does not join
/// an older published map to a later list.
pub(crate) fn observe_window_rows(world: &mut World, book: &TokenBook) -> Option<Vec<OsWindow>> {
    refresh_tokens(world, book);
    let first = super::windows::copy_windows()?;
    let tokens_first = book.published();
    refresh_tokens(world, book);
    let second = super::windows::copy_windows()?;
    let tokens_second = book.published();
    if !same_window_identity(&first, &second) || tokens_first != tokens_second {
        book.clear_published();
        return None;
    }
    Some(
        second
            .into_iter()
            .map(|row| OsWindow {
                cg_id: row.window_id,
                owner_pid: row.pid,
                layer: row.layer,
                alpha: row.alpha,
                frame: row.frame,
                on_screen: true,
                token: tokens_second.get(&row.window_id).copied(),
            })
            .collect(),
    )
}

/// Window rows, boot, and owner starts from one consistent observation.
///
/// A second read must match. Any gap returns `None` instead of joining the
/// two results. Blocks on the window list and process reads. No extra timeout
/// or cancellation; the caller is already on the accessibility thread.
pub(crate) fn observe_snapshot(
    world: &mut World,
    book: &TokenBook,
) -> Option<crate::scope_os::Snapshot> {
    let first = observe_window_rows(world, book)?;
    let snap = crate::scope_os::snapshot_from_rows(first);
    let second = observe_window_rows(world, book)?;
    let again = crate::scope_os::snapshot_from_rows(second);
    if snap != again {
        book.clear_published();
        return None;
    }
    Some(again)
}

fn same_window_identity(left: &[CgWindow], right: &[CgWindow]) -> bool {
    let ids = |rows: &[CgWindow]| -> Vec<(u32, u32)> {
        rows.iter().map(|row| (row.window_id, row.pid)).collect()
    };
    ids(left) == ids(right)
}

/// Facts for `ids` from this thread's window observation.
///
/// Blocks on the window list for the caller's budget. No extra timeout or
/// cancellation. A gap is [`not_proved`].
pub(crate) fn observe_ids(
    evidence: &Evidence,
    ids: &[u64],
    world: &mut World,
) -> Result<(Vec<ElementFact>, Snapshot)> {
    let Some(book) = evidence.token_book() else {
        return Err(not_proved());
    };
    let Some(snap) = observe_snapshot(world, &book) else {
        return Err(not_proved());
    };
    let mut facts = Vec::with_capacity(ids.len());
    for id in ids {
        facts.push(element_fact(world, *id).ok_or_else(not_proved)?);
    }
    Ok((facts, snap))
}

/// Refuse the mutation unless `scope` still owns every id.
///
/// `scope` is the admission captured with the job. A cleared pin is not an
/// unscoped grant. No evidence on a scoped job refuses. Unscoped jobs return
/// before any accessibility call.
pub(crate) fn prove_mutation(
    evidence: Option<&Evidence>,
    scope: &JobScope,
    ids: &[u64],
    world: &mut World,
) -> Result<()> {
    let Some(evidence) = evidence else {
        return if scope.is_scoped() {
            Err(not_proved())
        } else {
            Ok(())
        };
    };
    evidence.commit_ax_job(scope, |admitted| {
        let Some(admitted) = admitted else {
            return Ok(());
        };
        let (facts, snap) = observe_ids(evidence, ids, world)?;
        evidence.accept_facts(admitted, &facts, &snap)
    })
}

fn window_elements(world: &mut World, pid: u32) -> Vec<AXUIElementRef> {
    let Ok(pid) = i32::try_from(pid) else {
        return Vec::new();
    };
    let Ok(app) = world.application(pid) else {
        return Vec::new();
    };
    if world::set_messaging_timeout(app).is_err() {
        return Vec::new();
    }
    let Ok(Some(value)) = act::copy_attribute(app, "AXWindows") else {
        return Vec::new();
    };
    let Some(array) = array_from_created(value) else {
        world::release(value);
        return Vec::new();
    };
    let mut out = Vec::new();
    for index in 0..array.len() {
        let Some(item) = array.get(index) else {
            continue;
        };
        let raw = item.as_CFTypeRef();
        if !is_element(raw) {
            continue;
        }
        let retained = world::retain(raw) as AXUIElementRef;
        if !retained.is_null() {
            out.push(retained);
        }
    }
    out
}

fn array_from_created(value: CFTypeRef) -> Option<CFArray<CFType>> {
    if value.is_null() {
        return None;
    }
    // SAFETY: `value` is a live +1 object. Both type ids are process-global.
    // A mismatch leaves the retain with the caller.
    let is_array = unsafe { CFGetTypeID(value) == CFArray::<CFType>::type_id() };
    if !is_array {
        return None;
    }
    // SAFETY: the type id matches CFArray. The create rule takes the +1.
    Some(unsafe { CFArray::<CFType>::wrap_under_create_rule(value.cast()) })
}

fn window_element(element: AXUIElementRef) -> Option<(AXUIElementRef, bool)> {
    if let Ok(Some(value)) = act::copy_attribute(element, "AXWindow") {
        if is_element(value) {
            return Some((value as AXUIElementRef, true));
        }
        world::release(value);
    }
    if role_is_window(element) {
        Some((element, false))
    } else {
        None
    }
}

fn role_is_window(element: AXUIElementRef) -> bool {
    let Ok(Some(value)) = act::copy_attribute(element, "AXRole") else {
        return false;
    };
    // SAFETY: `value` is +1 from `copy_attribute`. The create rule releases it.
    let typed = unsafe { CFType::wrap_under_create_rule(value) };
    typed
        .downcast::<CFString>()
        .is_some_and(|text| text == "AXWindow")
}

fn read_pid(element: AXUIElementRef) -> Option<u32> {
    let mut pid = 0i32;
    // SAFETY: `element` is a live accessibility object. `pid` is writable.
    // The call does not release `element`.
    let status = unsafe {
        crate::diagnostics::ax_call("AXUIElementGetPid", element as usize, "", || {
            AXUIElementGetPid(element, &mut pid)
        })
    };
    if status != kAXErrorSuccess {
        None
    } else {
        u32::try_from(pid).ok().filter(|pid| *pid > 0)
    }
}

fn is_element(value: CFTypeRef) -> bool {
    if value.is_null() {
        return false;
    }
    // SAFETY: `value` is a live CoreFoundation object. Both type ids are global.
    unsafe { CFGetTypeID(value) == AXUIElementGetTypeID() }
}

fn cf_equal(left: usize, right: usize) -> bool {
    if left == 0 || right == 0 {
        return false;
    }
    // SAFETY: both pointers are live accessibility elements retained by the caller.
    unsafe { CFEqual(left as CFTypeRef, right as CFTypeRef) != 0 }
}
