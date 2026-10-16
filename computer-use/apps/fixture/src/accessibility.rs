//! Complete the fixture's writable ValuePattern without modifying egui.
use eframe::egui::{self, accesskit};

/// egui 0.36 emits a writable text role but does not consume SetValue. Read the
/// real accessibility event for this widget, never a test-side state command.
/// eframe already wakes its event loop for AccessKit; request another frame so
/// the new value is also published to the provider's next tree generation.
pub fn text_value(ui: &egui::Ui, response: &egui::Response) -> Option<String> {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.add_action(accesskit::Action::SetValue);
    });
    let target = response.id.accesskit_id();
    let value = ui.input(|input| {
        input.events.iter().find_map(|event| {
            let egui::Event::AccessKitActionRequest(request) = event else {
                return None;
            };
            if request.action != accesskit::Action::SetValue
                || request.target_tree != accesskit::TreeId::ROOT
                || request.target_node != target
            {
                return None;
            }
            match &request.data {
                Some(accesskit::ActionData::Value(value)) => Some(value.to_string()),
                _ => None,
            }
        })
    });
    if value.is_some() {
        ui.ctx().request_repaint();
    }
    value
}
