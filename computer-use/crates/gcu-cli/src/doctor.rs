//! Honest fake-only diagnostics; no OS permission claims or payload retention.
use crate::{exit::Failure, host::LocalHost};
use gcu_core::driver::Driver;
pub(crate) async fn run(driver: &dyn Driver, host: &LocalHost, json: bool) -> Result<u8, Failure> {
    let report = tokio::time::timeout(std::time::Duration::from_secs(5), driver.doctor())
        .await
        .map_err(|_| Failure::internal("Driver doctor timed out"))?;
    let value = serde_json::json!({
        "driver":"fake", "version":env!("CARGO_PKG_VERSION"), "ready":report.ready,
        "permissions":{"status":"not_applicable","note":"fake driver: no OS input or permission checks"},
        "capabilities":report.capabilities, "private_apis":{"status":"not_applicable","note":"fake driver has no private APIs"},
        "issues":report.issues, "logging":{"destination":"stderr","path":null,"payloads":false},
        "log_path":null, "recent_calls":host.summaries(),
        "recent_calls_path":host.summary_path(),
        "recent_calls_scope":"last completed process; up to 20 actions, retained at most seven days",
        "approval_ui":"no WebKit in this build; a desktop uses zenity, kdialog, osascript, or PowerShell, otherwise requests are denied",
        "config_path":crate::config::path().ok(),
    });
    if json {
        println!("{value}");
    } else {
        println!("Grok Computer Use {}\nDriver: fake (offline; no OS input)\nPermissions: not applicable\nPrivate APIs: not applicable\nLogs: stderr, metadata only\nRecent calls: {}\nSummary path: {}\nApproval window: no WebKit; desktop fallback or deny\nCapabilities: {}",env!("CARGO_PKG_VERSION"),value["recent_calls"],host.summary_path().display(),value["capabilities"]);
    }
    Ok(if report.ready { 0 } else { 3 })
}
