use super::*;
use serde_json::json;

fn wire(action: &str, parameters: serde_json::Value) -> serde_json::Value {
    let snapshot = "12345678-1234-1234-1234-123456789abc";
    json!({"kind":"act", "snapshotId":snapshot, "elementRef":format!("{snapshot}-1"),
        "action":action, "parameters":parameters})
}

#[test]
fn action_variants_reject_unknown_null_and_unsupported_parameters() {
    for (action, parameters) in [
        ("click", json!({})),
        ("set_value", json!({"text":"你好🙂"})),
        ("type_text", json!({"text":""})),
        ("scroll", json!({"delta":-2400})),
        ("wait", json!({"nameEquals":"ready"})),
    ] {
        let original = wire(action, parameters);
        let command: ExtensionActionCommand = serde_json::from_value(original.clone()).unwrap();
        assert!(command.valid());
        for pointer in ["", "/parameters"] {
            let mut value = original.clone();
            value
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("extra".into(), json!(true));
            assert!(
                serde_json::from_value::<ExtensionActionCommand>(value).is_err(),
                "{action} {pointer}"
            );
        }
        for field in ["kind", "snapshotId", "elementRef", "action", "parameters"] {
            let mut value = original.clone();
            value[field] = json!(null);
            assert!(
                serde_json::from_value::<ExtensionActionCommand>(value).is_err(),
                "{action} {field}"
            );
        }
    }
    for (action, parameters) in [
        ("click", json!({"count":null})),
        ("click", json!({"button":null})),
        ("wait", json!({"nameEquals":"ready", "timeoutMs":null})),
        ("click", json!({"button":"right"})),
        ("scroll", json!({"delta":0.5})),
        ("key", json!({"key":"Enter"})),
    ] {
        assert!(
            serde_json::from_value::<ExtensionActionCommand>(wire(action, parameters)).is_err()
        );
    }
}

#[test]
fn action_limits_match_unicode_and_single_semantic_action_contract() {
    for (action, parameters) in [
        ("click", json!({"count":2})),
        ("scroll", json!({"delta":-2401})),
        ("set_value", json!({"text":"🙂".repeat(4001)})),
        ("type_text", json!({"text":"a\u{0000}b"})),
        ("wait", json!({"nameEquals":"🙂".repeat(129)})),
        ("wait", json!({"nameEquals":"  "})),
        ("wait", json!({"nameEquals":"ready", "timeoutMs":0})),
        ("wait", json!({"nameEquals":"ready", "timeoutMs":10001})),
    ] {
        let command: ExtensionActionCommand =
            serde_json::from_value(wire(action, parameters)).unwrap();
        assert!(!command.valid());
    }
    let command: ExtensionActionCommand =
        serde_json::from_value(wire("type_text", json!({"text":"🙂".repeat(4000)}))).unwrap();
    assert!(command.valid());
    assert!(!format!("{command:?}").contains('🙂'));
    assert!(serde_json::to_vec(&command).unwrap().len() < ACTION_PACKET_BYTES / 2);
}
