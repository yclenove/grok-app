//! Opt-in diagnostic observer; leaves AccessKit actions and output untouched.
use eframe::egui::{self, plugin::Plugin};
use std::{io::Write, path::PathBuf, time::Instant};

struct ActivationTrace {
    path: PathBuf,
    started: Instant,
    actions: usize,
}
impl ActivationTrace {
    fn record(&self, value: serde_json::Value) {
        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            writeln!(file, "{value}")
        })();
        if let Err(error) = result {
            eprintln!("fixture activation trace: {error}");
        }
    }
}
impl Plugin for ActivationTrace {
    fn debug_name(&self) -> &'static str {
        "fixture activation evidence"
    }
    fn input_hook(&mut self, _: &egui::Context, input: &mut egui::RawInput) {
        for event in &input.events {
            if let egui::Event::AccessKitActionRequest(request) = event {
                self.actions += 1;
                self.record(serde_json::json!({
                    "kind": "accesskit_action", "pid": std::process::id(), "elapsed_us": self.started.elapsed().as_micros(),
                    "action": format!("{:?}", request.action), "target": format!("{:?}", request.target_node),
                    "focused_before_frame": input.focused
                }));
            }
        }
    }
    fn output_hook(&mut self, _: &egui::Context, output: &mut egui::FullOutput) {
        if self.actions == 0 {
            return;
        }
        let focus_commands = output
            .viewport_output
            .values()
            .flat_map(|v| &v.commands)
            .filter(|command| matches!(command, egui::ViewportCommand::Focus))
            .count();
        self.record(serde_json::json!({"kind": "frame_output", "pid": std::process::id(), "elapsed_us": self.started.elapsed().as_micros(), "accesskit_actions": self.actions, "focus_commands": focus_commands}));
        self.actions = 0;
    }
}

pub(crate) fn install(ctx: &egui::Context) {
    if let Some(directory) = std::env::var_os("GCU_FIXTURE_ACTIVATION_TRACE_DIR") {
        ctx.add_plugin(ActivationTrace {
            path: PathBuf::from(directory)
                .join(format!("fixture-activation-{}.jsonl", std::process::id())),
            started: Instant::now(),
            actions: 0,
        });
    }
}
