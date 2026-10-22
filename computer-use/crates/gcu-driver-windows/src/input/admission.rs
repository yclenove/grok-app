//! Foreground send order. Blocking validation finishes before the ledger and the gate.
//!
//! `bounded_admit` is the only check that runs inside the gate, immediately before insertion.
//! It is cancellation, deadline, foreground, and scalar window identity: no enumeration, no COM,
//! and no log write. Keyboard packets skip the gate and use the same check beside `SendInput`.
//!
//! A targeted packet validates once, outside the ledger and the gate, then rechecks that same
//! scalar identity before every post. The recheck does not require the foreground window.
use super::{api::Event, hook_probe, ledger::InputLedger};
use crate::uia::patterns::ActionContext;
use gcu_core::driver::{Delivery, DriverError, Result};

/// `GetWindowRect` edges. The gate compares these integers to a later read of that same call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FramePx {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Identity and mutable window state captured by the blocking validation.
///
/// The gate re-reads only fields that have a scalar Win32 query. `HwndStamp.process_created`
/// needs `OpenProcess` and is not stored here. See the ADR 0013 addendum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExpectedIdentity {
    pub root: u64,
    pub hwnd: u64,
    /// Child class from `GetClassNameW`, UTF-16 units, no trailing NUL.
    pub class_units: Vec<u16>,
    pub pid: u32,
    pub tid: u32,
    /// Root rectangle from `GetWindowRect` at validate time.
    pub frame: FramePx,
    /// Root `IsIconic` at validate time. A successful validate records `false`.
    pub minimized: bool,
    /// Top-level process id from `GetWindowThreadProcessId` on the root (`HwndStamp.pid`).
    pub root_pid: u32,
    /// Root class from `GetClassNameW`, UTF-16 units, no trailing NUL (`HwndStamp.class`).
    pub root_class_units: Vec<u16>,
    /// Window-generation marker (`HwndStamp.created`), from `GetPropW` when a name exists.
    pub created: u64,
    /// NUL-terminated generation property. Empty means this API has no scalar marker.
    pub generation_property: Vec<u16>,
    /// `GetDpiForWindow` at validate time. `0` is the API's own "unknown, treat as 96" value.
    pub dpi: u32,
}

/// Scalar Win32 reads. Implementations must not enumerate windows, start COM, or log.
pub(crate) trait ScalarAdmission {
    fn foreground(&self) -> u64;
    fn is_window(&self, hwnd: u64) -> bool;
    fn root(&self, hwnd: u64) -> u64;
    /// `(thread id, process id)` from `GetWindowThreadProcessId`.
    fn thread_process(&self, hwnd: u64) -> (u32, u32);
    /// `GetClassNameW` compared to prebuilt UTF-16 units. Must not allocate.
    fn class_matches(&self, hwnd: u64, class: &[u16]) -> bool;
    /// `GetWindowRect`. `None` means the rectangle could not be read.
    fn frame(&self, hwnd: u64) -> Option<FramePx>;
    /// `IsIconic`.
    fn iconic(&self, hwnd: u64) -> bool;
    /// `GetPropW` of the generation property. `None` means the marker is absent.
    fn generation(&self, hwnd: u64, property: &[u16]) -> Option<u64>;
    /// `GetDpiForWindow`.
    fn dpi(&self, hwnd: u64) -> u32;
}

/// Ledger and gate parameters for one already-built packet.
pub(crate) struct Dispatch<'a> {
    pub ledger: &'a InputLedger,
    pub target: &'a super::api::Target,
    pub events: &'a [Event],
    pub delivery: Delivery,
    pub context: &'a ActionContext,
    pub pointer: bool,
}

pub(crate) fn packet_is_pointer(events: &[Event]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            Event::Move(..) | Event::Button(..) | Event::Wheel(..)
        )
    })
}

/// Run `prepare` to completion, then take the ledger, then admit, then insert.
///
/// `prepare` is where enumeration, security checks, and COM belong. After it returns, a test
/// barrier may flip cancellation, the deadline, or the foreground window. `admit` observes
/// that flip. `insert` does not run when `admit` fails.
pub(crate) fn prepare_then_dispatch<T>(
    prepare: impl FnOnce() -> Result<T>,
    dispatch: Dispatch<'_>,
    admit: impl FnOnce(&T) -> Result<()>,
    insert: impl FnOnce(&T) -> Result<()>,
) -> Result<()> {
    let prepared = prepare()?;
    after_prepare();
    // Drops after `dispatch_packet` returns, so the ledger mutex is already released.
    // Also runs on unwind from the send closure.
    let _flush_gate = FlushGateDiag;
    let result = dispatch.ledger.dispatch_packet(
        dispatch.target,
        dispatch.events,
        dispatch.delivery,
        dispatch.context,
        || {
            if dispatch.pointer {
                match hook_probe::with_admitted_gate(
                    hook_probe::INJECTION_GATE_WAIT,
                    || admit(&prepared),
                    || insert(&prepared),
                ) {
                    Err(hook_probe::GateBusy) => Err(DriverError::Native {
                        message: hook_probe::gate_refusal_message(),
                        maybe_executed: false,
                    }),
                    Ok(result) => result,
                }
            } else {
                admit(&prepared)?;
                insert(&prepared)
            }
        },
    );
    // `FlushGateDiag` emits the acquire timing after this return drops the ledger guard.
    result
}

/// Flushes pointer-gate diagnostics after the ledger guard in `dispatch_packet` has dropped.
struct FlushGateDiag;
impl Drop for FlushGateDiag {
    fn drop(&mut self) {
        super::hook_probe::flush_pointer_gate_diag();
    }
}

/// One non-foreground packet. Full validation runs once, outside the ledger and the gate.
/// This branch does not take the pointer gate.
pub(crate) struct TargetedSend<'a> {
    pub ledger: &'a InputLedger,
    pub target: &'a super::api::Target,
    pub events: &'a [Event],
    pub delivery: Delivery,
    pub context: &'a ActionContext,
}

/// Post each targeted event after one packet validation and a scalar recheck.
///
/// `prepare` is the native-thread stamp, security, and recipient check. It runs once for the
/// packet, outside `InputLedger::dispatch` and outside the pointer gate. Before each post,
/// `check` and [`scalar_identity`] run, still outside the ledger, so a refusal does not reserve
/// the event and does not rewrite an earlier event's uncertainty. The scalar recheck does not
/// require the foreground window. This path does not take the pointer gate.
pub(crate) fn dispatch_targeted(
    send: TargetedSend<'_>,
    mut check: impl FnMut() -> Result<()>,
    prepare: impl FnOnce() -> Result<ExpectedIdentity>,
    live: &impl ScalarAdmission,
    mut post: impl FnMut(Event) -> Result<()>,
) -> Result<()> {
    let expected = prepare()?;
    for event in send.events.iter().copied() {
        check()?;
        scalar_identity(&expected, live)?;
        send.ledger
            .dispatch(send.target, event, send.delivery, send.context, |_| {
                post(event)
            })?;
    }
    Ok(())
}

/// Last admission check. Nothing here may block on enumeration, COM, or a log sink.
///
/// Rechecked with scalar calls: foreground, child liveness and ancestry, child thread and
/// process, child class, root process and class, the generation marker when its property name
/// was captured, the root `GetWindowRect`, `IsIconic`, and `GetDpiForWindow`.
/// `process_created`, the process token, and virtual-desktop membership stay outside.
pub(crate) fn bounded_admit(
    check: impl FnOnce() -> Result<()>,
    expected: &ExpectedIdentity,
    live: &impl ScalarAdmission,
) -> Result<()> {
    if let Err(error) = check() {
        return Err(not_executed(error));
    }
    if live.foreground() != expected.root {
        return Err(DriverError::CapabilityUnavailable);
    }
    scalar_identity(expected, live)
}

/// Gate scalar recheck without the foreground comparison.
///
/// Targeted posts use this. Foreground admission uses it after its own foreground test.
/// Nothing here may enumerate, start COM, open a process, or write a log.
fn scalar_identity(expected: &ExpectedIdentity, live: &impl ScalarAdmission) -> Result<()> {
    if !live.is_window(expected.hwnd) || live.root(expected.hwnd) != expected.root {
        return Err(DriverError::WindowGone);
    }
    let (tid, pid) = live.thread_process(expected.hwnd);
    if tid == 0 || tid != expected.tid || pid != expected.pid {
        return Err(DriverError::WindowGone);
    }
    if !live.class_matches(expected.hwnd, &expected.class_units) {
        return Err(DriverError::StaleElement);
    }
    let (root_tid, root_pid) = live.thread_process(expected.root);
    if root_tid == 0 || root_pid != expected.root_pid {
        return Err(DriverError::WindowGone);
    }
    if !live.class_matches(expected.root, &expected.root_class_units) {
        return Err(DriverError::WindowGone);
    }
    if !expected.generation_property.is_empty() {
        match live.generation(expected.root, &expected.generation_property) {
            Some(created) if created == expected.created => {}
            _ => return Err(DriverError::WindowGone),
        }
    }
    match live.frame(expected.root) {
        Some(frame) if frame == expected.frame => {}
        Some(_) => return Err(DriverError::InvalidRequest),
        None => return Err(DriverError::WindowGone),
    }
    if live.iconic(expected.root) != expected.minimized {
        return Err(DriverError::WindowMinimized);
    }
    if live.dpi(expected.root) != expected.dpi {
        return Err(DriverError::InvalidRequest);
    }
    Ok(())
}

/// This packet was not inserted. Drop the uncertainty bit `begin` may have raised for it.
pub(crate) fn not_executed(error: DriverError) -> DriverError {
    match error {
        DriverError::Timeout { .. } => DriverError::Timeout {
            maybe_executed: false,
        },
        DriverError::Cancelled { .. } => DriverError::Cancelled {
            maybe_executed: false,
        },
        DriverError::Crashed { .. } => DriverError::Crashed {
            maybe_executed: false,
        },
        DriverError::Native { message, .. } => DriverError::Native {
            message,
            maybe_executed: false,
        },
        DriverError::NotSettable { .. } => DriverError::NotSettable {
            maybe_executed: false,
        },
        other => other,
    }
}

fn after_prepare() {
    #[cfg(test)]
    {
        AFTER_PREPARE.with(|slot| {
            if let Some(hook) = slot.borrow_mut().take() {
                hook();
            }
        });
    }
}

#[cfg(test)]
thread_local! {
    static AFTER_PREPARE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn set_after_prepare(hook: impl FnOnce() + 'static) {
    AFTER_PREPARE.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
