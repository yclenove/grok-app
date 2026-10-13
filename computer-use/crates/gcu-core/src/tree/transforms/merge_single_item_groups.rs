use crate::model::*;
use crate::tree::*;

fn foldable(node: &ProcessedNode) -> bool {
    node.node.role == Role::Group
        && node.node.name.as_deref().is_none_or(str::is_empty)
        && node.node.value.is_none()
        && node.node.description.is_none()
        && node.node.help.is_none()
        && node.node.placeholder.is_none()
        && node.node.url.is_none()
        && !node.interactive()
        && !node.protected()
        && node.omitted_nodes == 0
        && node.children.len() == 1
}
/// Replace anonymous single-child groups with the child, retaining the child's handle.
pub fn transform(mut tree: ProcessedTree, _: &TreeOpts) -> ProcessedTree {
    post_order(&mut tree, &|node| {
        for child in &mut node.children {
            while foldable(child) {
                if let Some(grandchild) = child.children.pop() {
                    *child = grandchild;
                }
            }
        }
    });
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_support::*;
    #[test]
    fn merge_single_item_groups_anonymous_folded() {
        let tree = {
            input(vec![n(
                Role::Group,
                "",
                vec![n(Role::Button, "Save", vec![])],
            )])
        };
        check(
            "merge_single_item_groups_anonymous_folded",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn merge_single_item_groups_named_retained() {
        let tree = {
            input(vec![n(
                Role::Group,
                "Settings",
                vec![n(Role::Button, "Save", vec![])],
            )])
        };
        check(
            "merge_single_item_groups_named_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn merge_single_item_groups_multiple_retained() {
        let tree = {
            input(vec![n(
                Role::Group,
                "",
                vec![n(Role::Button, "A", vec![]), n(Role::Button, "B", vec![])],
            )])
        };
        check(
            "merge_single_item_groups_multiple_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
}
