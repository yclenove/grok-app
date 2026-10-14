//! Deterministic source coverage for native traversal completeness.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_core::{
    driver::{Cancel, TreeOpts},
    model::IncompleteReason,
};
use gcu_driver_linux::atspi::{
    build_raw_tree,
    source::{FakeNode, FakeSource},
};
use std::time::Duration;

fn root(children: usize) -> FakeNode {
    FakeNode {
        id: 1,
        role: atspi::Role::Frame,
        children: (0..children)
            .map(|i| FakeNode {
                id: i as u64 + 2,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

#[tokio::test]
async fn traversal_hard_node_cap_sets_incomplete() {
    for count in [4999, 5000] {
        let tree = build_raw_tree(
            &FakeSource::default(),
            &root(count),
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

#[tokio::test(start_paused = true)]
async fn traversal_deadline_after_root_preserves_partial_tree() {
    let tree = build_raw_tree(
        &FakeSource {
            delay: Duration::from_millis(50),
            ..Default::default()
        },
        &root(2),
        &TreeOpts {
            timeout: Duration::from_millis(80),
            ..Default::default()
        },
        &Cancel::default(),
    )
    .await
    .unwrap();
    assert!(tree.root.unwrap().children.is_empty());
    let fact = tree.incomplete.unwrap();
    assert_eq!(fact.reason, IncompleteReason::Deadline);
    assert_eq!(fact.read_nodes, 1);
}

#[tokio::test(start_paused = true)]
async fn traversal_two_second_ceiling_marks_deadline_with_longer_caller_timeout() {
    let start = tokio::time::Instant::now();
    let tree = build_raw_tree(
        &FakeSource {
            delay: Duration::from_millis(300),
            ..Default::default()
        },
        &root(50),
        &TreeOpts {
            timeout: Duration::from_secs(10),
            ..Default::default()
        },
        &Cancel::default(),
    )
    .await
    .unwrap();
    let root = tree.root.unwrap();
    let fact = tree.incomplete.unwrap();
    assert_eq!(fact.reason, IncompleteReason::Deadline);
    assert_eq!(fact.read_nodes as usize, 1 + root.children.len());
    assert!(root.children.len() < 50);
    assert!(start.elapsed() <= Duration::from_millis(2001));
}

#[tokio::test]
async fn traversal_children_error_sets_partial_error() {
    let tree = build_raw_tree(
        &FakeSource {
            error_on_id: Some(1),
            ..Default::default()
        },
        &root(1),
        &TreeOpts::default(),
        &Cancel::default(),
    )
    .await
    .unwrap();
    assert!(tree.root.unwrap().children.is_empty());
    assert_eq!(
        tree.incomplete.unwrap().reason,
        IncompleteReason::PartialError
    );
}

#[tokio::test]
async fn traversal_depth_or_zero_budget_sets_incomplete() {
    for (nodes, depth, reason) in [
        (0, 64, IncompleteReason::NodeCap),
        (5000, 0, IncompleteReason::DepthCap),
    ] {
        let tree = build_raw_tree(
            &FakeSource::default(),
            &root(1),
            &TreeOpts {
                max_tree_nodes: nodes,
                max_tree_depth: depth,
                ..Default::default()
            },
            &Cancel::default(),
        )
        .await
        .unwrap();
        assert_eq!(tree.incomplete.unwrap().reason, reason);
    }
}
