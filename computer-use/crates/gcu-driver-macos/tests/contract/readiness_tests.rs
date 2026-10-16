//! Scripted setup and selector regressions, runnable without a macOS desktop.

use std::time::Duration;

use gcu_core::model::{Incomplete, IncompleteReason, NativeHandle, RawNode, RawTree, Rect, Role};

use super::{fixture_tree, readiness};

fn node(id: &str, role: Role) -> RawNode {
    RawNode {
        role,
        native_role: "test".into(),
        subrole: None,
        name: Some(id.into()),
        value: None,
        description: None,
        placeholder: None,
        help: None,
        identifier: None,
        states: Default::default(),
        actions: Default::default(),
        frame: None,
        text_extras: None,
        url: None,
        children: Vec::new(),
        handle: NativeHandle::new(()),
        fingerprint: Default::default(),
    }
}

fn complete() -> RawTree {
    let mut root = node("fixture", Role::Window);
    root.children = fixture_tree::READY_CONTROLS
        .iter()
        .map(|id| node(id, Role::Button))
        .collect();
    RawTree {
        root: Some(root),
        incomplete: None,
    }
}

fn partial() -> RawTree {
    RawTree {
        root: Some(node("fixture", Role::Window)),
        incomplete: Some(Incomplete {
            reason: IncompleteReason::PartialError,
            read_nodes: 1,
            unread_nodes: None,
        }),
    }
}

pub async fn readiness_partial_then_complete_reports_activation_time() {
    tokio::time::pause();
    let mut polls = 0;
    let ready = readiness::wait("scripted_activation", |_, _| {
        polls += 1;
        let raw = if polls == 1 { partial() } else { complete() };
        async move { Ok(raw) }
    })
    .await
    .unwrap();
    assert_eq!(ready.evidence["polls"], 2);
    assert_eq!(
        ready.evidence["first"]["incomplete"]["reason"],
        "partial_error"
    );
    let activation = ready.evidence["first_incomplete_to_complete_ms"].as_f64().unwrap();
    assert!((100.0..=105.0).contains(&activation), "{activation}");
    assert_eq!(ready.evidence["last_incomplete"], serde_json::Value::Null);
    assert!(fixture_tree::missing(&ready.tree).is_empty());
}

pub async fn readiness_unmarked_partial_first_read_fails_adr0004() {
    let error = readiness::wait("unmarked_partial", |_, _| async {
        let mut raw = partial();
        raw.incomplete = None;
        Ok(raw)
    })
    .await
    .err()
    .unwrap();
    assert!(error.contains("ADR 0004"), "{error}");
    assert!(error.contains("\"polls\":1"), "{error}");
}

pub async fn readiness_complete_controls_with_incomplete_never_pass() {
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    let error = readiness::wait("persistent_partial", |_, _| async {
        let mut raw = complete();
        raw.incomplete = Some(Incomplete {
            read_nodes: fixture_tree::count(&raw) as u32,
            ..partial().incomplete.unwrap()
        });
        Ok(raw)
    })
    .await
    .err()
    .unwrap();
    assert_within_timer_granularity(started.elapsed(), readiness::BUDGET);
    assert!(error.contains("partial_error"), "{error}");
    assert!(error.contains("\"last_nodes\":9"), "{error}");
}

pub async fn readiness_hung_read_cancels_within_setup_budget() {
    tokio::time::pause();
    let mut token = None;
    let started = tokio::time::Instant::now();
    let error = readiness::wait("hung_read", |remaining, cancel| {
        assert!(remaining <= readiness::BUDGET);
        token = Some(cancel.clone());
        async move {
            tokio::time::sleep(Duration::from_secs(6)).await;
            Ok(complete())
        }
    })
    .await
    .err()
    .unwrap();
    assert_within_timer_granularity(started.elapsed(), readiness::BUDGET);
    assert!(token.unwrap().is_cancelled());
    assert!(error.contains("timed out during tree read"), "{error}");
}

pub fn canvas_label_collision_selects_grid_image_at_both_scales() {
    for scale in [1.0, 2.0] {
        let mut root = node("fixture", Role::Window);
        let mut tab = node("Canvas Grid / 画布网格 🎨", Role::Switch);
        tab.frame = Some(Rect {
            x: 488.875 * scale,
            y: 37.5 * scale,
            w: 155.78125 * scale,
            h: 18.0 * scale,
        });
        let mut canvas = node("grid_canvas", Role::Image);
        canvas.frame = Some(Rect {
            x: 863.0 * scale,
            y: 112.0 * scale,
            w: 402.0 * scale,
            h: 240.0 * scale,
        });
        root.children = vec![tab, canvas];
        let selected = fixture_tree::canvas(&root).unwrap();
        assert_eq!(selected.role, Role::Image);
        assert_eq!(selected.name.as_deref(), Some("grid_canvas"));
        let frame = selected.frame.unwrap();
        assert_eq!((frame.x + frame.w / 2.0) / scale, 1064.0);
        root.children.pop();
        assert!(fixture_tree::canvas(&root).is_none());
    }
}

pub fn grid_pixel_targets_exact_cell_inside_canvas_at_both_scales() {
    for scale in [1, 2] {
        let width = 600 * scale;
        let height = 400 * scale;
        let frame = Rect {
            x: 100.0 * f64::from(scale),
            y: 80.0 * f64::from(scale),
            w: 402.0 * f64::from(scale),
            h: 240.0 * f64::from(scale),
        };
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        // Fixture cell (3,2); allocated canvas width is 402 points, painted
        // grid width only 320. A proportional AX-frame guess would be wrong.
        for y in 160 * scale..200 * scale {
            for x in 220 * scale..260 * scale {
                let i = (y as usize * width as usize + x as usize) * 4;
                rgba[i..i + 4].copy_from_slice(&[188, 197, 100, 255]);
            }
        }
        // Same color outside the canvas must not be selected.
        rgba[..4].copy_from_slice(&[188, 197, 100, 255]);
        let at = fixture_tree::cell_pixel(&rgba, width, height, frame, [3, 2]).unwrap();
        let col = ((at.x / f64::from(scale) - 100.0) / 40.0).floor() as u32;
        let row = ((at.y / f64::from(scale) - 80.0) / 40.0).floor() as u32;
        assert_eq!([col, row], [3, 2]);
        assert!(fixture_tree::cell_pixel(&rgba, width, height, frame, [2, 1]).is_err());
    }
}

/// Paused tokio time rounds timer deadlines up to the next millisecond; the
/// property is that the wait is bounded by the budget, not that it is exact.
fn assert_within_timer_granularity(elapsed: std::time::Duration, budget: std::time::Duration) {
    assert!(
        elapsed >= budget && elapsed <= budget + std::time::Duration::from_millis(5),
        "{elapsed:?} vs {budget:?}"
    );
}
