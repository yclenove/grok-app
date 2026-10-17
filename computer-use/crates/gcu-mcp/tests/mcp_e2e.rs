//! Real NDJSON protocol surface and fake-desktop end-to-end behavior.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;
use gcu_driver_fake::{faults::Fault, FakeDriver};
use gcu_host_api::ApprovalDecision;
use serde_json::json;
use support::{text, Harness};
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn initialize_catalog_matches_implemented_protocol_subset() {
    let mut h = Harness::new("form", &[]);
    let init = h.initialize(false).await;
    assert_eq!(init["serverInfo"]["name"], "grok-computer-use");
    let instructions = init["instructions"].as_str().unwrap();
    assert!(instructions.chars().count() <= 1200);
    for future in ["wait_for", "computer_batch", "computer_script"] {
        assert!(!instructions.contains(future));
    }
    assert!(init["capabilities"]["tools"].is_object());
    let result = h.request("tools/list", json!({})).await;
    let registered = result["result"]["tools"].as_array().unwrap();
    assert_eq!(registered.len(), 15);
    assert!(registered.iter().any(|tool| tool["name"] == "wait_for"));
    assert!(registered
        .iter()
        .any(|tool| tool["name"] == "computer_batch"));
    let catalog = gcu_protocol::tools_list_json();
    for tool in registered {
        assert!(catalog.as_array().unwrap().contains(tool));
    }
    assert!(serde_json::to_vec(&result["result"]).unwrap().len() < 14_000);
    assert!(h.request("ping", json!({})).await["result"].is_object());
    for name in ["wait_for", "computer_batch", "computer_script"] {
        let r = h.call(name, json!({})).await;
        assert_eq!(
            r["structuredContent"]["error"]["code"],
            "capability_unavailable"
        );
    }
    h.close().await;
}

#[tokio::test]
async fn form_observe_click_type_submit_is_verified_by_external_oracle() {
    let mut h = Harness::new("form", &[ApprovalDecision::Session]);
    h.initialize(false).await;
    let s = h.call("get_app_state", json!({"app":"fake.form"})).await;
    assert!(text(&s).contains("1 text field"), "{s}");
    for (tool, args) in [
        ("click", json!({"app":"fake.form","element_index":1})),
        ("type_text", json!({"app":"fake.form","text":"Ada"})),
        ("click", json!({"app":"fake.form","element_index":3})),
    ] {
        let r = h.call(tool, args).await;
        assert_ne!(r["isError"], true, "{r}");
        assert_eq!(r["structuredContent"]["action"]["outcome"], "done");
        assert!(r["structuredContent"].get("elements").is_none());
    }
    let oracle = h.driver.oracle().await.unwrap();
    assert_eq!(oracle.text["text"], "Ada");
    assert_eq!(oracle.presses["submit"], 1);
    h.close().await;
}

#[tokio::test]
async fn stale_and_unknown_return_fresh_state_without_replay() {
    let mut h = Harness::new("form", &[ApprovalDecision::Session]);
    h.initialize(false).await;
    h.observe().await;
    let stale = h
        .call("click", json!({"app":"fake.form","element_index":999}))
        .await;
    assert_eq!(stale["isError"], true);
    assert!(text(&stale).starts_with("error: stale_element:"));
    assert!(stale["structuredContent"]["error"]["state"]["snapshot_id"].is_string());
    h.driver
        .inject_action(Fault::Timeout {
            maybe_executed: true,
        })
        .await
        .unwrap();
    let r = h
        .call("click", json!({"app":"fake.form","element_index":3}))
        .await;
    assert_eq!(r["structuredContent"]["action"]["outcome"], "unknown");
    assert_eq!(r["structuredContent"]["error"]["retryable"], false);
    assert_eq!(h.driver.perform_calls(), 1);
    assert_eq!(h.driver.oracle().await.unwrap().presses["submit"], 1);
    h.close().await;
}

#[tokio::test]
async fn every_registered_tool_reaches_its_real_handler() {
    let mut h = Harness::driver(
        FakeDriver::contract_fixture().unwrap(),
        &[
            ApprovalDecision::Session,
            ApprovalDecision::Session,
            ApprovalDecision::Session,
        ],
    );
    h.initialize(false).await;
    for (name, args) in [
        ("list_apps", json!({})),
        ("list_windows", json!({"app":"fake.form"})),
        ("capabilities", json!({})),
        ("launch_app", json!({"app":"fake.form"})),
        ("get_app_state", json!({"app":"fake.form"})),
        (
            "wait_for",
            json!({"app":"fake.form","until":{"text_present":"Submit"}}),
        ),
        (
            "computer_batch",
            json!({"app":"fake.form","steps":[{"wait_for":{"until":{"text_present":"Submit"}}}]}),
        ),
        ("click", json!({"app":"fake.form","element_index":1})),
        (
            "set_value",
            json!({"app":"fake.form","element_index":1,"value":"hello"}),
        ),
        (
            "select_text",
            json!({"app":"fake.form","element_index":1,"start":0,"length":2}),
        ),
        ("type_text", json!({"app":"fake.form","text":"z"})),
        ("press_key", json!({"app":"fake.form","key":"ctrl+a"})),
        (
            "perform_secondary_action",
            json!({"app":"fake.form","element_index":4,"action":"Increment"}),
        ),
        (
            "scroll",
            json!({"app":"fake.list","element_index":1,"direction":"down"}),
        ),
        (
            "drag",
            json!({"app":"fake.form","from_x":30,"from_y":200,"to_x":120,"to_y":200}),
        ),
    ] {
        if name == "scroll" {
            h.call(
                "get_app_state",
                json!({"app":"fake.list","include_screenshot":false}),
            )
            .await;
        }
        if name == "drag" {
            h.call("get_app_state", json!({"app":"fake.form"})).await;
        }
        let r = h.call(name, args).await;
        assert_ne!(r["isError"], true, "{name}: {r}");
        if name == "wait_for" || name == "computer_batch" {
            assert_eq!(r["structuredContent"]["action"]["delivery"], "none");
            assert_eq!(r["structuredContent"]["action"]["outcome"], "done");
        }
        if name == "computer_batch" {
            assert_eq!(r["structuredContent"]["steps"][0]["tool"], "wait_for");
            assert_eq!(r["structuredContent"]["steps"][0]["outcome"], "done");
        }
    }
    assert_eq!(h.driver.perform_calls(), 8);
    h.close().await;
}

#[tokio::test]
async fn concurrent_calls_all_respond_and_run_serially() {
    let mut h = Harness::new("form", &[ApprovalDecision::Session]);
    h.initialize(false).await;
    h.observe().await;
    h.driver
        .inject_action(Fault::Delay(std::time::Duration::from_millis(80)))
        .await
        .unwrap();
    for (id, value) in [(100, "A"), (101, "B"), (102, "C")] {
        h.send(
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
                "name":"type_text","arguments":{"app":"fake.form","text":value}
            }}),
        )
        .await;
    }
    let mut ids = Vec::new();
    for _ in 0..3 {
        let r = h.read().await;
        assert_ne!(r["result"]["isError"], true, "{r}");
        ids.push(r["id"].as_u64().unwrap());
    }
    ids.sort();
    assert_eq!(ids, [100, 101, 102]);
    assert_eq!(h.driver.oracle().await.unwrap().text["text"], "ABC");
    h.close().await;
}

#[tokio::test]
async fn malformed_lines_recover_and_unknown_fields_never_execute() {
    let mut h = Harness::new("form", &[]);
    h.initialize(false).await;
    for bad in [
        b"\n".to_vec(),
        b"not json\n".to_vec(),
        vec![b'x'; 1_048_577],
        vec![0xf0, 0x9f, b'\n'],
    ] {
        let mut bad = bad;
        if !bad.ends_with(b"\n") {
            bad.push(b'\n');
        }
        h.writer.write_all(&bad).await.unwrap();
        assert!(h.read().await["error"]["code"].is_number());
        assert!(h.request("ping", json!({})).await["result"].is_object());
    }
    let r = h
        .call(
            "click",
            json!({"app":"fake.form","element_index":3,"extra":true}),
        )
        .await;
    assert_eq!(r["isError"], true);
    assert_eq!(h.driver.perform_calls(), 0);
    let r = h
        .call("press_key", json!({"app":"fake.form","key":"not-a-key"}))
        .await;
    assert_eq!(r["structuredContent"]["error"]["code"], "invalid_key");
    assert_eq!(h.driver.perform_calls(), 0);
    h.close().await;
}

#[tokio::test]
async fn wait_for_text_present_returns_state_without_input() {
    let mut h = Harness::new("form", &[ApprovalDecision::Session]);
    h.initialize(false).await;
    let ready = h
        .call(
            "wait_for",
            json!({"app":"fake.form","until":{"text_present":"Submit"}}),
        )
        .await;
    assert_ne!(ready["isError"], true, "{ready}");
    assert_eq!(ready["structuredContent"]["action"]["outcome"], "done");
    assert_eq!(ready["structuredContent"]["action"]["delivery"], "none");
    assert!(text(&ready).contains("Submit"), "{ready}");
    let refused = h
        .call(
            "wait_for",
            json!({"app":"fake.form","until":{"text_present":"Submit"},"timeout_ms":30001}),
        )
        .await;
    assert_eq!(
        refused["structuredContent"]["error"]["code"],
        "capability_unavailable"
    );
    assert_eq!(h.driver.perform_calls(), 0);
    assert_eq!(h.driver.oracle().await.unwrap().text["text"], "");
    h.close().await;
}
