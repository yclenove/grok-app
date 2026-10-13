//! P0-06 tree behavior and regression coverage.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;
use gcu_core::{driver::TreeOpts, model::*, tree::Pipeline};
use support::*;

#[test]
fn tree_password_removed_by_budget_retains_sensitive_region() {
    let mut password = node(Role::TextField, "Password", vec![]);
    password.states |= States::PASSWORD;
    password.value = Some(NodeValue::Text("never-export-this".into()));
    password.frame = Some(Rect {
        x: 10.,
        y: 20.,
        w: 100.,
        h: 30.,
    });
    let tree = Pipeline::default().run(
        raw(vec![password]),
        &TreeOpts {
            max_tree_nodes: 1,
            ..Default::default()
        },
    );
    assert_eq!(tree.element_index.len(), 1);
    assert_eq!(tree.omitted_nodes, 1);
    assert_eq!(tree.sensitive_regions.len(), 1);
    assert_eq!(tree.sensitive_regions[0].frame.unwrap().y, 20.);
}

#[test]
fn tree_large_list_modal_retains_popup_and_focused_ancestor_chain() {
    let rows = (0..2000)
        .map(|i| node(Role::Row, &format!("Row {i}"), vec![]))
        .collect();
    let mut focus = node(Role::TextField, "Focus", vec![]);
    focus.states |= States::FOCUSED | States::SETTABLE;
    let mut modal = node(
        Role::Dialog,
        "Confirm",
        vec![node(Role::Group, "Focus ancestor", vec![focus])],
    );
    modal.states |= States::MODAL;
    let tree = Pipeline::default().run(
        raw(vec![node(Role::List, "Rows", rows), modal]),
        &TreeOpts::default(),
    );
    assert!(tree.element_index.len() <= 1200);
    assert_eq!(tree.popups.len(), 1);
    assert!(tree.focused.is_some());
    assert!(tree
        .nodes()
        .iter()
        .any(|(_, n)| n.node.name.as_deref() == Some("Focus ancestor")));
    assert_eq!(tree.omitted_nodes, 2000);
}

#[test]
fn tree_identical_inputs_have_identical_indices_and_handles() {
    let input = raw(vec![
        node(Role::Button, "One", vec![]),
        node(Role::Button, "Two", vec![]),
    ]);
    let first = Pipeline::default().run(input.clone(), &TreeOpts::default());
    let second = Pipeline::default().run(input, &TreeOpts::default());
    assert_eq!(first.root.as_ref().unwrap().element_index, 0);
    for (index, entry) in &first.element_index {
        assert_eq!(entry.fingerprint, second.element_index[index].fingerprint);
        assert!(entry.handle.ptr_eq(&second.element_index[index].handle));
    }
}

#[test]
fn tree_defaults_and_depth_budget_preserve_exact_omission_count() {
    assert_eq!(TreeOpts::default().max_tree_nodes, 1200);
    assert_eq!(TreeOpts::default().max_tree_depth, 64);
    let mut child = node(Role::Button, "Leaf", vec![]);
    for i in 0..70 {
        child = node(Role::Group, &format!("Ancestor {i}"), vec![child]);
    }
    let tree = Pipeline::default().run(raw(vec![child]), &TreeOpts::default());
    assert_eq!(tree.nodes().len(), 64);
    assert_eq!(tree.nodes().last().unwrap().0, 63);
    assert_eq!(tree.omitted_nodes, 8);
}

#[test]
fn tree_breadth_first_same_priority_retains_shallow_siblings() {
    let tree = Pipeline::default().run(
        raw(vec![
            node(
                Role::Group,
                "First",
                vec![node(Role::StaticText, "Deep", vec![])],
            ),
            node(Role::Group, "Second", vec![]),
        ]),
        &TreeOpts {
            max_tree_nodes: 3,
            ..Default::default()
        },
    );
    let names: Vec<_> = tree
        .nodes()
        .iter()
        .filter_map(|(_, n)| n.node.name.as_deref())
        .collect();
    assert_eq!(names, ["Fixture", "First", "Second"]);
    assert_eq!(tree.omitted_nodes, 1);
}

#[test]
fn tree_modal_and_titlebar_take_budget_before_background() {
    let mut title = node(Role::Group, "Title", vec![]);
    title.native_role = "UIA_TitleBarControlTypeId".into();
    let mut alert = node(
        Role::Unknown,
        "Alert",
        vec![node(Role::Button, "OK", vec![])],
    );
    alert.native_role = "AXAlert".into();
    alert.states |= States::MODAL;
    let tree = Pipeline::default().run(
        raw(vec![node(Role::Button, "Background", vec![]), title, alert]),
        &TreeOpts {
            max_tree_nodes: 4,
            ..Default::default()
        },
    );
    assert_eq!(tree.popups.len(), 1);
    let names: Vec<_> = tree
        .nodes()
        .iter()
        .filter_map(|(_, n)| n.node.name.as_deref())
        .collect();
    assert_eq!(names, ["Fixture", "Title", "Alert", "OK"]);
}

#[test]
fn tree_zero_budget_preserves_all_sensitive_metadata() {
    let mut password = node(Role::TextField, "Password", vec![]);
    password.states |= States::PASSWORD;
    let tree = Pipeline::default().run(
        raw(vec![password]),
        &TreeOpts {
            max_tree_nodes: 0,
            ..Default::default()
        },
    );
    assert!(tree.root.is_none());
    assert_eq!(tree.omitted_nodes, 2);
    assert_eq!(tree.sensitive_regions.len(), 1);
    assert!(!tree.sensitive_regions_reliable);
}

#[test]
fn tree_native_label_relations_resolve_before_structural_folding() {
    use gcu_core::tree::TitleAssociation;
    let input = raw(vec![
        node(Role::StaticText, "Email", vec![]),
        node(Role::TextField, "", vec![]),
    ]);
    let children = &input.root.as_ref().unwrap().children;
    let pipeline = Pipeline::with_title_elements(vec![TitleAssociation {
        control: children[1].fingerprint,
        label: children[0].fingerprint,
    }]);
    let tree = pipeline.run(input, &TreeOpts::default());
    assert!(tree
        .nodes()
        .iter()
        .any(|(_, n)| n.node.role == Role::TextField && n.node.name.as_deref() == Some("Email")));
}

#[test]
fn tree_web_link_folding_preserves_action_handle_and_native_role() {
    let mut link = node(Role::Link, "", vec![node(Role::StaticText, "Docs", vec![])]);
    link.url = Some("https://example.test/a(b)".into());
    link.actions.push(NodeAction::Press);
    let handle = link.handle.clone();
    let tree = Pipeline::default().run(
        raw(vec![node(Role::WebArea, "Page", vec![link])]),
        &TreeOpts::default(),
    );
    let (_, link) = tree
        .nodes()
        .into_iter()
        .find(|(_, n)| n.node.role == Role::Link)
        .unwrap();
    assert_eq!(
        link.node.name.as_deref(),
        Some("[Docs](https://example.test/a%28b%29)")
    );
    assert!(link.children.is_empty());
    assert!(link.node.handle.ptr_eq(&handle));
    assert_eq!(link.node.actions.as_slice(), [NodeAction::Press]);
}

#[test]
fn tree_repetitive_text_is_compacted_after_sibling_merging() {
    let tree = Pipeline::default().run(
        raw(vec![node(Role::StaticText, "Loading", vec![]); 5]),
        &TreeOpts::default(),
    );
    assert_eq!(tree.nodes().len(), 2);
    assert_eq!(tree.omitted_nodes, 4);
    assert_eq!(
        tree.root.unwrap().children[0].node.name.as_deref(),
        Some("Loading … 4 more")
    );
}
