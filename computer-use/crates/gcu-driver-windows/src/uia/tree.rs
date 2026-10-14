//! Portable UIA traversal. Native completeness survives core pruning/rendering.
use super::{
    cache::{Properties, BATCH_SIZE},
    roles::map_role,
    sensitive::is_execution_surface,
    source::{NodeId, UiaSource},
};
use gcu_core::{
    driver::{Cancel, DriverError, Result, TreeOpts},
    fingerprint,
    model::{
        Fingerprint, Incomplete, IncompleteReason, NativeHandle, NodeAction, RawNode, RawTree,
        States,
    },
};
use std::{collections::BTreeSet, future::Future, time::Duration};

/// Native read ceiling, independent of the router's DRIVER_DEADLINE.
pub const TREE_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum retained native nodes, including the window.
pub const NODE_CAP: usize = 5000;
/// Maximum native depth, counting the window as level one.
pub const DEPTH_CAP: usize = 64;

/// Build raw nodes from cached metadata. No rendering, policy, or redaction.
/// UIA rectangles already use physical pixels; only the window origin is removed.
/// Cancellation/fatal errors return errors; a deadline after retaining the root
/// returns that partial tree. An unread root returns Timeout. Unsupported HWNDs
/// can return a complete empty tree. Every await shares the same two-second budget.
pub async fn build_raw_tree(
    source: &dyn UiaSource,
    hwnd: u64,
    opts: &TreeOpts,
    cancel: &Cancel,
) -> Result<RawTree> {
    check_cancel(cancel)?;
    let empty_reason = if opts.max_tree_nodes == 0 {
        Some(IncompleteReason::NodeCap)
    } else if opts.max_tree_depth == 0 {
        Some(IncompleteReason::DepthCap)
    } else {
        None
    };
    if let Some(reason) = empty_reason {
        return Ok(RawTree {
            root: None,
            incomplete: Some(Incomplete {
                reason,
                read_nodes: 0,
                unread_nodes: None,
            }),
        });
    }
    let mut walk = Walk {
        source,
        cancel,
        deadline: tokio::time::Instant::now() + opts.timeout.min(TREE_TIMEOUT),
        max_nodes: opts.max_tree_nodes.min(NODE_CAP),
        max_depth: opts.max_tree_depth.min(DEPTH_CAP),
        count: 0,
        reason: None,
        origin: (0., 0.),
        visited: BTreeSet::new(),
    };
    let Some(root) = walk.read(source.root(hwnd)).await? else {
        return Ok(RawTree::default());
    };
    let mut raw = walk.node(root, 0).await?;
    fingerprint::refresh_tree(&mut raw, Fingerprint(0), 0);
    Ok(RawTree {
        root: Some(raw),
        incomplete: walk.reason.map(|reason| Incomplete {
            reason,
            read_nodes: walk.count as u32,
            unread_nodes: None,
        }),
    })
}
struct Walk<'a> {
    source: &'a dyn UiaSource,
    cancel: &'a Cancel,
    deadline: tokio::time::Instant,
    max_nodes: usize,
    max_depth: usize,
    count: usize,
    reason: Option<IncompleteReason>,
    origin: (f64, f64),
    visited: BTreeSet<NodeId>,
}
impl Walk<'_> {
    async fn read<T>(&self, future: impl Future<Output = Result<T>>) -> Result<T> {
        check_cancel(self.cancel)?;
        if tokio::time::Instant::now() >= self.deadline {
            return Err(DriverError::Timeout {
                maybe_executed: false,
            });
        }
        tokio::select! { biased;
            _ = self.cancel.cancelled() => Err(DriverError::Cancelled { maybe_executed: false }),
            value = tokio::time::timeout_at(self.deadline, future) => value.map_err(|_| DriverError::Timeout { maybe_executed: false })?,
        }
    }
    fn recover<T>(&mut self, result: Result<T>) -> Result<Option<T>> {
        match result {
            Ok(v) => Ok(Some(v)),
            Err(DriverError::Native { .. }) => {
                self.reason.get_or_insert(IncompleteReason::PartialError);
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
    async fn node(&mut self, id: NodeId, depth: usize) -> Result<RawNode> {
        let props = self.read(self.source.properties(id)).await;
        let props = self.recover(props)?.unwrap_or_else(|| Properties {
            partial: true,
            ..Default::default()
        });
        if props.partial {
            self.reason.get_or_insert(IncompleteReason::PartialError);
        }
        if depth == 0 {
            self.origin = props.frame.map_or((0., 0.), |r| (r.x, r.y));
        }
        let mut raw = map_properties(props, id, self.origin);
        self.count += 1;
        self.visited.insert(id);
        let mut after = None;
        loop {
            // One lookahead distinguishes an exactly-full/deep tree from a truncated one.
            let limit = (self.max_nodes - self.count).clamp(1, BATCH_SIZE);
            let page = self.read(self.source.children(id, after, limit)).await;
            let page = match page {
                Err(DriverError::Timeout { .. }) => {
                    self.reason = Some(IncompleteReason::Deadline);
                    break;
                }
                result => match self.recover(result)? {
                    Some(page) => page,
                    None => break,
                },
            };
            if page.nodes.len() > limit || (page.nodes.is_empty() && page.more) {
                self.reason.get_or_insert(IncompleteReason::PartialError);
                break;
            }
            for child in &page.nodes {
                check_cancel(self.cancel)?;
                if self.count >= self.max_nodes {
                    self.reason.get_or_insert(IncompleteReason::NodeCap);
                    return Ok(raw);
                }
                if depth + 1 >= self.max_depth {
                    self.reason.get_or_insert(IncompleteReason::DepthCap);
                    return Ok(raw);
                }
                if self.visited.contains(child) {
                    self.reason.get_or_insert(IncompleteReason::PartialError);
                    return Ok(raw);
                }
                match Box::pin(self.node(*child, depth + 1)).await {
                    Ok(child) => raw.children.push(child),
                    Err(DriverError::Timeout { .. }) => {
                        self.reason = Some(IncompleteReason::Deadline);
                        return Ok(raw);
                    }
                    Err(error) => return Err(error),
                }
                if self.reason == Some(IncompleteReason::Deadline) {
                    return Ok(raw);
                }
            }
            if !page.more {
                break;
            }
            after = page.nodes.last().copied();
        }
        Ok(raw)
    }
}
fn check_cancel(cancel: &Cancel) -> Result<()> {
    if cancel.is_cancelled() {
        Err(DriverError::Cancelled {
            maybe_executed: false,
        })
    } else {
        Ok(())
    }
}

fn map_properties(p: Properties, id: NodeId, origin: (f64, f64)) -> RawNode {
    let mut native_role = if p.control_type == 50037 {
        "UIA_TitleBarControlTypeId".to_owned()
    } else {
        format!("UIA_ControlType({})", p.control_type)
    };
    if is_execution_surface(&p) {
        native_role.push_str(":terminal");
    }
    let mut states = States::empty();
    for (state, enabled) in [
        (States::ENABLED, p.enabled),
        (States::FOCUSED, p.focused),
        (States::FOCUSABLE, p.focusable),
        (States::OFFSCREEN, p.offscreen),
        (States::PASSWORD, p.password),
        (States::REQUIRED, p.required),
        (States::MODAL, p.modal),
        (States::SELECTED, p.selected),
        (States::SELECTABLE, p.patterns.selection_item),
        (States::CHECKED, p.toggle_state == Some(1)),
        (States::MIXED, p.toggle_state == Some(2)),
        (
            States::EXPANDABLE,
            p.patterns.expand_collapse && p.expand_state != Some(3),
        ),
        (States::EXPANDED, matches!(p.expand_state, Some(1 | 2))),
    ] {
        states.set(state, enabled);
    }
    let settable = (p.patterns.value || p.patterns.range_value) && p.read_only == Some(false);
    states.set(States::SETTABLE, settable);
    states.set(
        States::EDITABLE,
        p.patterns.value && p.read_only == Some(false),
    );
    let mut actions = Vec::new();
    for (available, action) in [
        (
            super::patterns::press_patterns(p.control_type)
                .iter()
                .any(|pattern| {
                    pattern.available(&p.patterns)
                        && (*pattern != super::patterns::Pattern::ExpandCollapse
                            || p.expand_state != Some(3))
                }),
            NodeAction::Press,
        ),
        (p.patterns.toggle, NodeAction::Toggle),
        (p.patterns.selection_item, NodeAction::Select),
        (p.patterns.scroll_item, NodeAction::ScrollIntoView),
    ] {
        if available {
            actions.push(action);
        }
    }
    if p.patterns.expand_collapse && p.expand_state != Some(3) {
        actions.push(if matches!(p.expand_state, Some(1 | 2)) {
            NodeAction::Collapse
        } else {
            NodeAction::Expand
        });
    }
    if p.patterns.range_value && p.read_only == Some(false) {
        actions.extend([NodeAction::Increment, NodeAction::Decrement]);
    }
    if p.patterns.scroll {
        actions.extend([
            NodeAction::ScrollUp,
            NodeAction::ScrollDown,
            NodeAction::ScrollLeft,
            NodeAction::ScrollRight,
        ]);
    }
    let aria = p.aria_role.as_deref().or_else(|| {
        (p.control_type == 50030
            && (p.framework.as_deref() == Some("Chrome")
                || p.class_name
                    .as_deref()
                    .is_some_and(|c| c.starts_with("Chrome_"))))
        .then_some("document")
    });
    let role = map_role(p.control_type, aria, p.modal);
    RawNode {
        role,
        native_role: native_role.into(),
        subrole: p.aria_role.map(Into::into),
        name: p.name,
        value: if p.password { None } else { p.value },
        description: None,
        placeholder: None,
        help: p.help,
        identifier: p.identifier,
        states,
        actions: actions.into_iter().collect(),
        frame: p.frame.map(|r| gcu_core::model::Rect {
            x: r.x - origin.0,
            y: r.y - origin.1,
            ..r
        }),
        text_extras: None,
        url: None,
        children: Vec::new(),
        handle: NativeHandle::new(super::source::CachedElement {
            node: id,
            runtime_id: p.runtime_id,
        }),
        fingerprint: Fingerprint(0),
    }
}
