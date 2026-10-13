//! Wire-contract tests independent of any driver.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_protocol::{error::ErrorCode, tools::*, tools_list_json};
use serde_json::{json, Value};

#[test]
fn tools_list_default_surface_has_all_names_and_annotations() {
    let tools = tools_list_json();
    let tools = tools.as_array().unwrap();
    assert_eq!(tools.len(), 16);
    for (tool, name) in tools.iter().zip(ToolName::ALL) {
        assert_eq!(tool["name"], serde_json::to_value(name).unwrap());
        assert!(tool["description"].as_str().unwrap().len() <= 200);
        assert_eq!(tool["annotations"]["readOnlyHint"], name.is_read_only());
        assert_eq!(tool["annotations"]["destructiveHint"], !name.is_read_only());
        assert_eq!(tool["annotations"]["idempotentHint"], name.is_read_only());
        assert_eq!(tool["annotations"]["openWorldHint"], true);
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    }
    assert!(serde_json::to_vec(&tools_list_json()).unwrap().len() <= 14_000);
}

#[test]
fn tool_params_unknown_fields_are_rejected() {
    macro_rules! reject {
        ($ty:ty, $value:expr) => {{
            let mut value = $value;
            let _: $ty = serde_json::from_value(value.clone()).unwrap();
            value["unexpected"] = json!(true);
            assert!(
                serde_json::from_value::<$ty>(value).is_err(),
                stringify!($ty)
            );
        }};
    }
    reject!(ListAppsParams, json!({}));
    reject!(ListWindowsParams, json!({}));
    reject!(LaunchAppParams, json!({"app":"Fixture"}));
    reject!(GetAppStateParams, json!({"app":"Fixture"}));
    reject!(ClickParams, json!({"app":"Fixture","element_index":1}));
    reject!(
        PerformSecondaryActionParams,
        json!({"app":"Fixture","element_index":1,"action":"Press"})
    );
    reject!(
        SetValueParams,
        json!({"app":"Fixture","element_index":1,"value":"x"})
    );
    reject!(
        SelectTextParams,
        json!({"app":"Fixture","element_index":1,"text":"x"})
    );
    reject!(TypeTextParams, json!({"app":"Fixture","text":"x"}));
    reject!(PressKeyParams, json!({"app":"Fixture","key":"ctrl+a"}));
    reject!(
        ScrollParams,
        json!({"app":"Fixture","element_index":1,"direction":"down"})
    );
    reject!(
        DragParams,
        json!({"app":"Fixture","from_x":0,"from_y":0,"to_x":1,"to_y":1})
    );
    reject!(
        WaitForParams,
        json!({"app":"Fixture","until":{"stable":true}})
    );
    reject!(ComputerBatchParams, json!({"app":"Fixture","steps":[]}));
    reject!(ComputerScriptParams, json!({"code":"return 1;"}));
    reject!(CapabilitiesParams, json!({}));
}

#[test]
fn batch_architecture_example_roundtrips_and_validates() {
    let value = json!({"app":"TextEdit","steps":[
        {"click":{"element_index":5}},
        {"type_text":{"text":"季度总结"}},
        {"press_key":{"key":"cmd+s"}},
        {"wait_for":{"until":{"element":{"role":"sheet"}},"timeout_ms":3000}},
        {"expect":{"text_present":"存储为"}}
    ],"stop_on_error":true,"final_state":true});
    let batch: ComputerBatchParams = serde_json::from_value(value.clone()).unwrap();
    assert!(batch.validate().is_ok());
    assert_eq!(serde_json::to_value(batch).unwrap(), value);
}

#[test]
fn click_and_selection_alternatives_validate_exclusive_targets() {
    for (value, valid) in [
        (json!({"element_index":1}), true),
        (json!({"x":1,"y":2}), true),
        (json!({}), false),
        (json!({"x":1}), false),
        (json!({"element_index":1,"x":1,"y":2}), false),
        (json!({"element_index":1,"screenshot_id":"i_1"}), false),
    ] {
        let args: ClickArgs = serde_json::from_value(value).unwrap();
        assert_eq!(args.validate().is_ok(), valid);
    }
    for (value, valid) in [
        (json!({"element_index":1,"text":"hello"}), true),
        (json!({"element_index":1,"start":0,"length":3}), true),
        (json!({"element_index":1}), false),
        (json!({"element_index":1,"start":0}), false),
        (
            json!({"element_index":1,"text":"a","start":0,"length":1}),
            false,
        ),
    ] {
        let args: SelectTextArgs = serde_json::from_value(value).unwrap();
        assert_eq!(args.validate().is_ok(), valid);
    }
}

#[test]
fn defaults_follow_architecture_and_required_app_is_enforced() {
    let state: GetAppStateParams = serde_json::from_value(json!({"app":"Fixture"})).unwrap();
    assert!(state.include_screenshot);
    assert_eq!(state.text_limit, TextLimit::Characters(500));
    assert_eq!(state.max_tree_nodes, 1200);
    assert!(serde_json::from_value::<ClickParams>(json!({"element_index":1})).is_err());
    let click: ClickParams =
        serde_json::from_value(json!({"app":"Fixture","element_index":1})).unwrap();
    assert_eq!(click.args.click_count, 1);
    assert_eq!(click.args.mouse_button, MouseButton::Left);
}

#[test]
fn batch_invalid_step_and_resource_limits_fail_preflight() {
    for steps in [
        json!([]),
        json!([{"press_key":{"key":"bad_key"}}]),
        json!([{"click":{"x":1}}]),
        json!([{"wait_for":{"until":{"stable":true},"timeout_ms":30001}}]),
        Value::Array(vec![json!({"type_text":{"text":"x"}}); 33]),
    ] {
        let batch: ComputerBatchParams =
            serde_json::from_value(json!({"app":"Fixture","steps":steps})).unwrap();
        assert!(batch.validate().is_err());
    }
    for value in [
        json!({"click":{"element_index":1,"extra":true}}),
        json!({"click":{"element_index":1},"type_text":{"text":"x"}}),
        json!({"launch_app":{"app":"Fixture"}}),
        json!({"expect":{"unknown":true}}),
    ] {
        assert!(serde_json::from_value::<BatchStep>(value).is_err());
    }
    let script: ComputerScriptParams =
        serde_json::from_value(json!({"code":"中".repeat(7000)})).unwrap();
    assert!(script.validate().is_err());
}

#[test]
fn errors_all_wire_codes_match_architecture_and_roundtrip() {
    let expected = "app_not_found ambiguous_app app_denied approval_denied approval_timeout window_not_found window_gone stale_element stale_geometry stale_screenshot out_of_bounds element_not_actionable not_settable invalid_key key_blocked user_active stopped_by_user busy handoff_required elevation_required permission_missing capability_unavailable timeout driver_error internal";
    assert_eq!(ErrorCode::ALL.len(), 25);
    for (code, expected) in ErrorCode::ALL.iter().zip(expected.split_whitespace()) {
        assert_eq!(serde_json::to_value(code).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<ErrorCode>(json!(expected)).unwrap(),
            *code
        );
        assert_eq!(code.to_string(), expected);
    }
    assert!(serde_json::from_value::<ErrorCode>(json!("future_code")).is_err());
}

#[test]
fn generated_schema_preserves_target_alternatives_defaults_and_references() {
    let tools = tools_list_json();
    let click = &tools[4]["inputSchema"];
    assert_eq!(click["properties"]["click_count"]["default"], 1);
    assert_eq!(click["properties"]["mouse_button"]["default"], "left");
    assert_eq!(click["properties"]["element_index"]["type"], "integer");
    assert_eq!(click["properties"]["x"]["type"], "number");
    assert_eq!(click["properties"]["y"]["type"], "number");
    assert_eq!(click["oneOf"][0]["required"], json!(["element_index"]));
    assert_eq!(click["oneOf"][1]["required"], json!(["x", "y"]));
    assert_eq!(
        tools[7]["inputSchema"]["oneOf"][1]["required"],
        json!(["start", "length"])
    );
    assert_eq!(
        tools[10]["inputSchema"]["properties"]["pages"]["exclusiveMinimum"],
        0
    );
    assert_eq!(
        tools[12]["inputSchema"]["properties"]["timeout_ms"]["default"],
        5000
    );
    assert_eq!(
        tools[12]["inputSchema"]["properties"]["timeout_ms"]["maximum"],
        30000
    );
    assert_eq!(
        tools[13]["inputSchema"]["properties"]["steps"]["maxItems"],
        32
    );
    fn check_refs(value: &Value, root: &Value) {
        match value {
            Value::Object(map) => {
                if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
                    assert!(
                        root.pointer(reference.strip_prefix('#').unwrap()).is_some(),
                        "{reference}"
                    );
                }
                for child in map.values() {
                    check_refs(child, root);
                }
            }
            Value::Array(values) => {
                for child in values {
                    check_refs(child, root);
                }
            }
            _ => (),
        }
    }
    for tool in tools.as_array().unwrap() {
        check_refs(&tool["inputSchema"], &tool["inputSchema"]);
    }
}

#[test]
fn action_preflight_numeric_and_timeout_boundaries_are_enforced() {
    for (pages, valid) in [
        (0.0, false),
        (-1.0, false),
        (0.5, true),
        (f64::INFINITY, false),
        (f64::NAN, false),
    ] {
        let args = ScrollArgs {
            element_index: Some(1),
            x: None,
            y: None,
            direction: Direction::Down,
            pages,
        };
        assert_eq!(args.validate().is_ok(), valid);
    }
    for (count, valid) in [(0, false), (1, true), (2, true), (255, true)] {
        let mut click: ClickArgs = serde_json::from_value(json!({"element_index":1})).unwrap();
        click.click_count = count;
        assert_eq!(click.validate().is_ok(), valid);
    }
    for (timeout_ms, valid) in [
        (0, false),
        (1, true),
        (5000, true),
        (30000, true),
        (30001, false),
    ] {
        let wait = WaitForArgs {
            until: Condition::Stable(true),
            timeout_ms,
        };
        assert_eq!(wait.validate().is_ok(), valid);
        let script = ComputerScriptParams {
            code: "return 1;".into(),
            timeout_ms,
        };
        assert_eq!(script.validate().is_ok(), valid);
    }
    assert!(Condition::Stable(false).validate().is_err());
    let mut drag: DragArgs =
        serde_json::from_value(json!({"from_x":0,"from_y":0,"to_x":1,"to_y":1})).unwrap();
    assert!(drag.validate().is_ok());
    drag.to_y = f64::NAN;
    assert!(drag.validate().is_err());
    for (bytes, valid) in [(20000, true), (20001, false)] {
        let script = ComputerScriptParams {
            code: "x".repeat(bytes),
            timeout_ms: 30000,
        };
        assert_eq!(script.validate().is_ok(), valid);
    }
}
