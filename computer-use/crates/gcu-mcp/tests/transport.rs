//! Framing, bounded queues, profiles, and shutdown under adversarial input.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;
use gcu_driver_fake::FakeDriver;
use gcu_mcp::{
    codec::{Decoded, NdjsonCodec, MAX_LINE_BYTES},
    Profile, ServerConfig,
};
use serde_json::json;
use support::Harness;
use tokio::io::AsyncWriteExt;

#[test]
fn codec_chunk_boundaries_and_overlong_eof_recover() {
    let input = b"{\"word\":\"\xf0\x9f\x8c\x8d\"}\ninvalid\n{}\n";
    let whole = NdjsonCodec::default().push(input);
    for size in 1..=input.len() {
        let mut codec = NdjsonCodec::default();
        let split: Vec<_> = input
            .chunks(size)
            .flat_map(|chunk| codec.push(chunk))
            .collect();
        assert_eq!(split, whole);
        assert!(codec.finish().is_none());
    }
    let mut codec = NdjsonCodec::default();
    assert!(codec.push(&vec![b'x'; MAX_LINE_BYTES + 1]).is_empty());
    assert_eq!(codec.finish(), Some(Decoded::Error(-32600)));
    assert!(codec.finish().is_none());
    assert_eq!(codec.push(b"{}\n"), NdjsonCodec::default().push(b"{}\n"));
}

#[tokio::test]
async fn profiles_advertise_only_executable_tools_and_enforce_exposure() {
    for (profile, count) in [(Profile::Minimal, 9), (Profile::Full, 15)] {
        let mut h = Harness::configured(
            FakeDriver::from_scene("form").unwrap(),
            &[],
            ServerConfig {
                profile,
                browser_session: None,
            },
        );
        let init = h.initialize(false).await;
        assert!(init["instructions"].as_str().unwrap().chars().count() <= 1200);
        let listed = h.request("tools/list", json!({})).await;
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), count);
        assert!(serde_json::to_vec(&listed["result"]).unwrap().len() <= 14_000);
        if count == 9 {
            let names: Vec<_> = listed["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|tool| tool["name"].as_str())
                .collect();
            assert!(!names.contains(&"wait_for"));
            assert!(!names.contains(&"computer_batch"));
            for (name, args) in [
                ("launch_app", json!({"app":"fake.form"})),
                (
                    "wait_for",
                    json!({"app":"fake.form","until":{"text_present":"Submit"}}),
                ),
                (
                    "computer_batch",
                    json!({"app":"fake.form","steps":[{"wait_for":{"until":{"text_present":"Submit"}}}]}),
                ),
            ] {
                let result = h.call(name, args).await;
                assert_eq!(
                    result["structuredContent"]["error"]["code"], "capability_unavailable",
                    "{name}"
                );
            }
            assert!(h.approvals.app_requests().is_empty());
        }
        h.close().await;
    }
}

#[tokio::test]
async fn pending_call_limit_returns_busy_while_approval_is_waiting() {
    let mut h = Harness::new("form", &[]);
    h.initialize(false).await;
    for id in 100..133 {
        h.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"get_app_state","arguments":{"app":"fake.form","include_screenshot":false}}})).await;
    }
    let busy = h.read().await;
    assert_eq!(busy["result"]["structuredContent"]["error"]["code"], "busy");
    assert_eq!(h.driver.perform_calls(), 0);
    h.close().await;
}

#[tokio::test]
async fn invalid_rpc_envelopes_recover_and_truncated_utf8_eof_cleans_up() {
    let mut h = Harness::new("form", &[]);
    h.initialize(false).await;
    for value in [
        json!({"jsonrpc":"1.0","id":"bad","method":"ping"}),
        json!([]),
        json!({"jsonrpc":"2.0","id":false,"method":"ping"}),
    ] {
        h.send(value).await;
        assert_eq!(h.read().await["error"]["code"], -32600);
        assert!(h.request("ping", json!({})).await["result"].is_object());
    }
    let driver = h.driver.clone();
    h.writer.write_all(&[0xf0, 0x9f]).await.unwrap();
    h.writer.shutdown().await.unwrap();
    assert_eq!(h.read().await["error"]["code"], -32700);
    assert!(h.task.await.unwrap().is_ok());
    assert!(driver.release_calls() > 0);
}

#[tokio::test]
async fn failed_initialize_still_closes_and_releases_input() {
    let mut h = Harness::new("form", &[]);
    let driver = h.driver.clone();
    h.send(json!({"jsonrpc":"2.0","id":1,"method":"ping"}))
        .await;
    h.writer.shutdown().await.unwrap();
    assert!(h.task.await.unwrap().is_err());
    assert!(driver.release_calls() > 0);
}

#[tokio::test]
async fn aborted_connection_releases_input_and_deletes_private_files() {
    let mut h = Harness::new("form", &[gcu_host_api::ApprovalDecision::Session]);
    h.initialize(false).await;
    let result = h.call("get_app_state", json!({"app":"fake.form"})).await;
    let text = support::text(&result);
    let path = text
        .lines()
        .find_map(|line| line.strip_prefix("Screenshot file: "))
        .unwrap()
        .to_owned();
    assert!(std::path::Path::new(&path).exists());
    let driver = h.driver.clone();
    h.task.abort();
    assert!(h.task.await.unwrap_err().is_cancelled());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while std::path::Path::new(&path).exists() || driver.release_calls() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
