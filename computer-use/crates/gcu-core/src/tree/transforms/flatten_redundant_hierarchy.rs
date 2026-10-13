use crate::model::*;
use crate::tree::*;

fn wrapper(node: &ProcessedNode) -> bool {
    matches!(node.node.role, Role::Group | Role::Unknown)
        && !node.protected()
        && !node.interactive()
        && node.display_text().is_none()
        && node.node.value.is_none()
        && node.node.description.is_none()
        && node.node.placeholder.is_none()
        && node.node.help.is_none()
        && node.node.url.is_none()
        && node.omitted_nodes == 0
}
/// Splice anonymous Electron/WebView wrappers, preserving named/semantic containers.
pub fn transform(mut tree: ProcessedTree, _: &TreeOpts) -> ProcessedTree {
    post_order(&mut tree, &|node| {
        node.children = std::mem::take(&mut node.children)
            .into_iter()
            .flat_map(|child| {
                if wrapper(&child) {
                    child.children
                } else {
                    vec![child]
                }
            })
            .collect();
    });
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_support::*;
    #[test]
    fn flatten_redundant_hierarchy_wrapper_chain_flattened() {
        let tree = {
            input(vec![n(
                Role::Unknown,
                "",
                vec![n(
                    Role::Group,
                    "",
                    vec![n(Role::Button, "A", vec![]), n(Role::Button, "B", vec![])],
                )],
            )])
        };
        check(
            "flatten_redundant_hierarchy_wrapper_chain_flattened",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn flatten_redundant_hierarchy_semantic_container_retained() {
        let tree = {
            input(vec![n(
                Role::WebArea,
                "",
                vec![n(Role::Button, "A", vec![])],
            )])
        };
        check(
            "flatten_redundant_hierarchy_semantic_container_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn flatten_redundant_hierarchy_terminal_wrapper_retained() {
        let tree = {
            let mut a = n(
                Role::Unknown,
                "",
                vec![n(Role::StaticText, "Output", vec![])],
            );
            a.native_role = "terminal".into();
            input(vec![a])
        };
        check(
            "flatten_redundant_hierarchy_terminal_wrapper_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
}
