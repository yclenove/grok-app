//! Versioned App Computer Use protocol shared by the Host and adapters.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

/// YOLO / acceptEdits / always-approve never grant desktop control, even if
/// they later appear in an action's allowed parameter list.
pub fn forbidden_permission_param(parameters: &serde_json::Value) -> Option<String> {
    let obj = parameters.as_object()?;
    for key in obj.keys() {
        let normalized: String = key
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .flat_map(|c| c.to_lowercase())
            .collect();
        if matches!(
            normalized.as_str(),
            "yolo" | "acceptedits" | "alwaysapprove" | "alwaysapproveedits"
        ) {
            return Some(key.clone());
        }
    }
    None
}

pub fn click_button(parameters: &serde_json::Value) -> &'static str {
    match parameters.get("button").and_then(serde_json::Value::as_str) {
        Some("right") => "right",
        Some("middle") => "middle",
        _ => "left",
    }
}

pub fn click_count(parameters: &serde_json::Value) -> u32 {
    match parameters.get("count").and_then(serde_json::Value::as_u64) {
        Some(2) => 2,
        _ => 1,
    }
}

/// Named navigation keys only. Combos like alt+f4 / win+l are never mapped to Enter.
pub fn normalize_key(key: &str) -> Result<&'static str, String> {
    Ok(match key.trim().to_ascii_lowercase().as_str() {
        "return" | "enter" => "enter",
        "tab" => "tab",
        "escape" | "esc" => "escape",
        "space" => "space",
        "down" => "down",
        "up" => "up",
        "left" => "left",
        "right" => "right",
        "backspace" => "backspace",
        "delete" => "delete",
        "home" => "home",
        "end" => "end",
        "pageup" | "page_up" => "pageup",
        "pagedown" | "page_down" => "pagedown",
        _ => return Err("key is not in the allowed set".into()),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Click,
    SetValue,
    TypeText,
    Key,
    Scroll,
    Drag,
    Wait,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum ActionTarget {
    Element {
        #[serde(rename = "elementRef")]
        element_ref: String,
    },
    Coord {
        x: f64,
        y: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionRequest {
    pub version: u32,
    pub action_id: String,
    pub run_id: String,
    pub target_id: String,
    pub target_generation: u64,
    pub snapshot_id: String,
    pub geometry_revision: u64,
    pub action: ActionKind,
    pub target: ActionTarget,
    #[serde(default)]
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    Applied,
    Verified,
    Rejected,
    Unknown,
}

impl OutcomeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OutcomeKind::Applied => "applied",
            OutcomeKind::Verified => "verified",
            OutcomeKind::Rejected => "rejected",
            OutcomeKind::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionOutcome {
    pub action_id: String,
    pub run_id: String,
    pub kind: OutcomeKind,
    pub executed: bool,
    pub reason: Option<String>,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationImage {
    pub width: u32,
    pub height: u32,
    pub content_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub png_base64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ObservationNode {
    #[serde(rename = "ref")]
    pub node_ref: String,
    pub role: String,
    pub name: String,
    pub actions: Vec<String>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
}

impl ObservationNode {
    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        match (self.x, self.y, self.width, self.height) {
            (Some(bx), Some(by), Some(bw), Some(bh)) if bw > 0.0 && bh > 0.0 => {
                x >= bx && y >= by && x < bx + bw && y < by + bh
            }
            _ => false,
        }
    }
}

pub const COORDINATE_SPACE_IMAGE_PIXELS: &str = "image-pixels";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub version: u32,
    pub run_id: String,
    pub target_id: String,
    pub target_generation: u64,
    pub snapshot_id: String,
    pub captured_at: String,
    pub geometry_revision: u64,
    pub coordinate_space: String,
    pub image: ObservationImage,
    pub nodes: Vec<ObservationNode>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    pub truncated: bool,
    #[serde(default)]
    pub crop_x: u32,
    #[serde(default)]
    pub crop_y: u32,
    #[serde(default)]
    pub crop_width: u32,
    #[serde(default)]
    pub crop_height: u32,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default = "default_dpi")]
    pub dpi: f64,
    #[serde(default)]
    pub origin_x: i32,
    #[serde(default)]
    pub origin_y: i32,
    #[serde(default)]
    pub topology_revision: u64,
}

fn default_scale() -> f64 {
    1.0
}

fn default_dpi() -> f64 {
    96.0
}

/// Host-normalized observation: image pixels, one revision for capture and AX/DOM.
pub fn normalize_observation(obs: &mut Observation, topology_revision: u64) {
    obs.coordinate_space = COORDINATE_SPACE_IMAGE_PIXELS.to_string();
    obs.topology_revision = topology_revision;
    obs.geometry_revision = topology_revision;
    if obs.crop_width == 0 {
        obs.crop_width = obs.image.width;
    }
    if obs.crop_height == 0 {
        obs.crop_height = obs.image.height;
    }
    if !obs.scale.is_finite() || obs.scale <= 0.0 {
        obs.scale = 1.0;
    }
    if !obs.dpi.is_finite() || obs.dpi <= 0.0 {
        obs.dpi = 96.0;
    }
}

pub const OBSERVATION_NODE_CAP: usize = 64;
pub const OBSERVATION_PNG_B64_CAP: usize = 400_000;
pub const OBSERVATION_CANDIDATE_CAP: usize = 512;
pub const OBSERVATION_ROLE_CHARS: usize = 64;
pub const OBSERVATION_NAME_CHARS: usize = 256;
pub const OBSERVATION_TITLE_CHARS: usize = 512;
pub const OBSERVATION_URL_CHARS: usize = 2048;
pub const OBSERVATION_ARIA_CHARS: usize = 32_000;
pub const OBSERVATION_JSON_BYTES: usize = 128 * 1024;
pub const JS_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const MODEL_TRACE_CAP: usize = 32;
pub const TEXT_MAX_CHARS: usize = 4000;
pub const KEY_MAX_LEN: usize = 32;

/// Bound what the model sees. Oversized screenshots do not unlock coordinate clicks.
pub fn clamp_observation(obs: &mut Observation) {
    if let Some((end, _)) = obs.text.char_indices().nth(OBSERVATION_ARIA_CHARS) {
        obs.text.truncate(end);
        obs.truncated = true;
    }
    if obs.nodes.len() > OBSERVATION_NODE_CAP {
        obs.nodes.truncate(OBSERVATION_NODE_CAP);
        obs.truncated = true;
    }
    if obs
        .image
        .png_base64
        .as_ref()
        .is_some_and(|png| png.len() > OBSERVATION_PNG_B64_CAP)
    {
        obs.image.png_base64 = None;
        obs.truncated = true;
    }
}

impl ActionRequest {
    pub fn validate_schema(&self) -> Result<(), String> {
        if self.version != PROTOCOL_VERSION {
            return Err(format!("unsupported protocol version {}", self.version));
        }
        if let Some(key) = forbidden_permission_param(&self.parameters) {
            return Err(format!("{key} never grants desktop control"));
        }
        if self.target_generation < 1 {
            return Err("targetGeneration must be >= 1".into());
        }
        if self.action_id.trim().is_empty() {
            return Err("actionId required".into());
        }
        if self.run_id.trim().is_empty() {
            return Err("runId required".into());
        }
        if self.target_id.trim().is_empty() {
            return Err("targetId required".into());
        }
        for id in [
            &self.action_id,
            &self.run_id,
            &self.target_id,
            &self.snapshot_id,
        ] {
            if id.trim().is_empty() || id.len() > 256 {
                return Err("invalid identity length".into());
            }
        }
        match &self.target {
            ActionTarget::Element { element_ref }
                if element_ref.trim().is_empty() || element_ref.len() > 256 =>
            {
                return Err("invalid elementRef".into())
            }
            ActionTarget::Coord { x, y }
                if !x.is_finite() || !y.is_finite() || *x < 0.0 || *y < 0.0 =>
            {
                return Err("coordinates must be nonnegative finite numbers".into())
            }
            _ => {}
        }
        let parameters = self
            .parameters
            .as_object()
            .ok_or("parameters must be an object")?;
        let allowed: &[&str] = match self.action {
            ActionKind::Click => &["button", "count"],
            ActionKind::SetValue => &["text"],
            ActionKind::TypeText => &["text", "via"],
            ActionKind::Key => &["key"],
            ActionKind::Scroll => &["delta"],
            ActionKind::Drag => &["toX", "toY", "x1", "y1"],
            ActionKind::Wait => &["nameEquals", "timeoutMs"],
        };
        if parameters
            .keys()
            .any(|key| !allowed.contains(&key.as_str()))
        {
            return Err("unknown action parameter".into());
        }
        match self.action {
            ActionKind::Click => {
                if let Some(button) = parameters.get("button") {
                    match button.as_str() {
                        Some("left") | Some("right") | Some("middle") => {}
                        _ => return Err("button must be left, right, or middle".into()),
                    }
                }
                if let Some(count) = parameters.get("count") {
                    match count.as_u64() {
                        Some(1) | Some(2) => {}
                        _ => return Err("count must be 1 or 2".into()),
                    }
                }
            }
            ActionKind::SetValue | ActionKind::TypeText => {
                let text = parameters
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("text required")?;
                if text.chars().count() > TEXT_MAX_CHARS || text.contains('\0') {
                    return Err("invalid text length or NUL".into());
                }
                if self.action == ActionKind::TypeText {
                    if let Some(via) = parameters.get("via") {
                        if via.as_str() != Some("clipboard") {
                            return Err("via must be clipboard when set".into());
                        }
                    }
                }
            }
            ActionKind::Key => {
                let key = parameters
                    .get("key")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("key required")?;
                if key.is_empty() || key.len() > KEY_MAX_LEN {
                    return Err("invalid key".into());
                }
                normalize_key(key)?;
            }
            ActionKind::Scroll => {
                ScrollDelta::parse(&self.parameters)?;
            }
            ActionKind::Drag => {
                drag_destination(&self.parameters)?;
            }
            ActionKind::Wait => {
                let name = parameters
                    .get("nameEquals")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("nameEquals required")?;
                if name.trim().is_empty() || name.len() > 256 {
                    return Err("invalid nameEquals".into());
                }
                if let Some(timeout) = parameters.get("timeoutMs") {
                    match timeout.as_u64() {
                        Some(ms) if (1..=10_000).contains(&ms) => {}
                        _ => return Err("timeoutMs must be between 1 and 10000".into()),
                    }
                }
            }
        }
        match (&self.action, &self.target) {
            (ActionKind::Click, ActionTarget::Element { element_ref })
                if element_ref.trim().is_empty() =>
            {
                Err("elementRef required".into())
            }
            (ActionKind::Click, ActionTarget::Coord { x, y })
                if !x.is_finite() || !y.is_finite() =>
            {
                Err("coordinates must be finite".into())
            }
            (ActionKind::SetValue, ActionTarget::Element { element_ref })
                if element_ref.trim().is_empty() =>
            {
                Err("elementRef required".into())
            }
            (ActionKind::SetValue, ActionTarget::Coord { .. }) => {
                Err("set_value requires elementRef".into())
            }
            (ActionKind::TypeText, _) => {
                let text = self.parameters.get("text").and_then(|v| v.as_str());
                match text {
                    None => Err("type_text requires parameters.text".into()),
                    Some(s) if s.chars().count() > TEXT_MAX_CHARS => {
                        Err("type_text too long".into())
                    }
                    Some(_) => Ok(()),
                }
            }
            (ActionKind::Key, _) => {
                if self
                    .parameters
                    .get("key")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
                {
                    Err("key requires parameters.key".into())
                } else {
                    Ok(())
                }
            }
            (ActionKind::Wait, ActionTarget::Element { element_ref })
                if element_ref.trim().is_empty() =>
            {
                Err("elementRef required".into())
            }
            (ActionKind::Wait, ActionTarget::Coord { .. }) => {
                Err("wait requires elementRef".into())
            }
            _ => Ok(()),
        }
    }

    pub fn needs_desktop_input(&self) -> bool {
        match self.action {
            ActionKind::Click => matches!(self.target, ActionTarget::Coord { .. }),
            ActionKind::SetValue | ActionKind::Wait => false,
            ActionKind::TypeText | ActionKind::Key | ActionKind::Scroll | ActionKind::Drag => true,
        }
    }
}

mod drag;
pub use drag::destination as drag_destination;
mod scroll;
pub use scroll::ScrollDelta;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_protocol_version() {
        let req = ActionRequest {
            version: 99,
            action_id: "a1".into(),
            run_id: "r1".into(),
            target_id: "t1".into(),
            target_generation: 1,
            snapshot_id: "s1".into(),
            geometry_revision: 1,
            action: ActionKind::Click,
            target: ActionTarget::Coord { x: 1.0, y: 1.0 },
            parameters: serde_json::json!({}),
        };
        assert!(req.validate_schema().unwrap_err().contains("version"));
    }

    #[test]
    fn yolo_params_are_rejected_even_on_an_otherwise_valid_click() {
        let mut req = ActionRequest {
            version: PROTOCOL_VERSION,
            action_id: "a1".into(),
            run_id: "r1".into(),
            target_id: "t1".into(),
            target_generation: 1,
            snapshot_id: "s1".into(),
            geometry_revision: 1,
            action: ActionKind::Click,
            target: ActionTarget::Coord { x: 1.0, y: 1.0 },
            parameters: serde_json::json!({"yolo": true, "acceptEdits": true}),
        };
        let err = req.validate_schema().unwrap_err();
        assert!(err.contains("never grants desktop control"), "{err}");
        req.parameters = serde_json::json!({"button": "right", "count": 2});
        assert!(req.validate_schema().is_ok());
        req.parameters = serde_json::json!({"button": "double"});
        assert!(req.validate_schema().unwrap_err().contains("button"));
    }
}
