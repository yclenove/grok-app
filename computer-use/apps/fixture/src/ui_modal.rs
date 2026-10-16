//! Fixture modal rendering.
use super::*;
impl FixtureApp {
    /// Renders the modal confirmation dialog if [`FixtureState::modal_open`] is true.
    pub(super) fn render_modal_dialog(&mut self, ctx: &egui::Context) {
        if !self.state.modal_open {
            return;
        }

        let mut rendered = false;
        // A stationary modal needs no repaint loop. Continuous tree updates
        // contend with UIA provider reads; AccessKit actions already wake eframe.
        egui::Modal::new(egui::Id::new("modal_dialog")).show(ctx, |ui| {
            rendered = true;
            ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
                node.set_role(egui::accesskit::Role::Dialog);
                node.set_modal();
                node.set_label("Confirm Dialog / 确认对话框");
            });
            ui.label("Please confirm or cancel the action:");
            ui.label("请确认或取消当前操作：");
            ui.separator();
            ui.horizontal(|ui| {
                let confirm_btn = ui.button("OK / 确定 ✅");
                confirm_btn.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        "modal_confirm_button",
                    )
                });
                if confirm_btn.clicked() {
                    let source = detect_event_source(ui);
                    self.state.close_modal(true, source);
                    let _ = self.state.maybe_persist();
                    ctx.request_repaint();
                }

                let cancel_btn = ui.button("Cancel / 取消 ❌");
                cancel_btn.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "modal_cancel_button")
                });
                if cancel_btn.clicked() {
                    let source = detect_event_source(ui);
                    self.state.close_modal(false, source);
                    let _ = self.state.maybe_persist();
                    ctx.request_repaint();
                }
            });
        });

        if rendered && self.last_persisted_modal_open_count < self.state.modal_open_count {
            self.last_persisted_modal_open_count = self.state.modal_open_count;
            let _ = self.state.maybe_persist();
        }
    }
}
