use crate::model::*;
use crate::tree::*;

fn visible(node: &ProcessedNode) -> bool {
    !node.node.states.contains(States::OFFSCREEN)
        || node.protected()
        || node.children.iter().any(visible)
}
/// Remove wholly offscreen subtrees, accounting for clipped descendants at the parent.
pub fn transform(mut tree: ProcessedTree, _: &TreeOpts) -> ProcessedTree {
    post_order(&mut tree, &|node| {
        node.children.retain(|child| {
            if visible(child) {
                true
            } else {
                node.omitted_nodes += child.accounted_count();
                false
            }
        });
    });
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_support::*;
    #[test]
    fn prune_offscreen_hidden_leaf_counted() {
        let tree = {
            let mut a = n(Role::StaticText, "Hidden", vec![]);
            a.states |= States::OFFSCREEN;
            input(vec![a])
        };
        check(
            "prune_offscreen_hidden_leaf_counted",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn prune_offscreen_visible_descendant_retained() {
        let tree = {
            let mut a = n(
                Role::Group,
                "Container",
                vec![n(Role::Button, "Visible", vec![])],
            );
            a.states |= States::OFFSCREEN;
            input(vec![a])
        };
        check(
            "prune_offscreen_visible_descendant_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn prune_offscreen_focused_offscreen_retained() {
        let tree = {
            let mut a = n(Role::TextField, "Focus", vec![]);
            a.states |= States::OFFSCREEN | States::FOCUSED;
            input(vec![a])
        };
        check(
            "prune_offscreen_focused_offscreen_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
}
