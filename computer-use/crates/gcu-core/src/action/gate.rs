//! Fail-closed checks against the exact delivered snapshot and native handle.
//! All functions here are bounded pure memory work; native reads are in the router.
use super::{lock, observe::Fresh, ToolCall};
use crate::{
    model::{Geometry, RawNode, WindowPoint},
    redact::PixelHash,
    session::Session,
    snapshot::{SnapshotId, WindowKey},
    tree::IndexedElement,
};
use gcu_protocol::{tools::BatchStep, ErrorCode};

pub(crate) struct Gate {
    pub snapshot_id: Option<SnapshotId>,
    pub element: Option<IndexedElement>,
    pub points: Vec<WindowPoint>,
    pub hashes: Vec<PixelHash>,
    coordinate_elements: Vec<Option<IndexedElement>>,
    pub geometry: Option<Geometry>,
    pub screenshot_id: String,
}
/// Convert the model's screenshot pixels into physical window pixels. Rejects
/// non-finite/negative coordinates and points outside the delivered image.
/// Synchronous; no IO, timeout, cancellation, or native blocking.
pub fn screenshot_point(
    x: f64,
    y: f64,
    width: u32,
    height: u32,
    scale: f64,
) -> Result<WindowPoint, ErrorCode> {
    if ![x, y, scale].iter().all(|v| v.is_finite())
        || scale <= 0.
        || x < 0.
        || y < 0.
        || x >= f64::from(width)
        || y >= f64::from(height)
    {
        return Err(ErrorCode::OutOfBounds);
    }
    Ok(WindowPoint {
        x: x * scale,
        y: y * scale,
    })
}
pub(crate) fn prepare(
    session: &Session,
    call: &ToolCall,
    key: &WindowKey,
) -> Result<Gate, ErrorCode> {
    call.action.validate().map_err(|_| {
        if matches!(call.action, BatchStep::PressKey(_)) {
            ErrorCode::InvalidKey
        } else {
            ErrorCode::CapabilityUnavailable
        }
    })?;
    if matches!(call.action, BatchStep::WaitFor(_) | BatchStep::Expect(_)) {
        return Err(ErrorCode::CapabilityUnavailable);
    }
    let resources = lock(&session.actions);
    let store = &resources.snapshots;
    let latest = store.latest(key);
    let index = element_index(&call.action);
    let element = index
        .map(|i| {
            let latest = latest.ok_or(ErrorCode::StaleElement)?;
            store
                .resolve(key, latest.id(), i)
                .cloned()
                .map_err(|_| ErrorCode::StaleElement)
        })
        .transpose()?;
    let mut gate = Gate {
        snapshot_id: latest.map(|s| s.id()),
        element,
        points: Vec::new(),
        hashes: Vec::new(),
        coordinate_elements: Vec::new(),
        geometry: None,
        screenshot_id: String::new(),
    };
    let (points, requested) = coordinates(&call.action);
    if !points.is_empty() {
        let shot = store
            .latest_with_screenshot(key)
            .ok_or(ErrorCode::StaleScreenshot)?;
        gate.screenshot_id = format!("i_{}", shot.id().0);
        if requested.is_some_and(|id| id != gate.screenshot_id) {
            return Err(ErrorCode::StaleScreenshot);
        }
        let image = shot.screenshot().ok_or(ErrorCode::StaleScreenshot)?;
        let (width, height) = shot.delivery_size();
        let scale = f64::from(image.width()) / f64::from(width);
        gate.geometry = Some(shot.geometry());
        for (x, y) in points {
            let at = screenshot_point(x, y, width, height, scale)?;
            gate.hashes
                .push(image.target_hash(at).map_err(|_| ErrorCode::OutOfBounds)?);
            let hit = shot
                .index()
                .elements()
                .values()
                .filter(|e| {
                    e.frame.is_some_and(|f| {
                        at.x >= f.x && at.y >= f.y && at.x < f.x + f.w && at.y < f.y + f.h
                    })
                })
                .max_by_key(|e| {
                    let mut depth = 0;
                    let mut parent = e.parent;
                    while let Some(i) = parent {
                        depth += 1;
                        parent = shot.index().get(i).and_then(|e| e.parent);
                    }
                    depth
                })
                .cloned();
            gate.coordinate_elements.push(hit);
            gate.points.push(at);
        }
    }
    Ok(gate)
}
pub(crate) fn check(gate: &Gate, fresh: &Fresh) -> Result<(), ErrorCode> {
    if gate
        .geometry
        .is_some_and(|geometry| geometry != fresh.geometry())
    {
        return Err(ErrorCode::StaleGeometry);
    }
    if let Some(element) = &gate.element {
        let live = fresh
            .raw
            .root
            .as_ref()
            .and_then(|root| by_handle(root, element))
            .ok_or(ErrorCode::StaleElement)?;
        // Fingerprints include name + native identifier + ancestor path. Compare
        // exact role/frame too, since fingerprints quantize physical frames.
        if live.fingerprint != element.fingerprint
            || live.role != element.role
            || live.frame != element.frame
        {
            return Err(ErrorCode::StaleElement);
        }
    }
    for element in gate.coordinate_elements.iter().flatten() {
        let live = fresh
            .raw
            .root
            .as_ref()
            .and_then(|root| by_handle(root, element))
            .ok_or(ErrorCode::StaleScreenshot)?;
        if live.fingerprint != element.fingerprint
            || live.role != element.role
            || live.frame != element.frame
        {
            return Err(ErrorCode::StaleScreenshot);
        }
    }
    if !gate.points.is_empty() {
        let image = fresh.image.as_ref().ok_or(ErrorCode::StaleScreenshot)?;
        for (at, expected) in gate.points.iter().zip(&gate.hashes) {
            if image.target_hash(*at).map_err(|_| ErrorCode::OutOfBounds)? != *expected {
                return Err(ErrorCode::StaleScreenshot);
            }
        }
    }
    Ok(())
}
fn by_handle<'a>(root: &'a RawNode, element: &IndexedElement) -> Option<&'a RawNode> {
    let mut nodes = Vec::new();
    collect(root, &mut nodes);
    live_match(nodes, |node: &&'a RawNode| *node, element)
}

fn collect<'a>(node: &'a RawNode, out: &mut Vec<&'a RawNode>) {
    out.push(node);
    for child in &node.children {
        collect(child, out);
    }
}

/// The node in a fresh read that is the snapshot `element`. Drivers that reuse
/// handles match by handle. Real drivers mint new handles on every read, so
/// otherwise take the one node with the same identity: fingerprint (name,
/// identifier, ancestor path, quantized frame) plus exact role and frame.
pub(crate) fn live_match<'a, T>(
    nodes: Vec<T>,
    raw: impl Fn(&T) -> &'a RawNode,
    element: &IndexedElement,
) -> Option<T> {
    let mut same = Vec::new();
    for node in nodes {
        let live = raw(&node);
        if live.handle.ptr_eq(&element.handle) {
            return Some(node);
        }
        if live.fingerprint == element.fingerprint
            && live.role == element.role
            && live.frame == element.frame
        {
            same.push(node);
        }
    }
    if same.len() == 1 {
        same.pop()
    } else {
        None
    }
}
pub(crate) fn element_index(action: &BatchStep) -> Option<u32> {
    match action {
        BatchStep::Click(a) => a.element_index,
        BatchStep::PerformSecondaryAction(a) => Some(a.element_index),
        BatchStep::SetValue(a) => Some(a.element_index),
        BatchStep::SelectText(a) => Some(a.element_index),
        BatchStep::TypeText(a) => a.element_index,
        BatchStep::Scroll(a) => a.element_index,
        _ => None,
    }
}
fn coordinates(action: &BatchStep) -> (Vec<(f64, f64)>, Option<&str>) {
    match action {
        BatchStep::Click(a) => (
            a.x.zip(a.y).into_iter().collect(),
            a.screenshot_id.as_deref(),
        ),
        BatchStep::Scroll(a) => (a.x.zip(a.y).into_iter().collect(), None),
        BatchStep::Drag(a) => (
            vec![(a.from_x, a.from_y), (a.to_x, a.to_y)],
            a.screenshot_id.as_deref(),
        ),
        _ => (Vec::new(), None),
    }
}

#[cfg(test)]
mod live_match_tests {
    use super::{by_handle, IndexedElement};
    use crate::model::{Fingerprint, NativeHandle, NodeFlags, RawNode, Role, States};
    use gcu_protocol::Rect;

    fn node(name: &str, fingerprint: u64, children: Vec<RawNode>) -> RawNode {
        RawNode {
            role: Role::Button,
            native_role: "AXButton".into(),
            subrole: None,
            name: Some(name.into()),
            value: None,
            description: None,
            placeholder: None,
            help: None,
            identifier: None,
            states: States::default(),
            actions: Default::default(),
            frame: Some(Rect {
                x: 1.0,
                y: 2.0,
                w: 3.0,
                h: 4.0,
            }),
            text_extras: None,
            url: None,
            children,
            handle: NativeHandle::new(name.to_string()),
            fingerprint: Fingerprint(fingerprint),
        }
    }

    fn element(of: &RawNode) -> IndexedElement {
        IndexedElement {
            handle: of.handle.clone(),
            fingerprint: of.fingerprint,
            role: of.role,
            frame: of.frame,
            flags: NodeFlags::default(),
            parent: None,
        }
    }

    #[test]
    fn a_fresh_read_with_new_handles_still_finds_the_element() {
        let before = node("root", 1, vec![node("ok", 2, Vec::new())]);
        let wanted = element(&before.children[0]);
        // Same identity, newly minted handles, as a real driver returns.
        let fresh = node("root", 1, vec![node("ok", 2, Vec::new())]);
        let live = by_handle(&fresh, &wanted).unwrap();
        assert_eq!(live.name.as_deref(), Some("ok"));
        // A changed fingerprint or two identical candidates are stale.
        let moved = node("root", 1, vec![node("ok", 3, Vec::new())]);
        assert!(by_handle(&moved, &wanted).is_none());
        let twins = node(
            "root",
            1,
            vec![node("ok", 2, Vec::new()), node("ok", 2, Vec::new())],
        );
        assert!(by_handle(&twins, &wanted).is_none());
    }
}
