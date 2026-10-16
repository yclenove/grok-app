//! Main binary entry point for `gcu-fixture`.

use eframe::egui;
use gcu_fixture::ui::FixtureApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GCU Fixture")
            .with_inner_size([1280.0, 860.0])
            // The High UIPI fixture must never steal input from the Medium suite.
            .with_active(std::env::var("GCU_FIXTURE_NO_ACTIVATE").as_deref() != Ok("1"))
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "GCU Fixture",
        native_options,
        Box::new(|cc| Ok(Box::new(FixtureApp::new(cc)))),
    )
}
