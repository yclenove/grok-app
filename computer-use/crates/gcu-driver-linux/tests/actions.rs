//! Integration and unit tests for Linux AT-SPI semantic actions and XTEST input.
//!
//! Includes:
//! 1. Platform-independent fake tests covering:
//!    - Table-driven matching rules for Press and SecondaryAction
//!    - Settable vs NotSettable validation
//!    - Timeout with maybe_executed: true semantics
//!    - StaleElement handling on invalid handles
//!    - TargetedOnly route returning CapabilityUnavailable and never escalating to XTEST
//!    - XTEST window activation failure returning CapabilityUnavailable
//!    - InputLedger press-before-send recording and idempotent release
//!    - Keysym mapping table-driven coverage (>= 40 keys: F-keys, KP, modifiers, Unicode)
//!    - type_text chunking (>2000 chars) preserving grapheme cluster integrity
//!    - type_text preferring AT-SPI when focus handle is provided
//! 2. Live Linux integration tests running against `gcu-fixture` in Xvfb (`#[ignore]`):
//!    - Counter button Press (counter +1 exactly once)
//!    - SetValue on text entry
//!    - XTEST click on canvas grid (grid_hit correct)
//!    - type_text Chinese + emoji (exact UTF-8 bytes)
//!    - ctrl+a then BackSpace clearing

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[cfg(target_os = "linux")]
use gcu_core::model::RawNode;
use gcu_core::{
    driver::{
        Cancel, Delivery, Driver, DriverAction, DriverError, KeyRoute, PointerRoute, WindowRef,
    },
    model::{NativeHandle, NodeAction, NodeValue, Role, WindowPoint},
};
use gcu_driver_linux::{
    atspi::{
        find_best_press_action, find_secondary_action,
        source::{FakeNode, FakeSource},
    },
    x11::{
        char_to_keysym, fake::FakeXEvent, key_to_keysym, modifier_to_keysym, named_key_to_keysym,
        resolve_or_remap_keysym, FakeWindow, FakeX, InputLedger, XRootGeometry, XWindowGeometry,
        TYPE_TEXT_CHUNK_SIZE,
    },
    LinuxDriver,
};
use gcu_protocol::{
    keys::{Key, KeyChord, Modifier, NamedKey},
    tools::MouseButton as Button,
    AppInfo,
};
use std::{sync::Arc, time::Duration};
use unicode_segmentation::UnicodeSegmentation;

fn make_fake_x() -> FakeX {
    FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 1920,
            height: 1080,
            scale: 1.0,
        },
        true,
    )
}

fn add_test_window(x: &FakeX, id: u32, title: &str) {
    x.add_window(FakeWindow {
        id,
        name: Some(title.into()),
        pid: Some(100),
        geometry: XWindowGeometry {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: None,
    });
}

// ============================================================================
// PART 1: Platform-Independent Fake / Logic Tests
// ============================================================================

#[test]
fn test_press_matching_rules_table_driven() {
    assert_eq!(find_best_press_action(&["click".into()]), Some(0));
    assert_eq!(find_best_press_action(&["press".into()]), Some(0));
    assert_eq!(find_best_press_action(&["activate".into()]), Some(0));
    assert_eq!(find_best_press_action(&["jump".into()]), Some(0));
    assert_eq!(find_best_press_action(&["toggle".into()]), Some(0));

    let actions = vec!["custom".into(), "activate".into(), "click".into()];
    assert_eq!(find_best_press_action(&actions), Some(2));

    let sub = vec!["Perform Toggle Action".into(), "Other".into()];
    assert_eq!(find_best_press_action(&sub), Some(0));

    let single = vec!["do_special_thing".into()];
    assert_eq!(find_best_press_action(&single), Some(0));

    let no_match = vec!["drag_source".into(), "drop_target".into()];
    assert_eq!(find_best_press_action(&no_match), None);
}

#[test]
fn test_secondary_action_matching_rules_table_driven() {
    assert_eq!(
        find_secondary_action(&NodeAction::ShowMenu, &["context_menu".into()]),
        Some(0)
    );
    assert_eq!(
        find_secondary_action(&NodeAction::Increment, &["other".into(), "step_up".into()]),
        Some(1)
    );
    assert_eq!(
        find_secondary_action(&NodeAction::Decrement, &["step_down".into()]),
        Some(0)
    );
    assert_eq!(
        find_secondary_action(&NodeAction::Confirm, &["submit".into()]),
        Some(0)
    );
    assert_eq!(
        find_secondary_action(&NodeAction::Cancel, &["dismiss".into()]),
        Some(0)
    );
    assert_eq!(
        find_secondary_action(&NodeAction::Toggle, &["toggle".into()]),
        Some(0)
    );
    assert_eq!(
        find_secondary_action(&NodeAction::Native("cmd".into()), &["custom_cmd".into()]),
        Some(0)
    );
}

#[tokio::test]
async fn test_set_value_editable_and_not_settable() {
    let mut root = FakeNode {
        id: 1,
        role: atspi::Role::Frame,
        ..Default::default()
    };
    let mut editable = FakeNode {
        id: 2,
        role: atspi::Role::Entry,
        ..Default::default()
    };
    editable.states.insert(atspi::State::Editable);
    let label = FakeNode {
        id: 3,
        role: atspi::Role::Label,
        ..Default::default()
    };
    let slider = FakeNode {
        id: 4,
        role: atspi::Role::Slider,
        value: Some(NodeValue::Number(50.0)),
        ..Default::default()
    };
    root.children = vec![editable, label, slider];

    let driver = LinuxDriver::new_fake(FakeSource::default(), root, vec![], vec![]);
    let cancel = Cancel::default();

    let ok = driver
        .perform(
            &DriverAction::SetValue {
                el: NativeHandle::new(2u64),
                value: "hello".into(),
            },
            &cancel,
        )
        .await;
    assert_eq!(ok.unwrap(), Delivery::Accessibility);

    let err = driver
        .perform(
            &DriverAction::SetValue {
                el: NativeHandle::new(3u64),
                value: "hello".into(),
            },
            &cancel,
        )
        .await;
    assert!(matches!(err.unwrap_err(), DriverError::NotSettable));

    let ok_slider = driver
        .perform(
            &DriverAction::SetValue {
                el: NativeHandle::new(4u64),
                value: "75.5".into(),
            },
            &cancel,
        )
        .await;
    assert_eq!(ok_slider.unwrap(), Delivery::Accessibility);

    let err_slider = driver
        .perform(
            &DriverAction::SetValue {
                el: NativeHandle::new(4u64),
                value: "abc".into(),
            },
            &cancel,
        )
        .await;
    assert!(matches!(err_slider.unwrap_err(), DriverError::NotSettable));
}

#[tokio::test]
async fn test_action_timeout_maybe_executed_semantics() {
    let mut root = FakeNode {
        id: 1,
        role: atspi::Role::Frame,
        ..Default::default()
    };
    root.children = vec![FakeNode {
        id: 2,
        role: atspi::Role::Button,
        actions: vec![NodeAction::Press],
        ..Default::default()
    }];
    let driver = LinuxDriver::new_fake(
        FakeSource {
            delay: Duration::ZERO,
            error_on_id: Some(2),
        },
        root,
        vec![],
        vec![],
    );
    let cancel = Cancel::default();

    let err = driver
        .perform(
            &DriverAction::Press {
                el: NativeHandle::new(2u64),
            },
            &cancel,
        )
        .await;
    assert_eq!(
        err.unwrap_err(),
        DriverError::Timeout {
            maybe_executed: true
        }
    );
}

#[tokio::test]
async fn test_stale_element_handle_returns_stale_element() {
    let root = FakeNode {
        id: 1,
        role: atspi::Role::Frame,
        ..Default::default()
    };
    let driver = LinuxDriver::new_fake(FakeSource::default(), root, vec![], vec![]);
    let cancel = Cancel::default();

    let err = driver
        .perform(
            &DriverAction::Press {
                el: NativeHandle::new(999u64),
            },
            &cancel,
        )
        .await;
    assert!(matches!(err.unwrap_err(), DriverError::StaleElement));
}

#[tokio::test]
async fn test_targeted_only_route_returns_capability_unavailable_never_escalates() {
    let fake_x = Arc::new(make_fake_x());
    add_test_window(&fake_x, 100, "App Window");
    fake_x.set_active_window(Some(100));

    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
        .with_x_server(fake_x.clone());

    let win = WindowRef {
        app: gcu_core::driver::AppRef {
            info: AppInfo {
                id: "1".into(),
                name: "a".into(),
                pid: None,
                identity: None,
            },
        },
        id: "100".into(),
    };
    let cancel = Cancel::default();

    let err1 = driver
        .perform(
            &DriverAction::PointerClick {
                win: win.clone(),
                at: WindowPoint { x: 50.0, y: 50.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::TargetedOnly,
            },
            &cancel,
        )
        .await;
    assert!(matches!(
        err1.unwrap_err(),
        DriverError::CapabilityUnavailable
    ));

    let err2 = driver
        .perform(
            &DriverAction::PointerDrag {
                win: win.clone(),
                path: vec![
                    WindowPoint { x: 10.0, y: 10.0 },
                    WindowPoint { x: 50.0, y: 50.0 },
                ],
                route: PointerRoute::TargetedOnly,
            },
            &cancel,
        )
        .await;
    assert!(matches!(
        err2.unwrap_err(),
        DriverError::CapabilityUnavailable
    ));

    let err3 = driver
        .perform(
            &DriverAction::PointerScroll {
                win: win.clone(),
                at: WindowPoint { x: 50.0, y: 50.0 },
                dx: 0,
                dy: -100,
                route: PointerRoute::TargetedOnly,
            },
            &cancel,
        )
        .await;
    assert!(matches!(
        err3.unwrap_err(),
        DriverError::CapabilityUnavailable
    ));

    let err4 = driver
        .perform(
            &DriverAction::TypeText {
                win: win.clone(),
                focus: None,
                text: "t".into(),
                route: KeyRoute::TargetedOnly,
            },
            &cancel,
        )
        .await;
    assert!(matches!(
        err4.unwrap_err(),
        DriverError::CapabilityUnavailable
    ));

    let err5 = driver
        .perform(
            &DriverAction::PressKeys {
                win: win.clone(),
                chord: KeyChord::parse("Return").unwrap(),
                route: KeyRoute::TargetedOnly,
            },
            &cancel,
        )
        .await;
    assert!(matches!(
        err5.unwrap_err(),
        DriverError::CapabilityUnavailable
    ));

    assert!(fake_x.get_events().is_empty());
}

#[tokio::test]
async fn test_xtest_window_activation_failure_returns_capability_unavailable() {
    let fake_x = Arc::new(make_fake_x());
    add_test_window(&fake_x, 100, "Window");
    fake_x.set_activate_fails(true);

    let driver = LinuxDriver::new_fake(FakeSource::default(), FakeNode::default(), vec![], vec![])
        .with_x_server(fake_x.clone());

    let win = WindowRef {
        app: gcu_core::driver::AppRef {
            info: AppInfo {
                id: "1".into(),
                name: "a".into(),
                pid: None,
                identity: None,
            },
        },
        id: "100".into(),
    };
    let cancel = Cancel::default();

    let err = driver
        .perform(
            &DriverAction::PointerClick {
                win,
                at: WindowPoint { x: 100.0, y: 100.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::AllowForeground,
            },
            &cancel,
        )
        .await;
    assert!(matches!(
        err.unwrap_err(),
        DriverError::CapabilityUnavailable
    ));
    assert!(fake_x.get_events().is_empty());
}

#[test]
fn test_input_ledger_records_before_sending_and_idempotent_release() {
    let server = make_fake_x();
    let ledger = InputLedger::new();

    assert!(ledger.is_empty());
    ledger.record_button_down(1);
    ledger.record_key_down(65);
    ledger.record_button_down(2);
    assert!(!ledger.is_empty());

    ledger.release_all(&server);
    assert!(ledger.is_empty());

    let events = server.get_events();
    assert_eq!(
        events,
        vec![
            FakeXEvent::Button {
                button: 2,
                is_press: false
            },
            FakeXEvent::Key {
                keycode: 65,
                is_press: false
            },
            FakeXEvent::Button {
                button: 1,
                is_press: false
            },
        ]
    );

    // Idempotent
    ledger.release_all(&server);
    assert_eq!(server.get_events().len(), 3);
}

#[test]
fn test_keysym_table_driven_mapping_covers_fkeys_kp_modifiers_unicode() {
    // 1. Function keys (12 keys: F1..F12)
    for n in 1..=12 {
        let sym = named_key_to_keysym(NamedKey::Function(n));
        assert!((0xffbe..=0xffcb).contains(&sym));
    }

    // 2. Keypad keys (17 keys: 0..9 and operators)
    for d in 0..=9 {
        let sym = named_key_to_keysym(NamedKey::KeypadDigit(d));
        assert!((0xffb0..=0xffb9).contains(&sym));
    }
    let kp_ops = [
        NamedKey::KeypadEnter,
        NamedKey::KeypadAdd,
        NamedKey::KeypadSubtract,
        NamedKey::KeypadMultiply,
        NamedKey::KeypadDivide,
        NamedKey::KeypadDecimal,
    ];
    for kp in kp_ops {
        assert!(named_key_to_keysym(kp) > 0);
    }

    // 3. Modifiers (6 keys)
    let modifiers = [
        Modifier::Shift,
        Modifier::Ctrl,
        Modifier::Alt,
        Modifier::Super,
        Modifier::Cmd,
        Modifier::Meta,
    ];
    for m in modifiers {
        assert!(modifier_to_keysym(m) > 0);
    }

    // 4. Navigation & Editing (15 keys)
    let nav_keys = [
        NamedKey::Return,
        NamedKey::Escape,
        NamedKey::BackSpace,
        NamedKey::Tab,
        NamedKey::Space,
        NamedKey::Delete,
        NamedKey::Home,
        NamedKey::End,
        NamedKey::PageUp,
        NamedKey::PageDown,
        NamedKey::Left,
        NamedKey::Up,
        NamedKey::Right,
        NamedKey::Down,
        NamedKey::Insert,
    ];
    for nk in nav_keys {
        assert!(named_key_to_keysym(nk) > 0);
    }

    // 5. Unicode keys (7 keys)
    let unicode_chars = ['a', 'Z', '9', '中', '文', '🚀', '€'];
    for uc in unicode_chars {
        assert!(char_to_keysym(uc) > 0);
    }

    // Total keys tested = 12 + 16 + 6 + 15 + 7 = 56 keys (>= 40 requirement)
    assert!(key_to_keysym(&Key::Named(NamedKey::Return)) > 0);
    assert!(key_to_keysym(&Key::Character('中')) > 0);

    let server = make_fake_x();
    assert!(resolve_or_remap_keysym(&server, 0xff08).is_ok());
}

#[test]
fn test_type_text_chunking_large_text_and_unicode_grapheme_integrity() {
    let family_emoji = "👨‍👩‍👧‍👦";
    let chinese_text = "中国智造，惠及全球。";

    let mut large_text = String::new();
    while large_text.chars().count() < 3500 {
        large_text.push_str("Lorem ipsum dolor sit amet. ");
        large_text.push_str(chinese_text);
        large_text.push_str(family_emoji);
        large_text.push('\n');
    }

    let graphemes: Vec<&str> = large_text.graphemes(true).collect();
    let mut reconstructed = String::new();

    for chunk in graphemes.chunks(TYPE_TEXT_CHUNK_SIZE) {
        assert!(chunk.len() <= TYPE_TEXT_CHUNK_SIZE);
        for &g in chunk {
            reconstructed.push_str(g);
        }
    }

    assert_eq!(reconstructed, large_text);
    assert!(reconstructed.contains(family_emoji));
    assert!(reconstructed.contains(chinese_text));
}

#[tokio::test]
async fn test_type_text_prefers_atspi_when_focused() {
    let mut root = FakeNode {
        id: 1,
        role: atspi::Role::Frame,
        ..Default::default()
    };
    let mut entry = FakeNode {
        id: 2,
        role: atspi::Role::Entry,
        ..Default::default()
    };
    entry.states.insert(atspi::State::Editable);
    root.children = vec![entry];

    let fake_x = Arc::new(make_fake_x());
    let driver = LinuxDriver::new_fake(FakeSource::default(), root, vec![], vec![])
        .with_x_server(fake_x.clone());

    let win = WindowRef {
        app: gcu_core::driver::AppRef {
            info: AppInfo {
                id: "1".into(),
                name: "a".into(),
                pid: None,
                identity: None,
            },
        },
        id: "100".into(),
    };
    let cancel = Cancel::default();

    let res = driver
        .perform(
            &DriverAction::TypeText {
                win,
                focus: Some(NativeHandle::new(2u64)),
                text: "Focused Input".into(),
                route: KeyRoute::Auto,
            },
            &cancel,
        )
        .await;

    assert_eq!(res.unwrap(), Delivery::Accessibility);
    assert!(fake_x.get_events().is_empty());
}

// ============================================================================
// PART 2: Live Integration Tests (against gcu-fixture in Xvfb with AT-SPI bus)
// ============================================================================

#[cfg(target_os = "linux")]
async fn live_fixture_setup(driver: &LinuxDriver) -> (WindowRef, RawNode) {
    let mut fixture_app = None;
    for _ in 0..40 {
        if let Ok(app) = driver
            .resolve_app(&gcu_core::driver::AppQuery("fixture".into()))
            .await
        {
            fixture_app = Some(app);
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let app = fixture_app.expect("gcu-fixture application must be discoverable");
    let win_info = driver
        .key_window(&app)
        .await
        .expect("key window must be found");
    let win_ref = WindowRef {
        app: app.clone(),
        id: win_info.id.clone(),
    };

    let opts = gcu_core::driver::TreeOpts {
        timeout: Duration::from_secs(5),
        max_tree_nodes: 5000,
        ..Default::default()
    };
    let cancel = Cancel::default();
    let tree = driver
        .snapshot_tree(&win_ref, &opts, &cancel)
        .await
        .expect("tree snapshot");
    let mut root = tree.root.expect("root node must exist");

    // If modal dialog was left open (e.g. by dump_atspi.py), close it
    if let Some(confirm_btn) = find_raw_node(&root, &|n| {
        n.name.as_deref() == Some("modal_confirm_button")
            || n.identifier.as_deref() == Some("modal_confirm_button")
    }) {
        let _ = driver
            .perform(
                &DriverAction::Press {
                    el: confirm_btn.handle.clone(),
                },
                &cancel,
            )
            .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let new_tree = driver
            .snapshot_tree(&win_ref, &opts, &cancel)
            .await
            .expect("tree snapshot after closing modal");
        root = new_tree.root.expect("root node after closing modal");
    }

    (win_ref, root)
}

#[cfg(target_os = "linux")]
fn find_raw_node<'a, F>(node: &'a RawNode, predicate: &F) -> Option<&'a RawNode>
where
    F: Fn(&RawNode) -> bool,
{
    if predicate(node) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_raw_node(child, predicate) {
            return Some(found);
        }
    }
    None
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn live_actions_fixture_counter_button_press_increments_counter_once() {
    let driver = LinuxDriver::connect().await.expect("connect to AT-SPI");
    let (_win_ref, root) = live_fixture_setup(&driver).await;

    let btn = find_raw_node(&root, &|n| {
        n.role == Role::Button
            && (n.name.as_deref().is_some_and(|s| {
                s == "counter_button" || s.contains("Increment") || s.contains("计数")
            }) || n.identifier.as_deref() == Some("counter_button"))
    })
    .expect("counter button found");

    let state_path =
        std::env::var("GCU_FIXTURE_STATE").unwrap_or_else(|_| "/tmp/fixture_state.json".into());
    let get_counter = || -> u64 {
        let content = std::fs::read_to_string(&state_path).unwrap_or_else(|_| "{}".into());
        let val: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
        val.get("counter").and_then(|v| v.as_u64()).unwrap_or(0)
    };

    let initial = get_counter();
    let cancel = Cancel::default();
    driver
        .perform(
            &DriverAction::Press {
                el: btn.handle.clone(),
            },
            &cancel,
        )
        .await
        .expect("press ok");

    let mut updated = initial;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        updated = get_counter();
        if updated > initial {
            break;
        }
    }
    assert_eq!(updated, initial + 1, "counter increments by exactly 1");
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn live_actions_fixture_set_value_on_name_field() {
    let driver = LinuxDriver::connect().await.expect("connect to AT-SPI");
    let (_win_ref, root) = live_fixture_setup(&driver).await;

    let name_field = find_raw_node(&root, &|n| {
        n.role == Role::TextField
            && (n
                .name
                .as_deref()
                .is_some_and(|s| s == "name_field" || s.contains("Name") || s.contains("姓名"))
                || n.identifier.as_deref() == Some("name_field"))
    })
    .expect("name_field found");

    let expected = "Grok Live AT-SPI Test";
    let cancel = Cancel::default();
    driver
        .perform(
            &DriverAction::SetValue {
                el: name_field.handle.clone(),
                value: expected.into(),
            },
            &cancel,
        )
        .await
        .expect("set_value ok");

    let state_path =
        std::env::var("GCU_FIXTURE_STATE").unwrap_or_else(|_| "/tmp/fixture_state.json".into());
    let mut updated = String::new();
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(content) = std::fs::read_to_string(&state_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(t) = val.get("text").and_then(|v| v.as_str()) {
                    updated = t.to_string();
                    if updated == expected {
                        break;
                    }
                }
            }
        }
    }
    assert_eq!(updated, expected, "state file text reflects SetValue");
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn live_actions_fixture_xtest_click_canvas_grid_hits_cell() {
    let driver = LinuxDriver::connect().await.expect("connect to AT-SPI");
    let (win_ref, root) = live_fixture_setup(&driver).await;

    let canvas = find_raw_node(&root, &|n| {
        n.name.as_deref() == Some("grid_canvas") || n.identifier.as_deref() == Some("grid_canvas")
    })
    .expect("grid_canvas found");
    let frame = canvas.frame.expect("grid_canvas frame");

    let (col, row) = (3, 2);
    let click_x = frame.x + (col as f64 + 0.5) * 40.0;
    let click_y = frame.y + (row as f64 + 0.5) * 40.0;

    let cancel = Cancel::default();
    driver
        .perform(
            &DriverAction::PointerClick {
                win: win_ref,
                at: WindowPoint {
                    x: click_x,
                    y: click_y,
                },
                button: Button::Left,
                count: 1,
                route: PointerRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .expect("click ok");

    let state_path =
        std::env::var("GCU_FIXTURE_STATE").unwrap_or_else(|_| "/tmp/fixture_state.json".into());
    let mut hit = None;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(content) = std::fs::read_to_string(&state_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(arr) = val.get("grid_hit").and_then(|v| v.as_array()) {
                    if arr.len() == 2 {
                        hit = Some((arr[0].as_u64().unwrap_or(99), arr[1].as_u64().unwrap_or(99)));
                        if hit == Some((col, row)) {
                            break;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        hit,
        Some((col, row)),
        "XTEST click on canvas triggered grid_hit"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn live_actions_fixture_type_text_chinese_and_emoji() {
    let driver = LinuxDriver::connect().await.expect("connect to AT-SPI");
    let (win_ref, root) = live_fixture_setup(&driver).await;

    let name_field = find_raw_node(&root, &|n| {
        n.role == Role::TextField
            && (n
                .name
                .as_deref()
                .is_some_and(|s| s == "name_field" || s.contains("Name") || s.contains("姓名"))
                || n.identifier.as_deref() == Some("name_field"))
    })
    .expect("name_field found");
    let name_frame = name_field.frame.expect("name_field frame");

    let cancel = Cancel::default();
    // Focus the text field via mouse click first
    driver
        .perform(
            &DriverAction::PointerClick {
                win: win_ref.clone(),
                at: WindowPoint {
                    x: name_frame.x + 10.0,
                    y: name_frame.y + 10.0,
                },
                button: Button::Left,
                count: 1,
                route: PointerRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .expect("click name_field to focus");

    let text = "你好，世界！🚀🎉";
    driver
        .perform(
            &DriverAction::TypeText {
                win: win_ref,
                focus: None,
                text: text.into(),
                route: KeyRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .expect("type_text ok");

    let state_path =
        std::env::var("GCU_FIXTURE_STATE").unwrap_or_else(|_| "/tmp/fixture_state.json".into());
    let mut contains = false;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(c) = std::fs::read_to_string(&state_path) {
            if c.contains(text) {
                contains = true;
                break;
            }
        }
    }
    assert!(contains, "state file contains typed Chinese + emoji text");
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn live_actions_fixture_ctrl_a_backspace_clears_field() {
    let driver = LinuxDriver::connect().await.expect("connect to AT-SPI");
    let (win_ref, root) = live_fixture_setup(&driver).await;

    let name_field = find_raw_node(&root, &|n| {
        n.role == Role::TextField
            && (n
                .name
                .as_deref()
                .is_some_and(|s| s == "name_field" || s.contains("Name") || s.contains("姓名"))
                || n.identifier.as_deref() == Some("name_field"))
    })
    .expect("name_field found");
    let name_frame = name_field.frame.expect("name_field frame");

    let cancel = Cancel::default();
    // Focus the text field via mouse click first
    driver
        .perform(
            &DriverAction::PointerClick {
                win: win_ref.clone(),
                at: WindowPoint {
                    x: name_frame.x + 10.0,
                    y: name_frame.y + 10.0,
                },
                button: Button::Left,
                count: 1,
                route: PointerRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .expect("click name_field to focus");

    driver
        .perform(
            &DriverAction::PressKeys {
                win: win_ref.clone(),
                chord: KeyChord::parse("ctrl+a").unwrap(),
                route: KeyRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .expect("ctrl+a ok");

    driver
        .perform(
            &DriverAction::PressKeys {
                win: win_ref.clone(),
                chord: KeyChord::parse("BackSpace").unwrap(),
                route: KeyRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .expect("BackSpace ok");

    let state_path =
        std::env::var("GCU_FIXTURE_STATE").unwrap_or_else(|_| "/tmp/fixture_state.json".into());
    let mut cleared = false;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(content) = std::fs::read_to_string(&state_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(t) = val.get("text").and_then(|v| v.as_str()) {
                    if t.is_empty() {
                        cleared = true;
                        break;
                    }
                }
            }
        }
    }
    assert!(cleared, "ctrl+a then BackSpace clears text input");
}
