//! User interface implementation using `eframe` and `egui`.
//!
//! Provides all controls required by the P1-01 specification:
//! - Stable accessibility names/IDs for automation.
//! - Visual distinction and Chinese/English emoji labels.
//! - Canvas grid without accessibility children.
//! - Virtualized list with visible row tracking.
//! - Draggable box, modal dialog, and sensitive action buttons.

use crate::state::{
    compute_grid_cell, EventSource, FixtureState, CANVAS_GRID_COLS, CANVAS_GRID_ROWS,
    VIRTUAL_LIST_TOTAL_ROWS,
};
use eframe::egui;

/// Options list for the radio button group.
const RADIO_OPTIONS: [&str; 3] = [
    "Option 1 / 选项甲 🅰️",
    "Option 2 / 选项乙 🅱️",
    "Option 3 / 选项丙 🅲",
];

/// Options list for the combo-box dropdown.
const COMBO_OPTIONS: [&str; 4] = [
    "Item 1 / 第一项 🥇",
    "Item 2 / 第二项 🥈",
    "Item 3 / 第三项 🥉",
    "Item 4 / 第四项 🎖️",
];

/// Tab names for fixture navigation.
const TAB_NAMES: [&str; 4] = [
    "All Controls / 全部控件 🎛️",
    "Canvas Grid / 画布网格 🎨",
    "Virtual List / 虚拟列表 📜",
    "Event Log / 事件日志 📋",
];

/// Detects the probable input source ([`EventSource`]) from active `egui` input events.
pub fn detect_event_source(ui: &egui::Ui) -> EventSource {
    ui.input(|input| {
        for ev in &input.events {
            match ev {
                egui::Event::Key { .. } | egui::Event::Text(_) => return EventSource::Keyboard,
                egui::Event::PointerButton { .. } | egui::Event::PointerMoved(_) => {
                    return EventSource::Mouse;
                }
                _ => {}
            }
        }
        if input.pointer.any_click() || input.pointer.is_decidedly_dragging() {
            EventSource::Mouse
        } else {
            EventSource::Keyboard
        }
    })
}

/// Main `eframe` application for `gcu-fixture`.
pub struct FixtureApp {
    /// Pure state container and persistence manager.
    pub state: FixtureState,
    /// In-memory password input buffer.
    pub password_input: String,
    /// Transient drag position for the drag-and-drop test box.
    pub drag_pos: Option<egui::Pos2>,
}

impl Default for FixtureApp {
    fn default() -> Self {
        let app = Self {
            state: FixtureState::default(),
            password_input: String::new(),
            drag_pos: None,
        };
        // Persist initial state if GCU_FIXTURE_STATE is set.
        let _ = app.state.maybe_persist();
        app
    }
}

impl FixtureApp {
    /// Creates a new fixture app instance.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    /// Renders the top title and navigation bar.
    fn render_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("GCU Fixture / 跨平台自动化测试夹具");
            ui.separator();
            for (idx, name) in TAB_NAMES.iter().enumerate() {
                let is_selected = self.state.tab == idx;
                if ui.selectable_label(is_selected, *name).clicked() {
                    let source = detect_event_source(ui);
                    self.state.set_tab(idx, source);
                    let _ = self.state.maybe_persist();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let reset_btn = ui.button("Reset / 重置 🔄");
                reset_btn.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "reset_button")
                });
                if reset_btn.clicked() {
                    let source = detect_event_source(ui);
                    self.state.reset(source);
                    self.password_input.clear();
                    let _ = self.state.maybe_persist();
                }
            });
        });
    }

    /// Renders standard interactive form inputs (text, password, checkbox, slider, etc.).
    fn render_form_section(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(egui::RichText::new("Form Controls / 表单控件").strong());

            // 1. Single-line text input
            ui.horizontal(|ui| {
                ui.label("Name / 姓名 👤:");
                let text_res = ui.add(
                    egui::TextEdit::singleline(&mut self.state.text)
                        .id_salt("name_field")
                        .hint_text("Enter text / 输入姓名"),
                );
                text_res.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "name_field")
                });
                if text_res.changed() {
                    let source = detect_event_source(ui);
                    self.state.set_text(self.state.text.clone(), source);
                    let _ = self.state.maybe_persist();
                }
            });

            // 2. Multiline text input
            ui.horizontal(|ui| {
                ui.label("Notes / 备注 📝:");
                let multi_res = ui.add(
                    egui::TextEdit::multiline(&mut self.state.multiline)
                        .id_salt("multiline_field")
                        .desired_rows(2),
                );
                multi_res.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "multiline_field")
                });
                if multi_res.changed() {
                    let source = detect_event_source(ui);
                    self.state
                        .set_multiline(self.state.multiline.clone(), source);
                    let _ = self.state.maybe_persist();
                }
            });

            // 3. Password input (masked, plaintext never persisted)
            ui.horizontal(|ui| {
                ui.label("Password / 密码 🔒:");
                let pwd_res = ui.add(
                    egui::TextEdit::singleline(&mut self.password_input)
                        .password(true)
                        .id_salt("password_field")
                        .hint_text("Password / 密码"),
                );
                pwd_res.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "password_field")
                });
                if pwd_res.changed() {
                    let source = detect_event_source(ui);
                    self.state.set_password(&self.password_input, source);
                    let _ = self.state.maybe_persist();
                }
            });

            // 4. Checkbox
            let mut agree = self.state.checkbox;
            let check_res = ui.checkbox(&mut agree, "Agree / 同意 ✅");
            check_res.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    true,
                    self.state.checkbox,
                    "agree_checkbox",
                )
            });
            if check_res.changed() {
                let source = detect_event_source(ui);
                self.state.set_checkbox(agree, source);
                let _ = self.state.maybe_persist();
            }

            // 5. Radio group
            ui.horizontal(|ui| {
                ui.label("Radio / 单选 🔘:");
                for (idx, opt_label) in RADIO_OPTIONS.iter().enumerate() {
                    let mut is_cur = self.state.radio == idx;
                    let r_res = ui.radio_value(&mut is_cur, true, *opt_label);
                    if r_res.clicked() && self.state.radio != idx {
                        let source = detect_event_source(ui);
                        self.state.set_radio(idx, source);
                        let _ = self.state.maybe_persist();
                    }
                }
            });

            // 6. Combo-box dropdown
            ui.horizontal(|ui| {
                ui.label("Dropdown / 下拉 📋:");
                let cur_selected = self.state.combo_selected;
                let cur_label = COMBO_OPTIONS.get(cur_selected).copied().unwrap_or("Select");
                egui::ComboBox::from_id_salt("combo_box")
                    .selected_text(cur_label)
                    .show_ui(ui, |ui| {
                        for (idx, opt) in COMBO_OPTIONS.iter().enumerate() {
                            let mut is_sel = self.state.combo_selected == idx;
                            let item_res = ui.selectable_value(&mut is_sel, true, *opt);
                            if item_res.clicked() && self.state.combo_selected != idx {
                                let source = detect_event_source(ui);
                                self.state.set_combo_selected(idx, source);
                                let _ = self.state.maybe_persist();
                            }
                        }
                    });
            });

            // 7. Slider
            ui.horizontal(|ui| {
                ui.label("Volume / 音量 🔊:");
                let mut slider_val = self.state.slider;
                let slider_res =
                    ui.add(egui::Slider::new(&mut slider_val, 0.0..=100.0).text("Level"));
                slider_res.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "slider_field")
                });
                if slider_res.changed() {
                    let source = detect_event_source(ui);
                    self.state.set_slider(slider_val, source);
                    let _ = self.state.maybe_persist();
                }
            });

            // 8. Action buttons (Counter, Send, Submit, Modal)
            ui.horizontal_wrapped(|ui| {
                let counter_btn =
                    ui.button(format!("Increment / 计数 🔢 ({})", self.state.counter));
                counter_btn.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "counter_button")
                });
                if counter_btn.clicked() {
                    let source = detect_event_source(ui);
                    self.state.increment_counter(source);
                    let _ = self.state.maybe_persist();
                }

                let send_btn = ui.button(format!("Send / 发送 🚀 ({})", self.state.send_clicked));
                send_btn.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "send_button")
                });
                if send_btn.clicked() {
                    let source = detect_event_source(ui);
                    self.state.click_send(source);
                    let _ = self.state.maybe_persist();
                }

                let submit_btn =
                    ui.button(format!("Submit / 提交 📤 ({})", self.state.submitted_count));
                submit_btn.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "submit_button")
                });
                if submit_btn.clicked() {
                    let source = detect_event_source(ui);
                    self.state.submit_form(source);
                    let _ = self.state.maybe_persist();
                }

                let modal_btn = ui.button("Open Modal / 打开弹窗 💬");
                modal_btn.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "open_modal_button")
                });
                if modal_btn.clicked() {
                    let source = detect_event_source(ui);
                    self.state.open_modal(source);
                    let _ = self.state.maybe_persist();
                }
            });
        });
    }

    /// Renders the virtualized 2000-row list and the drag-and-drop interactive area.
    fn render_list_and_drag_section(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(egui::RichText::new("Virtual List & Drag / 列表与拖拽").strong());

            // 2000-row list virtualized with egui::ScrollArea
            ui.label(format!(
                "Visible rows: [{}, {}) / 可见行",
                self.state.list_visible_range.0, self.state.list_visible_range.1
            ));
            let row_height = 20.0;
            egui::ScrollArea::vertical()
                .id_salt("virtual_list_scroll")
                .max_height(180.0)
                .show_rows(ui, row_height, VIRTUAL_LIST_TOTAL_ROWS, |ui, row_range| {
                    self.state
                        .set_list_visible_range((row_range.start, row_range.end));
                    for i in row_range {
                        ui.label(format!("Item {:04} / 列表项 {:04}", i, i));
                    }
                });

            ui.separator();

            // Drag and drop test area
            ui.label("Drag and Drop / 拖拽方块到目标区:");
            ui.horizontal(|ui| {
                let drag_size = egui::vec2(80.0, 40.0);
                let (drag_rect, drag_res) = ui.allocate_exact_size(drag_size, egui::Sense::drag());
                drag_res.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "drag_source")
                });

                if drag_res.dragged() {
                    self.drag_pos = ui.input(|i| i.pointer.interact_pos());
                }

                let target_size = egui::vec2(130.0, 50.0);
                let (target_rect, target_res) =
                    ui.allocate_exact_size(target_size, egui::Sense::hover());
                target_res.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "drop_target")
                });

                if drag_res.drag_stopped() {
                    if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                        if target_rect.contains(pos) {
                            let source = detect_event_source(ui);
                            self.state.set_drag_drop_ok(true, source);
                            let _ = self.state.maybe_persist();
                        }
                    }
                    self.drag_pos = None;
                }

                // Paint source box
                let painter = ui.painter();
                let render_drag_rect = if let Some(pos) = self.drag_pos {
                    egui::Rect::from_center_size(pos, drag_size)
                } else {
                    drag_rect
                };
                painter.rect_filled(render_drag_rect, 4.0, egui::Color32::from_rgb(50, 120, 220));
                painter.text(
                    render_drag_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Drag Me 🟦",
                    egui::FontId::proportional(12.0),
                    egui::Color32::WHITE,
                );

                // Paint target box
                let target_color = if self.state.drag_drop_ok {
                    egui::Color32::from_rgb(30, 160, 60)
                } else {
                    egui::Color32::from_rgb(100, 100, 110)
                };
                painter.rect_stroke(
                    target_rect,
                    4.0,
                    egui::Stroke::new(2.0, target_color),
                    egui::StrokeKind::Inside,
                );
                let target_label = if self.state.drag_drop_ok {
                    "Dropped! ✅"
                } else {
                    "Target 🎯"
                };
                painter.text(
                    target_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    target_label,
                    egui::FontId::proportional(12.0),
                    target_color,
                );
            });
        });
    }

    /// Renders the self-drawn 8x6 grid canvas area.
    ///
    /// CRITICAL REQUIREMENT: This container MUST NOT expose any child nodes to accessibility.
    /// It exposes exactly one container node with `WidgetType::Image` (`grid_canvas`).
    fn render_canvas_section(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(egui::RichText::new("Canvas Grid (8x6) / 画布网格").strong());
            if let Some((col, row)) = self.state.grid_hit {
                ui.label(format!("Hit cell: col={col}, row={row} 🎯"));
            } else {
                ui.label("Click cells by screenshot coordinates (no AX children)");
            }

            let cell_w = 40.0;
            let cell_h = 40.0;
            let total_w = cell_w * CANVAS_GRID_COLS as f32;
            let total_h = cell_h * CANVAS_GRID_ROWS as f32;

            let (canvas_rect, canvas_res) =
                ui.allocate_exact_size(egui::vec2(total_w, total_h), egui::Sense::click());

            // Single accessible widget info: Image role, no sub-nodes.
            canvas_res.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "grid_canvas")
            });

            if canvas_res.clicked() {
                if let Some(pos) = canvas_res.interact_pointer_pos() {
                    if let Some(hit) = compute_grid_cell(
                        (pos.x, pos.y),
                        (canvas_rect.min.x, canvas_rect.min.y),
                        (cell_w, cell_h),
                        (CANVAS_GRID_COLS, CANVAS_GRID_ROWS),
                    ) {
                        let source = detect_event_source(ui);
                        self.state.set_grid_hit(Some(hit), source);
                        let _ = self.state.maybe_persist();
                    }
                }
            }

            // Draw grid cells via direct painting
            let painter = ui.painter();
            for r in 0..CANVAS_GRID_ROWS {
                for c in 0..CANVAS_GRID_COLS {
                    let min = egui::pos2(
                        canvas_rect.min.x + c as f32 * cell_w,
                        canvas_rect.min.y + r as f32 * cell_h,
                    );
                    let max = egui::pos2(min.x + cell_w, min.y + cell_h);
                    let cell_rect = egui::Rect::from_min_max(min, max);

                    // Distinct pastel colors per cell
                    let red = ((c * 31 + 45) % 180 + 50) as u8;
                    let green = ((r * 41 + 65) % 180 + 50) as u8;
                    let blue = (((c + r) * 29 + 85) % 180 + 50) as u8;
                    let bg_color = egui::Color32::from_rgb(red, green, blue);

                    painter.rect_filled(cell_rect, 1.0, bg_color);

                    let is_hit = self.state.grid_hit == Some((c, r));
                    let stroke = if is_hit {
                        egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 215, 0))
                    } else {
                        egui::Stroke::new(1.0, egui::Color32::from_gray(30))
                    };
                    painter.rect_stroke(cell_rect, 1.0, stroke, egui::StrokeKind::Inside);

                    let label = format!("{}\n{},{}", r * CANVAS_GRID_COLS + c, c, r);
                    painter.text(
                        cell_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(10.0),
                        egui::Color32::WHITE,
                    );
                }
            }
        });
    }

    /// Renders the recent chronological event log.
    fn render_event_log_section(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "Event Log / 最近事件 ({} / 50)",
                    self.state.event_log.len()
                ))
                .strong(),
            );
            egui::ScrollArea::vertical()
                .id_salt("event_log_scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    if self.state.event_log.is_empty() {
                        ui.label("No events recorded yet. / 暂无事件记录。");
                    } else {
                        for (idx, entry) in self.state.event_log.iter().enumerate().rev() {
                            let detail = entry.detail.as_deref().unwrap_or("-");
                            ui.label(format!(
                                "[#{idx:02}] {} | {} | {} ({})",
                                entry.source, entry.action, detail, entry.timestamp_ms
                            ));
                        }
                    }
                });
        });
    }

    /// Renders the modal confirmation dialog if [`FixtureState::modal_open`] is true.
    fn render_modal_dialog(&mut self, ctx: &egui::Context) {
        if !self.state.modal_open {
            return;
        }

        egui::Window::new("Confirm Dialog / 确认对话框")
            .id(egui::Id::new("modal_dialog"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
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
                    }

                    let cancel_btn = ui.button("Cancel / 取消 ❌");
                    cancel_btn.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            "modal_cancel_button",
                        )
                    });
                    if cancel_btn.clicked() {
                        let source = detect_event_source(ui);
                        self.state.close_modal(false, source);
                        let _ = self.state.maybe_persist();
                    }
                });
            });
    }
}

impl eframe::App for FixtureApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
