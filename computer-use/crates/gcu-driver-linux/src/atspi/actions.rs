//! AT-SPI semantic action dispatching, table-driven action matching, and settable validation.

use crate::atspi::bus::AtspiNodeRef;
use crate::atspi::source::{FakeNode, FakeSource};
use gcu_core::{
    driver::{Cancel, Delivery, DriverAction, DriverError, NATIVE_TIMEOUT},
    model::{NativeHandle, NodeAction, NodeValue},
};
use gcu_protocol::tools::Direction as Dir;
use std::sync::Arc;
use std::time::Duration;

/// Table of prioritized matching candidates for `Press` semantic actions.
pub const PRESS_CANDIDATE_PATTERNS: &[&str] = &[
    "click", "press", "activate", "primary", "jump", "toggle", "open", "select", "check",
];

/// Table of candidate patterns for `SecondaryAction` variants.
const SECONDARY_ACTION_PATTERNS: &[(NodeAction, &[&str])] = &[
    (
        NodeAction::ShowMenu,
        &["menu", "showmenu", "show_menu", "contextmenu", "popup"],
    ),
    (NodeAction::Increment, &["increment", "step_up", "+"]),
    (NodeAction::Decrement, &["decrement", "step_down", "-"]),
    (NodeAction::Confirm, &["confirm", "ok", "accept", "submit"]),
    (NodeAction::Cancel, &["cancel", "close", "dismiss", "abort"]),
    (NodeAction::Raise, &["raise", "bring_to_front"]),
    (NodeAction::Expand, &["expand", "open"]),
    (NodeAction::Collapse, &["collapse", "close"]),
    (
        NodeAction::ScrollIntoView,
        &["scrollintoview", "scroll_into_view", "scroll"],
    ),
    (NodeAction::ScrollUp, &["scrollup", "scroll_up", "up"]),
    (
        NodeAction::ScrollDown,
        &["scrolldown", "scroll_down", "down"],
    ),
    (
        NodeAction::ScrollLeft,
        &["scrollleft", "scroll_left", "left"],
    ),
    (
        NodeAction::ScrollRight,
        &["scrollright", "scroll_right", "right"],
    ),
    (NodeAction::Select, &["select", "choose"]),
    (NodeAction::Toggle, &["toggle", "check"]),
];

/// Find the best matching action index for a `Press` semantic request using prioritized pattern matching.
pub fn find_best_press_action(action_names: &[String]) -> Option<i32> {
    if action_names.is_empty() {
        return None;
    }

    // 1. Exact case-insensitive match against prioritized candidates
    for &pattern in PRESS_CANDIDATE_PATTERNS {
        for (idx, name) in action_names.iter().enumerate() {
            if name.trim().eq_ignore_ascii_case(pattern) {
                return Some(idx as i32);
            }
        }
    }

    // 2. Substring / contains match against prioritized candidates
    for &pattern in PRESS_CANDIDATE_PATTERNS {
        for (idx, name) in action_names.iter().enumerate() {
            if name.to_ascii_lowercase().contains(pattern) {
                return Some(idx as i32);
            }
        }
    }

    // 3. If there is only one advertised action, use it as fallback
    if action_names.len() == 1 {
        return Some(0);
    }

    None
}

/// Find the matching action index for an advertised [`NodeAction`].
pub fn find_secondary_action(action: &NodeAction, action_names: &[String]) -> Option<i32> {
    if action_names.is_empty() {
        return None;
    }

    match action {
        NodeAction::Press => find_best_press_action(action_names),
        NodeAction::Native(custom_name) => {
            let target = custom_name.to_ascii_lowercase();
            for (idx, name) in action_names.iter().enumerate() {
                let n_lower = name.to_ascii_lowercase();
                if n_lower == target || n_lower.contains(&target) {
                    return Some(idx as i32);
                }
            }
            None
        }
        other => {
            for (candidate_action, patterns) in SECONDARY_ACTION_PATTERNS {
                if candidate_action == other {
                    for &pat in *patterns {
                        for (idx, name) in action_names.iter().enumerate() {
                            let n_lower = name.to_ascii_lowercase();
                            if n_lower == pat || n_lower.contains(pat) {
                                return Some(idx as i32);
                            }
                        }
                    }
                }
            }
            None
        }
    }
}

/// Extract an `AtspiNodeRef` from a process-local [`NativeHandle`].
pub fn extract_atspi_node(handle: &NativeHandle) -> Option<AtspiNodeRef> {
    if let Some(node) = handle.downcast_ref::<AtspiNodeRef>() {
        Some(node.clone())
    } else {
        handle
            .downcast_ref::<Arc<AtspiNodeRef>>()
            .map(|arc_node| (**arc_node).clone())
    }
}

/// Extract a `FakeNode` id from a process-local [`NativeHandle`].
pub fn extract_fake_id(handle: &NativeHandle) -> Option<u64> {
    handle.downcast_ref::<u64>().copied()
}

fn bus_dest(dest: &str) -> Result<zbus::names::OwnedBusName, DriverError> {
    dest.to_string()
        .try_into()
        .map_err(|e: zbus::names::Error| DriverError::Native {
            message: e.to_string(),
            maybe_executed: false,
        })
}

fn obj_path(path: &str) -> Result<zbus::zvariant::OwnedObjectPath, DriverError> {
    path.try_into()
        .map_err(|e: zbus::zvariant::Error| DriverError::Native {
            message: format!("invalid object path: {e}"),
            maybe_executed: false,
        })
}

fn map_zbus_error(e: zbus::Error, before_dispatch: bool) -> DriverError {
    if let zbus::Error::MethodError(ref name, _, _) = e {
        let n = name.as_str();
        if n.contains("UnknownObject")
            || n.contains("ServiceUnknown")
            || n.contains("NameHasNoOwner")
        {
            return DriverError::StaleElement;
        }
        if n.contains("UnknownInterface") {
            return DriverError::NotSettable;
        }
    }
    DriverError::Native {
        message: e.to_string(),
        maybe_executed: !before_dispatch,
    }
}

/// Dispatch an AT-SPI semantic action, either to a live D-Bus bus or in-memory mock source.
pub async fn execute_atspi_action(
    conn: Option<&zbus::Connection>,
    fake_source: Option<&FakeSource>,
    fake_root: Option<&FakeNode>,
    req: &DriverAction,
    cancel: &Cancel,
) -> Result<Delivery, DriverError> {
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }

    if let Some(fake) = fake_source {
        return execute_fake_atspi_action(fake, fake_root, req, cancel).await;
    }

    let connection = conn.ok_or(DriverError::PermissionMissing {
        permission: "accessibility".into(),
    })?;

    // Wrap live D-Bus call in timeout with maybe_executed semantics
    tokio::time::timeout(NATIVE_TIMEOUT, async {
        match req {
            DriverAction::Press { el } => {
                let node_ref = extract_atspi_node(el).ok_or(DriverError::StaleElement)?;
                let dest = bus_dest(&node_ref.destination)?;
                let path = obj_path(&node_ref.path)?;
                let proxy = atspi::proxy::action::ActionProxy::builder(connection)
                    .destination(dest)
                    .map_err(|e| map_zbus_error(e, true))?
                    .path(path)
                    .map_err(|e| map_zbus_error(e, true))?
                    .build()
                    .await
                    .map_err(|e| map_zbus_error(e, true))?;

                let actions = proxy
                    .get_actions()
                    .await
                    .map_err(|e| map_zbus_error(e, false))?;
                if actions.is_empty() {
                    return Err(DriverError::ElementNotActionable);
                }
                let names: Vec<String> = actions.into_iter().map(|a| a.name).collect();
                let target_index =
                    find_best_press_action(&names).ok_or(DriverError::ElementNotActionable)?;
                let ok = proxy
                    .do_action(target_index)
                    .await
                    .map_err(|e| map_zbus_error(e, false))?;
                if !ok {
                    return Err(DriverError::Native {
                        message: "AT-SPI action returned false".into(),
                        maybe_executed: true,
                    });
                }
                Ok(Delivery::Accessibility)
            }
            DriverAction::SecondaryAction { el, action } => {
                let node_ref = extract_atspi_node(el).ok_or(DriverError::StaleElement)?;
                let dest = bus_dest(&node_ref.destination)?;
                let path = obj_path(&node_ref.path)?;
                let proxy = atspi::proxy::action::ActionProxy::builder(connection)
                    .destination(dest)
                    .map_err(|e| map_zbus_error(e, true))?
                    .path(path)
                    .map_err(|e| map_zbus_error(e, true))?
                    .build()
                    .await
                    .map_err(|e| map_zbus_error(e, true))?;

                let actions = proxy
                    .get_actions()
                    .await
                    .map_err(|e| map_zbus_error(e, false))?;
                if actions.is_empty() {
                    return Err(DriverError::ElementNotActionable);
                }
                let names: Vec<String> = actions.into_iter().map(|a| a.name).collect();
                let target_index = find_secondary_action(action, &names)
                    .ok_or(DriverError::ElementNotActionable)?;
                let ok = proxy
                    .do_action(target_index)
                    .await
                    .map_err(|e| map_zbus_error(e, false))?;
                if !ok {
                    return Err(DriverError::Native {
                        message: "AT-SPI secondary action returned false".into(),
                        maybe_executed: true,
                    });
                }
                Ok(Delivery::Accessibility)
            }
            DriverAction::SetValue { el, value } => {
                let node_ref = extract_atspi_node(el).ok_or(DriverError::StaleElement)?;
                let dest = bus_dest(&node_ref.destination)?;
                let path = obj_path(&node_ref.path)?;

                // 1. Try EditableText
                let edit_proxy =
                    atspi::proxy::editable_text::EditableTextProxy::builder(connection)
                        .destination(dest.clone())
                        .map_err(|e| map_zbus_error(e, true))?
                        .path(path.clone())
                        .map_err(|e| map_zbus_error(e, true))?
                        .build()
                        .await;

                if let Ok(proxy) = edit_proxy {
                    if let Ok(ok) = proxy.set_text_contents(value).await {
                        if ok {
                            return Ok(Delivery::Accessibility);
                        }
                    }
                }

                // 2. Try Value (e.g. slider / spinbox)
                if let Ok(num) = value.parse::<f64>() {
                    let val_proxy = atspi::proxy::value::ValueProxy::builder(connection)
                        .destination(dest)
                        .map_err(|e| map_zbus_error(e, true))?
                        .path(path)
                        .map_err(|e| map_zbus_error(e, true))?
                        .build()
                        .await;

                    if let Ok(proxy) = val_proxy {
                        if proxy.set_current_value(num).await.is_ok() {
                            return Ok(Delivery::Accessibility);
                        }
                    }
                }

                Err(DriverError::NotSettable)
            }
            DriverAction::SelectText { el, range } => {
                let node_ref = extract_atspi_node(el).ok_or(DriverError::StaleElement)?;
                let dest = bus_dest(&node_ref.destination)?;
                let path = obj_path(&node_ref.path)?;

                let text_proxy = atspi::proxy::text::TextProxy::builder(connection)
                    .destination(dest)
                    .map_err(|e| map_zbus_error(e, true))?
                    .path(path)
                    .map_err(|e| map_zbus_error(e, true))?
                    .build()
                    .await
                    .map_err(|e| map_zbus_error(e, true))?;

                let start = range.start as i32;
                let end = (range.start + range.length) as i32;
                let ok = text_proxy
                    .set_selection(0, start, end)
                    .await
                    .map_err(|e| map_zbus_error(e, false))?;
                if ok {
                    Ok(Delivery::Accessibility)
                } else {
                    Err(DriverError::ElementNotActionable)
                }
            }
            DriverAction::ScrollElement { el, dir, .. } => {
                let node_ref = extract_atspi_node(el).ok_or(DriverError::StaleElement)?;
                let dest = bus_dest(&node_ref.destination)?;
                let path = obj_path(&node_ref.path)?;

                let comp_proxy = atspi::proxy::component::ComponentProxy::builder(connection)
                    .destination(dest)
                    .map_err(|e| map_zbus_error(e, true))?
                    .path(path)
                    .map_err(|e| map_zbus_error(e, true))?
                    .build()
                    .await
                    .map_err(|e| map_zbus_error(e, true))?;

                let scroll_type = match dir {
                    Dir::Up => atspi::ScrollType::TopEdge,
                    Dir::Down => atspi::ScrollType::BottomEdge,
                    Dir::Left => atspi::ScrollType::LeftEdge,
                    Dir::Right => atspi::ScrollType::RightEdge,
                };

                let ok = comp_proxy
                    .scroll_to(scroll_type)
                    .await
                    .map_err(|e| map_zbus_error(e, false))?;
                if ok {
                    Ok(Delivery::Accessibility)
                } else {
                    Err(DriverError::ElementNotActionable)
                }
            }
            _ => Err(DriverError::CapabilityUnavailable),
        }
    })
    .await
    .map_err(|_| DriverError::Timeout {
        maybe_executed: true,
    })?
}

fn find_fake_node(root: &FakeNode, target_id: u64) -> Option<&FakeNode> {
    if root.id == target_id {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_fake_node(child, target_id) {
            return Some(found);
        }
    }
    None
}

async fn execute_fake_atspi_action(
    fake: &FakeSource,
    root: Option<&FakeNode>,
    req: &DriverAction,
    cancel: &Cancel,
) -> Result<Delivery, DriverError> {
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }

    if fake.delay > Duration::ZERO {
        tokio::time::sleep(fake.delay).await;
    }

    let root_node = root.ok_or(DriverError::WindowGone)?;

    match req {
        DriverAction::Press { el } => {
            let id = extract_fake_id(el).ok_or(DriverError::StaleElement)?;
            if fake.error_on_id == Some(id) {
                return Err(DriverError::Timeout {
                    maybe_executed: true,
                });
            }
            let target = find_fake_node(root_node, id).ok_or(DriverError::StaleElement)?;
            let names: Vec<String> = target
                .actions
                .iter()
                .map(|a| match a {
                    NodeAction::Press => "click".into(),
                    NodeAction::Native(s) => s.to_string(),
                    _ => "activate".into(),
                })
                .collect();
            if find_best_press_action(&names).is_some() {
                Ok(Delivery::Accessibility)
            } else {
                Err(DriverError::ElementNotActionable)
            }
        }
        DriverAction::SecondaryAction { el, action } => {
            let id = extract_fake_id(el).ok_or(DriverError::StaleElement)?;
            let target = find_fake_node(root_node, id).ok_or(DriverError::StaleElement)?;
            let names: Vec<String> = target
                .actions
                .iter()
                .map(|a| match a {
                    NodeAction::Press => "click".into(),
                    NodeAction::Native(s) => s.to_string(),
                    _ => "activate".into(),
                })
                .collect();
            if find_secondary_action(action, &names).is_some() {
                Ok(Delivery::Accessibility)
            } else {
                Err(DriverError::ElementNotActionable)
            }
        }
        DriverAction::SetValue { el, value } => {
            let id = extract_fake_id(el).ok_or(DriverError::StaleElement)?;
            let target = find_fake_node(root_node, id).ok_or(DriverError::StaleElement)?;
            if target.states.contains(atspi::State::Editable) {
                return Ok(Delivery::Accessibility);
            }
            if let Some(NodeValue::Range { .. }) | Some(NodeValue::Number(_)) = target.value {
                if value.parse::<f64>().is_ok() {
                    return Ok(Delivery::Accessibility);
                }
            }
            Err(DriverError::NotSettable)
        }
        DriverAction::SelectText { el, .. } => {
            let id = extract_fake_id(el).ok_or(DriverError::StaleElement)?;
            let target = find_fake_node(root_node, id).ok_or(DriverError::StaleElement)?;
            if target.text_extras.is_some() || target.states.contains(atspi::State::Editable) {
                Ok(Delivery::Accessibility)
            } else {
                Err(DriverError::ElementNotActionable)
            }
        }
        DriverAction::ScrollElement { el, .. } => {
            let id = extract_fake_id(el).ok_or(DriverError::StaleElement)?;
            let _ = find_fake_node(root_node, id).ok_or(DriverError::StaleElement)?;
            Ok(Delivery::Accessibility)
        }
        _ => Err(DriverError::CapabilityUnavailable),
    }
}
