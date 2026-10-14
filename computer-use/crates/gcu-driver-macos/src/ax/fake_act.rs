//! Scripted [`super::source::FakeAxSource`] actions. No AX calls.
//!
//! Lookups walk the bound tree by id. A missing id is not replaced with a
//! node that has the same title. Action methods do not honor
//! [`super::source::FakeAxSource::delay`]; that delay is for tree reads.

use gcu_core::driver::{DriverError, Result};
use gcu_core::model::TextRange;

use super::actions::ax_action_name;
use super::source::{AxCall, AxIdentity, FakeAct, FakeAxNode, FakeAxSource};

pub(super) async fn action_names(source: &FakeAxSource, node: &FakeAxNode) -> Result<Vec<String>> {
    require(source, node.id)?;
    if let Some(raw) = &node.raw_actions {
        return Ok(raw.clone());
    }
    Ok(node.actions.iter().map(ax_action_name).collect())
}

pub(super) async fn invoke(source: &FakeAxSource, node: &FakeAxNode, name: &str) -> Result<AxCall> {
    refuse_focus(name)?;
    require(source, node.id)?;
    match plan(source, name)? {
        Plan::Uncertain => uncertain(),
        Plan::Unavailable => Ok(AxCall::Unavailable),
        Plan::Unsupported => Ok(AxCall::Unsupported),
        Plan::Run(hang) => {
            if let Some(hang) = hang {
                tokio::time::sleep(hang).await;
            }
            if !advertised(node, name) {
                return Ok(AxCall::Unsupported);
            }
            Ok(AxCall::Performed)
        }
    }
}

pub(super) async fn attribute_settable(
    source: &FakeAxSource,
    node: &FakeAxNode,
    attribute: &str,
) -> Result<Option<bool>> {
    require(source, node.id)?;
    if let Some(over) = act(source)?.settable.get(attribute).copied() {
        return Ok(over);
    }
    Ok(match attribute {
        "AXValue" => Some(node.states.settable),
        "AXSelectedTextRange" => Some(node.states.editable || node.selected_text_range.is_some()),
        "AXSelectedChildren" => Some(matches!(
            node.role.as_str(),
            "AXList" | "AXOutline" | "AXTable" | "AXBrowser" | "AXScrollArea"
        )),
        _ => None,
    })
}

pub(super) async fn set_string(
    source: &FakeAxSource,
    node: &FakeAxNode,
    attribute: &str,
    _value: &str,
) -> Result<AxCall> {
    mutate(source, node, attribute).await
}

pub(super) async fn set_number(
    source: &FakeAxSource,
    node: &FakeAxNode,
    attribute: &str,
    _value: f64,
) -> Result<AxCall> {
    mutate(source, node, attribute).await
}

pub(super) async fn set_text_range(
    source: &FakeAxSource,
    node: &FakeAxNode,
    _range: TextRange,
) -> Result<AxCall> {
    mutate(source, node, "AXSelectedTextRange").await
}

pub(super) async fn select_child(
    source: &FakeAxSource,
    parent: &FakeAxNode,
    _child: &FakeAxNode,
) -> Result<AxCall> {
    mutate(source, parent, "AXSelectedChildren").await
}

pub(super) async fn parent(source: &FakeAxSource, node: &FakeAxNode) -> Result<Option<FakeAxNode>> {
    require(source, node.id)?;
    let root = root(source)?;
    Ok(root
        .as_ref()
        .and_then(|tree| find_parent(tree, node.id))
        .cloned())
}

pub(super) async fn fresh_identity(source: &FakeAxSource, node: &FakeAxNode) -> Result<AxIdentity> {
    require(source, node.id)?;
    let hang = act(source)?.hang_identity;
    if let Some(hang) = hang {
        tokio::time::sleep(hang).await;
    }
    require(source, node.id)?;
    let root = root(source)?;
    let Some(found) = root.as_ref().and_then(|tree| find(tree, node.id)) else {
        return Err(DriverError::StaleElement);
    };
    let mut role = found.role.clone();
    if act(source)?.twist_role {
        role.push_str("#stale");
    }
    Ok(AxIdentity {
        role,
        title: found.title.clone(),
        frame: found.frame,
    })
}

pub(super) async fn resolve(source: &FakeAxSource, id: u64) -> Result<Option<FakeAxNode>> {
    injected(source, id)?;
    if act(source)?.missing.contains(&id) {
        return Ok(None);
    }
    let root = root(source)?;
    Ok(root.as_ref().and_then(|tree| find(tree, id)).cloned())
}

pub(super) fn note(source: &FakeAxSource, entry: &str) {
    if let Ok(mut act) = source.act.lock() {
        act.log.push(entry.to_string());
    }
}

pub(super) fn calls(source: &FakeAxSource) -> Vec<String> {
    source
        .act
        .lock()
        .map(|act| act.log.clone())
        .unwrap_or_default()
}

async fn mutate(source: &FakeAxSource, node: &FakeAxNode, name: &str) -> Result<AxCall> {
    refuse_focus(name)?;
    require(source, node.id)?;
    match plan(source, name)? {
        Plan::Uncertain => uncertain(),
        Plan::Unavailable => Ok(AxCall::Unavailable),
        Plan::Unsupported => Ok(AxCall::Unsupported),
        Plan::Run(hang) => {
            if let Some(hang) = hang {
                tokio::time::sleep(hang).await;
            }
            Ok(AxCall::Performed)
        }
    }
}

enum Plan {
    Uncertain,
    Unavailable,
    Unsupported,
    Run(Option<std::time::Duration>),
}

fn plan(source: &FakeAxSource, name: &str) -> Result<Plan> {
    let mut act = act(source)?;
    if act.uncertain.iter().any(|item| item == name) || act.succeed_times == Some(0) {
        return Ok(Plan::Uncertain);
    }
    if act.unavailable.iter().any(|item| item == name) {
        return Ok(Plan::Unavailable);
    }
    if act.unsupported.iter().any(|item| item == name) {
        return Ok(Plan::Unsupported);
    }
    if let Some(left) = act.succeed_times {
        act.succeed_times = Some(left.saturating_sub(1));
    }
    Ok(Plan::Run(act.hang_mutation))
}

fn injected(source: &FakeAxSource, id: u64) -> Result<()> {
    if source.error_on_id == Some(id) {
        return Err(source.error.clone());
    }
    Ok(())
}

fn require(source: &FakeAxSource, id: u64) -> Result<()> {
    injected(source, id)?;
    if act(source)?.missing.contains(&id) {
        return Err(DriverError::StaleElement);
    }
    Ok(())
}

fn refuse_focus(name: &str) -> Result<()> {
    if super::actions::is_focus_substitute(name) {
        return Err(DriverError::ElementNotActionable);
    }
    Ok(())
}

fn uncertain() -> Result<AxCall> {
    Err(DriverError::Native {
        message: "accessibility action outcome is unknown".into(),
        maybe_executed: true,
    })
}

fn advertised(node: &FakeAxNode, name: &str) -> bool {
    if let Some(raw) = &node.raw_actions {
        return raw.iter().any(|item| item == name);
    }
    node.actions
        .iter()
        .any(|action| ax_action_name(action) == name)
}

fn act(source: &FakeAxSource) -> Result<std::sync::MutexGuard<'_, FakeAct>> {
    source.act.lock().map_err(|_| DriverError::Crashed {
        maybe_executed: false,
    })
}

fn root(source: &FakeAxSource) -> Result<Option<FakeAxNode>> {
    let guard = source.root.lock().map_err(|_| DriverError::Crashed {
        maybe_executed: false,
    })?;
    Ok(guard.clone())
}

fn find(node: &FakeAxNode, id: u64) -> Option<&FakeAxNode> {
    if node.id == id {
        return Some(node);
    }
    for list in child_lists(node).into_iter().flatten() {
        for child in list {
            if let Some(found) = find(child, id) {
                return Some(found);
            }
        }
    }
    None
}

fn find_parent(node: &FakeAxNode, id: u64) -> Option<&FakeAxNode> {
    for list in child_lists(node).into_iter().flatten() {
        for child in list {
            if child.id == id {
                return Some(node);
            }
            if let Some(found) = find_parent(child, id) {
                return Some(found);
            }
        }
    }
    None
}

fn child_lists(node: &FakeAxNode) -> [Option<&[FakeAxNode]>; 3] {
    [
        node.visible_children.as_deref(),
        node.contents.as_deref(),
        Some(node.children.as_slice()),
    ]
}
