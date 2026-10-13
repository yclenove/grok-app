//! Main binary entry point for `gcu-fixture`.

use eframe::egui;
use gcu_fixture::ui::FixtureApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GCU Fixture")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "GCU Fixture",
        native_options,
        Box::new(|cc| Ok(Box::new(FixtureApp::new(cc)))),
    )
}
