use crate::model::*;
use crate::tree::*;

fn label(node: &ProcessedNode) -> Option<&str> {
    if node.plain_text() {
        node.display_text()
    } else if !node.protected()
        && !node.interactive()
        && node.omitted_nodes == 0
        && node.node.value.is_none()
        && node.node.description.is_none()
        && node.node.help.is_none()
        && node.node.placeholder.is_none()
        && node.node.url.is_none()
        && matches!(
            node.node.role,
            Role::Group | Role::Row | Role::ListItem | Role::Cell
        )
        && node.display_text().is_none()
        && node.children.len() == 1
    {
        label(&node.children[0])
    } else {
        None
    }
}
/// Compact runs of at least four identical presentation subtrees, with an explicit count.
/// Distinct list rows and any actionable/identified static text remain separate.
pub fn transform(mut tree: ProcessedTree, _: &TreeOpts) -> ProcessedTree {
    post_order(&mut tree, &|node| {
        let children = std::mem::take(&mut node.children);
        let mut iter = children.into_iter().peekable();
        while let Some(mut first) = iter.next() {
            let key = label(&first).map(str::to_owned);
            let mut rest = Vec::new();
            if let Some(key) = &key {
                while iter
                    .peek()
                    .is_some_and(|next| label(next) == Some(key.as_str()))
                {
                    if let Some(next) = iter.next() {
                        rest.push(next);
                    }
                }
            }
            if rest.len() >= 3 {
                first.omitted_nodes += rest
                    .iter()
                    .map(ProcessedNode::accounted_count)
                    .sum::<usize>();
                first.omitted_nodes += first
                    .children
                    .iter()
                    .map(ProcessedNode::accounted_count)
                    .sum::<usize>();
                first.node.name =
                    Some(format!("{} … {} more", key.unwrap_or_default(), rest.len()));
                first.children.clear();
                node.children.push(first);
            } else {
                node.children.push(first);
                node.children.extend(rest);
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
    fn flatten_repetitive_static_text_repeated_presentations_compacted() {
        let tree = {
            input(vec![
                n(
                    Role::Group,
                    "",
                    vec![n(Role::StaticText, "Loading", vec![])]
                );
                5
            ])
        };
        check(
            "flatten_repetitive_static_text_repeated_presentations_compacted",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn flatten_repetitive_static_text_distinct_rows_retained() {
        let tree = {
            input(
                (0..5)
                    .map(|i| n(Role::StaticText, &format!("Row {i}"), vec![]))
                    .collect(),
            )
        };
        check(
            "flatten_repetitive_static_text_distinct_rows_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn flatten_repetitive_static_text_short_run_retained() {
        let tree = { input(vec![n(Role::StaticText, "Same", vec![]); 3]) };
        check(
            "flatten_repetitive_static_text_short_run_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
}
