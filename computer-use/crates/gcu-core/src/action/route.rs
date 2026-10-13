use super::{
    gate::{self, Gate},
    observe::Fresh,
    ToolCall,
};
use crate::{
    driver::{DriverAction, KeyRoute, PointerRoute},
    model::{NodeAction, NodeValue, RawNode, Role, States, TextRange},
    policy::{ActionIntent, Tool},
};
use gcu_protocol::{
    result::{ActionTarget, Delivery},
    tools::{BatchStep, Direction, MouseButton},
    Capabilities, ErrorCode, KeyChord, ToolName,
};

pub(crate) struct Plan {
    pub request: DriverAction,
    pub intent: ActionIntent,
    pub target: Option<ActionTarget>,
    pub delivery: Delivery,
}
pub(crate) fn tool(action: &BatchStep) -> ToolName {
    match action {
        BatchStep::Click(_) => ToolName::Click,
        BatchStep::PerformSecondaryAction(_) => ToolName::PerformSecondaryAction,
        BatchStep::SetValue(_) => ToolName::SetValue,
        BatchStep::SelectText(_) => ToolName::SelectText,
        BatchStep::TypeText(_) => ToolName::TypeText,
        BatchStep::PressKey(_) => ToolName::PressKey,
        BatchStep::Scroll(_) => ToolName::Scroll,
        BatchStep::Drag(_) => ToolName::Drag,
        BatchStep::WaitFor(_) => ToolName::WaitFor,
        BatchStep::Expect(_) => ToolName::ComputerBatch,
    }
}
pub(crate) fn pointer_needed(
    call: &ToolCall,
    gate: &Gate,
    fresh: &Fresh,
    caps: &Capabilities,
) -> bool {
    match &call.action {
        BatchStep::Click(a) => {
            gate.element.is_some()
                && (!caps.delivery.accessibility
                    || a.click_count != 1
                    || a.mouse_button != MouseButton::Left
                    || target(gate, fresh).is_none_or(|n| semantic_press(n).is_none()))
        }
        BatchStep::Scroll(_) => {
            gate.element.is_some()
                && (!caps.delivery.accessibility
                    || target(gate, fresh).is_none_or(|n| !scrollable(n.role)))
        }
        _ => false,
    }
}
fn target<'a>(gate: &Gate, fresh: &'a Fresh) -> Option<&'a RawNode> {
    gate.element.as_ref().and_then(|e| {
        fresh
            .tree
            .nodes()
            .into_iter()
            .find(|(_, n)| n.node.handle.ptr_eq(&e.handle))
            .map(|(_, n)| &n.node)
    })
}
fn semantic_press(n: &RawNode) -> Option<NodeAction> {
    [
        NodeAction::Press,
        NodeAction::Toggle,
        NodeAction::Select,
        NodeAction::Confirm,
    ]
    .into_iter()
    .find(|a| n.actions.contains(a))
}
fn scrollable(role: Role) -> bool {
    matches!(
        role,
        Role::ScrollArea | Role::List | Role::Table | Role::Outline | Role::Tree
    )
}
pub(crate) fn plan(
    call: &ToolCall,
    gate: &Gate,
    fresh: &Fresh,
    caps: &Capabilities,
) -> Result<Plan, ErrorCode> {
    let node = target(gate, fresh);
    let el = || {
        node.map(|n| n.handle.clone())
            .ok_or(ErrorCode::StaleElement)
    };
    let at = || {
        gate.points
            .first()
            .copied()
            .ok_or(ErrorCode::ElementNotActionable)
    };
    let semantic = caps.delivery.accessibility;
    let mut delivery = Delivery::Accessibility;
    let mut intent = ActionIntent::new(match call.action {
        BatchStep::Click(_) => Tool::Click,
        BatchStep::PerformSecondaryAction(_) => Tool::Secondary,
        BatchStep::SetValue(_) => Tool::SetValue,
        BatchStep::SelectText(_) => Tool::SetValue,
        BatchStep::TypeText(_) => Tool::TypeText,
        BatchStep::PressKey(_) => Tool::PressKey,
        BatchStep::Scroll(_) => Tool::Scroll,
        BatchStep::Drag(_) => Tool::Drag,
        _ => return Err(ErrorCode::CapabilityUnavailable),
    });
    intent.element_index = gate::element_index(&call.action);
    intent.at = gate.points.first().map(|p| (p.x, p.y));
    intent.snapshot_id = gate
        .snapshot_id
        .map_or_else(String::new, |id| format!("s_{}", id.0));
    let pointer_route =
        if caps.delivery.targeted_pointer || call.pointer_route == PointerRoute::TargetedOnly {
            PointerRoute::TargetedOnly
        } else {
            PointerRoute::AllowForeground
        };
    let key_route = if caps.delivery.targeted_keys {
        KeyRoute::TargetedOnly
    } else {
        KeyRoute::AllowForeground
    };
    let request = match &call.action {
        BatchStep::Click(_) if gate.points.is_empty() && semantic => {
            let action = node
                .and_then(semantic_press)
                .ok_or(ErrorCode::ElementNotActionable)?;
            if action == NodeAction::Press {
                DriverAction::Press { el: el()? }
            } else {
                DriverAction::SecondaryAction { el: el()?, action }
            }
        }
        BatchStep::Click(a) => {
            delivery = pointer_delivery(caps, pointer_route);
            DriverAction::PointerClick {
                win: fresh.win.clone(),
                at: at()?,
                button: a.mouse_button,
                count: a.click_count,
                route: pointer_route,
            }
        }
        BatchStep::PerformSecondaryAction(a) => {
            let action = node
                .and_then(|n| {
                    n.actions
                        .iter()
                        .find(|act| action_name(act).eq_ignore_ascii_case(&a.action))
                })
                .cloned()
                .ok_or(ErrorCode::ElementNotActionable)?;
            if !semantic {
                return Err(ErrorCode::CapabilityUnavailable);
            }
            DriverAction::SecondaryAction { el: el()?, action }
        }
        BatchStep::SetValue(a) => {
            intent.text = Some(a.value.clone());
            if !semantic {
                return Err(ErrorCode::CapabilityUnavailable);
            }
            DriverAction::SetValue {
                el: el()?,
                value: a.value.clone(),
            }
        }
        BatchStep::SelectText(a) => {
            if !semantic {
                return Err(ErrorCode::CapabilityUnavailable);
            }
            let range = if let Some(text) = &a.text {
                let value = match node.and_then(|n| n.value.as_ref()) {
                    Some(NodeValue::Text(v)) => v,
                    _ => return Err(ErrorCode::NotSettable),
                };
                let start = value.find(text).ok_or(ErrorCode::ElementNotActionable)?;
                TextRange {
                    start: value[..start].chars().count(),
                    length: text.chars().count(),
                }
            } else {
                TextRange {
                    start: a.start.unwrap_or_default() as usize,
                    length: a.length.unwrap_or_default() as usize,
                }
            };
            DriverAction::SelectText { el: el()?, range }
        }
        BatchStep::TypeText(a) => {
            intent.text = Some(a.text.clone());
            let focused = node.or_else(|| {
                fresh
                    .tree
                    .nodes()
                    .into_iter()
                    .find(|(_, n)| n.node.states.contains(States::FOCUSED))
                    .map(|(_, n)| &n.node)
            });
            if let Some(n) = focused.filter(|n| {
                semantic
                    && n.states.contains(States::SETTABLE)
                    && !n.states.contains(States::PASSWORD)
                    && !matches!(n.role, Role::WebArea | Role::Document)
            }) {
                if let Some(NodeValue::Text(previous)) = &n.value {
                    let value = if let Some(range) = n.text_extras.as_ref().and_then(|t| t.selected)
                    {
                        previous
                            .chars()
                            .take(range.start)
                            .chain(a.text.chars())
                            .chain(
                                previous
                                    .chars()
                                    .skip(range.start.saturating_add(range.length)),
                            )
                            .collect()
                    } else {
                        format!("{previous}{}", a.text)
                    };
                    DriverAction::SetValue {
                        el: n.handle.clone(),
                        value,
                    }
                } else {
                    return Err(ErrorCode::NotSettable);
                }
            } else {
                delivery = if caps.delivery.targeted_keys {
                    Delivery::TargetedEvent
                } else {
                    Delivery::Foreground
                };
                DriverAction::TypeText {
                    win: fresh.win.clone(),
                    focus: node.map(|n| n.handle.clone()),
                    text: a.text.clone(),
                    route: key_route,
                }
            }
        }
        BatchStep::PressKey(a) => {
            intent.key = Some(a.key.clone());
            delivery = if caps.delivery.targeted_keys {
                Delivery::TargetedEvent
            } else {
                Delivery::Foreground
            };
            DriverAction::PressKeys {
                win: fresh.win.clone(),
                chord: KeyChord::parse(&a.key).map_err(|_| ErrorCode::InvalidKey)?,
                route: key_route,
            }
        }
        BatchStep::Scroll(a) if gate.points.is_empty() && semantic => DriverAction::ScrollElement {
            el: el()?,
            dir: a.direction,
            pages: a.pages as f32,
        },
        BatchStep::Scroll(a) => {
            delivery = pointer_delivery(caps, pointer_route);
            let amount = (a.pages * 360.).clamp(1., f64::from(i32::MAX)) as i32;
            let (dx, dy) = match a.direction {
                Direction::Up => (0, -amount),
                Direction::Down => (0, amount),
                Direction::Left => (-amount, 0),
                Direction::Right => (amount, 0),
            };
            DriverAction::PointerScroll {
                win: fresh.win.clone(),
                at: at()?,
                dx,
                dy,
                route: pointer_route,
            }
        }
        BatchStep::Drag(_) => {
            delivery = pointer_delivery(caps, pointer_route);
            DriverAction::PointerDrag {
                win: fresh.win.clone(),
                path: gate.points.clone(),
                route: pointer_route,
            }
        }
        _ => return Err(ErrorCode::CapabilityUnavailable),
    };
    intent.foreground = delivery == Delivery::Foreground;
    if intent.foreground
        && (fresh.info.on_current_desktop == Some(false)
            || caps.delivery.foreground
                == gcu_protocol::capabilities::ForegroundCapability::Unavailable)
    {
        return Err(ErrorCode::CapabilityUnavailable);
    }
    let target = if let Some(index) = intent.element_index {
        Some(ActionTarget::Element {
            element_index: index,
            label: node.and_then(|n| n.name.clone()).unwrap_or_default(),
        })
    } else {
        gate.points.first().map(|_| {
            let (x, y) = match &call.action {
                BatchStep::Click(a) => (a.x.unwrap_or_default(), a.y.unwrap_or_default()),
                BatchStep::Scroll(a) => (a.x.unwrap_or_default(), a.y.unwrap_or_default()),
                BatchStep::Drag(a) => (a.from_x, a.from_y),
                _ => (0., 0.),
            };
            ActionTarget::Coordinates {
                x,
                y,
                screenshot_id: gate.screenshot_id.clone(),
            }
        })
    };
    Ok(Plan {
        request,
        intent,
        target,
        delivery,
    })
}
fn pointer_delivery(caps: &Capabilities, route: PointerRoute) -> Delivery {
    if caps.delivery.targeted_pointer || route == PointerRoute::TargetedOnly {
        Delivery::TargetedEvent
    } else {
        Delivery::Foreground
    }
}
fn action_name(action: &NodeAction) -> String {
    if let NodeAction::Native(s) = action {
        return s.to_string();
    }
    serde_json::to_value(action)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
        .replace('_', " ")
}
