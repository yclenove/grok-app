use crate::model::*;
use crate::tree::*;

fn meaningful(action: &NodeAction) -> bool {
    match action {
        NodeAction::ScrollIntoView => false,
        NodeAction::Native(name) => ![
            "scrolltovisible",
            "scrollintoview",
            "showdefaultui",
            "axscrolltovisible",
        ]
        .contains(
            &name
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase()
                .as_str(),
        ),
        _ => true,
    }
}
/// Keep meaningful advertised actions, including Press for exact action routing.
pub fn transform(mut tree: ProcessedTree, _: &TreeOpts) -> ProcessedTree {
    post_order(&mut tree, &|node| {
        let mut seen = Vec::new();
        node.node.actions.retain(|action| {
            if meaningful(action) && !seen.contains(action) {
                seen.push(action.clone());
                true
            } else {
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
    fn filter_actions_noise_removed() {
        let tree = {
            let mut a = n(Role::Button, "Actions", vec![]);
            a.actions = vec![
                NodeAction::Press,
                NodeAction::ScrollIntoView,
                NodeAction::Native("ShowDefaultUI".into()),
                NodeAction::ShowMenu,
            ]
            .into();
            input(vec![a])
        };
        check(
            "filter_actions_noise_removed",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn filter_actions_meaningful_retained() {
        let tree = {
            let mut a = n(Role::Slider, "Volume", vec![]);
            a.actions = vec![
                NodeAction::Increment,
                NodeAction::Decrement,
                NodeAction::Native("Inspect".into()),
            ]
            .into();
            input(vec![a])
        };
        check(
            "filter_actions_meaningful_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn filter_actions_duplicates_removed() {
        let tree = {
            let mut a = n(Role::ScrollArea, "Scroll", vec![]);
            a.actions = vec![NodeAction::ScrollDown, NodeAction::ScrollDown].into();
            input(vec![a])
        };
        check(
            "filter_actions_duplicates_removed",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
}
