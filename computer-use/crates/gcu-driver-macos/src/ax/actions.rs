//! Semantic accessibility actions. No macOS calls.
//!
//! Press tries `AXPress`, then `AXConfirm`, then `AXOpen`, then selecting a
//! row, then `AXPress` on a descendant. Each step runs only after the previous
//! one is definitely unsupported or unavailable. An uncertain result stops the
//! chain. `AXRaise`, `AXFocus`, and `AXFocused` are never used as a click.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gcu_core::driver::{Cancel, Delivery, DriverAction, DriverError, Result};
use gcu_core::model::{NativeHandle, NodeAction, Rect};

use super::source::{AxCall, AxHandle, AxIdentity, AxSource, AxStateBits};

const PRESS_CHAIN: &[&str] = &["AXPress", "AXConfirm", "AXOpen"];
const FRAME_EPSILON: f64 = 0.5;
const DESCENDANT_DEPTH: usize = 8;
const DESCENDANT_LIMIT: usize = 32;

/// True for element actions. Pointer, keyboard, and activate stay elsewhere.
pub(crate) fn is_semantic(action: &DriverAction) -> bool {
    matches!(
        action,
        DriverAction::Press { .. }
            | DriverAction::SecondaryAction { .. }
            | DriverAction::SetValue { .. }
            | DriverAction::SelectText { .. }
            | DriverAction::ScrollElement { .. }
    )
}

/// AX names that must not stand in for a click or a semantic action.
pub(crate) fn is_focus_substitute(name: &str) -> bool {
    matches!(name, "AXRaise" | "AXFocus" | "AXFocused")
}

/// Native name for one normalized action.
pub(crate) fn ax_action_name(action: &NodeAction) -> String {
    match action {
        NodeAction::Press => "AXPress",
        NodeAction::ShowMenu => "AXShowMenu",
        NodeAction::Increment => "AXIncrement",
        NodeAction::Decrement => "AXDecrement",
        NodeAction::Confirm => "AXConfirm",
        NodeAction::Cancel => "AXCancel",
        NodeAction::Raise => "AXRaise",
        NodeAction::Expand => "AXExpand",
        NodeAction::Collapse => "AXCollapse",
        NodeAction::ScrollIntoView => "AXScrollToVisible",
        NodeAction::ScrollUp => "AXScrollUpByPage",
        NodeAction::ScrollDown => "AXScrollDownByPage",
        NodeAction::ScrollLeft => "AXScrollLeftByPage",
        NodeAction::ScrollRight => "AXScrollRightByPage",
        NodeAction::Select => "AXPick",
        NodeAction::Toggle => "AXToggle",
        NodeAction::Native(name) => name.as_str(),
    }
    .to_string()
}

/// Perform one semantic accessibility action.
///
/// Blocks only on the source's futures. The whole call stops at `deadline`;
/// the driver passes [`gcu_core::driver::DRIVER_DEADLINE`] (4.75s). Cancel
/// before a mutation returns [`DriverError::Cancelled`] with
/// `maybe_executed = false`. Cancel after a call is dispatched returns
/// `maybe_executed = true`. An uncertain result is not retried and is not
/// replaced with the next fallback. This does not raise or focus the target.
pub async fn perform<S: AxSource>(
    source: &S,
    req: &DriverAction,
    cancel: &Cancel,
    deadline: Duration,
) -> Result<Delivery> {
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }
    tracing::debug!(kind = kind(req), "semantic accessibility action");
    let budget = Budget::new(deadline, cancel);
    let Some(element) = target(req) else {
        return Err(DriverError::CapabilityUnavailable);
    };
    let node = bind(source, element, &budget).await?;
    match req {
        DriverAction::Press { .. } => press(source, &node, &budget).await,
        DriverAction::SecondaryAction { action, .. } => {
            secondary(source, &node, action, &budget).await
        }
        DriverAction::SetValue { value, .. } => {
            super::value::set_value(source, &node, value, &budget).await
        }
        DriverAction::SelectText { range, .. } => {
            super::text::select_text(source, &node, *range, &budget).await
        }
        DriverAction::ScrollElement { dir, pages, .. } => {
            super::scroll::scroll_element(source, &node, *dir, *pages, &budget).await
        }
        _ => Err(DriverError::CapabilityUnavailable),
    }
}

pub(super) struct Budget<'a> {
    started: Instant,
    deadline: Duration,
    cancel: &'a Cancel,
    inflight: AtomicBool,
    committed: AtomicBool,
}

impl<'a> Budget<'a> {
    pub(super) fn new(deadline: Duration, cancel: &'a Cancel) -> Self {
        Self {
            started: Instant::now(),
            deadline,
            cancel,
            inflight: AtomicBool::new(false),
            committed: AtomicBool::new(false),
        }
    }

    fn uncertain(&self) -> bool {
        self.inflight.load(Ordering::Relaxed) || self.committed.load(Ordering::Relaxed)
    }

    pub(super) fn begin(&self) {
        self.inflight.store(true, Ordering::Relaxed);
    }

    /// `keep` remembers that an earlier mutation may have landed.
    pub(super) fn finish(&self, keep: bool) {
        if keep {
            self.committed.store(true, Ordering::Relaxed);
        }
        self.inflight.store(false, Ordering::Relaxed);
    }
}

pub(super) async fn guarded<T>(
    budget: &Budget<'_>,
    fut: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    if budget.cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: budget.uncertain(),
        });
    }
    let left = budget.deadline.saturating_sub(budget.started.elapsed());
    if left.is_zero() {
        return Err(DriverError::Timeout {
            maybe_executed: budget.uncertain(),
        });
    }
    tokio::select! {
        biased;
        _ = budget.cancel.cancelled() => Err(DriverError::Cancelled {
            maybe_executed: budget.uncertain(),
        }),
        result = tokio::time::timeout(left, fut) => match result {
            Ok(value) => value,
            Err(_) => Err(DriverError::Timeout {
                maybe_executed: budget.uncertain(),
            }),
        },
    }
}

pub(super) enum Step {
    Done,
    Continue,
}

/// Try one named action. Unsupported and unavailable continue. Any other error stops.
pub(super) async fn try_named<S: AxSource>(
    source: &S,
    node: &S::Node,
    names: &[String],
    states: &AxStateBits,
    name: &str,
    budget: &Budget<'_>,
    press_chain: bool,
) -> Result<Step> {
    if !names.iter().any(|item| item == name) {
        source.note(&format!("skip:{name}"));
        return Ok(Step::Continue);
    }
    if is_focus_substitute(name) {
        source.note(&format!("refuse:{name}"));
        if press_chain {
            return Ok(Step::Continue);
        }
        return Err(DriverError::ElementNotActionable);
    }
    if states.enabled == Some(false) {
        source.note(&format!("unavailable:{name}"));
        return Ok(Step::Continue);
    }
    budget.begin();
    source.note(&format!("perform:{name}"));
    match guarded(budget, source.invoke(node, name)).await {
        Ok(AxCall::Performed) => {
            budget.finish(true);
            Ok(Step::Done)
        }
        Ok(AxCall::Unsupported | AxCall::Unavailable) => {
            budget.finish(false);
            Ok(Step::Continue)
        }
        Err(error) => {
            budget.finish(error.maybe_executed());
            Err(error)
        }
    }
}

async fn press<S: AxSource>(source: &S, node: &S::Node, budget: &Budget<'_>) -> Result<Delivery> {
    let names = guarded(budget, source.action_names(node)).await?;
    let states = guarded(budget, source.states(node)).await?;
    for name in PRESS_CHAIN {
        if let Step::Done = try_named(source, node, &names, &states, name, budget, true).await? {
            return Ok(Delivery::Accessibility);
        }
    }
    if let Step::Done = select_row(source, node, budget).await? {
        return Ok(Delivery::Accessibility);
    }
    if press_descendants(source, node, budget).await? {
        return Ok(Delivery::Accessibility);
    }
    Err(DriverError::ElementNotActionable)
}

async fn secondary<S: AxSource>(
    source: &S,
    node: &S::Node,
    action: &NodeAction,
    budget: &Budget<'_>,
) -> Result<Delivery> {
    let name = ax_action_name(action);
    if is_focus_substitute(&name) {
        source.note(&format!("refuse:{name}"));
        return Err(DriverError::ElementNotActionable);
    }
    let names = guarded(budget, source.action_names(node)).await?;
    let states = guarded(budget, source.states(node)).await?;
    match try_named(source, node, &names, &states, &name, budget, false).await? {
        Step::Done => Ok(Delivery::Accessibility),
        Step::Continue => Err(DriverError::ElementNotActionable),
    }
}

async fn select_row<S: AxSource>(source: &S, node: &S::Node, budget: &Budget<'_>) -> Result<Step> {
    let role = guarded(budget, source.role(node)).await?;
    if role != "AXRow" && role != "AXOutlineRow" {
        return Ok(Step::Continue);
    }
    let Some(parent) = guarded(budget, source.parent(node)).await? else {
        source.note("skip:AXSelectedChildren");
        return Ok(Step::Continue);
    };
    match guarded(
        budget,
        source.attribute_settable(&parent, "AXSelectedChildren"),
    )
    .await?
    {
        Some(true) => {
            budget.begin();
            source.note("select:AXSelectedChildren");
            match guarded(budget, source.select_child(&parent, node)).await {
                Ok(AxCall::Performed) => {
                    budget.finish(true);
                    Ok(Step::Done)
                }
                Ok(AxCall::Unsupported | AxCall::Unavailable) => {
                    budget.finish(false);
                    Ok(Step::Continue)
                }
                Err(error) => {
                    budget.finish(error.maybe_executed());
                    Err(error)
                }
            }
        }
        Some(false) => {
            source.note("unavailable:AXSelectedChildren");
            Ok(Step::Continue)
        }
        None => {
            source.note("skip:AXSelectedChildren");
            Ok(Step::Continue)
        }
    }
}

async fn press_descendants<S: AxSource>(
    source: &S,
    node: &S::Node,
    budget: &Budget<'_>,
) -> Result<bool> {
    let mut stack = Vec::new();
    for child in guarded(budget, source.children(node))
        .await?
        .into_iter()
        .rev()
    {
        stack.push((child, 1usize));
    }
    let mut seen = 0usize;
    while let Some((current, depth)) = stack.pop() {
        if seen >= DESCENDANT_LIMIT {
            break;
        }
        seen += 1;
        let names = guarded(budget, source.action_names(&current)).await?;
        let states = guarded(budget, source.states(&current)).await?;
        if names.iter().any(|name| name == "AXPress") {
            if let Step::Done =
                try_named(source, &current, &names, &states, "AXPress", budget, true).await?
            {
                return Ok(true);
            }
        }
        if depth < DESCENDANT_DEPTH {
            let children = guarded(budget, source.children(&current)).await?;
            for child in children.into_iter().rev() {
                stack.push((child, depth + 1));
            }
        }
    }
    Ok(false)
}

async fn bind<S: AxSource>(
    source: &S,
    element: &NativeHandle,
    budget: &Budget<'_>,
) -> Result<S::Node> {
    let Some(handle) = element.downcast_ref::<AxHandle>() else {
        return Err(DriverError::StaleElement);
    };
    let Some(node) = guarded(budget, source.resolve(handle.id)).await? else {
        return Err(DriverError::StaleElement);
    };
    let fresh = guarded(budget, source.fresh_identity(&node)).await?;
    if !same_identity(&handle.identity, &fresh) {
        return Err(DriverError::StaleElement);
    }
    Ok(node)
}

fn same_identity(observed: &AxIdentity, fresh: &AxIdentity) -> bool {
    observed.role == fresh.role
        && observed.title == fresh.title
        && frames_close(observed.frame, fresh.frame)
}

fn frames_close(left: Option<Rect>, right: Option<Rect>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            (left.x - right.x).abs() <= FRAME_EPSILON
                && (left.y - right.y).abs() <= FRAME_EPSILON
                && (left.w - right.w).abs() <= FRAME_EPSILON
                && (left.h - right.h).abs() <= FRAME_EPSILON
        }
        _ => false,
    }
}

fn target(action: &DriverAction) -> Option<&NativeHandle> {
    match action {
        DriverAction::Press { el }
        | DriverAction::SecondaryAction { el, .. }
        | DriverAction::SetValue { el, .. }
        | DriverAction::SelectText { el, .. }
        | DriverAction::ScrollElement { el, .. } => Some(el),
        _ => None,
    }
}

fn kind(action: &DriverAction) -> &'static str {
    match action {
        DriverAction::Press { .. } => "press",
        DriverAction::SecondaryAction { .. } => "secondary",
        DriverAction::SetValue { .. } => "set_value",
        DriverAction::SelectText { .. } => "select_text",
        DriverAction::ScrollElement { .. } => "scroll",
        _ => "other",
    }
}
