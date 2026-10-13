//! Comprehensive AT-SPI accessibility tree test suite.
//!
//! Includes:
//! 1. Platform-independent pure logic tests driven by `FakeSource`
//! 2. Live Linux AT-SPI contract test running against `gcu-fixture` inside Xvfb (ignored by default)

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[cfg(target_os = "linux")]
use gcu_core::render::{render_text, RenderOpts};
use gcu_core::{
    driver::{AppQuery, Cancel, Driver, DriverError, TreeOpts, WindowRef},
    model::{NodeAction, NodeFlags, NodeValue, Rect, Role, States},
    tree::Pipeline,
};
use gcu_driver_linux::{
    atspi::{
        build_raw_tree,
        roles::map_atspi_role,
        sensitive::is_terminal_like,
        source::{FakeNode, FakeSource},
    },
    identity::{needs_reauth, same_process_instance, AppIdentity},
    LinuxDriver,
};
use gcu_protocol::{AppInfo, WindowInfo};
use std::{collections::HashMap, path::PathBuf, time::Duration};

#[tokio::test]
async fn fake_tree_build_basic_hierarchy_and_relative_coordinates() {
    let root = FakeNode {
        id: 1,
        role: atspi::Role::Frame,
        name: Some("Main Window".into()),
        extents: Some(Rect {
            x: 100.0,
            y: 200.0,
            w: 800.0,
            h: 600.0,
        }),
        children: vec![
            FakeNode {
                id: 2,
                role: atspi::Role::Button,
                name: Some("OK".into()),
                extents: Some(Rect {
                    x: 150.0,
                    y: 250.0,
                    w: 80.0,
                    h: 30.0,
                }),
                actions: vec![NodeAction::Press],
                ..Default::default()
            },
            FakeNode {
                id: 3,
                role: atspi::Role::Entry,
                name: Some("Username".into()),
                extents: Some(Rect {
                    x: 150.0,
                    y: 300.0,
                    w: 200.0,
                    h: 30.0,
                }),
                value: Some(NodeValue::Text("alice".into())),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let source = FakeSource::default();
    let opts = TreeOpts::default();
    let cancel = Cancel::default();

    let raw_tree = build_raw_tree(&source, &root, &opts, &cancel)
        .await
        .expect("tree should build");

    let root_node = raw_tree.root.expect("root should exist");
    assert_eq!(root_node.role, Role::Window);
    assert_eq!(root_node.name.as_deref(), Some("Main Window"));
    // Window root frame is normalized to origin (0, 0)
    assert_eq!(
        root_node.frame,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0
        })
    );
    assert_eq!(root_node.children.len(), 2);

    let button = &root_node.children[0];
    assert_eq!(button.role, Role::Button);
    assert_eq!(button.name.as_deref(), Some("OK"));
    // Relative to window origin (150 - 100 = 50, 250 - 200 = 50)
    assert_eq!(
        button.frame,
        Some(Rect {
            x: 50.0,
            y: 50.0,
            w: 80.0,
            h: 30.0
        })
    );
    assert_eq!(button.actions.as_slice(), &[NodeAction::Press]);

    let entry = &root_node.children[1];
    assert_eq!(entry.role, Role::TextField);
    assert_eq!(entry.name.as_deref(), Some("Username"));
    assert_eq!(
        entry.frame,
        Some(Rect {
            x: 50.0,
            y: 100.0,
            w: 200.0,
            h: 30.0
        })
    );
    assert_eq!(entry.value, Some(NodeValue::Text("alice".into())));
}

#[tokio::test]
async fn fake_tree_build_caps_at_hard_node_ceiling_5000() {
    fn make_branch(start_id: &mut u64, depth: usize) -> FakeNode {
        *start_id += 1;
        let id = *start_id;
        let mut children = Vec::new();
        if depth < 4 {
            for _ in 0..10 {
                children.push(make_branch(start_id, depth + 1));
            }
        }
        FakeNode {
            id,
            role: atspi::Role::Panel,
            name: Some(format!("node_{id}")),
            children,
            ..Default::default()
        }
    }

    let mut id_counter = 0;
    // Create a large tree with > 6000 nodes
    let mut root = FakeNode {
        id: 0,
        role: atspi::Role::Frame,
        name: Some("Root Window".into()),
        children: Vec::new(),
        ..Default::default()
    };
    for _ in 0..6 {
        root.children.push(make_branch(&mut id_counter, 0));
    }
    assert!(id_counter > 5500);

    let source = FakeSource::default();
    let opts = TreeOpts {
        max_tree_nodes: 10000, // caller asks for 10000, but driver caps at 5000
        ..Default::default()
    };
    let cancel = Cancel::default();

    let raw_tree = build_raw_tree(&source, &root, &opts, &cancel)
        .await
        .expect("tree should build");

    fn count_nodes(node: &gcu_core::model::RawNode) -> usize {
        1 + node.children.iter().map(count_nodes).sum::<usize>()
    }

    let total = count_nodes(&raw_tree.root.expect("root should exist"));
    assert!(
        total <= 5000,
        "total nodes should be capped at 5000, got {total}"
    );
}

#[tokio::test]
async fn fake_tree_build_enforces_timeout() {
    let root = FakeNode {
        id: 1,
        role: atspi::Role::Frame,
        name: Some("Window".into()),
        children: vec![FakeNode {
            id: 2,
            role: atspi::Role::Button,
            ..Default::default()
        }],
        ..Default::default()
    };

    let source = FakeSource {
        delay: Duration::from_millis(50),
        error_on_id: None,
    };
    let opts = TreeOpts {
        timeout: Duration::from_millis(10), // Insufficient timeout
        ..Default::default()
    };
    let cancel = Cancel::default();

    let res = build_raw_tree(&source, &root, &opts, &cancel).await;
    match res {
        Err(DriverError::Timeout { maybe_executed }) => {
            assert!(!maybe_executed);
        }
        other => panic!("expected Timeout, got {other:?}"),
    }
}

#[tokio::test]
async fn fake_tree_build_honors_cancellation() {
    let root = FakeNode {
        id: 1,
        role: atspi::Role::Frame,
        name: Some("Window".into()),
        ..Default::default()
    };

    let source = FakeSource::default();
    let opts = TreeOpts::default();
    let cancel = Cancel::default();
    cancel.cancel(); // Pre-cancelled

    let res = build_raw_tree(&source, &root, &opts, &cancel).await;
    match res {
        Err(DriverError::Cancelled { maybe_executed }) => {
            assert!(!maybe_executed);
        }
        other => panic!("expected Cancelled, got {other:?}"),
    }
}

#[tokio::test]
async fn fake_tree_build_maps_states_and_password() {
    let mut states = atspi::StateSet::default();
    states.insert(atspi::State::Enabled);
    states.insert(atspi::State::Focused);
    states.insert(atspi::State::Editable);
    states.insert(atspi::State::Checked);
    states.insert(atspi::State::Modal);

    let mut attrs = HashMap::new();
    attrs.insert("placeholder-text".into(), "Enter password".into());

    let root = FakeNode {
        id: 1,
        role: atspi::Role::Dialog,
        name: Some("Login Dialog".into()),
        states,
        attributes: attrs,
        children: vec![FakeNode {
            id: 2,
            role: atspi::Role::PasswordText,
            name: Some("Password Input".into()),
            value: Some(NodeValue::Text("super_secret".into())),
            ..Default::default()
        }],
        ..Default::default()
    };

    let source = FakeSource::default();
    let opts = TreeOpts::default();
    let cancel = Cancel::default();

    let raw_tree = build_raw_tree(&source, &root, &opts, &cancel)
        .await
        .expect("tree should build");

    let root_node = raw_tree.root.as_ref().expect("root should exist");
    assert!(root_node.states.contains(States::ENABLED));
    assert!(root_node.states.contains(States::FOCUSED));
    assert!(root_node.states.contains(States::EDITABLE));
    assert!(root_node.states.contains(States::SETTABLE));
    assert!(root_node.states.contains(States::CHECKED));
    assert!(root_node.states.contains(States::MODAL));

    let pass_node = &root_node.children[0];
    assert!(pass_node.states.contains(States::PASSWORD));

    // Verify that passing through the core pipeline censors the password
    let processed = Pipeline::default().run(raw_tree, &opts);
    let pass_processed = &processed.root.expect("processed root").children[0];
    assert_eq!(
        pass_processed.node.value,
        Some(NodeValue::Text("•••".into()))
    );
}

#[tokio::test]
async fn fake_tree_build_sensitive_terminal_like_marked() {
    let root = FakeNode {
        id: 1,
        role: atspi::Role::Frame,
        name: Some("Developer Environment".into()),
        children: vec![
            FakeNode {
                id: 2,
                role: atspi::Role::Terminal,
                name: Some("Embedded Console".into()),
                ..Default::default()
            },
            FakeNode {
                id: 3,
                role: atspi::Role::Panel,
                name: Some("Web SSH Session".into()),
                ..Default::default()
            },
            FakeNode {
                id: 4,
                role: atspi::Role::Button,
                name: Some("Save File".into()),
                actions: vec![NodeAction::Press],
                states: {
                    let mut s = atspi::StateSet::default();
                    s.insert(atspi::State::Enabled);
                    s.insert(atspi::State::Showing);
                    s
                },
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let source = FakeSource::default();
    let opts = TreeOpts::default();
    let cancel = Cancel::default();

    let raw_tree = build_raw_tree(&source, &root, &opts, &cancel)
        .await
        .expect("tree should build");

    let processed = Pipeline::default().run(raw_tree, &opts);
    let proc_root = processed.root.expect("processed root");

    assert_eq!(proc_root.children.len(), 3);
    // Terminal node has TERMINAL_LIKE flag
    assert!(proc_root.children[0]
        .flags
        .contains(NodeFlags::TERMINAL_LIKE));
    // Web SSH Session has TERMINAL_LIKE flag
    assert!(proc_root.children[1]
        .flags
        .contains(NodeFlags::TERMINAL_LIKE));
    // Ordinary button does NOT have TERMINAL_LIKE flag
    assert!(!proc_root.children[2]
        .flags
        .contains(NodeFlags::TERMINAL_LIKE));
}

#[tokio::test]
async fn linux_driver_fake_test_suite() {
    let fake_source = FakeSource::default();
    let root = FakeNode {
        id: 100,
        role: atspi::Role::Frame,
        name: Some("Fake App Window".into()),
        extents: Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 1024.0,
            h: 768.0,
        }),
        children: vec![FakeNode {
            id: 101,
            role: atspi::Role::Button,
            name: Some("Click Me".into()),
            ..Default::default()
        }],
        ..Default::default()
    };

    let apps = vec![AppInfo {
        id: ":fake-1".into(),
        name: "FakeApp".into(),
        pid: Some(9999),
        identity: None,
    }];

    let windows = vec![(
        ":fake-1".to_string(),
        WindowInfo {
            id: "/fake/win/100".into(),
            title: "Fake App Window".into(),
            frame: Rect {
                x: 0.0,
                y: 0.0,
                w: 1024.0,
                h: 768.0,
            },
            scale: 1.0,
            on_current_desktop: Some(true),
        },
    )];

    let driver = LinuxDriver::new_fake(fake_source, root, apps, windows);

    assert!(
        driver.capabilities().observe.tree,
        "observe.tree must be true"
    );
    assert!(
        driver.capabilities().observe.screenshot,
        "observe.screenshot must be true"
    );

    let doc = driver.doctor().await;
    assert!(doc.ready);

    let listed_apps = driver.list_apps().await.expect("list_apps");
    assert_eq!(listed_apps.len(), 1);
    assert_eq!(listed_apps[0].name, "FakeApp");

    let resolved = driver
        .resolve_app(&AppQuery("fake".into()))
        .await
        .expect("resolve_app");
    assert_eq!(resolved.info.name, "FakeApp");

    let key_win = driver.key_window(&resolved).await.expect("key_window");
    assert_eq!(key_win.title, "Fake App Window");

    let win_ref = WindowRef {
        app: resolved,
        id: key_win.id,
    };

    let opts = TreeOpts::default();
    let cancel = Cancel::default();
    let tree = driver
        .snapshot_tree(&win_ref, &opts, &cancel)
        .await
        .expect("snapshot_tree");
    let root_node = tree.root.expect("root node");
    assert_eq!(root_node.name.as_deref(), Some("Fake App Window"));
    assert_eq!(root_node.children.len(), 1);
    assert_eq!(root_node.children[0].name.as_deref(), Some("Click Me"));
}

#[test]
fn test_identity_reauth_scenarios_table() {
    let base = AppIdentity {
        exe_path: PathBuf::from("/usr/bin/app"),
        dev: 42,
        inode: 1001,
        sha256: "aabbcc".into(),
        pid: 1234,
        start_time: 500,
        desktop_id: Some("app.desktop".into()),
    };

    // Identical
    assert!(!needs_reauth(&base, &base));

    // In-place binary modification
    let mut updated_hash = base.clone();
    updated_hash.sha256 = "ddeeff".into();
    assert!(needs_reauth(&base, &updated_hash));

    // In-place file replaced with new inode
    let mut new_inode = base.clone();
    new_inode.inode = 1002;
    assert!(needs_reauth(&base, &new_inode));

    // Path replaced
    let mut new_path = base.clone();
    new_path.exe_path = PathBuf::from("/usr/bin/app_v2");
    assert!(needs_reauth(&base, &new_path));

    // PID reused: a persistent grant still applies (same image), but the process instance differs
    let mut pid_reused = base.clone();
    pid_reused.start_time = 600;
    assert!(!needs_reauth(&base, &pid_reused));
    assert!(!same_process_instance(&base, &pid_reused));

    // Desktop id updated but binary identical -> does not need reauth
    let mut new_desktop = base.clone();
    new_desktop.desktop_id = Some("app2.desktop".into());
    assert!(!needs_reauth(&base, &new_desktop));
}

#[test]
fn test_roles_mapping_table_integration() {
    assert_eq!(map_atspi_role(atspi::Role::Button).0, Role::Button);
    assert_eq!(map_atspi_role(atspi::Role::Entry).0, Role::TextField);
    assert_eq!(map_atspi_role(atspi::Role::PasswordText).0, Role::TextField);
    assert_eq!(map_atspi_role(atspi::Role::CheckBox).0, Role::CheckBox);
    assert_eq!(map_atspi_role(atspi::Role::RadioButton).0, Role::Radio);
    assert_eq!(map_atspi_role(atspi::Role::Terminal).0, Role::Unknown);
    assert_eq!(map_atspi_role(atspi::Role::DocumentWeb).0, Role::WebArea);
}

#[test]
fn test_terminal_like_heuristic_integration() {
    assert!(is_terminal_like(
        atspi::Role::Terminal,
        "terminal",
        Some("random")
    ));
    assert!(is_terminal_like(
        atspi::Role::Panel,
        "panel",
        Some("gnome-terminal")
    ));
    assert!(is_terminal_like(
        atspi::Role::Panel,
        "panel",
        Some("ssh session")
    ));
    assert!(!is_terminal_like(
        atspi::Role::Button,
        "button",
        Some("Submit")
    ));
}

/// Live AT-SPI test running against the fixture inside Xvfb.
///
/// Marked `#[ignore]` so it only runs when explicitly invoked by CI with `-- --ignored`.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn live_atspi_fixture_snapshot_send_button() {
    let driver = LinuxDriver::connect()
        .await
        .expect("must connect to live AT-SPI bus");

    // Discover apps or retry briefly for fixture to appear
    let mut fixture_app = None;
    for _ in 0..40 {
        if let Ok(app) = driver.resolve_app(&AppQuery("fixture".into())).await {
            fixture_app = Some(app);
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let app = fixture_app.expect("gcu-fixture application must be discoverable on AT-SPI bus");

    let win_info = driver
        .key_window(&app)
        .await
        .expect("fixture key window must be discoverable");

    let win_ref = WindowRef {
        app: app.clone(),
        id: win_info.id.clone(),
    };

    let opts = TreeOpts {
        timeout: Duration::from_secs(5),
        max_tree_nodes: 5000,
        ..Default::default()
    };
    let cancel = Cancel::default();

    let raw_tree = driver
        .snapshot_tree(&win_ref, &opts, &cancel)
        .await
        .expect("snapshot_tree should succeed on live fixture");

    assert!(
        raw_tree.root.is_some(),
        "fixture raw tree must have a root node"
    );

    // Pass through full core pipeline
    let processed = Pipeline::default().run(raw_tree, &opts);
    let render_opts = RenderOpts::new(app.info.clone(), win_info.clone());
    let rendered = render_text(&processed, &render_opts);

    println!(
        "=== RENDERED AT-SPI FIXTURE TREE ===\n{rendered}\n===================================="
    );

    assert!(
        rendered.contains("send_button") || rendered.contains("发送"),
        "rendered text must contain 'send_button' or '发送', got:\n{rendered}"
    );
}
