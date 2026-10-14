//! Flatten raw trees so native depth cannot overflow JSON's recursion limit.
use super::wire::Element;
use gcu_core::{
    driver::{DriverError, Result},
    model::*,
};
use serde::{Deserialize, Serialize};

/// Flat native tree, preserving native completeness independently of nodes.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tree {
    nodes: Vec<Node>,
    incomplete: Option<Incomplete>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    parent: Option<usize>,
    element: Element,
    fingerprint: u64,
    role: Role,
    native_role: String,
    subrole: Option<String>,
    name: Option<String>,
    value: Option<NodeValue>,
    description: Option<String>,
    placeholder: Option<String>,
    help: Option<String>,
    identifier: Option<String>,
    states: States,
    actions: Vec<NodeAction>,
    frame: Option<Rect>,
    text_extras: Option<TextExtras>,
    url: Option<String>,
}
impl Tree {
    pub(crate) fn encode(
        raw: RawTree,
        mut bind: impl FnMut(&NativeHandle) -> Result<Element>,
    ) -> Result<Self> {
        fn add(
            n: RawNode,
            parent: Option<usize>,
            output: &mut Vec<Node>,
            bind: &mut impl FnMut(&NativeHandle) -> Result<Element>,
        ) -> Result<()> {
            let index = output.len();
            let element = bind(&n.handle)?;
            output.push(Node {
                parent,
                element,
                fingerprint: n.fingerprint.0,
                role: n.role,
                native_role: n.native_role.to_string(),
                subrole: n.subrole.map(|s| s.to_string()),
                name: n.name,
                value: n.value,
                description: n.description,
                placeholder: n.placeholder,
                help: n.help,
                identifier: n.identifier,
                states: n.states,
                actions: n.actions.into_vec(),
                frame: n.frame,
                text_extras: n.text_extras,
                url: n.url,
            });
            for child in n.children {
                add(child, Some(index), output, bind)?;
            }
            Ok(())
        }
        let mut nodes = Vec::new();
        if let Some(root) = raw.root {
            add(root, None, &mut nodes, &mut bind)?;
        }
        Ok(Self {
            nodes,
            incomplete: raw.incomplete,
        })
    }
    pub(crate) fn valid(&self, epoch: u64, window: &super::wire::Window) -> bool {
        if self.nodes.len() > crate::uia::tree::NODE_CAP {
            return false;
        }
        let mut depths = Vec::new();
        for (i, n) in self.nodes.iter().enumerate() {
            let depth = match n.parent {
                None if i == 0 => 1,
                Some(p) if p < i => depths[p] + 1,
                _ => return false,
            };
            if depth > crate::uia::tree::DEPTH_CAP
                || n.element.epoch != epoch
                || &n.element.window != window
                || n.element.runtime_id.is_empty()
                || n.element.runtime_id.len() > 256
            {
                return false;
            }
            depths.push(depth);
        }
        true
    }
    pub(crate) fn decode(self) -> Result<RawTree> {
        if self.nodes.len() > crate::uia::tree::NODE_CAP {
            return Err(DriverError::InvalidRequest);
        }
        let mut nodes = Vec::new();
        let mut parents = Vec::new();
        let mut depths = Vec::new();
        for (i, n) in self.nodes.into_iter().enumerate() {
            let depth = match n.parent {
                None if i == 0 => 1,
                Some(p) if p < i => depths[p] + 1,
                _ => return Err(DriverError::InvalidRequest),
            };
            if depth > crate::uia::tree::DEPTH_CAP {
                return Err(DriverError::InvalidRequest);
            }
            depths.push(depth);
            parents.push(n.parent);
            nodes.push(Some(RawNode {
                handle: NativeHandle::new(n.element),
                fingerprint: Fingerprint(n.fingerprint),
                children: Vec::new(),
                role: n.role,
                native_role: n.native_role.into(),
                subrole: n.subrole.map(Into::into),
                name: n.name,
                value: n.value,
                description: n.description,
                placeholder: n.placeholder,
                help: n.help,
                identifier: n.identifier,
                states: n.states,
                actions: n.actions.into(),
                frame: n.frame,
                text_extras: n.text_extras,
                url: n.url,
            }));
        }
        // Reverse attachment preserves sibling order after reversing each child vector.
        for i in (1..nodes.len()).rev() {
            let child = nodes[i].take().ok_or(DriverError::InvalidRequest)?;
            let parent = parents[i].ok_or(DriverError::InvalidRequest)?;
            nodes[parent]
                .as_mut()
                .ok_or(DriverError::InvalidRequest)?
                .children
                .push(child);
        }
        let mut root = nodes.first_mut().and_then(Option::take);
        fn order(n: &mut RawNode) {
            n.children.reverse();
            for c in &mut n.children {
                order(c);
            }
        }
        if let Some(n) = &mut root {
            order(n);
        }
        Ok(RawTree {
            root,
            incomplete: self.incomplete,
        })
    }
}
