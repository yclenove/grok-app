//! Headless real egui action dispatch, including the external JSON oracle.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use eframe::egui::{self, accesskit};
use gcu_fixture::{
    accessibility,
    state::{EventSource, FixtureState},
};

fn frame(
    ctx: &egui::Context,
    input: egui::RawInput,
    draw: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, draw);
    // This headless test deliberately has no GPU to apply texture uploads.
    output.textures_delta.clear();
    output
}

#[test]
fn modal_frames_publish_dialog_without_continuous_repaint_or_tree_cycles() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = gcu_fixture::ui::FixtureApp::default();
    let _ = frame(&ctx, Default::default(), |ui| app.render(ui));
    app.state.open_modal(EventSource::Keyboard);
    // Let egui finish the initial area layout/animation.
    for i in 0..12 {
        let output = frame(
            &ctx,
            egui::RawInput {
                time: Some(i as f64),
                ..Default::default()
            },
            |ui| app.render(ui),
        );
        let update = output.platform_output.accesskit_update.unwrap();
        assert_acyclic(&update);
        if i == 11 {
            assert!(update
                .nodes
                .iter()
                .any(|(_, n)| n.role() == accesskit::Role::Dialog && n.is_modal()));
            assert!(output
                .viewport_output
                .values()
                .all(|v| v.repaint_delay > std::time::Duration::ZERO));
        }
    }
}

fn assert_acyclic(update: &accesskit::TreeUpdate) {
    use std::collections::{BTreeMap, BTreeSet};
    let nodes: BTreeMap<_, _> = update.nodes.iter().map(|(id, node)| (*id, node)).collect();
    assert_eq!(
        nodes.len(),
        update.nodes.len(),
        "duplicate AccessKit node IDs"
    );
    fn walk(
        id: accesskit::NodeId,
        nodes: &BTreeMap<accesskit::NodeId, &accesskit::Node>,
        path: &mut BTreeSet<accesskit::NodeId>,
        done: &mut BTreeSet<accesskit::NodeId>,
    ) {
        if done.contains(&id) {
            return;
        }
        assert!(
            path.insert(id),
            "cycle in AccessKit provider tree at {id:?}"
        );
        if let Some(node) = nodes.get(&id) {
            for child in node.children() {
                walk(*child, nodes, path, done);
            }
        }
        path.remove(&id);
        done.insert(id);
    }
    let mut done = BTreeSet::new();
    for id in nodes.keys() {
        walk(*id, &nodes, &mut BTreeSet::new(), &mut done);
    }
}

#[test]
fn modal_confirmation_via_accesskit_closes_once_and_publishes_next_tree() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = gcu_fixture::ui::FixtureApp::default();
    let _ = frame(&ctx, Default::default(), |ui| app.render(ui));
    app.state.open_modal(EventSource::Keyboard);
    let mut target = None;
    for _ in 0..3 {
        let output = frame(&ctx, Default::default(), |ui| app.render(ui));
        target = output
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("modal_confirm_button"))
            .map(|(id, _)| *id)
            .or(target);
    }
    let output = frame(
        &ctx,
        egui::RawInput {
            events: vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target_tree: accesskit::TreeId::ROOT,
                    target_node: target.expect("modal confirm control"),
                    data: None,
                },
            )],
            ..Default::default()
        },
        |ui| app.render(ui),
    );
    assert_acyclic(&output.platform_output.accesskit_update.unwrap());
    assert!(!app.state.modal_open);
    assert_eq!(app.state.modal_closed_count, 1);
    let next = frame(&ctx, Default::default(), |ui| app.render(ui));
    let update = next.platform_output.accesskit_update.unwrap();
    assert_acyclic(&update);
    assert!(!update.nodes.iter().any(|(_, n)| n.is_modal()));
    assert_eq!(app.state.modal_closed_count, 1);
}

#[test]
fn text_set_value_accesskit_request_updates_oracle_and_allows_next_tree_frame() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut state = FixtureState::default();
    let mut target = None;
    let _ = frame(&ctx, Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            target = Some(ui.text_edit_singleline(&mut state.text).id.accesskit_id());
        });
    });
    let request = accesskit::ActionRequest {
        action: accesskit::Action::SetValue,
        target_tree: accesskit::TreeId::ROOT,
        target_node: target.unwrap(),
        data: Some(accesskit::ActionData::Value("UIA 中文 🦀".into())),
    };
    let output = frame(
        &ctx,
        egui::RawInput {
            events: vec![egui::Event::AccessKitActionRequest(request)],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = ui.text_edit_singleline(&mut state.text);
                if let Some(value) = accessibility::text_value(ui, &response) {
                    state.set_text(value, EventSource::Keyboard);
                }
            });
        },
    );
    assert_eq!(state.text, "UIA 中文 🦀");
    assert!(output.platform_output.accesskit_update.is_some());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    state.write_to_file(&path).unwrap();
    let oracle: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(oracle["text"], "UIA 中文 🦀");
    let next = frame(&ctx, Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.text_edit_singleline(&mut state.text);
        });
    });
    assert!(next.platform_output.accesskit_update.is_some());
}

#[test]
fn fixture_set_value_full_frame_handles_unfocused_text_and_next_observation() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = gcu_fixture::ui::FixtureApp::default();
    let first = frame(&ctx, Default::default(), |ctx| {
        app.render(ctx);
    });
    let update = first.platform_output.accesskit_update.unwrap();
    let target = update
        .nodes
        .iter()
        .find(|(_, n)| n.label() == Some("name_field"))
        .unwrap()
        .0;
    let _ = frame(
        &ctx,
        egui::RawInput {
            events: vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::SetValue,
                    target_tree: accesskit::TreeId::ROOT,
                    target_node: target,
                    data: Some(accesskit::ActionData::Value("后台 中文 👩‍💻".into())),
                },
            )],
            ..Default::default()
        },
        |ctx| {
            app.render(ctx);
        },
    );
    assert_eq!(app.state.text, "后台 中文 👩‍💻");
    let next = frame(&ctx, Default::default(), |ctx| {
        app.render(ctx);
    });
    assert!(next.platform_output.accesskit_update.is_some());
    assert_eq!(
        app.state
            .event_log
            .iter()
            .filter(|e| e.action == "set_text")
            .count(),
        1
    );
}

#[test]
fn modal_open_and_close_preserve_canvas_pixels_and_childless_accessibility_node() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = gcu_fixture::ui::FixtureApp::default();
    let expected = egui::Color32::from_rgb(95, 115, 135);
    let mut canvas_bounds = None;
    for phase in 0..3 {
        if phase == 1 {
            app.state.open_modal(EventSource::Keyboard);
        } else if phase == 2 {
            app.state.close_modal(true, EventSource::Keyboard);
        }
        for frame_index in 0..12 {
            let output = frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 900.),
                    )),
                    time: Some((phase * 12 + frame_index) as f64),
                    ..Default::default()
                },
                |ui| app.render(ui),
            );
            if frame_index != 11 {
                continue;
            }
            let update = output.platform_output.accesskit_update.as_ref().unwrap();
            assert_acyclic(update);
            let canvas = &update
                .nodes
                .iter()
                .find(|(_, n)| n.label() == Some("grid_canvas"))
                .unwrap()
                .1;
            assert_eq!(canvas.role(), accesskit::Role::Image);
            assert!(canvas.children().is_empty());
            let bounds = canvas.bounds().unwrap();
            assert_eq!(*canvas_bounds.get_or_insert(bounds), bounds);
            assert_eq!(update.nodes.iter().any(|(_, n)| n.is_modal()), phase == 1);
            // Sample well inside cell (0,0), away from its border and text.
            // Check the final tessellated paint order, so a dim modal backdrop
            // cannot pass merely because the original grid shape still exists.
            let point = egui::pos2(bounds.x0 as f32 + 6., bounds.y0 as f32 + 6.);
            assert_eq!(flat_paint_at(&ctx, output, point), Some(expected));
        }
    }
}

/// Justified columns must not publish a wider `grid_canvas` than the 8×40 by 6×40 paint.
///
/// Windows C04 samples `(col + 0.25) * frame.w / 8`. Linux C06 clicks at `40px` cells
/// from the same origin. Both agree only when the accessible frame is the painted grid.
#[test]
fn wide_column_grid_accessible_bounds_match_painted_cells() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = gcu_fixture::ui::FixtureApp::default();
    // Same inner size as the fixture window. Three columns are then wider than 320.
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 860.));
    let mut output = None;
    for i in 0..4 {
        output = Some(frame(
            &ctx,
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(i as f64),
                ..Default::default()
            },
            |ui| app.render(ui),
        ));
    }
    let output = output.expect("layout frame");
    let update = output.platform_output.accesskit_update.as_ref().unwrap();
    let canvas = &update
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("grid_canvas"))
        .expect("grid_canvas")
        .1;
    assert_eq!(canvas.role(), accesskit::Role::Image);
    assert!(canvas.children().is_empty());
    let bounds = canvas.bounds().unwrap();
    let width = bounds.x1 - bounds.x0;
    let height = bounds.y1 - bounds.y0;
    assert_eq!(
        (width, height),
        (320.0, 240.0),
        "accessible grid {bounds:?} is not the painted 8×40 by 6×40 cells"
    );
    for (col, row) in [(0i32, 0i32), (2, 3), (7, 5)] {
        let point = egui::pos2(
            (bounds.x0 + (f64::from(col) + 0.25) * width / 8.0) as f32,
            (bounds.y0 + (f64::from(row) + 0.25) * height / 6.0) as f32,
        );
        assert!(
            bounds.x0 <= f64::from(point.x) && f64::from(point.x) < bounds.x1,
            "C04 sample {point:?} fell outside accessible grid {bounds:?}"
        );
        assert_eq!(
            flat_paint_at(&ctx, output.clone(), point),
            Some(grid_cell_color(col, row)),
            "grid ({col},{row}) at {point:?}"
        );
    }
    let hit = egui::pos2(
        (bounds.x0 + (7.0 + 0.25) * width / 8.0) as f32,
        (bounds.y0 + (5.0 + 0.25) * height / 6.0) as f32,
    );
    // Press and release in one frame. A gap above egui's 0.8s click limit is not a click.
    let _ = frame(
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(4.0),
            events: vec![
                egui::Event::PointerMoved(hit),
                egui::Event::PointerButton {
                    pos: hit,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
                egui::Event::PointerButton {
                    pos: hit,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        },
        |ui| app.render(ui),
    );
    assert_eq!(app.state.grid_hit, Some((7, 5)));
}

fn grid_cell_color(col: i32, row: i32) -> egui::Color32 {
    egui::Color32::from_rgb(
        ((col * 31 + 45) % 180 + 50) as u8,
        ((row * 41 + 65) % 180 + 50) as u8,
        (((col + row) * 29 + 85) % 180 + 50) as u8,
    )
}

fn flat_paint_at(
    ctx: &egui::Context,
    output: egui::FullOutput,
    point: egui::Pos2,
) -> Option<egui::Color32> {
    let mut painted = None;
    for primitive in ctx.tessellate(output.shapes, output.pixels_per_point) {
        if !primitive.clip_rect.contains(point) {
            continue;
        }
        if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
            for indices in mesh.indices.as_chunks::<3>().0 {
                let vertices =
                    [indices[0], indices[1], indices[2]].map(|i| mesh.vertices[i as usize]);
                let a = vertices[1].pos - vertices[0].pos;
                let b = vertices[2].pos - vertices[0].pos;
                if (a.x * b.y - a.y * b.x).abs() <= f32::EPSILON {
                    continue;
                }
                let sides = [0, 1, 2].map(|i| {
                    let edge = vertices[(i + 1) % 3].pos - vertices[i].pos;
                    let offset = point - vertices[i].pos;
                    edge.x * offset.y - edge.y * offset.x
                });
                if sides.iter().all(|s| *s >= 0.) || sides.iter().all(|s| *s <= 0.) {
                    assert!(vertices.iter().all(|v| v.uv == egui::epaint::WHITE_UV && v.color == vertices[0].color), "sample must stay inside a flat painted region");
                    if vertices[0].color != egui::Color32::TRANSPARENT {
                        painted = Some(vertices[0].color);
                    }
                }
            }
        }
    }
    painted
}
