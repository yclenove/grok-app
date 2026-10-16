//! One-shot native fixture diagnosis. Usage: driver_probe <fixture.app path>.
//! Set GCU_FIXTURE_STATE to the same JSON file used by the running fixture.
#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
#[path = "driver_probe/run.rs"]
mod native;

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // NSApplication must be created on main, before Tokio or any SCK/CG worker.
    if !gcu_driver_macos::input::ensure_appkit() {
        return Err("main-thread AppKit initialization failed".into());
    }
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: driver_probe <fixture.app path>; set GCU_FIXTURE_STATE for effect checks")?;
    let path = std::fs::canonicalize(path)?;
    if path.extension().and_then(|s| s.to_str()) != Some("app")
        || !path
            .file_name()
            .is_some_and(|s| s.to_string_lossy().to_lowercase().contains("fixture"))
    {
        return Err("expected the disposable fixture .app path (name containing fixture)".into());
    }
    gcu_driver_macos::diagnostics::enable();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(native::run(&path))
}
