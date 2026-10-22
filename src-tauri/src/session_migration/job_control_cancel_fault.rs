//! Test-only substitute for a busy `cancelling()` read in the worker.
use crate::session_migration::errors::ImportError;

struct ArmedCancelFault {
    error: ImportError,
    remaining: u32,
}

thread_local! {
    static CANCEL_CHECK_FAULT: std::cell::RefCell<Option<ArmedCancelFault>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) fn take_cancel_check_fault() -> Option<ImportError> {
    CANCEL_CHECK_FAULT.with(|slot| {
        let mut guard = slot.borrow_mut();
        let fault = guard.as_mut()?;
        if fault.remaining == 0 {
            *guard = None;
            return None;
        }
        fault.remaining -= 1;
        let error = fault.error.clone();
        if fault.remaining == 0 {
            *guard = None;
        }
        Some(error)
    })
}

/// One-shot (or counted) substitute for a busy `cancelling()` read.
/// The worker treats it as a failed check, not as a user cancel.
pub(in crate::session_migration) struct CancelCheckFaultGuard;

impl CancelCheckFaultGuard {
    pub(in crate::session_migration) fn arm(error: ImportError, times: u32) -> Self {
        assert!(times > 0, "cancel-check fault must fire at least once");
        CANCEL_CHECK_FAULT.with(|slot| {
            assert!(
                slot.borrow().is_none(),
                "cancel-check fault is already armed"
            );
            *slot.borrow_mut() = Some(ArmedCancelFault {
                error,
                remaining: times,
            });
        });
        Self
    }

    pub(in crate::session_migration) fn assert_consumed(&self) {
        CANCEL_CHECK_FAULT.with(|slot| {
            assert!(
                slot.borrow().is_none(),
                "cancel-check fault was not reached"
            );
        });
    }
}

impl Drop for CancelCheckFaultGuard {
    fn drop(&mut self) {
        CANCEL_CHECK_FAULT.with(|slot| *slot.borrow_mut() = None);
    }
}
