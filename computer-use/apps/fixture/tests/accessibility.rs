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
