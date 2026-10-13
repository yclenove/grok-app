//! Actual debug App-shell acceptance entry, not a separate WebView2 host.

#[cfg(all(debug_assertions, windows))]
fn main() {
    if let Err(error) = grok_app_lib::run_computer_use_app_probe() {
        eprintln!("{error}");
        std::process::exit(2);
    }
    // Ordinary App::run exits the process directly. The isolated run_return
    // entry reaches this check only after the same App/native cleanup callback.
    let ok = std::env::var("GROK_CU_APP_HARNESS_OUT")
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|report| {
            report.get("ok").and_then(serde_json::Value::as_bool) == Some(true)
                && report.get("mode").and_then(serde_json::Value::as_str)
                    == std::env::var("GROK_CU_APP_HARNESS_MODE").ok().as_deref()
        });
    std::process::exit(if ok { 0 } else { 2 });
}

#[cfg(not(all(debug_assertions, windows)))]
fn main() {
    eprintln!("App probe requires an isolated Windows debug WebView harness");
    std::process::exit(2);
}
