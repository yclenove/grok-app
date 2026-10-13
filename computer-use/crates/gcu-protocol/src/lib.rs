//! Shared computer-use tool data, errors, capabilities, and protocol versions.
//! All operations are synchronous pure data transformations: no IO, waits, or cancellation.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod capabilities;
pub mod error;
pub mod keys;
pub mod result;
pub mod tools;
pub mod types;

pub use capabilities::Capabilities;
pub use error::{ErrorCode, ToolError};
pub use keys::{KeyChord, KeyParseError};
pub use result::ActionResult;
pub use tools::*;
pub use types::{AppInfo, AppState, Rect, ScreenshotInfo, WindowInfo};

use schemars::{generate::SchemaSettings, JsonSchema, Schema};
use serde_json::{json, Value};

fn compact_metadata(schema: &mut Schema) {
    schemars::transform::transform_subschemas(&mut compact_metadata, schema);
    if let Some(object) = schema.as_object_mut() {
        object.remove("title");
        object.remove("description");
        object.remove("$schema");
        object.remove("format");
        // A documented unit variant derives as a const schema. After removing
        // prose, same-type scalar variants can use a shorter equivalent enum.
        let scalar_enum = object
            .get("oneOf")
            .and_then(Value::as_array)
            .and_then(|variants| {
                let kind = variants.first()?.get("type")?.clone();
                let values: Option<Vec<_>> = variants
                    .iter()
                    .map(|variant| {
                        let fields = variant.as_object()?;
                        if fields.len() == 2 && fields.get("type") == Some(&kind) {
                            fields.get("const").cloned()
                        } else {
                            None
                        }
                    })
                    .collect();
                let values = values?;
                if values
                    .iter()
                    .enumerate()
                    .any(|(i, v)| values[..i].contains(v))
                {
                    return None;
                }
                Some((kind, values))
            });
        if let Some((kind, values)) = scalar_enum {
            object.remove("oneOf");
            object.insert("type".into(), kind);
            object.insert("enum".into(), Value::Array(values));
        }
    }
}

fn tool<T: JsonSchema>(name: ToolName, description: &str) -> Value {
    // Draft 7 converts flattened unevaluatedProperties to additionalProperties
    // for clients with older validators. References remain local to each tool.
    let mut schema = SchemaSettings::draft07()
        .into_generator()
        .into_root_schema_for::<T>();
    compact_metadata(&mut schema);
    json!({
        "name": name.as_str(),
        "description": description,
        "inputSchema": schema,
        "annotations": {"readOnlyHint": name.is_read_only(), "destructiveHint": !name.is_read_only(),
            "idempotentHint": name.is_read_only(), "openWorldHint": true}
    })
}

/// Generate the sixteen-entry MCP tools/list `tools` array from parameter types.
/// This function performs no IO and does not enable tools whose implementation is pending.
pub fn tools_list_json() -> Value {
    Value::Array(vec![
        tool::<ListAppsParams>(ToolName::ListApps, "List running and recently used apps."),
        tool::<ListWindowsParams>(ToolName::ListWindows, "List windows, optionally filtered by app."),
        tool::<LaunchAppParams>(ToolName::LaunchApp, "Launch an authorized app."),
        tool::<GetAppStateParams>(ToolName::GetAppState, "Read an authorized window's tree and screenshot. App accepts name, ID, or path; window defaults to key window."),
        tool::<ClickParams>(ToolName::Click, "Click an element or a point in the latest screenshot."),
        tool::<PerformSecondaryActionParams>(ToolName::PerformSecondaryAction, "Perform an element's advertised secondary action."),
        tool::<SetValueParams>(ToolName::SetValue, "Replace a settable element's value."),
        tool::<SelectTextParams>(ToolName::SelectText, "Select literal text or a character range."),
        tool::<TypeTextParams>(ToolName::TypeText, "Type text, optionally focusing an element first."),
        tool::<PressKeyParams>(ToolName::PressKey, "Press an xdotool chord such as ctrl+a or cmd+shift+s."),
        tool::<ScrollParams>(ToolName::Scroll, "Scroll an element or screenshot point by pages."),
        tool::<DragParams>(ToolName::Drag, "Drag between points in the latest screenshot."),
        tool::<WaitForParams>(ToolName::WaitFor, "Wait for text, an element, stability, or a window title."),
        tool::<ComputerBatchParams>(ToolName::ComputerBatch, "Preflight up to 32 steps, then run within 60s. Steps inherit app/window. Never replay unknown outcomes."),
        tool::<ComputerScriptParams>(ToolName::ComputerScript, "Run up to 20 KB of sandboxed code when the script feature is enabled."),
        tool::<CapabilitiesParams>(ToolName::Capabilities, "Read capabilities and limits."),
    ])
}
