//! Raw-node helpers for the fake desktop, without OS calls.
use super::Oracle;
use gcu_core::{driver::*, fingerprint::refresh_tree, model::*};
pub(crate) fn find<'a>(node: &'a RawNode, id: &str) -> Option<&'a RawNode> {
    if node.identifier.as_deref() == Some(id) {
        return Some(node);
    }
    node.children.iter().find_map(|child| find(child, id))
}
pub(crate) fn find_mut<'a>(node: &'a mut RawNode, id: &str) -> Option<&'a mut RawNode> {
    if node.identifier.as_deref() == Some(id) {
        return Some(node);
    }
    node.children
        .iter_mut()
        .find_map(|child| find_mut(child, id))
}
pub(crate) fn remove(node: &mut RawNode, id: &str) -> bool {
    if let Some(i) = node
        .children
        .iter()
        .position(|n| n.identifier.as_deref() == Some(id))
    {
        node.children.remove(i);
        return true;
    }
    node.children.iter_mut().any(|child| remove(child, id))
}
pub(crate) fn focused(node: &RawNode) -> Option<&RawNode> {
    if node.states.contains(States::FOCUSED) {
        Some(node)
    } else {
        node.children.iter().find_map(focused)
    }
}
pub(crate) fn set_focus(node: &mut RawNode, id: &str) {
    node.states
        .set(States::FOCUSED, node.identifier.as_deref() == Some(id));
    for child in &mut node.children {
        set_focus(child, id);
    }
}
pub(crate) fn actionable(node: &RawNode) -> Result<()> {
    if !node.states.contains(States::ENABLED) {
        Err(DriverError::ElementNotActionable)
    } else {
        Ok(())
    }
}
pub(crate) fn settable(node: &RawNode) -> Result<()> {
    actionable(node)?;
    if node.states.intersects(States::SETTABLE | States::EDITABLE) {
        Ok(())
    } else {
        Err(DriverError::NotSettable)
    }
}
pub(crate) fn text(node: &RawNode) -> Result<&str> {
    match &node.value {
        Some(NodeValue::Text(value)) => Ok(value),
        _ => Err(DriverError::NotSettable),
    }
}
pub(crate) fn replace_selection(
    previous: &str,
    selected: Option<TextRange>,
    input: &str,
) -> String {
    let Some(range) = selected else {
        return format!("{previous}{input}");
    };
    previous
        .chars()
        .take(range.start)
        .chain(input.chars())
        .chain(
            previous
                .chars()
                .skip(range.start.saturating_add(range.length)),
        )
        .collect()
}
pub(crate) fn contains(frame: Rect, point: WindowPoint) -> bool {
    point.x >= frame.x
        && point.x < frame.x + frame.w
        && point.y >= frame.y
        && point.y < frame.y + frame.h
}
pub(crate) fn hit(node: &RawNode, point: WindowPoint) -> Option<&RawNode> {
    if node.states.contains(States::OFFSCREEN) || !node.frame.is_some_and(|r| contains(r, point)) {
        return None;
    }
    node.children
        .iter()
        .rev()
        .find_map(|child| hit(child, point))
        .or(Some(node))
}
pub(crate) fn scroll_hit(node: &RawNode, point: WindowPoint) -> Option<&RawNode> {
    if node.states.contains(States::OFFSCREEN) || !node.frame.is_some_and(|r| contains(r, point)) {
        return None;
    }
    node.children
        .iter()
        .rev()
        .find_map(|child| scroll_hit(child, point))
        .or_else(|| {
            matches!(node.role, Role::List | Role::ScrollArea | Role::Table).then_some(node)
        })
}
pub(crate) fn modal(node: &RawNode) -> Option<&RawNode> {
    if node.states.contains(States::MODAL) {
        Some(node)
    } else {
        node.children.iter().rev().find_map(modal)
    }
}
pub(crate) fn layout_lists(node: &mut RawNode, offset: i64) {
    if node.role == Role::List {
        if let Some(frame) = node.frame {
            for (i, row) in node.children.iter_mut().enumerate() {
                let y = frame.y + (i as i64 - offset) as f64 * 24.0;
                row.frame = Some(Rect {
                    x: frame.x,
                    y,
                    w: frame.w,
                    h: 24.0,
                });
                row.states.set(
                    States::OFFSCREEN,
                    y < frame.y || y + 24.0 > frame.y + frame.h,
                );
            }
        }
    } else {
        for child in &mut node.children {
            layout_lists(child, offset);
        }
    }
    refresh_tree(node, Fingerprint::default(), 0);
}
pub(crate) fn collect_values(node: &RawNode, oracle: &mut Oracle) {
    if let Some(id) = &node.identifier {
        match &node.value {
            Some(NodeValue::Text(text)) => {
                oracle.text.insert(id.clone(), text.clone());
            }
            Some(NodeValue::Range { value, .. } | NodeValue::Number(value)) => {
                oracle.numbers.insert(id.clone(), *value);
            }
            _ => {}
        }
    }
    for child in &node.children {
        collect_values(child, oracle);
    }
}
