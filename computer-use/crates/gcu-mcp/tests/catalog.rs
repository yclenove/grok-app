//! Frozen profiles, executable exposure gates, instructions, and wire budgets.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;

use gcu_core::{action::ActionRouter, driver::Driver, session::Session};
use gcu_driver_fake::{scene, FakeDriver};
use gcu_mcp::{Profile, ScriptBody, ScriptRunner, ServerConfig, IMPLEMENTED_TOOLS};
use gcu_protocol::ComputerScriptParams;
use serde_json::{json, Value};
use std::{future::Future, pin::Pin, sync::Arc};
use support::Harness;

const MINIMAL: [&str; 9] = [
    "list_apps",
    "list_windows",
    "get_app_state",
    "click",
    "perform_secondary_action",
    "set_value",
    "type_text",
    "press_key",
    "scroll",
];
const DEFAULT: [&str; 15] = [
    "list_apps",
    "list_windows",
    "launch_app",
    "get_app_state",
    "click",
    "perform_secondary_action",
    "set_value",
    "select_text",
    "type_text",
    "press_key",
    "scroll",
    "drag",
    "wait_for",
    "computer_batch",
    "capabilities",
];
const PROFILES: [(Profile, &str); 3] = [
    (Profile::Minimal, "minimal"),
    (Profile::Default, "default"),
    (Profile::Full, "full"),
];

struct Runner;
impl ScriptRunner for Runner {
    fn run<'a>(
        &'a self,
        _router: &'a ActionRouter,
        _session: &'a Session,
        _call_id: &'a str,
        _params: ComputerScriptParams,
    ) -> Pin<Box<dyn Future<Output = ScriptBody> + Send + 'a>> {
        Box::pin(async {
            ScriptBody {
                output: "catalog runner".into(),
                error: None,
            }
        })
    }
}

fn harness(profile: Profile, runner: bool, browser_window: bool) -> Harness {
    // A window/web area's presence or self-reported name is not a trusted L1
    // binding (ADR 0010). No browser executor is assembled in this build.
    let source = if browser_window {
        scene::builtin("webarea")
            .unwrap()
            .replace("Fixture webarea", "cu-browser")
    } else {
        "(apps: [])".into()
    };
    Harness::configured(
        FakeDriver::from_ron(&source).unwrap(),
        &[],
        ServerConfig {
            profile,
            script: runner.then(|| Arc::new(Runner) as Arc<dyn ScriptRunner>),
            route_note: String::new(),
        },
    )
}

fn names(tools: &Value) -> Vec<&str> {
    tools
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect()
}

fn golden(name: &str) -> Value {
    serde_json::from_str(match name {
        "minimal" => include_str!("../../gcu-protocol/golden/profiles/minimal.json"),
        "default" => include_str!("../../gcu-protocol/golden/profiles/default.json"),
        "full" => include_str!("../../gcu-protocol/golden/profiles/full.json"),
        _ => unreachable!(),
    })
    .unwrap()
}

#[tokio::test]
async fn minimal_with_or_without_runner_keeps_exact_nine_core_tools() {
    for runner in [false, true] {
        let mut h = harness(Profile::Minimal, runner, false);
        h.initialize(false).await;
        let listed = h.request("tools/list", json!({})).await;
        assert_eq!(names(&listed["result"]["tools"]), MINIMAL);
        for name in [
            "launch_app",
            "wait_for",
            "computer_batch",
            "computer_script",
        ] {
            let refused = h.call(name, json!({})).await;
            assert_eq!(
                refused["structuredContent"]["error"]["code"],
                "capability_unavailable"
            );
        }
        assert_eq!(h.driver.perform_calls(), 0);
        h.close().await;
    }
}

#[tokio::test]
async fn default_with_or_without_runner_keeps_fifteen_shipped_tools() {
    for runner in [false, true] {
        let mut h = harness(Profile::Default, runner, false);
        h.initialize(false).await;
        let listed = h.request("tools/list", json!({})).await;
        assert_eq!(names(&listed["result"]["tools"]), DEFAULT);
        h.close().await;
    }
}

#[tokio::test]
async fn full_feature_runner_and_window_matrix_registers_only_executable_tools() {
    // Run this target both without features and with --features script. The
    // injected runner alone must never bypass compile-time or Unix gates.
    for runner in [false, true] {
        for browser_window in [false, true] {
            let mut h = harness(Profile::Full, runner, browser_window);
            let windows = h.driver.list_windows(None).await.unwrap();
            assert_eq!(windows.len(), usize::from(browser_window));
            h.initialize(false).await;
            let listed = h.request("tools/list", json!({})).await;
            let actual = names(&listed["result"]["tools"]);
            let mut expected = DEFAULT.to_vec();
            let script_ready = cfg!(all(unix, feature = "script")) && runner;
            if script_ready {
                expected.insert(14, "computer_script");
            }
            assert_eq!(actual, expected);
            for name in actual {
                assert!(IMPLEMENTED_TOOLS.iter().any(|tool| tool.as_str() == name));
            }
            let refused = h
                .call(
                    "browser_navigate",
                    json!({"url":"https://example.invalid/"}),
                )
                .await;
            assert_eq!(
                refused["structuredContent"]["error"]["code"],
                "capability_unavailable"
            );
            let script = h.call("computer_script", json!({"code":"return 1;"})).await;
            if script_ready {
                assert_ne!(script["isError"], true, "{script}");
                assert!(support::text(&script).contains("catalog runner"));
            } else {
                assert_eq!(
                    script["structuredContent"]["error"]["code"],
                    "capability_unavailable"
                );
            }
            assert_eq!(h.driver.perform_calls(), 0);
            h.close().await;
        }
    }
}

#[tokio::test]
async fn every_profile_matches_golden_and_protocol_within_unchanged_byte_budget() {
    let protocol = gcu_protocol::tools_list_json();
    for (profile, name) in PROFILES {
        let snapshot = golden(name);
        let mut expected_names = match profile {
            Profile::Minimal => MINIMAL.to_vec(),
            Profile::Default | Profile::Full => DEFAULT.to_vec(),
        };
        if profile == Profile::Full {
            expected_names.insert(14, "computer_script");
        }
        assert_eq!(names(&snapshot), expected_names);
        assert!(serde_json::to_vec(&snapshot).unwrap().len() <= 14_000);
        for runner in [false, true] {
            let mut h = harness(profile, runner, false);
            h.initialize(false).await;
            let listed = h.request("tools/list", json!({})).await;
            let actual = &listed["result"]["tools"];
            let expected: Vec<_> = snapshot
                .as_array()
                .unwrap()
                .iter()
                .filter(|tool| {
                    tool["name"] != "computer_script"
                        || (cfg!(all(unix, feature = "script")) && runner)
                })
                .cloned()
                .collect();
            assert_eq!(actual, &json!(expected), "{name}, runner={runner}");
            assert!(serde_json::to_vec(actual).unwrap().len() <= 14_000);
            for tool in actual.as_array().unwrap() {
                assert!(protocol.as_array().unwrap().contains(tool));
                assert!(IMPLEMENTED_TOOLS
                    .iter()
                    .any(|name| tool["name"] == name.as_str()));
            }
            h.close().await;
        }
    }
}

#[tokio::test]
async fn instructions_for_each_profile_reference_only_registered_tools() {
    for (profile, name) in PROFILES {
        for runner in [false, true] {
            let mut h = harness(profile, runner, false);
            let init = h.initialize(false).await;
            let instructions = init["instructions"].as_str().unwrap();
            assert!(instructions.contains(&format!("{name} profile")));
            assert!(instructions.chars().count() <= 1200);
            let listed = h.request("tools/list", json!({})).await;
            let registered = names(&listed["result"]["tools"]);
            for tool in instructions.split('`').skip(1).step_by(2) {
                assert!(registered.contains(&tool), "{name}: {tool}");
            }
            for token in instructions.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
                if gcu_protocol::ToolName::ALL
                    .iter()
                    .any(|tool| tool.as_str() == token)
                    || token == "browser_navigate"
                {
                    assert!(registered.contains(&token), "{name}: {token}");
                }
            }
            assert_eq!(instructions.contains("computer_batch"), name != "minimal");
            assert_eq!(instructions.contains("wait_for"), name != "minimal");
            assert_eq!(
                instructions.contains("computer_script"),
                name == "full" && cfg!(all(unix, feature = "script")) && runner
            );
            assert!(instructions.contains("L0 input checks do not block browser redirects"));
            assert!(!instructions.contains("browser_navigate"));
            h.close().await;
        }
    }
}
