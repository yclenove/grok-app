//! Portable AX tree tests. Live macOS smokes are ignored.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::Arc,
    time::{Duration, Instant},
};

use gcu_core::driver::{
    AppQuery, Cancel, CaptureOpts, Driver, DriverAction, DriverError, TreeOpts, WindowRef,
};
use gcu_core::model::{NodeValue, Rect, Role, States, TextRange};
use gcu_driver_macos::ax::{
    build_raw_tree, map_ax_role, needs_enhanced_accessibility, role_table, select_children,
    AccessibilityToggles, AxEventSource, AxStateBits, CountRelease, EventHub, FakeAxEvents,
    FakeAxNode, FakeAxSource, FakeToggleStore, Owned, RunLoopControl, ToggleName,
    MAX_NODES_CEILING,
};
use gcu_driver_macos::testing::{cg_window, rect, signed_facts, FakeMacApi};
use gcu_driver_macos::MacosDriver;

fn point(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect { x, y, w, h }
}

fn group(id: u64, title: &str) -> FakeAxNode {
    FakeAxNode {
        id,
        role: "AXGroup".into(),
        title: Some(title.into()),
        ..FakeAxNode::default()
    }
}

fn opts_timeout(timeout: Duration) -> TreeOpts {
    TreeOpts {
        timeout,
        ..TreeOpts::default()
    }
}

#[test]
fn role_table_maps_at_least_50_roles() {
    let table = role_table();
    assert!(table.len() >= 50);
    for row in table {
        let mapped = map_ax_role(row.ax, None);
        assert_eq!(mapped.role, row.role, "{}", row.ax);
        assert_eq!(mapped.native_role.as_str(), row.ax);
        assert_eq!(mapped.password, row.password, "{}", row.ax);
        assert_eq!(mapped.modal, row.modal, "{}", row.ax);
    }
    let password = map_ax_role("AXSecureTextField", None);
    assert_eq!(password.role, Role::TextField);
    assert!(password.password);
    let sheet = map_ax_role("AXSheet", None);
    assert_eq!(sheet.role, Role::Sheet);
    assert!(sheet.modal);
    let dialog = map_ax_role("AXWindow", Some("AXDialog"));
    assert_eq!(dialog.role, Role::Dialog);
    assert!(dialog.modal);
    assert_eq!(dialog.subrole.as_deref(), Some("AXDialog"));
    let unknown = map_ax_role("AXColorWell", None);
    assert_eq!(unknown.role, Role::Unknown);
    assert_eq!(unknown.native_role.as_str(), "AXColorWell");
}

#[test]
fn visible_children_win_including_an_empty_list() {
    let visible = vec![1];
    let contents = vec![2];
    let children = vec![3];
    assert_eq!(
        select_children(Some(visible), Some(contents.clone()), children.clone()),
        vec![1]
    );
    assert_eq!(
        select_children(Some(vec![]), Some(contents.clone()), children.clone()),
        Vec::<i32>::new()
    );
    assert_eq!(
        select_children(None, Some(contents), children.clone()),
        vec![2]
    );
    assert_eq!(select_children(None, None, children), vec![3]);
}

#[tokio::test]
async fn tree_scales_frames_and_keeps_fields() {
    let child = FakeAxNode {
        id: 2,
        role: "AXButton".into(),
        title: Some("Save".into()),
        frame: Some(point(110.0, 220.0, 10.0, 10.0)),
        identifier: Some("save-id".into()),
        placeholder: Some("name".into()),
        help: Some("Saves".into()),
        description: Some("primary".into()),
        states: AxStateBits {
            focused: Some(true),
            ..AxStateBits::default()
        },
        ..FakeAxNode::default()
    };
    let root = FakeAxNode {
        id: 1,
        role: "AXWindow".into(),
        title: Some("Editor".into()),
        frame: Some(point(100.0, 200.0, 80.0, 40.0)),
        children: vec![child],
        ..FakeAxNode::default()
    };
    let tree = build_raw_tree(
        &FakeAxSource::default(),
        &root,
        &TreeOpts::default(),
        &Cancel::default(),
        2.0,
    )
    .await
    .unwrap();
    let root = tree.root.unwrap();
    assert_eq!(root.role, Role::Window);
    assert_eq!(root.frame, Some(point(0.0, 0.0, 160.0, 80.0)));
    assert_eq!(root.children.len(), 1);
    let child = &root.children[0];
    assert_eq!(child.role, Role::Button);
    assert_eq!(child.name.as_deref(), Some("Save"));
    assert_eq!(child.frame, Some(point(20.0, 40.0, 20.0, 20.0)));
    assert_eq!(child.identifier.as_deref(), Some("save-id"));
    assert_eq!(child.placeholder.as_deref(), Some("name"));
    assert_eq!(child.help.as_deref(), Some("Saves"));
    assert!(child
        .states
        .contains(States::ENABLED | States::FOCUSED | States::FOCUSABLE));
    assert_ne!(root.fingerprint, gcu_core::model::Fingerprint(0));
    let again = build_raw_tree(
        &FakeAxSource::default(),
        &root_node(),
        &TreeOpts::default(),
        &Cancel::default(),
        2.0,
    )
    .await
    .unwrap();
    assert_eq!(
        again.root.unwrap().fingerprint,
        build_raw_tree(
            &FakeAxSource::default(),
            &root_node(),
            &TreeOpts::default(),
            &Cancel::default(),
            2.0
        )
        .await
        .unwrap()
        .root
        .unwrap()
        .fingerprint
    );
}

fn root_node() -> FakeAxNode {
    FakeAxNode {
        id: 1,
        role: "AXWindow".into(),
        title: Some("Editor".into()),
        frame: Some(point(100.0, 200.0, 80.0, 40.0)),
        ..FakeAxNode::default()
    }
}

#[tokio::test]
async fn password_node_does_not_keep_the_secret() {
    let root = FakeAxNode {
        id: 1,
        role: "AXSecureTextField".into(),
        title: Some("Password".into()),
        value: Some(NodeValue::Text("s3cret".into())),
        selected_text_range: Some(TextRange {
            start: 0,
            length: 6,
        }),
        ..FakeAxNode::default()
    };
    let tree = build_raw_tree(
        &FakeAxSource::default(),
        &root,
        &TreeOpts::default(),
        &Cancel::default(),
        1.0,
    )
    .await
    .unwrap();
    let node = tree.root.unwrap();
    assert!(node.states.contains(States::PASSWORD | States::EDITABLE));
    assert_eq!(node.value, Some(NodeValue::Text("•••".into())));
    assert!(node.text_extras.is_none());
    let rendered = format!("{node:?}");
    assert!(!rendered.contains("s3cret"));
}

#[tokio::test]
async fn text_is_truncated_with_a_marker() {
    let root = FakeAxNode {
        id: 1,
        role: "AXTextArea".into(),
        value: Some(NodeValue::Text("abcdefghij".into())),
        number_of_characters: Some(25),
        ..FakeAxNode::default()
    };
    let opts = TreeOpts {
        text_limit: Some(4),
        ..TreeOpts::default()
    };
    let tree = build_raw_tree(
        &FakeAxSource::default(),
        &root,
        &opts,
        &Cancel::default(),
        1.0,
    )
    .await
    .unwrap();
    let NodeValue::Text(text) = tree.root.unwrap().value.unwrap() else {
        panic!("text value");
    };
    assert_eq!(text, "abcd…(+21 chars)");
}

#[tokio::test]
async fn budgets_stop_the_walk() {
    let mut root = group(1, "root");
    for id in 2..30 {
        root.children.push(group(id, "child"));
    }
    let opts = TreeOpts {
        max_tree_nodes: 5,
        ..TreeOpts::default()
    };
    let tree = build_raw_tree(
        &FakeAxSource::default(),
        &root,
        &opts,
        &Cancel::default(),
        1.0,
    )
    .await
    .unwrap();
    let root = tree.root.unwrap();
    assert_eq!(root.children.len(), 4);

    let opts = TreeOpts {
        max_tree_nodes: 10_000,
        max_tree_depth: 1,
        ..TreeOpts::default()
    };
    let deep = FakeAxNode {
        children: vec![group(2, "child")],
        ..group(1, "root")
    };
    let tree = build_raw_tree(
        &FakeAxSource::default(),
        &deep,
        &opts,
        &Cancel::default(),
        1.0,
    )
    .await
    .unwrap();
    assert!(tree.root.unwrap().children.is_empty());

    let opts = TreeOpts {
        max_tree_nodes: 0,
        ..TreeOpts::default()
    };
    let empty = build_raw_tree(
        &FakeAxSource::default(),
        &deep,
        &opts,
        &Cancel::default(),
        1.0,
    )
    .await
    .unwrap();
    assert!(empty.root.is_none());
    const _: () = assert!(MAX_NODES_CEILING == 5000);
}

#[tokio::test]
async fn timeout_and_cancel_stop_before_the_delay() {
    let source = FakeAxSource {
        delay: Duration::from_millis(200),
        ..FakeAxSource::default()
    };
    let started = Instant::now();
    let error = build_raw_tree(
        &source,
        &group(1, "root"),
        &opts_timeout(Duration::from_millis(20)),
        &Cancel::default(),
        1.0,
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        DriverError::Timeout {
            maybe_executed: false
        }
    );
    assert!(started.elapsed() < Duration::from_millis(150));

    let slow = FakeAxSource {
        delay: Duration::from_secs(30),
        ..FakeAxSource::default()
    };
    let cancel = Cancel::default();
    let cancel_task = cancel.clone();
    let started = Instant::now();
    let handle = tokio::spawn(async move {
        build_raw_tree(
            &slow,
            &group(1, "root"),
            &opts_timeout(Duration::from_secs(5)),
            &cancel_task,
            1.0,
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    cancel.cancel();
    let error = handle.await.unwrap().unwrap_err();
    assert_eq!(
        error,
        DriverError::Cancelled {
            maybe_executed: false
        }
    );
    assert!(started.elapsed() < Duration::from_secs(2));

    let pre = Cancel::default();
    pre.cancel();
    let error = build_raw_tree(
        &FakeAxSource {
            delay: Duration::from_secs(30),
            ..FakeAxSource::default()
        },
        &group(1, "root"),
        &TreeOpts::default(),
        &pre,
        1.0,
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        DriverError::Cancelled {
            maybe_executed: false
        }
    );
}

#[tokio::test]
async fn terminal_heuristic_marks_native_role() {
    for title in ["Integrated Terminal", "Web SSH", "Debug Console", "bash"] {
        let root = FakeAxNode {
            title: Some(title.into()),
            ..group(1, title)
        };
        let tree = build_raw_tree(
            &FakeAxSource::default(),
            &root,
            &TreeOpts::default(),
            &Cancel::default(),
            1.0,
        )
        .await
        .unwrap();
        let role = tree.root.unwrap().native_role;
        assert!(
            role.to_ascii_lowercase().contains("terminal"),
            "{title} -> {role}"
        );
    }
    let save = FakeAxNode {
        role: "AXButton".into(),
        title: Some("Save".into()),
        ..FakeAxNode::default()
    };
    let tree = build_raw_tree(
        &FakeAxSource::default(),
        &save,
        &TreeOpts::default(),
        &Cancel::default(),
        1.0,
    )
    .await
    .unwrap();
    assert!(!tree
        .root
        .unwrap()
        .native_role
        .to_ascii_lowercase()
        .contains("terminal"));
}

#[test]
fn toggles_restore_after_enable_and_after_panic() {
    let store = FakeToggleStore::shared();
    let toggles = AccessibilityToggles::new(Arc::clone(&store));
    toggles.enable(9).unwrap();
    assert_eq!(
        store.value(9, ToggleName::EnhancedUserInterface),
        Some(true)
    );
    assert_eq!(store.value(9, ToggleName::ManualAccessibility), Some(true));
    toggles.restore();
    assert_eq!(
        store.value(9, ToggleName::EnhancedUserInterface),
        Some(false)
    );
    assert_eq!(store.value(9, ToggleName::ManualAccessibility), Some(false));

    let store = FakeToggleStore::shared();
    let shared = Arc::clone(&store);
    let caught = catch_unwind(AssertUnwindSafe(|| {
        let toggles = AccessibilityToggles::new(shared);
        toggles.enable(3).unwrap();
        panic!("session dropped");
    }));
    assert!(caught.is_err());
    assert_eq!(
        store.value(3, ToggleName::EnhancedUserInterface),
        Some(false)
    );
}

#[test]
fn enhanced_accessibility_matches_electron_ids_only() {
    assert!(needs_enhanced_accessibility(
        "com.google.Chrome",
        "Google Chrome"
    ));
    assert!(needs_enhanced_accessibility("com.microsoft.VSCode", "Code"));
    assert!(needs_enhanced_accessibility(
        "com.example.electron.app",
        "Notes"
    ));
    assert!(needs_enhanced_accessibility(
        "com.example.Notes",
        "Electron"
    ));
    assert!(!needs_enhanced_accessibility(
        "com.apple.TextEdit",
        "TextEdit"
    ));
    assert!(!needs_enhanced_accessibility(
        "com.example.Notes",
        "Visual Studio Code"
    ));
}

#[test]
fn owned_value_releases_once() {
    let counter = CountRelease::new();
    drop(Owned::new(7, counter.clone()));
    assert_eq!(counter.hits(), 1);
    let mut owned = Owned::new(8, counter.clone());
    assert_eq!(owned.take(), Some(8));
    drop(owned);
    assert_eq!(counter.hits(), 1);
}

#[test]
fn run_loop_control_stops_on_drop() {
    let mut control = RunLoopControl::start();
    control.stop();
    assert!(control.finished());
}

#[tokio::test]
async fn fake_events_and_driver_snapshot() {
    let events = FakeAxEvents::new();
    let mut stream = events.subscribe("cg:1:2").unwrap();
    events.emit(gcu_core::driver::UiEvent::Changed);
    let event = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
    assert_eq!(event, Some(gcu_core::driver::UiEvent::Changed));

    let (driver, win) = session(true, false).await;
    let source = Arc::new(FakeAxSource::default());
    let root = FakeAxNode {
        role: "AXWindow".into(),
        title: Some("Editor".into()),
        frame: Some(point(0.0, 0.0, 10.0, 10.0)),
        ..FakeAxNode::default()
    };
    driver.inject_tree(source, root);
    let tree = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(tree.root.unwrap().role, Role::Window);
    let caps = driver.capabilities();
    assert!(caps.observe.tree && !caps.observe.screenshot && caps.observe.events);
    assert!(driver.subscribe(&win).is_some());

    let (ready, listed) = session(true, true).await;
    let windows = ready.list_windows(Some(&listed.app)).await.unwrap();
    let win = WindowRef {
        app: listed.app,
        id: windows[0].id.clone(),
    };
    assert_eq!(
        ready
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
    assert_eq!(
        ready
            .perform(&DriverAction::Activate { win }, &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
}

#[tokio::test]
async fn missing_accessibility_wins_over_injection() {
    let (driver, win) = session(false, true).await;
    driver.inject_tree(Arc::new(FakeAxSource::default()), group(1, "root"));
    assert_eq!(
        driver
            .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::PermissionMissing {
            permission: "accessibility".into(),
        }
    );
}

#[tokio::test]
async fn release_all_input_restores_tracked_toggles() {
    let (driver, _win) = session(true, true).await;
    let store = FakeToggleStore::shared();
    let toggles = AccessibilityToggles::new(Arc::clone(&store));
    toggles.enable(4).unwrap();
    driver.track_toggles(toggles);
    driver.release_all_input().await;
    assert_eq!(store.value(4, ToggleName::ManualAccessibility), Some(false));
}

#[tokio::test]
async fn hub_emit_reaches_a_subscriber() {
    let hub = EventHub::new();
    let mut stream = hub.subscribe();
    hub.emit(gcu_core::driver::UiEvent::GeometryChanged);
    let event = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
    assert_eq!(event, Some(gcu_core::driver::UiEvent::GeometryChanged));
}

async fn session(accessibility: bool, screen: bool) -> (MacosDriver, WindowRef) {
    let api = Arc::new(FakeMacApi::default());
    let facts = signed_facts("com.example.Editor", "/Applications/Editor.app");
    api.add_running(4, &facts);
    api.update(|state| {
        state.accessibility = accessibility;
        state.screen_recording = screen;
        state
            .cg_windows
            .push(cg_window(7, 4, "Editor", rect(0.0, 0.0, 80.0, 40.0)));
    });
    let driver = MacosDriver::with_api(api).await.unwrap();
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let win = WindowRef {
        app,
        id: "cg:7:4".into(),
    };
    (driver, win)
}

/// Ignored render smokes. Grant Accessibility, leave the apps open, then:
/// `cargo test -p gcu-driver-macos --test ax_tree -- --ignored --test-threads=1`
#[cfg(target_os = "macos")]
mod live {
    use super::*;
    use gcu_core::render::{render_text, RenderOpts};
    use gcu_core::tree::Pipeline;

    #[tokio::test]
    #[ignore = "grant Accessibility and Screen Recording; leave TextEdit, Finder, and System Settings open"]
    async fn textedit_finder_and_system_settings_render() {
        let driver = MacosDriver::new().await.expect("unelevated macOS driver");
        for name in ["TextEdit", "Finder", "System Settings"] {
            smoke(&driver, name).await;
        }
    }

    #[tokio::test]
    #[ignore = "set GCU_FIXTURE_APP; grant Accessibility and Screen Recording"]
    async fn fixture_when_launched_renders_a_tree() {
        let path = std::env::var("GCU_FIXTURE_APP").expect("Set GCU_FIXTURE_APP");
        let driver = MacosDriver::new().await.expect("unelevated macOS driver");
        let app = driver
            .resolve_app(&AppQuery::from(path.as_str()))
            .await
            .unwrap();
        if app.info.pid.is_none() {
            driver.launch_app(&app).await.unwrap();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            let resolved = driver
                .resolve_app(&AppQuery::from(path.as_str()))
                .await
                .unwrap();
            if resolved.info.pid.is_some() {
                if let Ok(window) = driver.key_window(&resolved).await {
                    smoke_window(&driver, &resolved, window).await;
                    return;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "fixture window did not appear"
            );
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    async fn smoke(driver: &MacosDriver, name: &str) {
        let app = driver.resolve_app(&AppQuery::from(name)).await.unwrap();
        let window = driver.key_window(&app).await.unwrap();
        smoke_window(driver, &app, window).await;
    }

    async fn smoke_window(
        driver: &MacosDriver,
        app: &gcu_core::driver::AppRef,
        window: gcu_protocol::WindowInfo,
    ) {
        let win = WindowRef {
            app: app.clone(),
            id: window.id.clone(),
        };
        let raw = driver
            .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap();
        let label = app.info.name.clone();
        assert!(raw.root.is_some(), "{label} had no accessibility root");
        let processed = Pipeline::default().run(raw, &TreeOpts::default());
        let text = render_text(&processed, &RenderOpts::new(app.info.clone(), window));
        assert!(!text.trim().is_empty(), "{label} render was empty");
    }
}
