use super::*;
use crate::model::*;
use serde_json::{json, Value};

pub(crate) fn n(role: Role, name: &str, children: Vec<RawNode>) -> RawNode {
    RawNode {
        role,
        native_role: format!("{role:?}").into(),
        subrole: None,
        name: (!name.is_empty()).then(|| name.into()),
        value: None,
        description: None,
        placeholder: None,
        help: None,
        identifier: None,
        states: States::ENABLED,
        actions: Default::default(),
        frame: None,
        text_extras: None,
        url: None,
        children,
        handle: NativeHandle::new(()),
        fingerprint: Fingerprint::default(),
    }
}
pub(crate) fn input(children: Vec<RawNode>) -> ProcessedTree {
    let mut root = n(Role::Window, "Fixture", children);
    crate::fingerprint::refresh_tree(&mut root, Fingerprint::default(), 0);
    prepare::prepare(RawTree {
        root: Some(root),
        incomplete: None,
    })
}
fn shape(node: &ProcessedNode) -> Value {
    json!({"index": (node.element_index != UNASSIGNED_INDEX).then_some(node.element_index),
        "role": node.node.role, "name": node.node.name, "value": node.node.value,
        "states": node.node.states, "flags": format!("{:?}", node.flags),
        "actions": node.node.actions, "omitted": node.omitted_nodes,
        "children": node.children.iter().map(shape).collect::<Vec<_>>()})
}
pub(crate) fn check(
    name: &str,
    transform: Transform,
    tree: ProcessedTree,
    opts: TreeOpts,
) -> ProcessedTree {
    let before = tree.root.as_ref().map(shape);
    let after = transform(tree, &opts);
    let io = json!({"input": before, "output": after.root.as_ref().map(shape), "omitted": after.omitted_nodes});
    insta::with_settings!({prepend_module_to_snapshot => false}, {
        insta::assert_snapshot!(name, canonical_json(&io));
    });
    after
}

/// Pretty JSON with object keys sorted recursively, independent of whether any crate in the
/// workspace enables `serde_json/preserve_order` (feature unification would reorder keys).
fn canonical_json(value: &serde_json::Value) -> String {
    fn sort(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let sorted: std::collections::BTreeMap<_, _> =
                    map.iter().map(|(k, v)| (k.clone(), sort(v))).collect();
                serde_json::Value::Object(sorted.into_iter().collect())
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(sort).collect())
            }
            other => other.clone(),
        }
    }
    serde_json::to_string_pretty(&sort(value)).unwrap()
}
