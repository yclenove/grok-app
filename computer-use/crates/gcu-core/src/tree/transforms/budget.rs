use crate::tree::*;

/// Apply breadth-first prioritized node/depth budgets, retaining modal/focus paths first.
pub fn transform(tree: ProcessedTree, opts: &TreeOpts) -> ProcessedTree {
    super::super::budget::apply(tree, opts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use crate::tree::test_support::*;
    #[test]
    fn budget_node_limit_accounted() {
        let tree = {
            input(vec![
                n(Role::Button, "A", vec![]),
                n(Role::Button, "B", vec![]),
                n(Role::Button, "C", vec![]),
            ])
        };
        check(
            "budget_node_limit_accounted",
            transform,
            tree,
            TreeOpts {
                max_tree_nodes: 2,
                ..Default::default()
            },
        );
    }
    #[test]
    fn budget_focus_over_background() {
        let tree = {
            let mut a = n(Role::TextField, "Focus", vec![]);
            a.states |= States::FOCUSED;
            input(vec![n(Role::Button, "Background", vec![]), a])
        };
        check(
            "budget_focus_over_background",
            transform,
            tree,
            TreeOpts {
                max_tree_nodes: 2,
                ..Default::default()
            },
        );
    }
    #[test]
    fn budget_depth_limit_accounted() {
        let tree = {
            input(vec![n(
                Role::Group,
                "Level 2",
                vec![n(Role::Button, "Level 3", vec![])],
            )])
        };
        check(
            "budget_depth_limit_accounted",
            transform,
            tree,
            TreeOpts {
                max_tree_depth: 2,
                ..Default::default()
            },
        );
    }
}
