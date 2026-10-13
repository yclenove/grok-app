//! Architecture examples establish the stable result and capability wire shapes.
#![allow(clippy::unwrap_used)]
use gcu_protocol::{
    capabilities::Capabilities,
    error::ToolError,
    result::{ActionResult, Delivery, Effect, Outcome},
    types::AppState,
};
use serde_json::{json, Value};

fn action() -> Value {
    json!({
        "tool":"click", "target":{"element_index":22,"label":"发送"},
        "delivery":"accessibility", "outcome":"done", "effect":"changed", "verify":null,
        "duration_ms":182, "settle":{"ms":240,"reason":"tree_stable"},
        "notes":["Window was not frontmost; delivered without activation."]
    })
}
fn state() -> Value {
    json!({
        "app":{"id":"com.example.fixture","name":"Fixture","pid":1432},
        "window":{"id":"8812","title":"Fixture","frame":{"x":0.0,"y":0.0,"w":1280.0,"h":860.0},"scale":2.0},
        "snapshot_id":"s_7f3a", "screenshot":{"id":"i_92c1","width":1280,"height":860,"mime":"image/png"},
        "focused":21, "truncated":{"omitted_nodes":143}, "action":null
    })
}
#[test]
fn action_architecture_example_roundtrips_with_verification_and_coordinate_target() {
    for target in [
        json!({"element_index":22,"label":"发送"}),
        json!({"x":840.0,"y":812.0,"screenshot_id":"i_92c1"}),
        Value::Null,
    ] {
        for verify in [Value::Null, json!({"expect":"saved","state":"satisfied"})] {
            let mut value = action();
            value["target"] = target.clone();
            value["verify"] = verify;
            let result: ActionResult = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(result).unwrap(), value);
        }
    }
}
#[test]
fn result_enum_wire_semantics_preserve_unknown_and_no_delivery() {
    for (outcome, wire) in [
        (Outcome::Done, "done"),
        (Outcome::Refused, "refused"),
        (Outcome::Failed, "failed"),
        (Outcome::Unknown, "unknown"),
    ] {
        assert_eq!(serde_json::to_value(outcome).unwrap(), wire);
    }
    for (delivery, wire) in [
        (Delivery::Accessibility, "accessibility"),
        (Delivery::TargetedEvent, "targeted_event"),
        (Delivery::Foreground, "foreground"),
        (Delivery::Clipboard, "clipboard"),
        (Delivery::None, "none"),
    ] {
        assert_eq!(serde_json::to_value(delivery).unwrap(), wire);
    }
    assert_eq!(
        serde_json::to_value(Effect::NoVisibleChange).unwrap(),
        "no_visible_change"
    );
}
#[test]
fn observation_architecture_example_omits_elements_until_full_detail() {
    let value = state();
    let observation: AppState = serde_json::from_value(value.clone()).unwrap();
    assert!(observation.elements.is_none());
    assert_eq!(serde_json::to_value(observation).unwrap(), value);
    let mut value = state();
    value["elements"] = json!([{"index":21,"role":"text_area","name":null,"states":["settable","focused"],"actions":[],"frame":null}]);
    value["action"] = action();
    let observation: AppState = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(observation).unwrap(), value);
}
#[test]
fn stale_error_fresh_state_roundtrips_and_text_header_is_stable() {
    let value = json!({"code":"stale_element","message":"Element changed.","hint":"Use the fresh state.","retryable":true,"state":state()});
    let error: ToolError = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(error.to_string(), "error: stale_element: Element changed.");
    assert_eq!(serde_json::to_value(error).unwrap(), value);
    let value = json!({"code":"driver_error","message":"Delivery uncertain.","hint":"Do not replay.","retryable":false});
    let error: ToolError = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(error).unwrap(), value);
}
#[test]
fn capabilities_architecture_example_preserves_runtime_declarations() {
    let value = json!({
        "platform":"macos", "version":"15.1", "observe":{"tree":true,"screenshot":true,"events":true},
        "delivery":{"accessibility":true,"targeted_pointer":true,"targeted_keys":true,"foreground":"policy"},
        "background_operation":"full", "user_activity_guard":true, "overlay_excluded_from_capture":true,
        "private_apis":{"skylight":"available"}, "limits":{"max_tree_nodes":5000,"max_image_edge":2560,"type_text_max":8192}
    });
    let capabilities: Capabilities = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(capabilities).unwrap(), value);
}
#[test]
fn action_mixed_targets_unknown_outcomes_and_extra_fields_are_rejected() {
    let mut value = action();
    value["target"]["x"] = json!(2);
    assert!(serde_json::from_value::<ActionResult>(value).is_err());
    let mut value = action();
    value["outcome"] = json!("success");
    assert!(serde_json::from_value::<ActionResult>(value).is_err());
    let mut value = state();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AppState>(value).is_err());
}
