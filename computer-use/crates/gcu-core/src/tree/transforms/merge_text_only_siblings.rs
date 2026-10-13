use crate::model::*;
use crate::tree::*;

/// Merge adjacent pure static text; interactive/protected/identified nodes are barriers.
pub fn transform(mut tree: ProcessedTree, _: &TreeOpts) -> ProcessedTree {
    post_order(&mut tree, &|node| {
        let mut children: Vec<ProcessedNode> = Vec::new();
        for child in std::mem::take(&mut node.children) {
            if let Some(previous) = children.last_mut().filter(|p| {
                p.plain_text() && child.plain_text() && p.display_text() != child.display_text()
            }) {
                let text = [previous.display_text(), child.display_text()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                previous.node.name = Some(text);
                previous.node.value = None;
                previous.node.frame = union(previous.node.frame, child.node.frame);
            } else {
                children.push(child);
            }
        }
        node.children = children;
    });
    tree
}
fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(Rect {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            w: (a.x + a.w).max(b.x + b.w) - a.x.min(b.x),
            h: (a.y + a.h).max(b.y + b.h) - a.y.min(b.y),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_support::*;
    #[test]
    fn merge_text_only_siblings_adjacent_text_merged() {
        let tree = {
            input(vec![
                n(Role::StaticText, "Hello", vec![]),
                n(Role::StaticText, "world", vec![]),
            ])
        };
        check(
            "merge_text_only_siblings_adjacent_text_merged",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn merge_text_only_siblings_button_barrier_retained() {
        let tree = {
            input(vec![
                n(Role::StaticText, "A", vec![]),
                n(Role::Button, "B", vec![]),
                n(Role::StaticText, "C", vec![]),
            ])
        };
        check(
            "merge_text_only_siblings_button_barrier_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn merge_text_only_siblings_focused_text_retained() {
        let tree = {
            let mut a = n(Role::StaticText, "Focus", vec![]);
            a.states |= States::FOCUSED;
            input(vec![a, n(Role::StaticText, "Other", vec![])])
        };
        check(
            "merge_text_only_siblings_focused_text_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
}
