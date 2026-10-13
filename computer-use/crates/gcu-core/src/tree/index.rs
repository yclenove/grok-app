use super::*;
use crate::model::{NodeFlags, States};

fn walk(node: &mut ProcessedNode, parent: Option<u32>, tree: &mut ProcessedTree) {
    let index = tree.element_index.len() as u32;
    node.element_index = index;
    tree.element_index.insert(
        index,
        IndexedElement {
            handle: node.node.handle.clone(),
            fingerprint: node.node.fingerprint,
            role: node.node.role,
            frame: node.node.frame,
            flags: node.flags,
            parent,
        },
    );
    if node.node.states.contains(States::FOCUSED) {
        tree.focused = Some(index);
    }
    if super::prepare::modal(&node.node) {
        tree.popups.push(PopupInfo {
            index,
            role: node.node.role,
            frame: node.node.frame,
        });
    }
    if node.flags.contains(NodeFlags::TERMINAL_LIKE) {
        tree.terminal_like_indices.push(index);
    }
    for child in &mut node.children {
        walk(child, Some(index), tree);
    }
}
pub(crate) fn assign(mut tree: ProcessedTree) -> ProcessedTree {
    tree.element_index.clear();
    tree.focused = None;
    tree.popups.clear();
    tree.terminal_like_indices.clear();
    if let Some(mut root) = tree.root.take() {
        walk(&mut root, None, &mut tree);
        tree.omitted_nodes = 0;
        fn omissions(n: &ProcessedNode) -> usize {
            n.omitted_nodes + n.children.iter().map(omissions).sum::<usize>()
        }
        tree.omitted_nodes = omissions(&root);
        tree.root = Some(root);
    }
    tree
}
