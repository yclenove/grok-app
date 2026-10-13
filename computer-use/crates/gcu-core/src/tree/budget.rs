use super::*;
use crate::model::NodeFlags;
use std::collections::HashSet;

/// Shared with the renderer's byte budget: lower numbers have higher priority.
pub(crate) fn priority(node: &ProcessedNode, depth: usize) -> u8 {
    if depth == 0 {
        0
    } else if node.flags.contains(NodeFlags::FOCUS_PATH) {
        1
    } else if node
        .flags
        .intersects(NodeFlags::MODAL_CONTENT | NodeFlags::MODAL_PATH)
    {
        2
    } else if node.flags.contains(NodeFlags::TITLE_BAR) {
        3
    } else if node.interactive() {
        4
    } else if node.flags.contains(NodeFlags::SUMMARY) {
        6
    } else {
        5
    }
}
fn number(node: &mut ProcessedNode, parent: Option<u32>, parents: &mut Vec<Option<u32>>) {
    node.element_index = parents.len() as u32;
    parents.push(parent);
    for child in &mut node.children {
        number(child, Some(node.element_index), parents);
    }
}
fn retain(node: &mut ProcessedNode, selected: &HashSet<u32>) {
    node.children.retain_mut(|child| {
        if selected.contains(&child.element_index) {
            retain(child, selected);
            true
        } else {
            node.omitted_nodes += child.accounted_count();
            false
        }
    });
}
pub(crate) fn apply(mut tree: ProcessedTree, opts: &TreeOpts) -> ProcessedTree {
    let mut parents = Vec::new();
    if let Some(root) = &mut tree.root {
        number(root, None, &mut parents);
    }
    let modal = tree
        .nodes()
        .iter()
        .any(|(_, n)| super::prepare::modal(&n.node));
    let mut candidates: Vec<_> = tree
        .nodes()
        .into_iter()
        .filter(|(depth, node)| {
            *depth < opts.max_tree_depth
                && (!modal
                    || *depth <= 1
                    || node.flags.intersects(
                        NodeFlags::FOCUS_PATH
                            | NodeFlags::MODAL_CONTENT
                            | NodeFlags::MODAL_PATH
                            | NodeFlags::TITLE_BAR,
                    ))
        })
        .map(|(depth, node)| (priority(node, depth), depth, node.element_index))
        .collect();
    candidates.sort_unstable();
    let mut selected = HashSet::new();
    for (_, _, index) in candidates {
        let mut path = Vec::new();
        let mut current = Some(index);
        while let Some(id) = current {
            if selected.contains(&id) {
                break;
            }
            path.push(id);
            current = parents[id as usize];
        }
        if selected.len().saturating_add(path.len()) <= opts.max_tree_nodes {
            selected.extend(path);
        }
    }
    if let Some(root) = &mut tree.root {
        if selected.contains(&root.element_index) {
            if modal {
                for child in &mut root.children {
                    if !child.flags.intersects(
                        NodeFlags::MODAL_CONTENT
                            | NodeFlags::MODAL_PATH
                            | NodeFlags::FOCUS_PATH
                            | NodeFlags::TITLE_BAR,
                    ) {
                        child.flags |= NodeFlags::SUMMARY;
                    }
                }
            }
            retain(root, &selected);
            tree.omitted_nodes = tree.nodes().iter().map(|(_, n)| n.omitted_nodes).sum();
        } else {
            tree.omitted_nodes = root.accounted_count();
            tree.root = None;
        }
    }
    tree
}
