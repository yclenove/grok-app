//! Diagnostic entry, compiled only by the explicit App probe or FFI contracts.
use super::{adapter::ComputerUseAdapter, macos_adapter::MacosAdapter};

pub fn run_native_selftest() -> Result<(), String> {
    let adapter = MacosAdapter::new();
    if !adapter.capabilities().observe_screenshot {
        return Err("not_run: Screen Recording or Accessibility permission missing".into());
    }
    if adapter.list_targets()?.is_empty() {
        return Err("not_run: no on-screen windows (unsigned or empty session)".into());
    }
    Ok(())
}
