//! Semantic pattern priorities and owned commands. No COM objects or input fallback.
use super::cache::Patterns;
use gcu_core::{
    driver::{Cancel, Dir, DriverError, Result},
    model::TextRange,
};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

/// UIA pattern family; selection and expansion commands share their native pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pattern {
    /// InvokePattern.
    Invoke,
    /// TogglePattern.
    Toggle,
    /// SelectionItemPattern.
    SelectionItem,
    /// ExpandCollapsePattern.
    ExpandCollapse,
    /// ValuePattern.
    Value,
    /// RangeValuePattern.
    RangeValue,
    /// ScrollPattern.
    Scroll,
    /// ScrollItemPattern.
    ScrollItem,
    /// TextPattern.
    Text,
    /// LegacyIAccessiblePattern, last-resort default action only.
    Legacy,
}
impl Pattern {
    /// Whether the observed/fresh availability bag advertises this pattern.
    pub fn available(self, p: &Patterns) -> bool {
        match self {
            Self::Invoke => p.invoke,
            Self::Toggle => p.toggle,
            Self::SelectionItem => p.selection_item,
            Self::ExpandCollapse => p.expand_collapse,
            Self::Value => p.value,
            Self::RangeValue => p.range_value,
            Self::Scroll => p.scroll,
            Self::ScrollItem => p.scroll_item,
            Self::Text => p.text,
            Self::Legacy => p.legacy,
        }
    }
}
/// Priority table by UIA control type. Only equivalent activation patterns qualify;
/// value, range, text and scroll never substitute for a press. Legacy is always last.
pub fn press_patterns(control_type: i32) -> &'static [Pattern] {
    use Pattern::*;
    match control_type {
        50002 => &[Toggle, Invoke, SelectionItem, Legacy], // checkbox
        50007 | 50019 | 50024 | 50029 => &[SelectionItem, Invoke, Legacy],
        50003 | 50023 => &[ExpandCollapse, Invoke, SelectionItem, Legacy],
        _ => &[Invoke, Toggle, SelectionItem, ExpandCollapse, Legacy],
    }
}
/// One owned semantic command. Debug deliberately omits replacement text.
#[derive(Clone, PartialEq)]
pub enum PatternAction {
    /// Activate.
    Invoke,
    /// Toggle the native state.
    Toggle,
    /// Select the exact item.
    Select,
    /// Expand the exact element.
    Expand,
    /// Collapse the exact element.
    Collapse,
    /// Replace writable text.
    SetValue(String),
    /// Replace a writable numeric value.
    SetRangeValue(f64),
    /// Increment/decrement by native small change.
    AdjustRange(bool),
    /// Scroll one page or a positive fractional page.
    Scroll {
        /// Native scroll direction.
        dir: Dir,
        /// Positive page fraction, no greater than one.
        pages: f32,
    },
    /// Bring an item into its container's viewport without focus/activation.
    ScrollIntoView,
    /// Select Unicode-scalar positions, verified before native Select.
    SelectText(TextRange),
    /// Perform the native legacy default action.
    LegacyDefault,
}
impl PatternAction {
    /// Native family for availability checks and fake scripting.
    pub fn pattern(&self) -> Pattern {
        match self {
            Self::Invoke => Pattern::Invoke,
            Self::Toggle => Pattern::Toggle,
            Self::Select => Pattern::SelectionItem,
            Self::Expand | Self::Collapse => Pattern::ExpandCollapse,
            Self::SetValue(_) => Pattern::Value,
            Self::SetRangeValue(_) | Self::AdjustRange(_) => Pattern::RangeValue,
            Self::Scroll { .. } => Pattern::Scroll,
            Self::ScrollIntoView => Pattern::ScrollItem,
            Self::SelectText(_) => Pattern::Text,
            Self::LegacyDefault => Pattern::Legacy,
        }
    }
}
impl std::fmt::Debug for PatternAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PatternAction")
            .field(&self.pattern())
            .finish()
    }
}
/// Only these explicit no-effect results permit the next fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternResult {
    /// Native delivery acknowledged (business effect checked by core).
    Performed,
    /// Pattern/method definitely unsupported; zero effects.
    Unsupported,
    /// Pattern currently unavailable; zero effects.
    Unavailable,
}
/// Classified provider failure. Only explicit zero-effect absence allows fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternError {
    /// Pattern/interface is unsupported before any effect.
    Unsupported,
    /// Pattern is disabled or its operation is not currently valid.
    Unavailable,
    /// All other failures stop the chain, preserving execution uncertainty.
    Failure(DriverError),
}
impl From<DriverError> for PatternError {
    fn from(error: DriverError) -> Self {
        Self::Failure(error)
    }
}
impl PatternError {
    /// Classify a UIA HRESULT without OS bindings. `dispatched` means the
    /// mutating COM method has been called; disappearance then becomes unknown.
    pub fn from_hresult(status: i32, dispatched: bool) -> Self {
        match status as u32 {
            0x80040204 | 0x80004001 => Self::Unsupported, // UIA_NOTSUPPORTED / E_NOTIMPL
            0x80004002 if !dispatched => Self::Unsupported, // E_NOINTERFACE at acquisition
            0x80040200 | 0x80131509 => Self::Unavailable, // disabled / invalid operation
            0x80040201 if !dispatched => Self::Failure(DriverError::StaleElement),
            0x80131505 | 0x800705b4 => Self::Failure(DriverError::Timeout {
                maybe_executed: dispatched,
            }),
            _ => Self::Failure(DriverError::Native {
                message: format!("UIA HRESULT {:08x}", status as u32),
                maybe_executed: dispatched,
            }),
        }
    }
}

/// Shared dispatch marker. Sources set it immediately before a native mutation,
/// never while queued or reading metadata. It survives earlier successful steps.
#[derive(Clone)]
pub struct ActionContext {
    cancel: Cancel,
    state: Arc<AtomicU8>,
    deadline: Option<std::time::Instant>,
}
impl ActionContext {
    pub(crate) fn new(cancel: &Cancel) -> Self {
        Self {
            cancel: cancel.clone(),
            state: Arc::new(AtomicU8::new(0)),
            deadline: None,
        }
    }
    pub(crate) fn until(cancel: &Cancel, deadline: std::time::Instant) -> Self {
        Self {
            deadline: Some(deadline),
            ..Self::new(cancel)
        }
    }
    /// Nonblocking cancellation/deadline check, with uncertainty from earlier steps.
    pub fn check(&self) -> Result<()> {
        if self.cancel.is_cancelled() {
            Err(DriverError::Cancelled {
                maybe_executed: self.close(),
            })
        } else if self
            .deadline
            .is_some_and(|d| std::time::Instant::now() >= d)
        {
            Err(DriverError::Timeout {
                maybe_executed: self.close(),
            })
        } else if self.state.load(Ordering::Acquire) & 2 != 0 {
            Err(DriverError::Timeout {
                maybe_executed: self.maybe_executed(),
            })
        } else {
            Ok(())
        }
    }
    /// Mark actual mutation dispatch. Return the preceding uncertainty marker.
    pub fn begin(&self) -> Result<bool> {
        self.check()?;
        // This CAS shares the same atomic as close(). If the deadline wins,
        // no later dispatch can start; if dispatch wins, the caller sees unknown.
        self.state
            .try_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                (state & 2 == 0).then_some(state | 1)
            })
            .map(|previous| previous & 1 != 0)
            .map_err(|_| DriverError::Timeout {
                maybe_executed: self.maybe_executed(),
            })
    }
    /// Restore the preceding marker only after a proven no-effect result.
    pub fn restore(&self, previous: bool) {
        if !previous {
            self.state.fetch_and(!1, Ordering::AcqRel);
        }
    }
    /// Whether at least one mutation might have executed.
    pub fn maybe_executed(&self) -> bool {
        self.state.load(Ordering::Acquire) & 1 != 0
    }
    pub(crate) fn closed(&self) -> bool {
        self.state.load(Ordering::Acquire) & 2 != 0
    }
    pub(crate) fn close(&self) -> bool {
        self.state.fetch_or(2, Ordering::AcqRel) & 1 != 0
    }
    pub(crate) fn error(&self, error: DriverError) -> DriverError {
        if !self.maybe_executed() || error.maybe_executed() {
            return error;
        }
        match error {
            DriverError::Timeout { .. } => DriverError::Timeout {
                maybe_executed: true,
            },
            DriverError::Cancelled { .. } => DriverError::Cancelled {
                maybe_executed: true,
            },
            DriverError::Crashed { .. } => DriverError::Crashed {
                maybe_executed: true,
            },
            _ => DriverError::Native {
                message: "UIA operation interrupted after dispatch".into(),
                maybe_executed: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deadline_and_dispatch_have_one_atomic_winner() {
        for _ in 0..64 {
            let context = ActionContext::new(&Cancel::default());
            let dispatch = context.clone();
            let gate = Arc::new(std::sync::Barrier::new(2));
            let ready = gate.clone();
            let worker = std::thread::spawn(move || {
                ready.wait();
                dispatch.begin()
            });
            gate.wait();
            let unknown = context.close();
            let started = worker.join().unwrap().is_ok();
            assert_eq!(started, unknown);
            assert!(
                context.begin().is_err(),
                "no dispatch after deadline closure"
            );
        }
    }
    #[test]
    fn unsupported_restores_uncertainty_without_reopening_closed_dispatch() {
        let context = ActionContext::new(&Cancel::default());
        let previous = context.begin().unwrap();
        assert!(context.close());
        context.restore(previous);
        assert!(!context.maybe_executed());
        assert!(context.begin().is_err());
    }
}
