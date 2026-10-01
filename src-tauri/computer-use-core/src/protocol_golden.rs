//! Cross-language golden contract tests. Fail if Rust drifts from catalog.json.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::protocol::{
    ActionRequest, JS_MAX_SAFE_INTEGER, KEY_MAX_LEN, MODEL_TRACE_CAP, OBSERVATION_ARIA_CHARS,
    OBSERVATION_CANDIDATE_CAP, OBSERVATION_JSON_BYTES, OBSERVATION_NAME_CHARS,
    OBSERVATION_NODE_CAP, OBSERVATION_PNG_B64_CAP, OBSERVATION_ROLE_CHARS, OBSERVATION_TITLE_CHARS,
    OBSERVATION_URL_CHARS, PROTOCOL_VERSION, TEXT_MAX_CHARS,
};

fn golden_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/computer-use-protocol/golden")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap_or_else(|e| panic!("{path:?}: {e}")))
        .unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

fn catalog() -> Value {
    read_json(&golden_root().join("catalog.json"))
}

fn reject_action(body: &Value) -> String {
    match serde_json::from_value::<ActionRequest>(body.clone()) {
        Err(e) => e.to_string(),
        Ok(req) => req
            .validate_schema()
            .err()
            .unwrap_or_else(|| "accepted invalid action".into()),
    }
}

#[test]
fn catalog_matches_rust_constants() {
    let c = catalog();
    assert_eq!(c["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(c["observationNodeCap"], OBSERVATION_NODE_CAP);
    assert_eq!(c["observationPngB64Cap"], OBSERVATION_PNG_B64_CAP);
    assert_eq!(c["observationCandidateCap"], OBSERVATION_CANDIDATE_CAP);
    assert_eq!(c["observationRoleChars"], OBSERVATION_ROLE_CHARS);
    assert_eq!(c["observationNameChars"], OBSERVATION_NAME_CHARS);
    assert_eq!(c["observationTitleChars"], OBSERVATION_TITLE_CHARS);
    assert_eq!(c["observationUrlChars"], OBSERVATION_URL_CHARS);
    assert_eq!(c["observationAriaChars"], OBSERVATION_ARIA_CHARS);
    assert_eq!(c["observationJsonBytes"], OBSERVATION_JSON_BYTES);
    assert_eq!(c["jsMaxSafeInteger"], JS_MAX_SAFE_INTEGER);
    assert_eq!(c["modelTraceCap"], MODEL_TRACE_CAP);
    assert_eq!(c["textMaxChars"], TEXT_MAX_CHARS);
    assert_eq!(c["keyMaxLen"], KEY_MAX_LEN);
    let action_kinds: Vec<String> = c["actionKinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        action_kinds,
        [
            "click",
            "set_value",
            "type_text",
            "key",
            "scroll",
            "drag",
            "wait"
        ]
    );
    let worker_kinds: Vec<String> = c["workerActionKinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        worker_kinds,
        [
            "click",
            "set_value",
            "type_text",
            "select",
            "key",
            "scroll",
            "drag",
            "wait"
        ]
    );
    let tools: Vec<String> = c["modelTools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        tools.iter().map(String::as_str).collect::<Vec<_>>(),
        crate::tools::MODEL_TOOLS
    );
    let host_only: Vec<String> = c["hostOnlyTools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        host_only.iter().map(String::as_str).collect::<Vec<_>>(),
        crate::tools::HOST_ONLY_TOOLS
    );
    let codes: Vec<String> = c["errorCodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        codes,
        [
            "feature_disabled",
            "run_not_found",
            "target_unauthorized",
            "identity_mismatch",
            "dead_target",
            "stop_requested",
            "lease_held",
            "duplicate_action",
            "schema",
            "adapter",
            "timeout",
        ]
    );
    for code in &codes {
        assert!(
            crate::error::ERROR_CODES.contains(&code.as_str()),
            "missing {code}"
        );
    }
}

#[test]
fn golden_valid_actions_parse_and_validate() {
    let dir = golden_root().join("actions/valid");
    let mut n = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        n += 1;
        let body = read_json(&path);
        let req: ActionRequest = serde_json::from_value(body).unwrap_or_else(|e| {
            panic!("{}: {e}", path.display());
        });
        req.validate_schema()
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
    assert!(n >= 7, "expected golden valid actions, got {n}");
}

#[test]
fn golden_drag_destinations_are_complete_and_unambiguous() {
    let rows = read_json(&golden_root().join("drag-destinations.json"));
    for target in [
        serde_json::json!({"elementRef":"source-ref"}),
        serde_json::json!({"x":1,"y":1}),
    ] {
        for row in rows.as_array().unwrap() {
            let body = serde_json::json!({
                "version":1,"actionId":"drag-contract","runId":"r","targetId":"t",
                "targetGeneration":1,"snapshotId":"s","geometryRevision":1,
                "action":"drag","target":target,"parameters":row["parameters"]
            });
            let req: ActionRequest = serde_json::from_value(body).unwrap();
            assert_eq!(
                req.validate_schema().is_ok(),
                row["valid"].as_bool().unwrap(),
                "{} / {target}",
                row["name"]
            );
        }
    }
}

#[test]
fn golden_invalid_actions_fail_closed() {
    let dir = golden_root().join("actions/invalid");
    let mut n = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        n += 1;
        let fixture = read_json(&path);
        let needle = fixture["errorContains"].as_str().unwrap();
        let err = reject_action(&fixture["body"]);
        assert!(
            err.to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase()),
            "{}: expected {needle:?} in {err}",
            path.display()
        );
    }
    assert!(n >= 9, "expected golden invalid actions, got {n}");
}

#[test]
fn extra_field_and_nonfinite_coords_fail_closed() {
    let mut valid = read_json(&golden_root().join("actions/valid/click-element.json"));
    valid
        .as_object_mut()
        .unwrap()
        .insert("unknownField".into(), Value::Bool(true));
    let err = reject_action(&valid);
    assert!(
        err.to_ascii_lowercase().contains("unknown"),
        "extra field: {err}"
    );

    let req = ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "a1".into(),
        run_id: "r1".into(),
        target_id: "t1".into(),
        target_generation: 1,
        snapshot_id: "s1".into(),
        geometry_revision: 1,
        action: crate::protocol::ActionKind::Click,
        target: crate::protocol::ActionTarget::Coord {
            x: f64::NAN,
            y: 1.0,
        },
        parameters: serde_json::json!({}),
    };
    assert!(
        req.validate_schema().unwrap_err().contains("finite")
            || req.validate_schema().unwrap_err().contains("nonnegative")
    );
    let inf = ActionRequest {
        target: crate::protocol::ActionTarget::Coord {
            x: f64::INFINITY,
            y: 1.0,
        },
        ..req
    };
    assert!(inf.validate_schema().is_err());
}

#[test]
fn golden_illegal_urls_never_pass() {
    let rows = read_json(&golden_root().join("urls/invalid.json"));
    for row in rows.as_array().unwrap() {
        let url = row["url"].as_str().unwrap();
        let needle = row["errorContains"].as_str().unwrap();
        let err = crate::browser::is_allowed_navigate_url(url)
            .err()
            .unwrap_or_else(|| panic!("{url} was accepted"));
        let text = err.to_string();
        assert!(
            text.to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase()),
            "{url}: expected {needle:?} in {text}"
        );
    }
    crate::browser::is_allowed_navigate_url("https://example.com/a").unwrap();
}

#[test]
fn model_observe_json_omits_png_bytes() {
    use crate::broker::{BrokerOptions, ComputerUseBroker};
    use crate::fake::FakeAdapter;
    use crate::ipc::SessionBinding;
    use std::sync::Arc;
    use std::time::Duration;

    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    let broker = ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir()
                .join(format!("cu-golden-obs-{}.lease", uuid::Uuid::new_v4())),
            action_timeout: Duration::from_secs(2),
            ..BrokerOptions::default()
        },
    );
    broker.open_run("s", "r").unwrap();
    broker.authorize_target("r", &fake.fixture_id()).unwrap();
    let result = crate::tools::dispatch(
        &broker,
        &SessionBinding {
            session_id: "s".into(),
            run_id: "r".into(),
        },
        "computer_observe",
        serde_json::json!({}),
    );
    assert_eq!(result["isError"], false);
    let content = result["content"].as_array().unwrap();
    assert!(content.iter().any(|p| p["type"] == "image"));
    let text = content.iter().find(|p| p["type"] == "text").unwrap()["text"]
        .as_str()
        .unwrap();
    assert!(
        !text.contains("pngBase64") && !text.contains("png_base64"),
        "model JSON text must not repeat png bytes: {text}"
    );
}

#[test]
fn host_only_tools_are_not_on_the_model_surface() {
    let c = catalog();
    for name in c["hostOnlyTools"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        assert!(
            !crate::tools::MODEL_TOOLS.contains(&name),
            "{name} leaked into model tools"
        );
    }
}
