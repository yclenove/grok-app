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
    let activation = ready.evidence["first_incomplete_to_complete_ms"]
        .as_f64()
        .unwrap();
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

pub fn unnamed_popup_between_radios_and_slider_is_the_combo() {
    let mut root = node("fixture", Role::Window);
    root.children = vec![
        place(Role::Radio, rect(40.0, 196.0, 80.0, 18.0)),
        place(Role::PopUpButton, rect(141.6, 216.9, 129.9, 18.0)),
        place(Role::Slider, rect(40.0, 238.0, 160.0, 18.0)),
        place(Role::PopUpButton, rect(40.0, 400.0, 80.0, 18.0)),
    ];
    let found = fixture_tree::combo_popup(&root).unwrap();
    assert_eq!(found.role, Role::PopUpButton);
    assert!(found.name.is_none());
    assert!((found.frame.unwrap().y - 216.9).abs() < 0.01);
    root.children
        .push(place(Role::PopUpButton, rect(20.0, 220.0, 40.0, 18.0)));
    assert!(fixture_tree::combo_popup(&root).is_none());
    let mut only = node("fixture", Role::Window);
    only.children = vec![place(Role::PopUpButton, rect(1.0, 2.0, 3.0, 4.0))];
    assert_eq!(
        fixture_tree::combo_popup(&only).unwrap().frame.unwrap().y,
        2.0
    );
}

pub fn scrollbar_static_text_rows_locate_the_virtual_list() {
    let mut root = node("fixture", Role::Window);
    let mut rows = vec![
        place(Role::StaticText, rect(439.0, 76.0, 402.0, 15.0)),
        place(Role::ScrollBar, rect(831.0, 112.0, 10.0, 180.0)),
        place(Role::StaticText, rect(439.0, 274.0, 500.0, 15.0)),
        place(Role::StaticText, rect(439.0, 304.0, 80.0, 18.0)),
    ];
    for index in 0..9 {
        rows.push(place(
            Role::StaticText,
            rect(439.0, 112.0 + f64::from(index) * 18.0, 402.0, 15.0),
        ));
    }
    root.children = rows;
    let at = fixture_tree::virtual_list_point(&root).unwrap();
    assert!((at.x - 640.0).abs() < 0.01, "{at:?}");
    assert!((at.y - 191.5).abs() < 0.01, "{at:?}");
    root.children
        .push(place(Role::ScrollBar, rect(20.0, 112.0, 10.0, 180.0)));
    for index in 0..3 {
        root.children.push(place(
            Role::StaticText,
            rect(-2.0, 112.0 + f64::from(index) * 18.0, 40.0, 15.0),
        ));
    }
    assert!(fixture_tree::virtual_list_point(&root).is_none());
}

fn place(role: Role, frame: Rect) -> RawNode {
    let mut node = node("unnamed", role);
    node.name = None;
    node.identifier = None;
    node.frame = Some(frame);
    node
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect { x, y, w, h }
}

/// Paused tokio time rounds timer deadlines up to the next millisecond; the
/// property is that the wait is bounded by the budget, not that it is exact.
fn assert_within_timer_granularity(elapsed: std::time::Duration, budget: std::time::Duration) {
    assert!(
        elapsed >= budget && elapsed <= budget + std::time::Duration::from_millis(5),
        "{elapsed:?} vs {budget:?}"
    );
}
