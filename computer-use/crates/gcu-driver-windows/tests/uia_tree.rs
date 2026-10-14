//! Portable UIA tree contracts. No Windows desktop or COM required.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_core::{
    driver::{Cancel, TreeOpts},
    model::{IncompleteReason, NodeValue, Role, States},
};
use gcu_driver_windows::uia::{
    cache::Properties,
    source::{FakeNode, FakeUia},
    tree::build_raw_tree,
};

fn fixture(children: usize) -> FakeUia {
    FakeUia::new(FakeNode {
        properties: Properties {
            control_type: 50032,
            name: Some("Fixture".into()),
            ..Default::default()
        },
        children: (0..children).map(|_| FakeNode::default()).collect(),
    })
}

#[tokio::test]
async fn traversal_hard_node_cap_sets_incomplete() {
    for count in [4999, 5000] {
        let tree = build_raw_tree(
            &fixture(count),
            8,
            &TreeOpts {
                max_tree_nodes: 10_000,
                ..Default::default()
            },
            &Cancel::default(),
        )
        .await
        .unwrap();
        assert_eq!(tree.root.unwrap().children.len(), 4999);
        if count == 4999 {
            assert!(tree.incomplete.is_none());
        } else {
            let fact = tree.incomplete.unwrap();
            assert_eq!(fact.reason, IncompleteReason::NodeCap);
            assert_eq!(fact.read_nodes, 5000);
            assert!(fact.unread_nodes.is_none());
        }
    }
}

#[tokio::test]
async fn password_cached_value_never_leaves_source_mapping() {
    let source = FakeUia::new(FakeNode {
        properties: Properties {
            control_type: 50004,
            password: true,
            value: Some(NodeValue::Text("secret".into())),
            ..Default::default()
        },
        ..Default::default()
    });
    let tree = build_raw_tree(&source, 8, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    let node = tree.root.unwrap();
    assert_eq!(node.role, Role::TextField);
    assert!(node.states.contains(States::PASSWORD));
    // Secret redaction belongs to core; the driver never needs to read a password value.
    assert!(node.value.is_none());
}

use gcu_core::{
    driver::DriverError,
    model::{NodeFlags, Rect},
    render::{render_structured, render_text, RenderOpts},
    tree::Pipeline,
};
use gcu_driver_windows::uia::{
    cache::{Patterns, BATCH_SIZE, PROPERTY_IDS},
    events::{self, EventKind, FakeUiaEvents, UiaEventSource},
    roles::{map_role, CONTROL_ROLES},
    sensitive::is_execution_surface,
};
use std::{future::poll_fn, time::Duration};

#[path = "uia_tree/fixtures.rs"]
mod fixtures;
use fixtures::{count, mixed_web_fixture, native, render_opts, timeout};

#[test]
fn roles_documented_control_table_maps_every_id_and_preserves_unknown() {
    assert_eq!(CONTROL_ROLES.len(), 41);
    for (offset, (id, role)) in CONTROL_ROLES.iter().enumerate() {
        assert_eq!(*id, 50000 + offset as i32);
        assert_eq!(map_role(*id, None, false), *role);
    }
    assert_eq!(map_role(12345, None, false), Role::Unknown);
    assert_eq!(map_role(50032, None, true), Role::Dialog);
    assert_eq!(map_role(50030, Some("document"), false), Role::WebArea);
    assert_eq!(map_role(50025, Some("heading"), false), Role::Heading);
    assert_eq!(map_role(50000, Some("heading"), false), Role::Button);
}

#[test]
fn cache_plan_has_all_state_and_availability_properties_without_duplicates() {
    let unique: std::collections::BTreeSet<_> = PROPERTY_IDS.iter().collect();
    assert_eq!(unique.len(), PROPERTY_IDS.len());
    for id in [
        30001, 30003, 30005, 30008, 30009, 30010, 30011, 30019, 30022, 30028, 30031, 30033, 30034,
        30035, 30036, 30040, 30041, 30043, 30045, 30046, 30047, 30048, 30049, 30050, 30070, 30077,
        30079, 30086,
    ] {
        assert!(PROPERTY_IDS.contains(&id));
    }
    assert!(PROPERTY_IDS.iter().all(|id| *id >= 30000)); // no pattern-object IDs (10000+)
}

#[tokio::test]
async fn cached_properties_map_states_patterns_ranges_and_physical_frames() {
    let source = FakeUia::new(FakeNode {
        properties: Properties {
            control_type: 50032,
            frame: Some(Rect {
                x: 100.,
                y: 200.,
                w: 640.,
                h: 480.,
            }),
            ..Default::default()
        },
        children: vec![FakeNode {
            properties: Properties {
                control_type: 50015,
                name: Some("Volume".into()),
                enabled: false,
                focusable: true,
                focused: true,
                offscreen: true,
                selected: true,
                toggle_state: Some(2),
                expand_state: Some(1),
                read_only: Some(false),
                value: Some(NodeValue::Range {
                    value: 5.,
                    min: 0.,
                    max: 10.,
                }),
                frame: Some(Rect {
                    x: 120.,
                    y: 230.,
                    w: 150.,
                    h: 30.,
                }),
                patterns: Patterns {
                    invoke: true,
                    toggle: true,
                    selection_item: true,
                    expand_collapse: true,
                    range_value: true,
                    scroll: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }],
    });
    let raw = build_raw_tree(&source, 8, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert!(raw.incomplete.is_none());
    let root = raw.root.unwrap();
    let node = &root.children[0];
    assert_eq!(root.frame.unwrap().x, 0.);
    assert_eq!(
        node.frame,
        Some(Rect {
            x: 20.,
            y: 30.,
            w: 150.,
            h: 30.
        })
    );
    assert!(node.states.contains(
        States::FOCUSED
            | States::FOCUSABLE
            | States::OFFSCREEN
            | States::MIXED
            | States::SELECTABLE
            | States::SELECTED
            | States::EXPANDED
            | States::SETTABLE
    ));
    assert!(!node.states.contains(States::ENABLED));
    assert_eq!(node.actions.len(), 10);
    assert!(node
        .actions
        .contains(&gcu_core::model::NodeAction::Increment));
    assert!(node
        .actions
        .contains(&gcu_core::model::NodeAction::Decrement));
    assert_eq!(
        node.value,
        Some(NodeValue::Range {
            value: 5.,
            min: 0.,
            max: 10.
        })
    );
}

#[tokio::test]
async fn native_budget_and_paging_use_default_1200_and_bounded_children() {
    let source = fixture(1400);
    let tree = build_raw_tree(&source, 8, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(count(tree.root.as_ref().unwrap()), 1200);
    let fact = tree.incomplete.unwrap();
    assert_eq!(fact.reason, IncompleteReason::NodeCap);
    assert_eq!(fact.read_nodes, 1200);
    assert_eq!(source.stats().1, BATCH_SIZE);
}

#[tokio::test]
async fn zero_node_or_depth_budget_marks_unread_without_source_calls() {
    for (nodes, depth, reason) in [
        (0, 64, IncompleteReason::NodeCap),
        (5000, 0, IncompleteReason::DepthCap),
    ] {
        let source = fixture(1);
        let tree = build_raw_tree(
            &source,
            8,
            &TreeOpts {
                max_tree_nodes: nodes,
                max_tree_depth: depth,
                ..Default::default()
            },
            &Cancel::default(),
        )
        .await
        .unwrap();
        assert!(tree.root.is_none());
        assert_eq!(source.stats().0, 0);
        let fact = tree.incomplete.unwrap();
        assert_eq!(fact.read_nodes, 0);
        assert_eq!(fact.reason, reason);
    }
}

#[tokio::test]
async fn depth_cap_and_exact_depth_boundary_report_truthfully() {
    for children in [false, true] {
        let source = fixture(usize::from(children));
        let tree = build_raw_tree(
            &source,
            8,
            &TreeOpts {
                max_tree_depth: 1,
                ..Default::default()
            },
            &Cancel::default(),
        )
        .await
        .unwrap();
        assert!(tree.root.unwrap().children.is_empty());
        assert_eq!(
            tree.incomplete.map(|i| (i.reason, i.read_nodes)),
            children.then_some((IncompleteReason::DepthCap, 1))
        );
    }
    let mut root = FakeNode::default();
    for _ in 0..70 {
        root = FakeNode {
            children: vec![root],
            ..Default::default()
        };
    }
    let tree = build_raw_tree(
        &FakeUia::new(root),
        8,
        &TreeOpts {
            max_tree_depth: usize::MAX,
            ..Default::default()
        },
        &Cancel::default(),
    )
    .await
    .unwrap();
    assert_eq!(count(tree.root.as_ref().unwrap()), 64);
    assert_eq!(tree.incomplete.unwrap().reason, IncompleteReason::DepthCap);
}

#[tokio::test(start_paused = true)]
async fn deadline_unread_root_returns_timeout() {
    let source = fixture(1);
    source.set_delay(Duration::from_secs(3));
    let result = build_raw_tree(&source, 8, &TreeOpts::default(), &Cancel::default()).await;
    assert_eq!(result.unwrap_err(), timeout());
}

#[tokio::test(start_paused = true)]
async fn deadline_after_root_retains_nodes_and_honors_two_second_ceiling() {
    for budget in [Duration::from_millis(200), Duration::from_secs(10)] {
        let source = fixture(200);
        source.set_delay(Duration::from_millis(30));
        let start = tokio::time::Instant::now();
        let tree = build_raw_tree(
            &source,
            8,
            &TreeOpts {
                timeout: budget,
                ..Default::default()
            },
            &Cancel::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            tree.incomplete.as_ref().unwrap().reason,
            IncompleteReason::Deadline
        );
        assert_eq!(
            tree.incomplete.unwrap().read_nodes as usize,
            count(tree.root.as_ref().unwrap())
        );
        assert!(start.elapsed() <= budget.min(Duration::from_secs(2)) + Duration::from_millis(1));
        assert!(tree.root.unwrap().children.len() < 200);
    }
}

#[tokio::test]
async fn partial_property_or_child_errors_preserve_parent_and_fatal_errors_propagate() {
    for on_children in [false, true] {
        let source = fixture(2);
        if on_children {
            source.fail_children(1, native());
        } else {
            source.fail_properties(2, native());
        }
        let tree = build_raw_tree(&source, 8, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap();
        let fact = tree.incomplete.unwrap();
        assert_eq!(fact.reason, IncompleteReason::PartialError);
        assert_eq!(fact.read_nodes as usize, count(tree.root.as_ref().unwrap()));
    }
    for error in [
        DriverError::PermissionMissing {
            permission: "UIA".into(),
        },
        DriverError::Crashed {
            maybe_executed: false,
        },
        DriverError::Cancelled {
            maybe_executed: false,
        },
    ] {
        let source = fixture(1);
        source.fail_properties(2, error.clone());
        assert_eq!(
            build_raw_tree(&source, 8, &TreeOpts::default(), &Cancel::default())
                .await
                .unwrap_err(),
            error
        );
    }
}

#[tokio::test]
async fn completeness_first_reason_retained_but_deadline_takes_precedence() {
    for (error, expected) in [
        (native(), IncompleteReason::PartialError),
        (timeout(), IncompleteReason::Deadline),
    ] {
        let source = FakeUia::new(FakeNode {
            properties: Properties {
                partial: true,
                ..Default::default()
            },
            children: vec![FakeNode::default()],
        });
        source.fail_properties(2, error);
        let tree = build_raw_tree(&source, 8, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap();
        assert_eq!(tree.incomplete.unwrap().reason, expected);
    }
}

#[tokio::test]
async fn cancelled_before_read_makes_zero_source_calls() {
    let source = fixture(10);
    let cancel = Cancel::default();
    cancel.cancel();
    let error = build_raw_tree(&source, 8, &TreeOpts::default(), &cancel)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        DriverError::Cancelled {
            maybe_executed: false
        }
    );
    assert_eq!(source.stats().0, 0);
}

#[tokio::test(start_paused = true)]
async fn cancellation_during_source_wait_returns_promptly_without_partial_success() {
    let source = fixture(10);
    source.set_delay(Duration::from_secs(3));
    let cancel = Cancel::default();
    let stop = cancel.clone();
    let trigger = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        stop.cancel();
    });
    let start = tokio::time::Instant::now();
    let error = build_raw_tree(&source, 8, &TreeOpts::default(), &cancel)
        .await
        .unwrap_err();
    trigger.await.unwrap();
    assert_eq!(
        error,
        DriverError::Cancelled {
            maybe_executed: false
        }
    );
    assert!(start.elapsed() < Duration::from_millis(100));
}

#[tokio::test]
async fn events_all_categories_invalidate_coalesce_and_drop_unregisters() {
    let source = FakeUiaEvents::default();
    let version = source.version();
    let mut stream = events::subscribe(&source, 8).unwrap();
    assert_eq!(source.registrations(), 1);
    source.emit(9, EventKind::Structure);
    assert_eq!(version.get(), 0);
    for (i, kind) in [EventKind::Structure, EventKind::Property, EventKind::Focus]
        .into_iter()
        .enumerate()
    {
        source.emit(8, kind);
        assert_eq!(version.get(), i as u64 + 1);
        assert_eq!(
            poll_fn(|cx| stream.as_mut().poll_next(cx)).await,
            Some(gcu_core::driver::UiEvent::Changed)
        );
    }
    for _ in 0..10_000 {
        source.emit(8, EventKind::Property);
    }
    assert_eq!(version.get(), 10_003);
    assert_eq!(
        poll_fn(|cx| stream.as_mut().poll_next(cx)).await,
        Some(gcu_core::driver::UiEvent::Changed)
    );
    drop(stream);
    assert_eq!(source.registrations(), 0);
    source.emit(8, EventKind::Focus);
    assert_eq!(version.get(), 10_003);
}

#[test]
fn sensitive_terminal_ide_web_heuristics_positive_and_negative_controls() {
    for name in [
        "Windows Terminal",
        "Console",
        "PowerShell",
        "REPL",
        "Run Task...",
        "Run Build Task",
        "Start Debugging",
        "Run and Debug",
        "Run Python File",
        "Web SSH Session",
        "Cloud Shell",
        "Container Console",
        "Remote Desktop",
        "Debug Console",
        "终端",
        "运行任务",
    ] {
        assert!(
            is_execution_surface(&Properties {
                name: Some(name.into()),
                ..Default::default()
            }),
            "{name}"
        );
    }
    for name in [
        "Save",
        "Username",
        "Accept Terms",
        "Search",
        "Document editor",
        "Task description",
        "Debugging guide",
        "Command palette",
    ] {
        assert!(
            !is_execution_surface(&Properties {
                name: Some(name.into()),
                ..Default::default()
            }),
            "{name}"
        );
    }
    assert!(is_execution_surface(&Properties {
        class_name: Some("ConsoleWindowClass".into()),
        ..Default::default()
    }));
    assert!(is_execution_surface(&Properties {
        aria_role: Some("terminal".into()),
        ..Default::default()
    }));
}

#[tokio::test]
async fn fixture_snapshot_core_renders_roles_focus_password_and_execution_flags() {
    let opts = TreeOpts::default();
    let raw = build_raw_tree(&mixed_web_fixture(), 8, &opts, &Cancel::default())
        .await
        .unwrap();
    let tree = Pipeline::default().run(raw, &opts);
    let rendered = render_text(&tree, &render_opts());
    assert!(!rendered.contains("fixture-secret"));
    insta::assert_snapshot!("mixed_web_render", rendered);
    for (_, node) in tree.nodes() {
        match node.node.name.as_deref() {
            Some("Continue" | "Run Task..." | "Debug Console REPL") => {
                assert!(node.flags.contains(NodeFlags::TERMINAL_LIKE))
            }
            Some("Save draft" | "Workspace") => {
                assert!(!node.flags.contains(NodeFlags::TERMINAL_LIKE))
            }
            _ => {}
        }
    }
    assert!(tree
        .nodes()
        .into_iter()
        .any(|(_, n)| n.node.role == Role::WebArea));
    assert_eq!(tree.sensitive_regions.len(), 1);
    assert!(tree.sensitive_regions_reliable);
}

#[tokio::test]
async fn incomplete_tree_reaches_core_and_text_structured_truncation() {
    let source = fixture(2);
    let opts = TreeOpts {
        max_tree_nodes: 1,
        ..Default::default()
    };
    let raw = build_raw_tree(&source, 8, &opts, &Cancel::default())
        .await
        .unwrap();
    let mut tree = Pipeline::default().run(raw, &opts);
    tree.state_version = Some(1);
    tree.observed_at = Some(Duration::ZERO);
    let frame = gcu_core::redact::CapturedFrame::new(
        gcu_core::driver::Capture {
            bytes: Vec::new(),
            width: 1,
            height: 1,
            mime: "image/png".into(),
            geometry: gcu_core::model::Geometry {
                frame: render_opts().window.frame,
                scale: 1.5,
            },
        },
        1,
        Duration::ZERO,
    );
    assert_eq!(
        gcu_core::redact::apply(&tree, frame).unwrap_err(),
        gcu_core::redact::RedactionError::NeedsHandoff
    );
    assert!(!tree.sensitive_regions_reliable);
    assert_eq!(
        tree.incomplete.as_ref().unwrap().reason,
        IncompleteReason::NodeCap
    );
    let text = render_text(&tree, &render_opts());
    assert!(text.contains("native tree incomplete"));
    let json = serde_json::to_value(render_structured(&tree, &render_opts())).unwrap();
    assert_eq!(json["truncated"]["native_incomplete"]["read_nodes"], 1);
    insta::assert_snapshot!("native_incomplete_render", text);
}
