use crate::model::*;
use crate::tree::*;

fn empty(node: &ProcessedNode) -> bool {
    node.node.name.as_deref().is_none_or(str::is_empty)
        && node.node.value.is_none()
        && node.node.description.is_none()
        && node.node.placeholder.is_none()
        && node.node.help.is_none()
}
/// Remove disabled, unnamed, valueless leaves while retaining safety/focus metadata.
pub fn transform(mut tree: ProcessedTree, _: &TreeOpts) -> ProcessedTree {
    post_order(&mut tree, &|node| {
        node.children.retain(|child| {
            child.node.states.contains(States::ENABLED)
                || !empty(child)
                || !child.children.is_empty()
                || child.protected()
                || child.omitted_nodes != 0
        })
    });
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_support::*;
    #[test]
    fn prune_empty_disabled_empty_disabled_removed() {
        let tree = {
            let mut a = n(Role::Button, "", vec![]);
            a.states = States::empty();
            input(vec![a])
        };
        check(
            "prune_empty_disabled_empty_disabled_removed",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn prune_empty_disabled_named_disabled_retained() {
        let tree = {
            let mut a = n(Role::Button, "Unavailable", vec![]);
            a.states = States::empty();
            input(vec![a])
        };
        check(
            "prune_empty_disabled_named_disabled_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn prune_empty_disabled_value_disabled_retained() {
        let tree = {
            let mut a = n(Role::TextField, "", vec![]);
            a.states = States::empty();
            a.value = Some(NodeValue::Number(0.));
            input(vec![a])
        };
        check(
            "prune_empty_disabled_value_disabled_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
}
