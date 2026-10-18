//! Map a shared-tab snapshot onto one accessibility web area.
//!
//! Frames in the snapshot are tab-viewport physical pixels. Published frames are
//! window physical pixels: web-area origin plus the viewport frame. A document
//! or web-area wrapper is not nested under the accessibility web area.

use super::{wire_id, PendingDom};
use crate::fingerprint::{refresh_tree, FRAME_QUANTUM_PX};
use crate::model::{
    Fingerprint, NativeHandle, NodeAction, NodeValue, RawNode, RawTree, Rect, Role, States,
};
use serde::Deserialize;
use smallvec::SmallVec;
use smol_str::SmolStr;
use std::collections::HashSet;

pub(crate) struct Parsed {
    pub snapshot_id: String,
    pub handoff: bool,
    pub width: f64,
    pub height: f64,
    root: DomNode,
    incomplete: Option<crate::model::Incomplete>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    kind: String,
    snapshot: Snap,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snap {
    id: String,
    root: DomNode,
    #[serde(rename = "frameSpace")]
    frame_space: String,
    geometry: Geometry,
    incomplete: Option<crate::model::Incomplete>,
    handoff: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Geometry {
    width: f64,
    height: f64,
    scale: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DomNode {
    id: String,
    role: Role,
    native_role: String,
    name: Option<String>,
    value: Option<NodeValue>,
    states: Vec<String>,
    frame: Option<Rect>,
    children: Vec<DomNode>,
}

pub(crate) struct Collected {
    pub nodes: Vec<(u64, String)>,
    pub ok: bool,
}

/// Accept snapshot JSON, or `{kind:snapshot, snapshot}`. Anything else, including
/// a cookie or storage field, is rejected. Pure memory work; no wait or cancellation.
pub(crate) fn parse(value: &serde_json::Value) -> Option<Parsed> {
    let snap = if value.get("kind").is_some() {
        let envelope: Envelope = serde_json::from_value(value.clone()).ok()?;
        if envelope.kind != "snapshot" {
            return None;
        }
        envelope.snapshot
    } else {
        serde_json::from_value(value.clone()).ok()?
    };
    if snap.frame_space != "tab_viewport_physical"
        || !wire_id(&snap.id)
        || !snap.geometry.scale.is_finite()
        || snap.geometry.scale <= 0.0
        || !snap.geometry.width.is_finite()
        || !snap.geometry.height.is_finite()
        || snap.geometry.width < 0.0
        || snap.geometry.height < 0.0
    {
        return None;
    }
    let mut seen = HashSet::new();
    let mut count = 0usize;
    if !valid_node(&snap.root, 1, &mut count, &mut seen) {
        return None;
    }
    Some(Parsed {
        snapshot_id: snap.id,
        handoff: snap.handoff,
        width: snap.geometry.width,
        height: snap.geometry.height,
        root: snap.root,
        incomplete: snap.incomplete,
    })
}

pub(crate) fn locate(root: Option<&RawNode>, fingerprint: Fingerprint) -> Option<&RawNode> {
    fn walk(node: &RawNode, fingerprint: Fingerprint) -> Option<&RawNode> {
        if node.role == Role::WebArea && node.fingerprint == fingerprint {
            return Some(node);
        }
        node.children
            .iter()
            .find_map(|child| walk(child, fingerprint))
    }
    root.and_then(|root| walk(root, fingerprint))
}

pub(crate) fn accepted_origin(node: Option<&RawNode>, expected: Rect) -> Option<Rect> {
    let live = node?.frame?;
    let close = |left: f64, right: f64| (left - right).abs() <= FRAME_QUANTUM_PX;
    if [live.x, live.y, live.w, live.h]
        .iter()
        .all(|value| value.is_finite())
        && close(live.x, expected.x)
        && close(live.y, expected.y)
        && close(live.w, expected.w)
        && close(live.h, expected.h)
        && live.w > 0.0
        && live.h > 0.0
    {
        Some(live)
    } else {
        None
    }
}

pub(crate) fn size_matches(width: f64, height: f64, origin: Rect) -> bool {
    (width - origin.w).abs() <= FRAME_QUANTUM_PX && (height - origin.h).abs() <= FRAME_QUANTUM_PX
}

/// Splice DOM children under the matching web area and recompute fingerprints.
/// The input tree is not modified. None leaves the caller on the original tree.
pub(crate) fn splice(
    raw: &RawTree,
    fingerprint: Fingerprint,
    origin: Rect,
    parsed: &Parsed,
) -> Option<RawTree> {
    let mut tree = raw.clone();
    let root = tree.root.as_mut()?;
    let web = find_mut(root, fingerprint)?;
    let mut children = Vec::new();
    for node in published(&parsed.root) {
        children.push(convert(node, origin, parsed.width, parsed.height, 1)?);
    }
    web.children = children;
    if tree.incomplete.is_none() {
        tree.incomplete = parsed.incomplete;
    }
    if let Some(root) = tree.root.as_mut() {
        refresh_tree(root, Fingerprint::default(), 0);
    }
    Some(tree)
}

pub(crate) fn collect(root: &RawNode) -> Collected {
    let mut ax = HashSet::new();
    let mut nodes = Vec::new();
    let mut seen = HashSet::new();
    let mut ok = true;
    walk_collect(root, &mut ax, &mut nodes, &mut seen, &mut ok);
    if nodes
        .iter()
        .any(|(fingerprint, _)| ax.contains(fingerprint))
    {
        ok = false;
    }
    Collected { nodes, ok }
}

pub(crate) fn install_handle(
    root: &mut RawNode,
    fingerprint: u64,
    node_id: &str,
    handle: NativeHandle,
) -> bool {
    if let Some(marker) = root.handle.downcast_ref::<PendingDom>() {
        if marker.node_id == node_id && root.fingerprint.0 == fingerprint {
            root.handle = handle;
            return true;
        }
    }
    for child in &mut root.children {
        if install_handle(child, fingerprint, node_id, handle.clone()) {
            return true;
        }
    }
    false
}

fn walk_collect(
    node: &RawNode,
    ax: &mut HashSet<u64>,
    nodes: &mut Vec<(u64, String)>,
    seen: &mut HashSet<u64>,
    ok: &mut bool,
) {
    if let Some(marker) = node.handle.downcast_ref::<PendingDom>() {
        if !seen.insert(node.fingerprint.0) {
            *ok = false;
        }
        nodes.push((node.fingerprint.0, marker.node_id.clone()));
    } else {
        ax.insert(node.fingerprint.0);
    }
    for child in &node.children {
        walk_collect(child, ax, nodes, seen, ok);
    }
}

fn find_mut(node: &mut RawNode, fingerprint: Fingerprint) -> Option<&mut RawNode> {
    if node.role == Role::WebArea && node.fingerprint == fingerprint {
        return Some(node);
    }
    for child in &mut node.children {
        if let Some(found) = find_mut(child, fingerprint) {
            return Some(found);
        }
    }
    None
}

fn published(root: &DomNode) -> Vec<&DomNode> {
    let mut layer = vec![root];
    for _ in 0..2 {
        if layer.len() == 1 && matches!(layer[0].role, Role::Document | Role::WebArea) {
            layer = layer[0].children.iter().collect();
        }
    }
    layer
}

fn valid_node(node: &DomNode, depth: usize, count: &mut usize, seen: &mut HashSet<String>) -> bool {
    *count += 1;
    if depth > 64 || *count > 5000 || !seen.insert(node.id.clone()) || !node_shape(node) {
        return false;
    }
    node.children
        .iter()
        .all(|child| valid_node(child, depth + 1, count, seen))
}

fn node_shape(node: &DomNode) -> bool {
    if !wire_id(&node.id)
        || node.native_role.chars().count() > 128
        || node.native_role.contains('\0')
        || node
            .name
            .as_deref()
            .is_some_and(|name| name.chars().count() > 500 || name.contains('\0'))
        || !value_ok(&node.value)
    {
        return false;
    }
    node.states.iter().all(|state| parse_state(state).is_some())
        && node.frame.is_none_or(|frame| {
            [frame.x, frame.y, frame.w, frame.h]
                .iter()
                .all(|value| value.is_finite())
                && frame.w >= 0.0
                && frame.h >= 0.0
        })
}

fn value_ok(value: &Option<NodeValue>) -> bool {
    match value {
        Some(NodeValue::Text(text)) => !text.contains('\0') && text.chars().count() <= 8192,
        Some(NodeValue::Number(number)) => number.is_finite(),
        Some(NodeValue::Range { value, min, max }) => {
            [value, min, max].iter().all(|number| number.is_finite())
        }
        Some(NodeValue::Bool(_)) | None => true,
    }
}

fn convert(node: &DomNode, origin: Rect, width: f64, height: f64, depth: usize) -> Option<RawNode> {
    if depth > 64 {
        return None;
    }
    let mut states = States::empty();
    for name in &node.states {
        states |= parse_state(name)?;
    }
    let password = states.contains(States::PASSWORD)
        || node
            .name
            .as_deref()
            .is_some_and(|name| name.to_ascii_lowercase().contains("password"))
        || node.native_role.to_ascii_lowercase().contains("password");
    if password {
        states |= States::PASSWORD;
    }
    let frame = match node.frame {
        Some(frame) if inside(frame, width, height) => Some(Rect {
            x: origin.x + frame.x,
            y: origin.y + frame.y,
            w: frame.w,
            h: frame.h,
        }),
        Some(_) => return None,
        None => None,
    };
    let mut children = Vec::new();
    for child in &node.children {
        children.push(convert(child, origin, width, height, depth + 1)?);
    }
    Some(RawNode {
        role: node.role,
        native_role: SmolStr::new(&node.native_role),
        subrole: None,
        name: node.name.clone(),
        value: if password {
            Some(NodeValue::Text("•••".into()))
        } else {
            node.value.clone()
        },
        description: None,
        placeholder: None,
        help: None,
        identifier: None,
        states,
        actions: actions_for(node.role, states),
        frame,
        text_extras: None,
        url: None,
        children,
        handle: NativeHandle::new(PendingDom {
            node_id: node.id.clone(),
        }),
        fingerprint: Fingerprint::default(),
    })
}

fn inside(frame: Rect, width: f64, height: f64) -> bool {
    frame.x >= -FRAME_QUANTUM_PX
        && frame.y >= -FRAME_QUANTUM_PX
        && frame.x + frame.w <= width + FRAME_QUANTUM_PX
        && frame.y + frame.h <= height + FRAME_QUANTUM_PX
}

fn actions_for(role: Role, states: States) -> SmallVec<[NodeAction; 4]> {
    let mut actions = SmallVec::new();
    if !states.contains(States::ENABLED) {
        return actions;
    }
    match role {
        Role::Button | Role::Link | Role::MenuItem | Role::Tab => actions.push(NodeAction::Press),
        Role::CheckBox | Role::Radio | Role::Switch => {
            actions.push(NodeAction::Press);
            actions.push(NodeAction::Toggle);
        }
        _ => {}
    }
    actions
}

fn parse_state(name: &str) -> Option<States> {
    Some(match name {
        "enabled" => States::ENABLED,
        "focused" => States::FOCUSED,
        "focusable" => States::FOCUSABLE,
        "selected" => States::SELECTED,
        "selectable" => States::SELECTABLE,
        "expanded" => States::EXPANDED,
        "expandable" => States::EXPANDABLE,
        "checked" => States::CHECKED,
        "mixed" => States::MIXED,
        "editable" => States::EDITABLE,
        "settable" => States::SETTABLE,
        "password" => States::PASSWORD,
        "offscreen" => States::OFFSCREEN,
        "modal" => States::MODAL,
        "busy" => States::BUSY,
        "required" => States::REQUIRED,
        _ => return None,
    })
}
