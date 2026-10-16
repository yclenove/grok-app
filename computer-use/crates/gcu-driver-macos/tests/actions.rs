//! Portable semantic-action tests. The live macOS oracle is ignored.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gcu_core::driver::{
    AppQuery, Button, Cancel, Delivery, Dir, Driver, DriverAction, DriverError, PointerRoute,
    Result, WindowRef, DRIVER_DEADLINE,
};
use gcu_core::model::{NativeHandle, NodeAction, NodeValue, Rect, TextRange, WindowPoint};
use gcu_driver_macos::ax::{perform, AxSource, AxStateBits, FakeAct, FakeAxNode, FakeAxSource};
use gcu_driver_macos::testing::{cg_window, rect, signed_facts, FakeMacApi};
use gcu_driver_macos::{api::MacApi, MacElement, MacosDriver};

fn button(id: u64, actions: &[&str]) -> FakeAxNode {
    FakeAxNode {
        id,
        role: "AXButton".into(),
        title: Some("Go".into()),
        frame: Some(Rect {
            x: 1.0,
            y: 2.0,
            w: 30.0,
            h: 16.0,
        }),
        raw_actions: Some(actions.iter().map(|name| (*name).to_string()).collect()),
        states: AxStateBits {
            enabled: Some(true),
            ..AxStateBits::default()
        },
        ..FakeAxNode::default()
    }
}

fn field(role: &str, settable: bool) -> FakeAxNode {
    let mut node = button(1, &[]);
    node.role = role.into();
    node.states.settable = settable;
    node.states.editable = settable;
    node
}

fn script(fill: impl FnOnce(&mut FakeAct)) -> FakeAct {
    let mut act = FakeAct::default();
    fill(&mut act);
    act
}

async fn run(
    node: FakeAxNode,
    act: FakeAct,
    action: impl Fn(NativeHandle) -> DriverAction,
    deadline: Duration,
    cancel: &Cancel,
) -> (Result<Delivery>, Vec<String>) {
    let source = FakeAxSource {
        act: Arc::new(Mutex::new(act)),
        ..FakeAxSource::default()
    };
    let handle = source.handle(&node);
    source.bind_root(node);
    let result = perform(&source, &action(handle), cancel, deadline).await;
    (result, source.calls())
}

async fn press(node: FakeAxNode, act: FakeAct) -> (Result<Delivery>, Vec<String>) {
    run(
        node,
        act,
        |el| DriverAction::Press { el },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await
}

fn no_focus(calls: &[String]) {
    assert!(calls.iter().all(|entry| {
        !entry.starts_with("perform:AXRaise")
            && !entry.starts_with("perform:AXFocus")
            && !entry.starts_with("set:AXFocused")
    }));
}

#[tokio::test]
async fn press_axpress_performs_once() {
    let (result, calls) = press(button(1, &["AXPress"]), FakeAct::default()).await;
    assert_eq!(result.unwrap(), Delivery::Accessibility);
    assert_eq!(calls, ["perform:AXPress"]);
    no_focus(&calls);
}

#[tokio::test]
async fn press_missing_axpress_falls_through_to_confirm() {
    let (result, calls) = press(button(1, &["AXConfirm"]), FakeAct::default()).await;
    assert_eq!(result.unwrap(), Delivery::Accessibility);
    assert_eq!(calls, ["skip:AXPress", "perform:AXConfirm"]);
    no_focus(&calls);
}

#[tokio::test]
async fn press_unsupported_result_continues_once() {
    let act = script(|act| act.unsupported = vec!["AXPress".into()]);
    let (result, calls) = press(button(1, &["AXPress", "AXConfirm"]), act).await;
    assert_eq!(result.unwrap(), Delivery::Accessibility);
    assert_eq!(calls, ["perform:AXPress", "perform:AXConfirm"]);
}

#[tokio::test]
async fn press_disabled_element_does_not_call_and_presses_descendant() {
    let mut parent = button(1, &["AXPress"]);
    parent.states.enabled = Some(false);
    parent.children = vec![button(2, &["AXPress"])];
    let (result, calls) = press(parent, FakeAct::default()).await;
    assert_eq!(result.unwrap(), Delivery::Accessibility);
    assert_eq!(
        calls,
        [
            "unavailable:AXPress",
            "skip:AXConfirm",
            "skip:AXOpen",
            "perform:AXPress"
        ]
    );
    no_focus(&calls);
}

#[tokio::test]
async fn press_uncertain_outcome_does_not_try_the_next_action() {
    let act = script(|act| act.uncertain = vec!["AXPress".into()]);
    let (result, calls) = press(button(1, &["AXPress", "AXConfirm"]), act).await;
    assert_eq!(
        result.unwrap_err(),
        DriverError::Native {
            message: "accessibility action outcome is unknown".into(),
            maybe_executed: true,
        }
    );
    assert_eq!(calls, ["perform:AXPress"]);
}

#[tokio::test]
async fn press_only_raise_is_not_a_click() {
    let (result, calls) = press(button(1, &["AXRaise", "AXFocus"]), FakeAct::default()).await;
    assert_eq!(result.unwrap_err(), DriverError::ElementNotActionable);
    assert_eq!(calls, ["skip:AXPress", "skip:AXConfirm", "skip:AXOpen"]);
    assert!(calls
        .iter()
        .all(|entry| !entry.contains("AXRaise") && !entry.contains("AXFocus")));
}

#[tokio::test]
async fn press_selects_a_row_without_raising() {
    let row = FakeAxNode {
        id: 2,
        role: "AXRow".into(),
        title: Some("Row".into()),
        raw_actions: Some(Vec::new()),
        ..button(2, &[])
    };
    let list = FakeAxNode {
        id: 1,
        role: "AXList".into(),
        raw_actions: Some(Vec::new()),
        children: vec![row.clone()],
        ..FakeAxNode::default()
    };
    let source = FakeAxSource::default();
    source.bind_root(list);
    let result = perform(
        &source,
        &DriverAction::Press {
            el: source.handle(&row),
        },
        &Cancel::default(),
        DRIVER_DEADLINE,
    )
    .await;
    assert_eq!(result.unwrap(), Delivery::Accessibility);
    assert_eq!(
        source.calls(),
        [
            "skip:AXPress",
            "skip:AXConfirm",
            "skip:AXOpen",
            "select:AXSelectedChildren"
        ]
    );
    no_focus(&source.calls());
}

#[tokio::test]
async fn press_stale_id_is_not_replaced_by_a_same_title_sibling() {
    let first = button(1, &["AXPress"]);
    let root = FakeAxNode {
        id: 9,
        role: "AXGroup".into(),
        children: vec![first.clone(), button(2, &["AXPress"])],
        ..FakeAxNode::default()
    };
    let source = FakeAxSource {
        act: Arc::new(Mutex::new(script(|act| act.missing = vec![1]))),
        ..FakeAxSource::default()
    };
    source.bind_root(root);
    let error = perform(
        &source,
        &DriverAction::Press {
            el: source.handle(&first),
        },
        &Cancel::default(),
        DRIVER_DEADLINE,
    )
    .await
    .unwrap_err();
    assert_eq!(error, DriverError::StaleElement);
    assert!(source.calls().is_empty());
}

#[tokio::test]
async fn press_identity_change_is_stale_and_does_not_retry() {
    let act = script(|act| act.twist_role = true);
    let (result, calls) = press(button(1, &["AXPress"]), act).await;
    assert_eq!(result.unwrap_err(), DriverError::StaleElement);
    assert!(calls.is_empty());
}

#[tokio::test]
async fn press_frame_epsilon_accepts_half_a_point_and_rejects_a_larger_move() {
    let mut near = button(1, &["AXPress"]);
    let source = FakeAxSource::default();
    let handle = source.handle(&near);
    near.frame.as_mut().unwrap().x += 0.4;
    source.bind_root(near);
    assert_eq!(
        perform(
            &source,
            &DriverAction::Press { el: handle },
            &Cancel::default(),
            DRIVER_DEADLINE
        )
        .await
        .unwrap(),
        Delivery::Accessibility
    );
    let mut far = button(1, &["AXPress"]);
    let source = FakeAxSource::default();
    let handle = source.handle(&far);
    far.frame.as_mut().unwrap().x += 2.0;
    source.bind_root(far);
    assert_eq!(
        perform(
            &source,
            &DriverAction::Press { el: handle },
            &Cancel::default(),
            DRIVER_DEADLINE
        )
        .await
        .unwrap_err(),
        DriverError::StaleElement
    );
    assert!(source.calls().is_empty());
}

#[tokio::test]
async fn press_timeout_and_cancel_distinguish_maybe_executed() {
    let slow_id = script(|act| act.hang_identity = Some(Duration::from_secs(2)));
    let (result, calls) = run(
        button(1, &["AXPress"]),
        slow_id,
        |el| DriverAction::Press { el },
        Duration::from_millis(40),
        &Cancel::default(),
    )
    .await;
    assert_eq!(
        result.unwrap_err(),
        DriverError::Timeout {
            maybe_executed: false
        }
    );
    assert!(calls.is_empty());

    let slow = script(|act| act.hang_mutation = Some(Duration::from_secs(2)));
    let (result, calls) = run(
        button(1, &["AXPress", "AXConfirm"]),
        slow,
        |el| DriverAction::Press { el },
        Duration::from_millis(40),
        &Cancel::default(),
    )
    .await;
    assert_eq!(
        result.unwrap_err(),
        DriverError::Timeout {
            maybe_executed: true
        }
    );
    assert_eq!(calls, ["perform:AXPress"]);

    let cancel = Cancel::default();
    cancel.cancel();
    let (result, calls) = run(
        button(1, &["AXPress"]),
        FakeAct::default(),
        |el| DriverAction::Press { el },
        DRIVER_DEADLINE,
        &cancel,
    )
    .await;
    assert_eq!(
        result.unwrap_err(),
        DriverError::Cancelled {
            maybe_executed: false
        }
    );
    assert!(calls.is_empty());

    let cancel = Cancel::default();
    let flag = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        flag.cancel();
    });
    let hanging = script(|act| act.hang_mutation = Some(Duration::from_secs(2)));
    let (result, calls) = run(
        button(1, &["AXPress", "AXConfirm"]),
        hanging,
        |el| DriverAction::Press { el },
        Duration::from_secs(2),
        &cancel,
    )
    .await;
    assert_eq!(
        result.unwrap_err(),
        DriverError::Cancelled {
            maybe_executed: true
        }
    );
    assert_eq!(calls, ["perform:AXPress"]);
}

#[tokio::test]
async fn secondary_performs_show_menu_and_refuses_focus_substitutes() {
    let (result, calls) = run(
        button(1, &["AXShowMenu"]),
        FakeAct::default(),
        |el| DriverAction::SecondaryAction {
            el,
            action: NodeAction::ShowMenu,
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap(), Delivery::Accessibility);
    assert_eq!(calls, ["perform:AXShowMenu"]);

    let (result, calls) = run(
        button(1, &["AXRaise"]),
        FakeAct::default(),
        |el| DriverAction::SecondaryAction {
            el,
            action: NodeAction::Raise,
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::ElementNotActionable);
    assert_eq!(calls, ["refuse:AXRaise"]);

    let (result, calls) = run(
        button(1, &["AXFocused"]),
        FakeAct::default(),
        |el| DriverAction::SecondaryAction {
            el,
            action: NodeAction::Native("AXFocused".into()),
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::ElementNotActionable);
    assert_eq!(calls, ["refuse:AXFocused"]);

    let (result, calls) = run(
        button(1, &["AXPress"]),
        FakeAct::default(),
        |el| DriverAction::SecondaryAction {
            el,
            action: NodeAction::ShowMenu,
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::ElementNotActionable);
    assert_eq!(calls, ["skip:AXShowMenu"]);
}

#[tokio::test]
async fn set_value_writes_only_settable_non_sensitive_values() {
    let secret = "s3cret-value";
    let mut node = field("AXTextField", true);
    node.value = Some(NodeValue::Text("old".into()));
    let (result, calls) = run(
        node,
        FakeAct::default(),
        |el| DriverAction::SetValue {
            el,
            value: secret.into(),
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap(), Delivery::Accessibility);
    assert_eq!(calls, ["set:AXValue"]);
    assert!(calls.iter().all(|entry| !entry.contains(secret)));

    let (result, calls) = run(
        field("AXTextField", false),
        FakeAct::default(),
        |el| DriverAction::SetValue {
            el,
            value: "next".into(),
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::NotSettable);
    assert_eq!(calls, ["refuse:not-settable"]);

    let (result, calls) = run(
        field("AXSecureTextField", true),
        FakeAct::default(),
        |el| DriverAction::SetValue {
            el,
            value: "hunter2".into(),
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::NotSettable);
    assert_eq!(calls, ["refuse:sensitive"]);
    assert!(calls.iter().all(|entry| !entry.contains("hunter2")));

    let mut terminal = field("AXTextArea", true);
    terminal.title = Some("Terminal".into());
    let (result, calls) = run(
        terminal,
        FakeAct::default(),
        |el| DriverAction::SetValue {
            el,
            value: "ls".into(),
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::NotSettable);
    assert_eq!(calls, ["refuse:sensitive"]);

    let mut slider = field("AXSlider", true);
    slider.value = Some(NodeValue::Number(50.0));
    let source = FakeAxSource::default();
    let handle = source.handle(&slider);
    source.bind_root(slider);
    assert_eq!(
        perform(
            &source,
            &DriverAction::SetValue {
                el: handle.clone(),
                value: "40".into()
            },
            &Cancel::default(),
            DRIVER_DEADLINE,
        )
        .await
        .unwrap(),
        Delivery::Accessibility
    );
    assert_eq!(
        perform(
            &source,
            &DriverAction::SetValue {
                el: handle,
                value: "nope".into()
            },
            &Cancel::default(),
            DRIVER_DEADLINE,
        )
        .await
        .unwrap_err(),
        DriverError::NotSettable
    );
    assert_eq!(source.calls(), ["set:AXValue", "refuse:not-settable"]);

    let act = script(|act| act.uncertain = vec!["AXValue".into()]);
    let (result, calls) = run(
        field("AXTextField", true),
        act,
        |el| DriverAction::SetValue {
            el,
            value: "next".into(),
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert!(result.unwrap_err().maybe_executed());
    assert_eq!(calls, ["set:AXValue"]);
}

#[tokio::test]
async fn select_text_requires_a_settable_native_range() {
    let (result, calls) = run(
        field("AXTextField", true),
        FakeAct::default(),
        |el| DriverAction::SelectText {
            el,
            range: TextRange {
                start: 1,
                length: 2,
            },
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap(), Delivery::Accessibility);
    assert_eq!(calls, ["set:AXSelectedTextRange"]);

    let (result, calls) = run(
        field("AXStaticText", false),
        FakeAct::default(),
        |el| DriverAction::SelectText {
            el,
            range: TextRange {
                start: 0,
                length: 1,
            },
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::ElementNotActionable);
    assert!(calls.is_empty());

    let (result, calls) = run(
        field("AXTextField", true),
        FakeAct::default(),
        |el| DriverAction::SelectText {
            el,
            range: TextRange {
                start: usize::MAX,
                length: 1,
            },
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::InvalidRequest);
    assert!(calls.is_empty());
}

#[tokio::test]
async fn scroll_uses_page_actions_and_stops_on_an_uncertain_page() {
    let (result, calls) = run(
        button(1, &["AXScrollDownByPage"]),
        FakeAct::default(),
        |el| DriverAction::ScrollElement {
            el,
            dir: Dir::Down,
            pages: 1.2,
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap(), Delivery::Accessibility);
    assert_eq!(
        calls,
        ["perform:AXScrollDownByPage", "perform:AXScrollDownByPage"]
    );

    let (result, calls) = run(
        button(1, &["AXPress"]),
        FakeAct::default(),
        |el| DriverAction::ScrollElement {
            el,
            dir: Dir::Up,
            pages: 1.0,
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::CapabilityUnavailable);
    assert_eq!(calls, ["skip:AXScrollUpByPage"]);

    let act = script(|act| act.succeed_times = Some(1));
    let (result, calls) = run(
        button(1, &["AXScrollDownByPage"]),
        act,
        |el| DriverAction::ScrollElement {
            el,
            dir: Dir::Down,
            pages: 3.0,
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert!(result.unwrap_err().maybe_executed());
    assert_eq!(
        calls,
        ["perform:AXScrollDownByPage", "perform:AXScrollDownByPage"]
    );

    let (result, calls) = run(
        button(1, &["AXScrollDownByPage"]),
        FakeAct::default(),
        |el| DriverAction::ScrollElement {
            el,
            dir: Dir::Down,
            pages: 0.0,
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::InvalidRequest);
    assert!(calls.is_empty());

    let (result, calls) = run(
        button(2, &["AXScrollDownByPage"]),
        script(|act| act.missing = vec![2]),
        |el| DriverAction::ScrollElement {
            el,
            dir: Dir::Down,
            pages: f32::NAN,
        },
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), DriverError::StaleElement);
    assert!(calls.is_empty());
}

#[tokio::test]
async fn set_select_and_scroll_are_stale_before_any_call() {
    let actions: [fn(NativeHandle) -> DriverAction; 3] = [
        |el| DriverAction::SetValue {
            el,
            value: "n".into(),
        },
        |el| DriverAction::SelectText {
            el,
            range: TextRange {
                start: 0,
                length: 1,
            },
        },
        |el| DriverAction::ScrollElement {
            el,
            dir: Dir::Left,
            pages: 1.0,
        },
    ];
    for action in actions {
        let mut node = field("AXTextField", true);
        node.id = 4;
        node.raw_actions = Some(vec!["AXScrollLeftByPage".into()]);
        let (result, calls) = run(
            node,
            script(|act| act.missing = vec![4]),
            action,
            DRIVER_DEADLINE,
            &Cancel::default(),
        )
        .await;
        assert_eq!(result.unwrap_err(), DriverError::StaleElement);
        assert!(calls.is_empty());
    }
}

async fn session(accessibility: bool) -> (Arc<FakeMacApi>, MacosDriver, WindowRef) {
    let api = Arc::new(FakeMacApi::default());
    let facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
    api.add_running(4, &facts);
    api.update(|state| {
        state.accessibility = accessibility;
        state.frontmost = Some(11);
        state
            .cg_windows
            .push(cg_window(7, 4, "Editor", rect(0.0, 0.0, 80.0, 40.0)));
    });
    let driver = MacosDriver::with_api(Arc::clone(&api) as Arc<dyn MacApi>)
        .await
        .unwrap();
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let windows = driver.list_windows(Some(&app)).await.unwrap();
    let win = WindowRef {
        app,
        id: windows[0].id.clone(),
    };
    (api, driver, win)
}

#[tokio::test]
async fn driver_press_does_not_raise_or_activate() {
    let (api, driver, _win) = session(true).await;
    let source = Arc::new(FakeAxSource::default());
    let node = button(1, &["AXPress"]);
    let handle = source.handle(&node);
    driver.inject_tree(source, node);
    assert_eq!(
        driver
            .perform(&DriverAction::Press { el: handle }, &Cancel::default())
            .await
            .unwrap(),
        Delivery::Accessibility
    );
    let state = api.state().unwrap();
    assert!(state.raises.is_empty());
    assert_eq!(state.frontmost, Some(11));
}

#[tokio::test]
async fn driver_semantic_press_host_stopped_cancels_before_ax() {
    use gcu_driver_macos::input::{
        activity::ActivityMonitor,
        clock::RuntimeClock,
        hotkey::{Guards, SharedStopLatch},
        FakeEvents,
    };
    let (_api, mut driver, _win) = session(true).await;
    let source = Arc::new(FakeAxSource::default());
    let node = button(1, &["AXPress"]);
    let handle = source.handle(&node);
    driver.inject_tree(source.clone(), node);
    for stopped in [true, false] {
        let clock = Arc::new(RuntimeClock::default());
        let events = Arc::new(FakeEvents::ready());
        events.set_guards(Arc::new(Guards::new(
            clock.clone(),
            Arc::new(ActivityMonitor::new(clock)),
            Arc::new(SharedStopLatch::new(stopped)),
            3000,
        )));
        driver.inject_events(events);
        let result = driver
            .perform(
                &DriverAction::Press { el: handle.clone() },
                &Cancel::default(),
            )
            .await;
        if stopped {
            assert_eq!(
                result,
                Err(DriverError::Cancelled {
                    maybe_executed: false
                })
            );
            assert!(source.calls().is_empty());
        } else {
            assert_eq!(result, Ok(Delivery::Accessibility));
            assert_eq!(source.calls(), ["perform:AXPress"]);
        }
    }
}

#[tokio::test]
async fn driver_reports_semantic_actions_without_foreground() {
    let (_api, driver, win) = session(true).await;
    let caps = driver.capabilities();
    assert!(
        caps.version.contains("P1-03")
            && caps.version.contains("P1-04")
            && caps.version.contains("P1-05")
    );
    assert!(caps.delivery.accessibility);
    assert!(!caps.delivery.targeted_pointer && !caps.delivery.targeted_keys);
    assert_eq!(
        caps.delivery.foreground,
        gcu_protocol::capabilities::ForegroundCapability::Unavailable
    );
    assert_eq!(
        caps.background_operation,
        gcu_protocol::capabilities::BackgroundOperation::Partial
    );
    assert_eq!(
        driver
            .perform(
                &DriverAction::Activate { win: win.clone() },
                &Cancel::default()
            )
            .await
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
    assert_eq!(
        driver
            .perform(
                &DriverAction::PointerClick {
                    win,
                    at: WindowPoint { x: 1.0, y: 1.0 },
                    button: Button::Left,
                    count: 1,
                    route: PointerRoute::TargetedOnly,
                },
                &Cancel::default(),
            )
            .await
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
}

#[tokio::test]
async fn driver_semantic_action_without_a_source_is_not_connected() {
    let (_api, driver, _win) = session(true).await;
    let handle = FakeAxSource::default().handle(&button(1, &["AXPress"]));
    assert_eq!(
        driver
            .perform(&DriverAction::Press { el: handle }, &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::Native {
            message: "accessibility action source is not connected".into(),
            maybe_executed: false,
        }
    );
}

#[tokio::test]
async fn driver_wrong_handle_type_is_stale() {
    let (_api, driver, win) = session(true).await;
    driver.inject_tree(Arc::new(FakeAxSource::default()), button(1, &["AXPress"]));
    assert_eq!(
        driver
            .perform(
                &DriverAction::Press {
                    el: NativeHandle::new(MacElement { window: win })
                },
                &Cancel::default(),
            )
            .await
            .unwrap_err(),
        DriverError::StaleElement
    );
}

#[tokio::test]
async fn driver_missing_accessibility_does_not_run_the_action() {
    let api = Arc::new(FakeMacApi::default());
    api.update(|state| {
        state.accessibility = false;
    });
    let driver = MacosDriver::with_api(Arc::clone(&api) as Arc<dyn MacApi>)
        .await
        .unwrap();
    let source = Arc::new(FakeAxSource::default());
    let node = button(1, &["AXPress"]);
    let handle = source.handle(&node);
    driver.inject_tree(Arc::clone(&source), node);
    assert_eq!(
        driver
            .perform(&DriverAction::Press { el: handle }, &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::PermissionMissing {
            permission: "accessibility".into()
        }
    );
    assert!(source.calls().is_empty());
}

#[cfg(target_os = "macos")]
#[path = "actions/live.rs"]
mod live;
