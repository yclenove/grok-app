//! Shared computer-use tool data, errors, capabilities, and protocol versions.
//! All operations are synchronous pure data transformations: no IO, waits, or cancellation.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

/// Built-in browser gates. L1 stays closed until a platform proves network limits.
pub mod builtin_browser;
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
    tool_named::<T>(name.as_str(), description, name.is_read_only())
}

fn tool_named<T: JsonSchema>(name: &str, description: &str, read_only: bool) -> Value {
    // Draft 7 converts flattened unevaluatedProperties to additionalProperties
    // for clients with older validators. References remain local to each tool.
    let mut schema = SchemaSettings::draft07()
        .into_generator()
        .into_root_schema_for::<T>();
    compact_metadata(&mut schema);
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema,
        "annotations": {"readOnlyHint": read_only, "destructiveHint": !read_only,
            "idempotentHint": read_only, "openWorldHint": true}
    })
}

/// Conditional `browser_navigate` tool. It is not part of [`tools_list_json`].
///
/// The default catalog byte budget is unchanged. Callers add this object only
/// while a proven binding is visible to that session.
pub fn browser_navigate_tool_json() -> Value {
    tool_named::<builtin_browser::BrowserNavigateParams>(
        "browser_navigate",
        "Open a URL in the built-in browser.",
        false,
    )
}

/// Implemented tools plus [`browser_navigate_tool_json`].
///
/// `computer_script` stays only in [`tools_list_json`]. This catalog is the
/// byte-budget check for a session that can see the conditional tool. It does
/// not raise the 14000-byte limit and is not the default catalog.
#[must_use]
pub fn browser_conditional_catalog_json() -> Value {
    let Value::Array(mut tools) = tools_list_json() else {
        return Value::Array(vec![browser_navigate_tool_json()]);
    };
    tools.retain(|tool| tool.get("name").and_then(Value::as_str) != Some("computer_script"));
    tools.push(browser_navigate_tool_json());
    Value::Array(tools)
}

/// Generate the sixteen-entry MCP tools/list `tools` array from parameter types.
/// This function performs no IO and does not enable tools whose implementation is pending.
pub fn tools_list_json() -> Value {
    Value::Array(vec![
        tool::<ListAppsParams>(ToolName::ListApps, "List running and recently used apps."),
        tool::<ListWindowsParams>(ToolName::ListWindows, "List windows, optionally filtered by app."),
        tool::<LaunchAppParams>(ToolName::LaunchApp, "Launch an authorized app."),
        tool::<GetAppStateParams>(ToolName::GetAppState, "Read an authorized window's tree and safe screenshot. App accepts name, ID, or path; window defaults to key window. include_screenshot: true (default), false, or auto by tree quality."),
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
