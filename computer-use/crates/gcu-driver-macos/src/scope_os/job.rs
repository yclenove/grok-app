//! Immutable scope carried by one AX or CG job.
//!
//! Whether a job is scoped is fixed when the job is admitted. A later
//! `ClearPin`, timeout, or drop does not change it. A scoped job with no
//! binding is refused for its whole life. Callers must not infer scope from
//! the mutable pin slot.

use gcu_core::driver::Result;
use gcu_core::scope::WindowBinding;

use super::evidence::{not_proved, Evidence};

/// Admission captured for one job. Clone is a value, not a view of the pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JobScope {
    scoped: bool,
    expected: Option<WindowBinding>,
}

impl JobScope {
    /// Work that was not admitted under a scope.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub(crate) fn unscoped() -> Self {
        Self {
            scoped: false,
            expected: None,
        }
    }

    /// Work admitted under a scope. `None` stays a refusal.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub(crate) fn scoped(expected: Option<WindowBinding>) -> Self {
        Self {
            scoped: true,
            expected,
        }
    }

    /// Copy the pin once. Later clears of that slot do not change the copy.
    ///
    /// Memory only. No blocking, timeout, or cancellation. Enforcing mode
    /// without a pin is not a scoped admission; the caller that looked up a
    /// window passes [`Self::scoped`] itself.
    pub(crate) fn capture(evidence: &Evidence) -> Self {
        match evidence.pinned() {
            Some(binding) => Self::scoped(Some(binding)),
            None => Self::unscoped(),
        }
    }

    /// Scope for a job that is about to start.
    ///
    /// A stored pin is copied. A scoped session whose pin is already gone is
    /// scoped with no binding, which stays a refusal. Anything else is
    /// unscoped. Memory only. No blocking, timeout, or cancellation.
    pub(crate) fn admit(evidence: &Evidence) -> Self {
        let captured = Self::capture(evidence);
        if evidence.session_scoped() && !captured.is_scoped() {
            return Self::scoped(None);
        }
        captured
    }

    /// True when this job was admitted under a scope.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub(crate) fn is_scoped(&self) -> bool {
        self.scoped
    }

    /// Binding captured at admission, if there was one.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub(crate) fn expected(&self) -> Option<&WindowBinding> {
        self.expected.as_ref()
    }

    /// True when a scoped job has no binding. That job must not mutate.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    /// macOS jobs and tests are the callers.
    #[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
    pub(crate) fn missing(&self) -> bool {
        self.scoped && self.expected.is_none()
    }

    /// Binding this job must prove.
    ///
    /// `Ok(None)` is an unscoped job. `Ok(Some)` is the admitted window.
    /// `Err` is a scoped job whose binding is missing. The pin slot is not read.
    /// Memory only. No blocking, timeout, or cancellation.
    /// macOS jobs and tests are the callers.
    #[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
    pub(crate) fn binding(&self) -> Result<Option<&WindowBinding>> {
        if !self.scoped {
            return Ok(None);
        }
        if self.missing() {
            return Err(not_proved());
        }
        Ok(self.expected.as_ref())
    }
}
