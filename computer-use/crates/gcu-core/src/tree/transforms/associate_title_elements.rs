use crate::model::*;
use crate::tree::*;

/// Merge explicitly associated native labels into unnamed controls. The label node
/// remains available by its own identity, especially if another control shares it.
pub fn transform(mut tree: ProcessedTree, _: &TreeOpts) -> ProcessedTree {
    let labels: Vec<_> = tree
        .title_elements
        .iter()
        .filter_map(|relation| {
            tree.nodes()
                .iter()
                .find(|(_, n)| n.node.fingerprint == relation.label)
                .filter(|(_, n)| !n.node.states.contains(States::PASSWORD))
                .and_then(|(_, n)| n.display_text())
                .map(|text| (relation.control, text.to_owned()))
        })
        .collect();
    post_order(&mut tree, &|node| {
        if node.node.name.as_deref().is_none_or(str::is_empty) {
            if let Some((_, label)) = labels.iter().find(|(id, _)| *id == node.node.fingerprint) {
                node.node.name = Some(label.clone());
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
    fn associate_title_elements_native_label_associated() {
        let tree = {
            let mut t = input(vec![
                n(Role::StaticText, "Email", vec![]),
                n(Role::TextField, "", vec![]),
            ]);
            let c = &t.root.as_ref().unwrap().children;
            t.title_elements = vec![TitleAssociation {
                control: c[1].node.fingerprint,
                label: c[0].node.fingerprint,
            }];
            t
        };
        check(
            "associate_title_elements_native_label_associated",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn associate_title_elements_existing_name_retained() {
        let tree = {
            let mut t = input(vec![
                n(Role::StaticText, "Label", vec![]),
                n(Role::TextField, "Existing", vec![]),
            ]);
            let c = &t.root.as_ref().unwrap().children;
            t.title_elements = vec![TitleAssociation {
                control: c[1].node.fingerprint,
                label: c[0].node.fingerprint,
            }];
            t
        };
        check(
            "associate_title_elements_existing_name_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn associate_title_elements_password_label_refused() {
        let tree = {
            let mut a = n(Role::TextField, "Protected", vec![]);
            a.states |= States::PASSWORD;
            let mut t = input(vec![a, n(Role::TextField, "", vec![])]);
            let c = &t.root.as_ref().unwrap().children;
            t.title_elements = vec![TitleAssociation {
                control: c[1].node.fingerprint,
                label: c[0].node.fingerprint,
            }];
            t
        };
        check(
            "associate_title_elements_password_label_refused",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
}
