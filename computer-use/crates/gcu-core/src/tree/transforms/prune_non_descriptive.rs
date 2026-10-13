use crate::model::*;
use crate::tree::*;

fn descriptive(node: &ProcessedNode) -> bool {
    node.protected()
        || node.interactive()
        || node.display_text().is_some()
        || node.node.value.is_some()
        || node.node.description.is_some()
        || node.node.placeholder.is_some()
        || node.node.help.is_some()
        || node.node.url.is_some()
        || node.omitted_nodes != 0
        || node.node.role == Role::Canvas
        || node.children.iter().any(descriptive)
}
/// Drop empty noninteractive subtrees; canvas and protected surfaces always survive.
pub fn transform(mut tree: ProcessedTree, _: &TreeOpts) -> ProcessedTree {
    post_order(&mut tree, &|node| node.children.retain(descriptive));
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_support::*;
    #[test]
    fn prune_non_descriptive_empty_subtree_removed() {
        let tree = { input(vec![n(Role::Group, "", vec![n(Role::Unknown, "", vec![])])]) };
        check(
            "prune_non_descriptive_empty_subtree_removed",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn prune_non_descriptive_interactive_descendant_retained() {
        let tree = { input(vec![n(Role::Group, "", vec![n(Role::Button, "", vec![])])]) };
        check(
            "prune_non_descriptive_interactive_descendant_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn prune_non_descriptive_descriptive_value_retained() {
        let tree = {
            let mut a = n(Role::StaticText, "", vec![]);
            a.value = Some(NodeValue::Text("Description".into()));
            input(vec![a])
        };
        check(
            "prune_non_descriptive_descriptive_value_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
}
