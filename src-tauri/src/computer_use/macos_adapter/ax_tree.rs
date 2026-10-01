//! Selected-window AX snapshots and exact retained element references.
//! Preview refs never acquire authority. No text values/password fields are read.
use super::ax_api::{Budget, Element};
use super::*;
use grok_computer_use_core::execution::ActionCancellation;
use grok_computer_use_core::protocol::ObservationNode;
use std::collections::VecDeque;

#[path = "ax_tree_wait.rs"]
mod wait;
pub(super) use wait::wait;

const MAX_NODES: usize = 256;
const MAX_DEPTH: usize = 32;

struct Node {
    element: Element,
    parent: Option<usize>,
    reference: String,
    role: String,
    name: String,
    bounds: Option<WindowBounds>,
    press: bool,
    set_value: bool,
    type_text: bool,
    key: bool,
    wait: bool,
}
pub(super) struct Tree {
    nodes: Vec<Node>,
    pub(super) display: Vec<ObservationNode>,
    pub(super) truncated: bool,
}

fn protected(budget: &Budget<'_>, element: &Element, role: &str) -> Result<bool, String> {
    Ok(role == "AXSecureTextField"
        || budget.text(element.raw(), "AXSubrole")?.as_deref() == Some("AXSecureTextField"))
}
fn text_role(role: &str) -> bool {
    matches!(role, "AXTextField" | "AXTextArea" | "AXComboBox")
}
fn name(budget: &Budget<'_>, element: &Element) -> Result<String, String> {
    if let Some(title) = budget.text(element.raw(), "AXTitle")? {
        return Ok(title);
    }
    Ok(budget
        .text(element.raw(), "AXDescription")?
        .unwrap_or_default())
}
fn image_bounds(bounds: Option<WindowBounds>, frame: &CapturedFrame) -> Option<WindowBounds> {
    let b = bounds?;
    let w = frame.bounds;
    let x = b.x.max(w.x);
    let y = b.y.max(w.y);
    let right = (b.x + b.width).min(w.x + w.width);
    let bottom = (b.y + b.height).min(w.y + w.height);
    if right <= x || bottom <= y {
        return None;
    }
    let sx = f64::from(frame.width) / w.width;
    let sy = f64::from(frame.height) / w.height;
    WindowBounds::new(
        (x - w.x) * sx,
        (y - w.y) * sy,
        (right - x) * sx,
        (bottom - y) * sy,
    )
    .ok()
}

pub(super) fn collect(
    adapter: &MacosAdapter,
    target: &str,
    frame: CapturedFrame,
    cancel: &ActionCancellation,
) -> Result<Tree, String> {
    let instance = WindowInstance::parse(target)?;
    adapter
        .windows
        .with_window(instance, frame.bounds, cancel, |root, budget| {
            let mut pending = VecDeque::from([(root.retained()?, None, 0)]);
            let mut nodes: Vec<Node> = Vec::new();
            let mut display = Vec::new();
            let mut truncated = false;
            while let Some((element, parent, depth)) = pending.pop_front() {
                budget.check()?;
                if nodes.len() >= MAX_NODES {
                    truncated = true;
                    break;
                }
                if nodes.iter().any(|n| n.element.same(&element)) {
                    truncated = true;
                    continue;
                }
                if let Some(parent_index) = parent {
                    let ancestor: &Node = &nodes[parent_index];
                    if !budget
                        .related(&element, "AXParent", instance.window.pid)?
                        .same(&ancestor.element)
                        || !budget
                            .related(&element, "AXWindow", instance.window.pid)?
                            .same(root)
                    {
                        return Err("AX child escaped its observed window/parent".into());
                    }
                } else if !element.same(root) {
                    return Err("AX root changed".into());
                }
                let role = budget.role(element.raw())?;
                // Do this BEFORE names, values, actions, or children can be read.
                if protected(budget, &element, &role)? {
                    continue;
                }
                let name = name(budget, &element)?;
                let bounds = budget.optional_bounds(element.raw())?;
                let pixels = image_bounds(bounds, &frame);
                let hidden = budget.flag(element.raw(), "AXHidden")? == Some(true);
                let interactive = pixels.is_some()
                    && !hidden
                    && budget.flag(element.raw(), "AXEnabled")? == Some(true);
                let press = interactive && budget.can_press(&element)?;
                let set_value =
                    interactive && text_role(&role) && budget.value_settable(&element)?;
                let key =
                    interactive && keyboard::focused(budget, root, &element, instance.window.pid)?;
                let type_text = key && text_role(&role) && text::supported(budget, &element)?;
                let mut actions = Vec::new();
                let wait = pixels.is_some() && !hidden;
                if wait {
                    actions.push("wait".into());
                }
                if press {
                    actions.push("click".into());
                }
                if set_value {
                    actions.push("set_value".into());
                }
                if key {
                    actions.push("key".into());
                }
                if type_text {
                    actions.push("type_text".into());
                }
                let reference = format!("mac-ax-{}", uuid::Uuid::new_v4());
                let clipped = role.chars().count() > 128 || name.chars().count() > 512;
                truncated |= clipped;
                display.push(ObservationNode {
                    node_ref: reference.clone(),
                    role: role.chars().take(128).collect(),
                    name: name.chars().take(512).collect(),
                    actions,
                    truncated: clipped,
                    x: pixels.map(|b| b.x),
                    y: pixels.map(|b| b.y),
                    width: pixels.map(|b| b.width),
                    height: pixels.map(|b| b.height),
                });
                let index = nodes.len();
                let room = MAX_NODES.saturating_sub(nodes.len() + pending.len() + 1);
                if hidden {
                    // A hidden container cannot expose actionable descendants
                    // merely because a stale child still reports visible.
                } else if depth + 1 < MAX_DEPTH {
                    let (children, clipped) =
                        budget.children(&element, instance.window.pid, room)?;
                    truncated |= clipped;
                    pending.extend(
                        children
                            .into_iter()
                            .map(|child| (child, Some(index), depth + 1)),
                    );
                } else {
                    truncated = true;
                }
                nodes.push(Node {
                    element,
                    parent,
                    reference,
                    role,
                    name,
                    bounds,
                    press,
                    set_value,
                    type_text,
                    key,
                    wait,
                });
            }
            budget.check()?;
            Ok(Tree {
                nodes,
                display,
                truncated,
            })
        })
}

pub(super) fn act(
    adapter: &MacosAdapter,
    req: &DispatchRequest,
    owner: &mut grok_computer_use_core::native_action::NativeActionGuard<'_>,
) -> Result<AdapterActResult, String> {
    match req.action {
        ActionKind::Click => {
            if super::super::protocol::click_count(&req.parameters) != 1
                || super::super::protocol::click_button(&req.parameters) != "left"
            {
                return Err("AXPress supports only a single primary semantic click".into());
            }
        }
        ActionKind::SetValue | ActionKind::TypeText | ActionKind::Key => {}
        _ => return Err("unsupported AX action; no input fallback".into()),
    }
    let shared = adapter.snapshots.get(&req.run_id, &req.target_id)?;
    let frame = shared.frame();
    let tree = shared.payload().try_lock().ok_or("AX snapshot is busy")?;
    if frame.snapshot_id != req.snapshot_id || frame.revision() != req.geometry_revision {
        return Err("AX snapshot/geometry is stale".into());
    }
    let instance = WindowInstance::parse(&req.target_id)?;
    let coordinate = match req.target {
        ActionTarget::Coord { x, y } => {
            if !matches!(req.action, ActionKind::Key | ActionKind::TypeText)
                || !shared.has_screenshot()
            {
                return Err("coordinate text/key requires its fresh model screenshot".into());
            }
            Some(coordinate::Binding::new(
                pointer::point(adapter, req, &shared, x, y)?,
                frame.bounds,
            )?)
        }
        _ => None,
    };
    let index = match &req.target {
        ActionTarget::Element { element_ref } => tree
            .nodes
            .iter()
            .position(|n| n.reference == *element_ref)
            .ok_or("unknown AX reference")?,
        ActionTarget::Coord { .. } => {
            let binding = coordinate.as_ref().ok_or("coordinate binding required")?;
            adapter.windows.with_window(
                instance,
                frame.bounds,
                &req.cancellation,
                |root, budget| {
                    let hit = binding.hit(budget, root, instance.window)?;
                    tree.nodes
                        .iter()
                        .position(|n| n.element.same(&hit) && binding.contains(n.bounds))
                        .ok_or_else(|| {
                            "coordinate does not identify an exact observed AX control".into()
                        })
                },
            )?
        }
    };
    let node = &tree.nodes[index];
    if (req.action == ActionKind::Click && !node.press)
        || (req.action == ActionKind::SetValue && !node.set_value)
        || (req.action == ActionKind::TypeText && !node.type_text)
        || (req.action == ActionKind::Key && !node.key)
    {
        return Err("AX node did not advertise the requested action".into());
    }
    let value = (req.action == ActionKind::SetValue)
        .then(|| ax_api::PreparedValue::new(&req.parameters))
        .transpose()?;
    let key = (req.action == ActionKind::Key)
        .then(|| keyboard::PreparedKey::new(&req.parameters))
        .transpose()?;
    let append = (req.action == ActionKind::TypeText)
        .then(|| text::PreparedText::new(&req.parameters))
        .transpose()?;
    // Native string allocation may race revocation or a change to the target.
    req.admit(adapter)?;
    adapter
        .windows
        .with_window(instance, frame.bounds, &req.cancellation, |root, budget| {
            if !tree.nodes[0].element.same(root) {
                return Err("AX snapshot root was replaced".into());
            }
            let mut current = index;
            loop {
                let item = &tree.nodes[current];
                let role = budget.role(item.element.raw())?;
                if protected(budget, &item.element, &role)?
                    || budget.flag(item.element.raw(), "AXHidden")? == Some(true)
                    || role != item.role
                    || name(budget, &item.element)? != item.name
                    || budget.optional_bounds(item.element.raw())? != item.bounds
                {
                    return Err("AX element identity or presentation changed".into());
                }
                match item.parent {
                    Some(parent) => {
                        let ancestor = &tree.nodes[parent].element;
                        if !budget
                            .related(&item.element, "AXParent", instance.window.pid)?
                            .same(ancestor)
                            || !budget
                                .related(&item.element, "AXWindow", instance.window.pid)?
                                .same(root)
                            || !budget
                                .children(ancestor, instance.window.pid, MAX_NODES)?
                                .0
                                .iter()
                                .any(|n| n.same(&item.element))
                        {
                            return Err(
                                "AX element no longer belongs to its observed ancestry".into()
                            );
                        }
                        current = parent;
                    }
                    None if current == 0 => break,
                    _ => return Err("AX ancestry does not reach the selected root".into()),
                }
            }
            if budget.flag(node.element.raw(), "AXEnabled")? != Some(true)
                || budget.flag(node.element.raw(), "AXHidden")? == Some(true)
            {
                return Err("AX control is no longer interactive".into());
            }
            if if value.is_some() {
                !text_role(&node.role) || !budget.value_settable(&node.element)?
            } else if key.is_some() {
                !keyboard::focused(budget, root, &node.element, instance.window.pid)?
            } else if append.is_some() {
                !text_role(&node.role)
                    || !text::supported(budget, &node.element)?
                    || !keyboard::focused(budget, root, &node.element, instance.window.pid)?
            } else {
                !budget.can_press(&node.element)?
            } {
                return Err("AX action is no longer available".into());
            }
            let before = || {
                req.cancellation.check()?;
                if !adapter.input_available() {
                    return Err("macOS input permission was revoked".into());
                }
                if let Some(binding) = coordinate.as_ref() {
                    // No nested WindowBindings lock: retain the same root while
                    // rechecking the live hit before focus/protection checks.
                    binding.verify(budget, root, &node.element, instance.window)?;
                }
                if (key.is_some() || append.is_some())
                    && !keyboard::focused(budget, root, &node.element, instance.window.pid)?
                {
                    return Err("original AX keyboard focus changed before dispatch".into());
                }
                let role = budget.role(node.element.raw())?;
                if protected(budget, &node.element, &role)?
                    || role != node.role
                    || budget.flag(node.element.raw(), "AXHidden")? == Some(true)
                    || budget.flag(node.element.raw(), "AXEnabled")? != Some(true)
                    || budget.optional_bounds(node.element.raw())? != node.bounds
                {
                    return Err("AX control changed before native write".into());
                }
                if window_bounds(instance.window)? != frame.bounds
                    || capture::display_revision()? != frame.display_revision
                {
                    return Err("macOS geometry changed before AX write".into());
                }
                identity::validate(instance.window)?;
                if !adapter.input_available() {
                    return Err("macOS input permission was revoked during AX validation".into());
                }
                // Retaining an Arc only protects memory; Stop/release/new
                // observations can revoke its authority during AX reads.
                adapter
                    .snapshots
                    .require_current(&req.run_id, &req.target_id, &shared)?;
                req.cancellation.check()
            };
            if let Some(key) = key.as_ref() {
                key.dispatch(adapter, req, &shared, root, owner, before)?;
                return Ok(AdapterActResult {
                    applied: true,
                    outcome: None,
                    postcondition_ok: false,
                    verifiable: false,
                    detail: "macos named key pair queued; target effect unverified".into(),
                });
            }
            if let Some(append) = append.as_ref() {
                let applied = append.append(budget, &node.element, owner, before, || {
                    req.cancellation.check()?;
                    if !adapter.input_available() {
                        return Err("macOS permission revoked before text insertion".into());
                    }
                    adapter
                        .snapshots
                        .require_current(&req.run_id, &req.target_id, &shared)
                })?;
                return Ok(AdapterActResult {
                    applied,
                    outcome: None,
                    postcondition_ok: false,
                    verifiable: false,
                    detail: "macos AX text append returned; target effect unverified".into(),
                });
            }
            let operation = if value.is_some() {
                "AXSetValue"
            } else {
                "AXPress"
            };
            let status = match value.as_ref() {
                Some(value) => budget.set_value(&node.element, value, before)?,
                None => budget.press(&node.element, before)?,
            };
            if status != 0 {
                owner.retain_until_native_recovery();
                return Err(format!(
                    "{operation} completion unknown ({status}); replay forbidden"
                ));
            }
            req.cancellation.check()?;
            Ok(AdapterActResult {
                applied: true,
                outcome: None,
                postcondition_ok: false,
                verifiable: false,
                detail: format!("macos {operation} returned; postcondition not verified"),
            })
        })
}
