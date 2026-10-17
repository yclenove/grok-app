//! Five `wait_for` matchers against one observation. No IO, waits, or delivery.

use crate::{
    model::{NodeValue, RawNode, RawTree, Role, States},
    settle::poll,
};
use gcu_protocol::{tools::Condition, ErrorCode};

/// Hash evidence for [`Condition::Stable`]. A quiet first sample is already stable.
/// A change event clears it until two later usable samples agree.
#[derive(Default)]
pub(crate) struct Stability {
    saw_event: bool,
    previous: Option<u64>,
}

impl Stability {
    pub(crate) fn invalidate(&mut self) {
        self.saw_event = true;
        self.previous = None;
    }
}

/// Reject conditions that can never succeed. Pure; no observation.
pub(crate) fn supported(until: &Condition) -> Result<(), ErrorCode> {
    match until {
        Condition::Stable(false) => Err(ErrorCode::CapabilityUnavailable),
        Condition::Element(element) if parse_role(&element.role).is_none() => {
            Err(ErrorCode::CapabilityUnavailable)
        }
        _ => Ok(()),
    }
}

/// Whether this sample satisfies `until`. Password values are omitted: they are not
/// part of the delivered observation. Pure memory work.
pub(crate) fn met(
    until: &Condition,
    raw: &RawTree,
    title: &str,
    stability: &mut Stability,
) -> Result<bool, ErrorCode> {
    Ok(match until {
        Condition::TextPresent(needle) => contains_text(raw, title, needle),
        Condition::TextAbsent(needle) => !contains_text(raw, title, needle),
        Condition::Element(element) => {
            let Some(role) = parse_role(&element.role) else {
                return Err(ErrorCode::CapabilityUnavailable);
            };
            raw.root
                .as_ref()
                .is_some_and(|root| role_matches(root, role, element.name.as_deref()))
        }
        Condition::WindowTitle(expected) => {
            if expected.is_empty() {
                title.is_empty()
            } else {
                title.contains(expected)
            }
        }
        Condition::Stable(true) => stable_sample(raw, stability)?,
        Condition::Stable(false) => return Err(ErrorCode::CapabilityUnavailable),
    })
}

fn stable_sample(raw: &RawTree, stability: &mut Stability) -> Result<bool, ErrorCode> {
    if !poll::usable(raw) {
        stability.previous = None;
        return Ok(false);
    }
    let current = poll::hash(raw)?;
    if !stability.saw_event && stability.previous.is_none() {
        stability.previous = Some(current);
        return Ok(true);
    }
    let ready = stability.previous == Some(current);
    stability.previous = Some(current);
    Ok(ready)
}

fn parse_role(role: &str) -> Option<Role> {
    serde_json::from_value(serde_json::Value::String(role.to_owned())).ok()
}

fn role_matches(node: &RawNode, role: Role, name: Option<&str>) -> bool {
    let name_ok = name.is_none_or(|expected| node.name.as_deref() == Some(expected));
    (node.role == role && name_ok)
        || node
            .children
            .iter()
            .any(|child| role_matches(child, role, name))
}

fn contains_text(raw: &RawTree, title: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return title.is_empty() && raw.root.is_none();
    }
    title.contains(needle)
        || raw
            .root
            .as_ref()
            .is_some_and(|root| node_contains(root, needle))
}

fn node_contains(node: &RawNode, needle: &str) -> bool {
    let password = node.states.contains(States::PASSWORD);
    if node
        .name
        .as_deref()
        .is_some_and(|name| name.contains(needle))
    {
        return true;
    }
    if !password {
        if node
            .value
            .as_ref()
            .is_some_and(|value| value_text(value).contains(needle))
        {
            return true;
        }
        if node
            .text_extras
            .as_ref()
            .and_then(|extra| extra.text.as_deref())
            .is_some_and(|text| text.contains(needle))
        {
            return true;
        }
    }
    for field in [&node.description, &node.placeholder, &node.help] {
        if field.as_deref().is_some_and(|text| text.contains(needle)) {
            return true;
        }
    }
    node.children
        .iter()
        .any(|child| node_contains(child, needle))
}

fn value_text(value: &NodeValue) -> String {
    match value {
        NodeValue::Text(text) => text.clone(),
        NodeValue::Number(number) => number.to_string(),
        NodeValue::Bool(value) => value.to_string(),
        NodeValue::Range { value, .. } => value.to_string(),
    }
}
