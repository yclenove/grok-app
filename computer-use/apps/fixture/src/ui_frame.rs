//! Frame dispatch and raw input oracle recording.
use super::*;

impl eframe::App for FixtureApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.render(ui);
    }
}
impl FixtureApp {
    /// Render one real fixture frame; also usable by headless accessibility tests.
    pub fn render(&mut self, ui: &mut egui::Ui) {
        let mut key_recorded = false;
        ui.input(|input| {
            for ev in &input.events {
                match ev {
                    egui::Event::Key {
                        key,
                        pressed,
                        repeat,
                        modifiers,
                        ..
                    } => {
                        if !repeat {
                            self.state.record_key(
                                format!("{key:?}"),
                                *pressed,
                                modifiers.ctrl,
                                modifiers.shift,
                                modifiers.alt,
                            );
                            key_recorded = true;
                        }
                    }
                    egui::Event::Text(t) => {
                        self.state.record_key(
                            format!("Text({t})"),
                            true,
                            input.modifiers.ctrl,
                            input.modifiers.shift,
                            input.modifiers.alt,
                        );
                        key_recorded = true;
                    }
                    _ => {}
                }
            }
        });
        if key_recorded {
            let _ = self.state.maybe_persist();
        }

        egui::CentralPanel::default().show(ui, |ui| {
            self.render_top_bar(ui);
            ui.separator();
            match self.state.tab {
                0 => {
                    // All controls visible simultaneously in 3 columns
                    ui.columns(3, |columns| {
                        self.render_form_section(&mut columns[0]);
                        self.render_list_and_drag_section(&mut columns[1]);
                        self.render_canvas_section(&mut columns[2]);
                    });
                }
                1 => {
                    self.render_canvas_section(ui);
                }
                2 => {
                    self.render_list_and_drag_section(ui);
                }
                3 => {
                    self.render_event_log_section(ui);
                }
                _ => {
                    self.state.tab = 0;
                }
            }
        });

        let ctx = ui.ctx().clone();
        self.render_modal_dialog(&ctx);
    }
}
