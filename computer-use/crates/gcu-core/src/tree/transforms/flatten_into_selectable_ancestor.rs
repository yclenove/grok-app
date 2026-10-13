use crate::model::*;
use crate::tree::*;

fn presentation(node: &ProcessedNode) -> bool {
    !node.protected()
        && !node.interactive()
        && node.omitted_nodes == 0
        && node.node.description.is_none()
        && node.node.help.is_none()
        && node.node.url.is_none()
        && node.node.placeholder.is_none()
        && !node
            .node
            .states
            .intersects(States::CHECKED | States::MIXED | States::BUSY | States::REQUIRED)
        && matches!(node.node.value, None | Some(NodeValue::Text(_)))
        && matches!(
            node.node.role,
            Role::Cell | Role::Group | Role::StaticText | Role::Image
        )
        && node.children.iter().all(presentation)
}
fn collect(node: &ProcessedNode, text: &mut Vec<String>) {
    if let Some(label) = node.display_text() {
        text.push(label.to_owned());
    }
    if let Some(NodeValue::Text(value)) = &node.node.value {
        if Some(value.as_str()) != node.node.name.as_deref() && !value.is_empty() {
            text.push(value.clone());
        }
    }
    for child in &node.children {
        collect(child, text);
    }
}
/// Fold pure presentation descendants into selectable row/list-item names.
pub fn transform(mut tree: ProcessedTree, _: &TreeOpts) -> ProcessedTree {
    post_order(&mut tree, &|node| {
        if !node
            .node
            .states
            .intersects(States::SELECTABLE | States::SELECTED)
        {
            return;
        }
        let mut text = Vec::new();
        if let Some(name) = node.display_text() {
            text.push(name.to_owned());
        }
        node.children.retain(|child| {
            if presentation(child) {
                collect(child, &mut text);
                false
            } else {
                true
            }
        });
        text.dedup();
        if !text.is_empty() {
            node.node.name = Some(text.join(" — "));
        }
    });
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_support::*;
    #[test]
    fn flatten_into_selectable_ancestor_row_cells_merged() {
        let tree = {
            let mut a = n(
                Role::Row,
                "",
                vec![n(Role::Cell, "A", vec![]), n(Role::Cell, "B", vec![])],
            );
            a.states |= States::SELECTABLE;
            input(vec![a])
        };
        check(
            "flatten_into_selectable_ancestor_row_cells_merged",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn flatten_into_selectable_ancestor_interactive_child_retained() {
        let tree = {
            let mut a = n(
                Role::Row,
                "Row",
                vec![
                    n(Role::Button, "Edit", vec![]),
                    n(Role::StaticText, "Name", vec![]),
                ],
            );
            a.states |= States::SELECTED;
            input(vec![a])
        };
        check(
            "flatten_into_selectable_ancestor_interactive_child_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn flatten_into_selectable_ancestor_focused_child_retained() {
        let tree = {
            let mut a = n(Role::StaticText, "Focus", vec![]);
            a.states |= States::FOCUSED;
            let mut row = n(Role::Row, "", vec![a]);
            row.states |= States::SELECTABLE;
            input(vec![row])
        };
        check(
            "flatten_into_selectable_ancestor_focused_child_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
}
