//! Resolve an intent to a processed node and its safety context.
//!
//! Lookup is synchronous and does not block, time out, or cancel. It does not
//! read the filesystem. Coordinate hits pick the deepest node whose frame
//! contains the point. An element index wins over coordinates.

use super::ActionIntent;
use crate::model::{NodeFlags, Role};
use crate::tree::{ProcessedNode, ProcessedTree};

/// A node plus the web-area and execution-surface flags of it and its ancestors.
pub(crate) struct Resolved<'a> {
    /// Matched node.
    pub node: &'a ProcessedNode,
    /// The node is inside a web area, or is one.
    pub in_web: bool,
    /// The node or an ancestor is a terminal, console, REPL, debugger, or task runner.
    pub execution: bool,
}

/// Resolve `intent` in `tree`.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: shared
/// references only.
pub(crate) fn resolve<'a>(tree: &'a ProcessedTree, intent: &ActionIntent) -> Option<Resolved<'a>> {
    let root = tree.root.as_ref()?;
    if let Some(index) = intent.element_index {
        return find_index(root, index, false, false);
    }
    let (x, y) = intent.at?;
    hit(root, x, y, false, false)
}

/// Focused node, if the pipeline recorded one.
pub(crate) fn focused(tree: &ProcessedTree) -> Option<&ProcessedNode> {
    let index = tree.focused?;
    find_index(tree.root.as_ref()?, index, false, false).map(|hit| hit.node)
}

/// Human-visible labels. Values are omitted so typed secrets are not scanned.
pub(crate) fn labels(node: &ProcessedNode) -> Vec<&str> {
    [
        node.node.name.as_deref(),
        node.node.description.as_deref(),
        node.node.placeholder.as_deref(),
        node.node.help.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn find_index<'a>(
    node: &'a ProcessedNode,
    index: u32,
    in_web: bool,
    execution: bool,
) -> Option<Resolved<'a>> {
    let in_web = in_web || node.node.role == Role::WebArea;
    let execution = execution || is_execution(node);
    if node.element_index == index {
        return Some(Resolved {
            node,
            in_web,
            execution,
        });
    }
    node.children
        .iter()
        .find_map(|child| find_index(child, index, in_web, execution))
}

fn hit<'a>(
    node: &'a ProcessedNode,
    x: f64,
    y: f64,
    in_web: bool,
    execution: bool,
) -> Option<Resolved<'a>> {
    if !contains(node, x, y) {
        return None;
    }
    let in_web = in_web || node.node.role == Role::WebArea;
    let execution = execution || is_execution(node);
    for child in &node.children {
        if let Some(found) = hit(child, x, y, in_web, execution) {
            return Some(found);
        }
    }
    Some(Resolved {
        node,
        in_web,
        execution,
    })
}

fn contains(node: &ProcessedNode, x: f64, y: f64) -> bool {
    node.node.frame.is_some_and(|frame| {
        x >= frame.x && y >= frame.y && x < frame.x + frame.w && y < frame.y + frame.h
    })
}

pub(crate) fn is_execution(node: &ProcessedNode) -> bool {
    if node.flags.contains(NodeFlags::TERMINAL_LIKE) {
        return true;
    }
    let native = node.node.native_role.to_ascii_lowercase();
    native.contains("terminal")
        || native.contains("console")
        || native.contains("repl")
        || native.contains("debug")
        || native.contains("task runner")
        || native.contains("taskrunner")
        || native.contains("task-runner")
}

/// Whether any interactive node is execution-like, and whether any is not.
pub(crate) fn execution_mix(tree: &ProcessedTree) -> (bool, bool) {
    let mut execution = false;
    let mut other = false;
    for (_, node) in tree.nodes() {
        if !node.interactive() {
            continue;
        }
        if is_execution(node) {
            execution = true;
        } else {
            other = true;
        }
    }
    (execution, other)
}
