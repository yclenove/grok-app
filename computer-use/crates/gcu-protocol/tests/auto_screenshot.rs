//! The auto extension preserves boolean callers and the true default (ADR 0011).
#![allow(clippy::unwrap_used)]
use gcu_protocol::{tools::*, tools_list_json};
use serde_json::json;

#[test]
fn screenshot_mode_boolean_and_auto_roundtrip_preserves_input_shape() {
    for input in [json!(true), json!(false), json!("auto")] {
        let args: GetAppStateParams =
            serde_json::from_value(json!({"app":"Fixture","include_screenshot":input})).unwrap();
        assert_eq!(
            serde_json::to_value(args).unwrap()["include_screenshot"],
            input
        );
    }
    let args: GetAppStateParams = serde_json::from_value(json!({"app":"Fixture"})).unwrap();
    assert_eq!(args.include_screenshot, ScreenshotMode::Explicit(true));
    assert_eq!(
        serde_json::to_value(args).unwrap()["include_screenshot"],
        true
    );
}

#[test]
fn screenshot_mode_unknown_or_non_scalar_input_is_rejected() {
    for input in [
        json!("true"),
        json!("AUTO"),
        json!("always"),
        json!(null),
        json!(0),
        json!({}),
        json!([]),
    ] {
        assert!(serde_json::from_value::<GetAppStateParams>(
            json!({"app":"Fixture","include_screenshot":input})
        )
        .is_err());
    }
}

#[test]
fn screenshot_schema_inline_boolean_or_auto_keeps_true_default_and_golden() {
    let tools = tools_list_json();
    let schema = &tools[3]["inputSchema"]["properties"]["include_screenshot"];
    assert_eq!(
        schema,
        &json!({
            "anyOf":[{"type":"boolean"},{"type":"string","enum":["auto"]}],
            "default":true
        })
    );
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../golden/tools.json")).unwrap();
    assert_eq!(tools, golden);
    assert!(serde_json::to_vec(&tools).unwrap().len() <= 14_000);
}
