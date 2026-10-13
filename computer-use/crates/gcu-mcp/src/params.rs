use gcu_core::action::ToolCall;
use gcu_protocol::*;
use serde::de::DeserializeOwned;
use serde_json::Value;
pub(crate) fn parse<T: DeserializeOwned>(value: Value) -> Result<T, Box<ToolError>> {
    serde_json::from_value(value).map_err(|_| {
        Box::new(invalid(
            "Invalid arguments; check the registered inputSchema.",
        ))
    })
}
pub(crate) fn invalid(message: &str) -> ToolError {
    ToolError {
        code: ErrorCode::CapabilityUnavailable,
        message: message.into(),
        hint: "Correct the arguments before calling again.".into(),
        retryable: false,
        state: None,
    }
}
pub(crate) fn action(name: &str, value: Value, id: String) -> Result<ToolCall, Box<ToolError>> {
    macro_rules! action {
        ($ty:ty, $variant:ident) => {{
            let p: $ty = parse(value)?;
            let mut call = ToolCall::new(id, p.app, BatchStep::$variant(p.args));
            call.window = p.window;
            call.action.validate().map_err(|e| {
                let mut error = invalid(e.0);
                if matches!(call.action, BatchStep::PressKey(_)) {
                    error.code = ErrorCode::InvalidKey;
                }
                Box::new(error)
            })?;
            Ok(call)
        }};
    }
    match name {
        "click" => action!(ClickParams, Click),
        "perform_secondary_action" => action!(PerformSecondaryActionParams, PerformSecondaryAction),
        "set_value" => action!(SetValueParams, SetValue),
        "select_text" => action!(SelectTextParams, SelectText),
        "type_text" => action!(TypeTextParams, TypeText),
        "press_key" => action!(PressKeyParams, PressKey),
        "scroll" => action!(ScrollParams, Scroll),
        "drag" => action!(DragParams, Drag),
        _ => Err(Box::new(invalid(
            "This tool will be available in a later release.",
        ))),
    }
}
