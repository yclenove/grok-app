//! Portable semantic routing. Every fallback requires explicit no-effect evidence.
use super::{
    cache::Properties,
    patterns::*,
    source::{NodeId, UiaSource},
    UiaElement,
};
use gcu_core::{
    driver::*,
    model::{NativeHandle, NodeAction},
};
use std::future::Future;

/// Extract an exact semantic target; input/foreground actions are not semantic.
pub(crate) fn target(action: &DriverAction) -> Option<&NativeHandle> {
    match action {
        DriverAction::Press { el }
        | DriverAction::SecondaryAction { el, .. }
        | DriverAction::SetValue { el, .. }
        | DriverAction::SelectText { el, .. }
        | DriverAction::ScrollElement { el, .. } => Some(el),
        _ => None,
    }
}
/// Execute an exact observed UIA target with one shared DRIVER_DEADLINE.
/// Cancellation before dispatch proves no effect; cancellation/timeout during a
/// mutation reports uncertainty. No uncertain result is retried or escalated.
/// Source implementations must revalidate identity on their worker before writing.
pub async fn perform(
    source: &dyn UiaSource,
    hwnd: u64,
    req: &DriverAction,
    cancel: &Cancel,
) -> Result<Delivery> {
    perform_scoped(
        source,
        hwnd,
        req,
        cancel,
        &crate::scope_identity::JobScope::unscoped(),
    )
    .await
}
/// Semantic action under one scope copied by the caller.
///
/// [`JobScope::Unscoped`] keeps the historical route: a foreign owner and a
/// missing probe still dispatch. A scoped job requires the element's own
/// process and top-level window. A missing scope is refused and is not treated
/// as unscoped.
pub(crate) async fn perform_scoped(
    source: &dyn UiaSource,
    hwnd: u64,
    req: &DriverAction,
    cancel: &Cancel,
    scope: &crate::scope_identity::JobScope,
) -> Result<Delivery> {
    perform_until(
        source,
        hwnd,
        req,
        cancel,
        tokio::time::Instant::now() + DRIVER_DEADLINE,
        scope,
    )
    .await
}
pub(crate) async fn perform_until(
    source: &dyn UiaSource,
    hwnd: u64,
    req: &DriverAction,
    cancel: &Cancel,
    deadline: tokio::time::Instant,
    scope: &crate::scope_identity::JobScope,
) -> Result<Delivery> {
    let budget = Budget {
        deadline,
        cancel,
        context: ActionContext::new(cancel),
    };
    budget.context.check()?;
    let el = target(req).ok_or(DriverError::CapabilityUnavailable)?;
    let observed = el
        .downcast_ref::<UiaElement>()
        .ok_or(DriverError::StaleElement)?;
    let node = observed.node;
    let element_hwnd = observed.hwnd;
    let window_id = observed.window.id.clone();
    let props = budget.read(source.validate(node, hwnd)).await?;
    // Unscoped work does not consult the owner probe. Scoped work proves the
    // element's own process and top-level window before any pattern dispatch.
    if let Some(claim) = scope.claim()? {
        if crate::scope_identity::parse_stamp(&window_id)
            .is_some_and(|stamp| stamp.hwnd != claim.hwnd)
        {
            return Err(DriverError::ScopeDenied);
        }
        let owner = budget.read(source.owner_pid(node)).await?;
        let fact =
            owner
                .filter(|pid| *pid != 0)
                .map(|owner_pid| crate::scope_identity::FragmentFact {
                    owner_pid,
                    top_hwnd: element_hwnd,
                });
        crate::scope_identity::admit_fragment(claim, fact)?;
    }
    let result = match req {
        DriverAction::SetValue { value, .. } => {
            super::value::set(source, node, hwnd, &props, value, &budget).await
        }
        _ if props.password
            || super::sensitive::is_execution_surface(&props)
            || !props.enabled
            || props.partial =>
        {
            Err(DriverError::ElementNotActionable)
        }
        DriverAction::Press { .. } => press(source, node, hwnd, &props, &budget).await,
        DriverAction::SecondaryAction { action, .. } => {
            secondary(source, node, hwnd, &props, action, &budget).await
        }
        DriverAction::SelectText { range, .. } => {
            range
                .start
                .checked_add(range.length)
                .filter(|end| *end <= i32::MAX as usize)
                .ok_or(DriverError::InvalidRequest)?;
            finish(
                budget
                    .step(
                        source,
                        node,
                        hwnd,
                        &props,
                        PatternAction::SelectText(*range),
                    )
                    .await?,
                DriverError::ElementNotActionable,
            )
        }
        DriverAction::ScrollElement { dir, pages, .. } => {
            super::scroll::scroll(source, node, hwnd, &props, *dir, *pages, &budget).await
        }
        _ => Err(DriverError::CapabilityUnavailable),
    };
    result.map_err(|e| budget.context.error(e))
}
pub(super) struct Budget<'a> {
    deadline: tokio::time::Instant,
    cancel: &'a Cancel,
    context: ActionContext,
}
impl Drop for Budget<'_> {
    fn drop(&mut self) {
        self.context.close();
    }
}
impl Budget<'_> {
    pub(super) async fn read<T>(&self, future: impl Future<Output = Result<T>>) -> Result<T> {
        self.context.check()?;
        if tokio::time::Instant::now() >= self.deadline {
            return Err(DriverError::Timeout {
                maybe_executed: self.context.close(),
            });
        }
        tokio::select! { biased;
            _ = self.cancel.cancelled() => Err(DriverError::Cancelled { maybe_executed: self.context.close() }),
            value = tokio::time::timeout_at(self.deadline, future) => value.map_err(|_| DriverError::Timeout { maybe_executed: self.context.close() })?,
        }
    }
    pub(super) async fn step(
        &self,
        source: &dyn UiaSource,
        node: NodeId,
        hwnd: u64,
        props: &Properties,
        action: PatternAction,
    ) -> Result<PatternResult> {
        if !action.pattern().available(&props.patterns) {
            return Ok(PatternResult::Unsupported);
        }
        self.read(source.action(node, hwnd, action, self.context.clone()))
            .await
    }
}
pub(super) fn finish(result: PatternResult, unavailable: DriverError) -> Result<Delivery> {
    match result {
        PatternResult::Performed => Ok(Delivery::Accessibility),
        PatternResult::TargetedPerformed => Ok(Delivery::TargetedEvent),
        _ => Err(unavailable),
    }
}
async fn press(
    source: &dyn UiaSource,
    node: NodeId,
    hwnd: u64,
    props: &Properties,
    budget: &Budget<'_>,
) -> Result<Delivery> {
    for pattern in press_patterns(props.control_type) {
        let action = match pattern {
            Pattern::Invoke => PatternAction::Invoke,
            Pattern::Toggle => PatternAction::Toggle,
            Pattern::SelectionItem => PatternAction::Select,
            Pattern::ExpandCollapse if matches!(props.expand_state, Some(1 | 2)) => {
                PatternAction::Collapse
            }
            Pattern::ExpandCollapse => PatternAction::Expand,
            Pattern::Legacy => PatternAction::LegacyDefault,
            _ => continue,
        };
        match budget.step(source, node, hwnd, props, action).await? {
            PatternResult::Performed => return Ok(Delivery::Accessibility),
            PatternResult::TargetedPerformed => return Ok(Delivery::TargetedEvent),
            PatternResult::Unsupported | PatternResult::Unavailable => (),
        }
    }
    Err(DriverError::ElementNotActionable)
}
async fn secondary(
    source: &dyn UiaSource,
    node: NodeId,
    hwnd: u64,
    props: &Properties,
    action: &NodeAction,
    budget: &Budget<'_>,
) -> Result<Delivery> {
    let native = match action {
        NodeAction::Press | NodeAction::Confirm => {
            return press(source, node, hwnd, props, budget).await
        }
        NodeAction::Toggle => PatternAction::Toggle,
        NodeAction::Select => PatternAction::Select,
        NodeAction::Expand => PatternAction::Expand,
        NodeAction::Collapse => PatternAction::Collapse,
        NodeAction::Increment => PatternAction::AdjustRange(true),
        NodeAction::Decrement => PatternAction::AdjustRange(false),
        NodeAction::ScrollIntoView => PatternAction::ScrollIntoView,
        NodeAction::ScrollUp => {
            return super::scroll::scroll(source, node, hwnd, props, Dir::Up, 1., budget).await
        }
        NodeAction::ScrollDown => {
            return super::scroll::scroll(source, node, hwnd, props, Dir::Down, 1., budget).await
        }
        NodeAction::ScrollLeft => {
            return super::scroll::scroll(source, node, hwnd, props, Dir::Left, 1., budget).await
        }
        NodeAction::ScrollRight => {
            return super::scroll::scroll(source, node, hwnd, props, Dir::Right, 1., budget).await
        }
        // UIA has no background semantic ShowMenu/Cancel/Raise. Arbitrary names
        // must never become SetFocus or a guessed default action.
        _ => return Err(DriverError::ElementNotActionable),
    };
    finish(
        budget.step(source, node, hwnd, props, native).await?,
        DriverError::ElementNotActionable,
    )
}

pub(super) fn same_identity(old: &Properties, fresh: &Properties) -> bool {
    old.control_type == fresh.control_type
        && old.name == fresh.name
        && old.identifier == fresh.identifier
        && old.aria_role == fresh.aria_role
        && old.class_name == fresh.class_name
        && old.frame == fresh.frame
}
