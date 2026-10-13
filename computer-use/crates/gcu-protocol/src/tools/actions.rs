//! Reusable action payloads; full tools add app/window, batches inherit them.

use super::{Condition, ParamError};
use crate::keys::KeyChord;
use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Serialize};
use serde_json::json;

fn one() -> u8 {
    1
}
fn one_page() -> f64 {
    1.0
}
fn wait_timeout() -> u32 {
    5000
}

/// Mouse button for pointer clicks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    /// Primary button.
    #[default]
    Left,
    /// Secondary button.
    Right,
    /// Middle button.
    Middle,
}

/// Scroll direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Up.
    Up,
    /// Down.
    Down,
    /// Left.
    Left,
    /// Right.
    Right,
}

fn click_schema(schema: &mut Schema) {
    schema.insert("oneOf".into(), json!([
        {"required":["element_index"],"not":{"anyOf":[{"required":["x"]},{"required":["y"]},{"required":["screenshot_id"]}]}},
        {"required":["x","y"],"not":{"required":["element_index"]}}
    ]));
}

fn scroll_schema(schema: &mut Schema) {
    if let Some(pages) = schema
        .get_mut("properties")
        .and_then(|properties| properties.get_mut("pages"))
    {
        if let Some(object) = pages.as_object_mut() {
            object.remove("minimum");
            object.insert("exclusiveMinimum".into(), json!(0));
        }
    }
    schema.insert(
        "oneOf".into(),
        json!([
            {"required":["element_index"],"not":{"anyOf":[{"required":["x"]},{"required":["y"]}]}},
            {"required":["x","y"],"not":{"required":["element_index"]}}
        ]),
    );
}

fn selection_schema(schema: &mut Schema) {
    schema.insert(
        "oneOf".into(),
        json!([
            {"required":["text"],"not":{"anyOf":[{"required":["start"]},{"required":["length"]}]}},
            {"required":["start","length"],"not":{"required":["text"]}}
        ]),
    );
}

/// Indexed element or screenshot-pixel click payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = click_schema)]
pub struct ClickArgs {
    /// Accessibility element, exclusive with coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32")]
    pub element_index: Option<u32>,
    /// Horizontal screenshot pixel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub x: Option<f64>,
    /// Vertical screenshot pixel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub y: Option<f64>,
    /// Screenshot ID, defaulting to the latest for this window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_id: Option<String>,
    /// Number of clicks, default 1.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    #[schemars(range(min = 1), extend("default" = 1))]
    pub click_count: u8,
    /// Button, default left.
    #[serde(default, skip_serializing_if = "is_left")]
    #[schemars(extend("default" = "left"))]
    pub mouse_button: MouseButton,
}
fn is_one(n: &u8) -> bool {
    *n == 1
}
fn is_left(b: &MouseButton) -> bool {
    *b == MouseButton::Left
}

impl ClickArgs {
    /// Check exclusive targeting and click count synchronously; no IO or waits.
    pub fn validate(&self) -> Result<(), ParamError> {
        validate_target(self.element_index, self.x, self.y)?;
        if self.element_index.is_some() && self.screenshot_id.is_some() {
            return Err(ParamError("screenshot_id requires coordinates"));
        }
        if self.click_count == 0 {
            return Err(ParamError("click_count must be positive"));
        }
        Ok(())
    }
}

pub(super) fn validate_target(
    index: Option<u32>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<(), ParamError> {
    match (index, x, y) {
        (Some(_), None, None) => Ok(()),
        (None, Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok(()),
        _ => Err(ParamError(
            "provide element_index or both finite x and y, exclusively",
        )),
    }
}

/// Perform an advertised secondary action on one element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PerformSecondaryActionArgs {
    /// Indexed element.
    pub element_index: u32,
    /// Advertised action name.
    pub action: String,
}

/// Replace an element's settable value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetValueArgs {
    /// Indexed element.
    pub element_index: u32,
    /// Replacement value, including an empty string to clear it.
    pub value: String,
}

/// Select literal text or a character range, exclusively.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = selection_schema)]
pub struct SelectTextArgs {
    /// Indexed text element.
    pub element_index: u32,
    /// Literal text to select.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub text: Option<String>,
    /// Zero-based character offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32")]
    pub start: Option<u32>,
    /// Number of characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32")]
    pub length: Option<u32>,
}

impl SelectTextArgs {
    /// Check exclusive text/range selection with no IO, timeout, or cancellation.
    pub fn validate(&self) -> Result<(), ParamError> {
        match (&self.text, self.start, self.length) {
            (Some(_), None, None) | (None, Some(_), Some(_)) => Ok(()),
            _ => Err(ParamError(
                "provide text or both start and length, exclusively",
            )),
        }
    }
}

/// Type text into current focus, optionally focusing an indexed element first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeTextArgs {
    /// Text to type.
    pub text: String,
    /// Element to focus first, if specified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_index: Option<u32>,
}

/// Press one xdotool-style key chord.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PressKeyArgs {
    /// Chord, such as ctrl+a or cmd+shift+s.
    pub key: String,
}
impl PressKeyArgs {
    /// Parse the chord synchronously; no IO, timeout, or cancellation occurs.
    pub fn validate(&self) -> Result<(), crate::keys::KeyParseError> {
        KeyChord::parse(&self.key).map(|_| ())
    }
}

/// Scroll an indexed element or a screenshot point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = scroll_schema)]
pub struct ScrollArgs {
    /// Accessibility element, exclusive with coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32")]
    pub element_index: Option<u32>,
    /// Horizontal screenshot pixel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub x: Option<f64>,
    /// Vertical screenshot pixel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub y: Option<f64>,
    /// Scroll direction.
    pub direction: Direction,
    /// Page count, default 1.
    #[serde(default = "one_page", skip_serializing_if = "is_one_page")]
    #[schemars(extend("default" = 1))]
    pub pages: f64,
}
fn is_one_page(n: &f64) -> bool {
    *n == 1.0
}
impl ScrollArgs {
    /// Check target and positive page count synchronously, without IO or waits.
    pub fn validate(&self) -> Result<(), ParamError> {
        validate_target(self.element_index, self.x, self.y)?;
        if !self.pages.is_finite() || self.pages <= 0.0 {
            return Err(ParamError("pages must be positive and finite"));
        }
        Ok(())
    }
}

/// Drag between screenshot-pixel points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DragArgs {
    /// Start horizontal pixel.
    pub from_x: f64,
    /// Start vertical pixel.
    pub from_y: f64,
    /// End horizontal pixel.
    pub to_x: f64,
    /// End vertical pixel.
    pub to_y: f64,
    /// Screenshot ID, defaulting to the latest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_id: Option<String>,
}
impl DragArgs {
    /// Check finite coordinates synchronously; freshness and bounds belong to core.
    pub fn validate(&self) -> Result<(), ParamError> {
        if [self.from_x, self.from_y, self.to_x, self.to_y]
            .iter()
            .all(|n| n.is_finite())
        {
            Ok(())
        } else {
            Err(ParamError("drag coordinates must be finite"))
        }
    }
}

/// Wait for a condition, bounded by timeout_ms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaitForArgs {
    /// Condition to observe.
    pub until: Condition,
    /// Timeout, default 5000 milliseconds, maximum 30000.
    #[serde(default = "wait_timeout", skip_serializing_if = "is_wait_timeout")]
    #[schemars(range(min = 1, max = 30000), extend("default" = 5000))]
    pub timeout_ms: u32,
}
fn is_wait_timeout(n: &u32) -> bool {
    *n == 5000
}
impl WaitForArgs {
    /// Validate the wait specification without actually blocking or observing.
    pub fn validate(&self) -> Result<(), ParamError> {
        if !(1..=30000).contains(&self.timeout_ms) {
            return Err(ParamError("timeout_ms must be 1..=30000"));
        }
        self.until.validate()
    }
}
