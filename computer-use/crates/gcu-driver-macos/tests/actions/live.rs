//! Ignored fixture oracle. Grant Accessibility, then:
//! `cargo test -p gcu-driver-macos --test actions -- --ignored --test-threads=1`
use super::*;
use gcu_core::driver::TreeOpts;
use gcu_core::model::{RawNode, Role, States};

#[tokio::test]
#[ignore = "set GCU_FIXTURE_APP and GCU_FIXTURE_STATE; grant Accessibility; do not click the prompt"]
async fn live_fixture_semantic_actions_match_state() {
    let app_path = std::env::var("GCU_FIXTURE_APP").expect("Set GCU_FIXTURE_APP");
    let _state = std::env::var("GCU_FIXTURE_STATE").expect("Set GCU_FIXTURE_STATE");
    let driver = MacosDriver::new().await.expect("unelevated macOS driver");
    let app = driver
        .resolve_app(&AppQuery::from(app_path.as_str()))
        .await
        .unwrap();
    if app.info.pid.is_none() {
        driver.launch_app(&app).await.unwrap();
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let win = loop {
        let resolved = driver
            .resolve_app(&AppQuery::from(app_path.as_str()))
            .await
            .unwrap();
        if resolved.info.pid.is_some() {
            if let Ok(window) = driver.key_window(&resolved).await {
                break WindowRef {
                    app: resolved,
                    id: window.id,
                };
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fixture window did not appear"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let raw = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let root = raw.root.expect("fixture tree");
    let before = read_state();
    let counter = find(&root, &|node| {
        named(node, &["counter_button", "Increment", "计数"])
    })
    .expect("counter");
    let initial = num(&before, "counter");
    driver
        .perform(
            &DriverAction::Press {
                el: counter.handle.clone(),
            },
            &Cancel::default(),
        )
        .await
        .expect("press counter");
    assert_eq!(wait_num("counter", initial + 1).await, initial + 1);

    let slider = find(&root, &|node| {
        node.role == Role::Slider || named(node, &["slider_field", "Level"])
    })
    .expect("slider");
    driver
        .perform(
            &DriverAction::SetValue {
                el: slider.handle.clone(),
                value: "40".into(),
            },
            &Cancel::default(),
        )
        .await
        .expect("set slider");
    assert!((wait_f64("slider", 40.0).await - 40.0).abs() < 1.0);

    let password = find(&root, &|node| {
        node.states.contains(States::PASSWORD)
            || node.native_role.contains("Secure")
            || named(node, &["Password", "密码"])
    })
    .expect("password field");
    let password_len = num(&before, "password_len");
    assert_eq!(
        driver
            .perform(
                &DriverAction::SetValue {
                    el: password.handle.clone(),
                    value: "not-written".into()
                },
                &Cancel::default(),
            )
            .await
            .unwrap_err(),
        DriverError::NotSettable {
            maybe_executed: false
        }
    );
    assert!(!std::fs::read_to_string(state_path())
        .unwrap_or_default()
        .contains("not-written"));
    assert_eq!(num(&read_state(), "password_len"), password_len);

    let checkbox = find(&root, &|node| {
        named(node, &["agree_checkbox", "Agree", "同意"])
    })
    .expect("checkbox");
    let checked = before
        .get("checkbox")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    driver
        .perform(
            &DriverAction::Press {
                el: checkbox.handle.clone(),
            },
            &Cancel::default(),
        )
        .await
        .expect("press checkbox");
    assert_eq!(wait_bool("checkbox", !checked).await, !checked);

    let name =
        find(&root, &|node| named(node, &["name_field", "姓名", "Name"])).expect("name field");
    match driver
        .perform(
            &DriverAction::SetValue {
                el: name.handle.clone(),
                value: "grok-name".into(),
            },
            &Cancel::default(),
        )
        .await
    {
        Ok(_) => assert_eq!(wait_text("text", "grok-name").await, "grok-name"),
        Err(DriverError::NotSettable { .. }) => {
            assert_ne!(
                read_state()
                    .get("text")
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
                "grok-name"
            );
        }
        Err(error) => panic!("name field: {error}"),
    }
    if let Some(scroller) = find(&root, &|node| node.role == Role::ScrollArea) {
        let range_before = visible_range(&before);
        match driver
            .perform(
                &DriverAction::ScrollElement {
                    el: scroller.handle.clone(),
                    dir: Dir::Down,
                    pages: 1.0,
                },
                &Cancel::default(),
            )
            .await
        {
            Ok(_) => assert_ne!(wait_range(range_before).await, range_before),
            Err(DriverError::CapabilityUnavailable) => {
                assert_eq!(visible_range(&read_state()), range_before)
            }
            Err(error) => panic!("scroll: {error}"),
        }
    }
    let selected = TextRange {
        start: 0,
        length: 2,
    };
    driver
        .perform(
            &DriverAction::SelectText {
                el: name.handle.clone(),
                range: selected,
            },
            &Cancel::default(),
        )
        .await
        .expect("select text");
    let again = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let fresh = find(again.root.as_ref().unwrap(), &|node| {
        named(node, &["name_field", "姓名", "Name"])
    })
    .expect("name field after select");
    assert_eq!(
        fresh.text_extras.as_ref().and_then(|text| text.selected),
        Some(selected)
    );
}

fn state_path() -> String {
    std::env::var("GCU_FIXTURE_STATE").unwrap()
}

fn read_state() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(state_path()).unwrap_or_else(|_| "{}".into()))
        .unwrap_or_else(|_| serde_json::json!({}))
}

fn num(state: &serde_json::Value, key: &str) -> u64 {
    state.get(key).and_then(|value| value.as_u64()).unwrap_or(0)
}

fn named(node: &RawNode, tokens: &[&str]) -> bool {
    let id = node.identifier.as_deref().unwrap_or("");
    let name = node.name.as_deref().unwrap_or("");
    tokens
        .iter()
        .any(|token| id == *token || name.contains(token))
}

async fn wait_num(key: &str, want: u64) -> u64 {
    for _ in 0..30 {
        let value = num(&read_state(), key);
        if value == want {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    num(&read_state(), key)
}

async fn wait_f64(key: &str, want: f64) -> f64 {
    for _ in 0..30 {
        let value = read_state()
            .get(key)
            .and_then(|item| item.as_f64())
            .unwrap_or(0.0);
        if (value - want).abs() < 1.0 {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    read_state()
        .get(key)
        .and_then(|item| item.as_f64())
        .unwrap_or(0.0)
}

async fn wait_bool(key: &str, want: bool) -> bool {
    for _ in 0..30 {
        let value = read_state()
            .get(key)
            .and_then(|item| item.as_bool())
            .unwrap_or(false);
        if value == want {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    read_state()
        .get(key)
        .and_then(|item| item.as_bool())
        .unwrap_or(false)
}

async fn wait_text(key: &str, want: &str) -> String {
    for _ in 0..30 {
        let value = read_state()
            .get(key)
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string();
        if value == want {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    read_state()
        .get(key)
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string()
}

fn visible_range(state: &serde_json::Value) -> (u64, u64) {
    let Some(range) = state
        .get("list_visible_range")
        .and_then(|value| value.as_array())
    else {
        return (0, 0);
    };
    (
        range.first().and_then(|value| value.as_u64()).unwrap_or(0),
        range.get(1).and_then(|value| value.as_u64()).unwrap_or(0),
    )
}

async fn wait_range(before: (u64, u64)) -> (u64, u64) {
    for _ in 0..30 {
        let value = visible_range(&read_state());
        if value != before {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    visible_range(&read_state())
}

fn find<'a>(node: &'a RawNode, pred: &dyn Fn(&RawNode) -> bool) -> Option<&'a RawNode> {
    if pred(node) {
        return Some(node);
    }
    node.children.iter().find_map(|child| find(child, pred))
}
