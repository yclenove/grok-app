use crate::tree::*;

/// Assign deterministic preorder indices and rebuild all policy-facing index metadata.
pub fn transform(tree: ProcessedTree, _: &TreeOpts) -> ProcessedTree {
    super::super::index::assign(tree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use crate::tree::test_support::*;
    #[test]
    fn assign_indices_preorder_and_focus() {
        let tree = {
            let mut a = n(Role::TextField, "Focus", vec![]);
            a.states |= States::FOCUSED;
            input(vec![
                n(Role::Group, "Group", vec![a]),
                n(Role::Button, "Next", vec![]),
            ])
        };
        check(
            "assign_indices_preorder_and_focus",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn assign_indices_empty_tree_index_empty() {
        let tree = { ProcessedTree::default() };
        check(
            "assign_indices_empty_tree_index_empty",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn assign_indices_modal_and_terminal_recorded() {
        let tree = {
            let mut a = n(
                Role::Dialog,
                "Modal",
                vec![n(Role::Unknown, "Console", vec![])],
            );
            a.states |= States::MODAL;
            input(vec![a])
        };
        check(
            "assign_indices_modal_and_terminal_recorded",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
}
