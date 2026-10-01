//! Real extension pairing probe. Control stays on a private child pipe, never HTTP.
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use grok_computer_use_core::broker::ComputerUseBroker;
use serde_json::{json, Value};

mod completion;
mod dispatch;
mod mcp;
mod resources;
mod restart;

const PAIRING_CHECKS: &[&str] = &[
    "stable-extension-id",
    "pair-with-app-confirmation",
    "no-popup-key-message",
    "extension-revoke",
    "app-revoke",
    "worker-restart",
    "heartbeat-survives-without-popup",
    "browser-close-expires-host-key",
    "late-heartbeat-rejected",
    "old-heartbeat-cannot-touch-new-pairing",
    "unshared-tab-is-not-inspected",
    "explicit-share-current-tab",
    "typed-observation-through-extension",
    "screenshot-requires-active-tab",
    "capture-normalization",
    "mcp-observation-through-existing-adapter",
    "mcp-act-through-existing-adapter",
    "mcp-act-action-id-dedupe",
    "mcp-act-rejects-coordinate-and-stale-snapshot",
    "mcp-existing-session-isolation",
    "mcp-preview-keeps-model-refs",
    "mcp-stop-cancels-existing-observation",
    "observation-race-tab-switch",
    "observation-race-scroll",
    "observation-race-unshare",
    "observation-race-reload",
    "observation-race-script-timeout",
    "observation-race-deadline",
    "observation-large-document",
    "explicit-unshare-keeps-tab-open",
    "pending-share-navigation-does-not-offer",
    "same-url-reload-revokes-candidate",
    "navigation-revokes-old-document",
    "tab-close-revokes-candidate",
    "non-loopback-needs-action-grant",
    "restricted-page-cannot-share",
];

fn pairing_pass_line(kind: &str) -> String {
    format!(
        "gate: existing_tab_extension_pairing PASS browser={kind} checks={} host=process-wide-broker evidence=pairing-lease-share-observation-mcp-actions-stop-and-capture-denial",
        PAIRING_CHECKS.len()
    )
}

struct ProbeHostCleanup;
impl Drop for ProbeHostCleanup {
    fn drop(&mut self) {
        super::shutdown_product();
    }
}

pub fn run_live_extension_pair_gate() -> Result<(), String> {
    run_live_extension_pair(false, ProbeMode::Pairing)
}

pub fn run_live_extension_pair_required() -> Result<(), String> {
    run_live_extension_pair(true, ProbeMode::Pairing)
}

pub fn run_live_extension_toolbar_required() -> Result<(), String> {
    run_live_extension_pair(true, ProbeMode::Toolbar)
}

pub fn run_live_extension_completion_required() -> Result<(), String> {
    run_live_extension_pair(true, ProbeMode::Completion)
}

pub fn run_live_extension_dispatch_required() -> Result<(), String> {
    run_live_extension_pair(true, ProbeMode::Dispatch)
}

pub fn run_live_extension_retention_required() -> Result<(), String> {
    run_live_extension_pair(true, ProbeMode::Retention)
}

pub fn run_live_extension_restart_required() -> Result<(), String> {
    run_live_extension_pair(true, ProbeMode::Restart)
}

pub fn run_live_extension_host_retirement_required() -> Result<(), String> {
    run_live_extension_pair(true, ProbeMode::HostRetirement)
}

pub fn run_live_extension_browser_exit_required() -> Result<(), String> {
    run_live_extension_pair(true, ProbeMode::BrowserExit)
}

pub fn run_restart_host_fixture() -> Result<(), String> {
    restart::run_host()
}

enum ProbeMode {
    Pairing,
    Toolbar,
    Completion,
    Dispatch,
    Retention,
    Restart,
    HostRetirement,
    BrowserExit,
}

fn run_live_extension_pair(required: bool, mode: ProbeMode) -> Result<(), String> {
    let toolbar = matches!(mode, ProbeMode::Toolbar);
    let completion = matches!(mode, ProbeMode::Completion);
    let retention = matches!(mode, ProbeMode::Retention);
    let host_retirement = matches!(mode, ProbeMode::HostRetirement);
    let browser_exit = matches!(mode, ProbeMode::BrowserExit);
    let restarting = matches!(mode, ProbeMode::Restart | ProbeMode::HostRetirement);
    let dispatch = matches!(
        mode,
        ProbeMode::Dispatch
            | ProbeMode::Retention
            | ProbeMode::Restart
            | ProbeMode::HostRetirement
            | ProbeMode::BrowserExit
    );
    let Some((chrome, kind)) = capable_live_browser() else {
        if required {
            return Err("not_run: no isolated-extension-capable Chromium binary".into());
        }
        println!("gate: existing_tab_extension_pairing not_run (Chromium unavailable)");
        return Ok(());
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let home = restart::owned_home()?;
    let _cleanup = ProbeHostCleanup;
    let script = root.join(if dispatch {
        "tools/computer-use-probe/existing-dispatch-live.mjs"
    } else if completion {
        "tools/computer-use-probe/existing-completion-live.mjs"
    } else {
        "tools/computer-use-probe/pairing-live.mjs"
    });
    let node = super::runtime::resolve(super::runtime::JS_RUNTIME)?;
    let mut host = restart::ControlHost::new(restarting, &home)?;
    let mut resources = resources::ProbeResources::create()?;
    let mut command = crate::process_util::command(node);
    if toolbar {
        command.arg(&script).arg("--toolbar-gesture");
    } else {
        command.arg(&script);
    }
    if retention {
        command.arg("--bfcache");
    }
    if host_retirement {
        command.arg("--host-retirement");
    } else if browser_exit {
        command.arg("--browser-exit");
    } else if restarting {
        command.arg("--app-restart");
    }
    command.env_clear();
    for name in ["SystemRoot", "TEMP", "TMP", "TMPDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    #[cfg(windows)]
    {
        // Node 20/libuv child pipes fail with ENOENT/ENOTCONN when PATH is absent,
        // even for an absolute browser executable. Do not inherit the user's PATH.
        let system = std::env::var_os("SystemRoot").ok_or("Windows system root missing")?;
        command.env("PATH", PathBuf::from(system).join("System32"));
    }
    let mut child = command
        .arg("--app-host")
        .arg("--browser")
        .arg(chrome)
        .arg("--owned-profile")
        .arg(&resources.profile)
        .arg("--profile-owner")
        .arg(&resources.owner)
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| "could not start the extension pairing probe")?;
    if let Err(error) = resources.attach(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let mut input = child.stdin.take().ok_or("pairing probe input missing")?;
    let output = child.stdout.take().ok_or("pairing probe output missing")?;
    let (tx, rx) = mpsc::channel();
    let control_limit = if dispatch { 64 * 1024 } else { 4096 };
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break };
            if line.len() > control_limit || tx.send(line).is_err() {
                break;
            }
        }
    });
    // The crash matrix starts a separate real Host for every cut/order. Keep its total
    // budget proportional to the case count; individual IPC/action deadlines stay unchanged.
    let probe_seconds = if host_retirement {
        180
    } else if restarting {
        restart::CHECKS.len() as u64 * 30
    } else if toolbar {
        480
    } else {
        180
    };
    let deadline = Instant::now() + Duration::from_secs(probe_seconds);
    let result: Result<(), String> = (|| {
        let mut passed = false;
        loop {
            if Instant::now() >= deadline {
                return Err("extension pairing probe deadline exceeded".into());
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => {
                    let value: Value = serde_json::from_str(&line)
                        .map_err(|_| "invalid probe control response")?;
                    if value["event"] == "passed" {
                        let expected = if browser_exit {
                            json!([
                                "execution-clock-real-transactions",
                                "production-sw-negotiates",
                                "browser-exit-recovers-original-owner"
                            ])
                        } else if host_retirement {
                            json!(restart::RETIREMENT_CHECKS)
                        } else if restarting {
                            json!(restart::CHECKS)
                        } else if retention {
                            json!(dispatch::RETENTION_CHECKS)
                        } else if dispatch {
                            json!(dispatch::CHECKS)
                        } else if completion {
                            json!([
                                "real-pairing-and-observation",
                                "click-once",
                                "duplicate-is-zero-dispatch",
                                "cjk-input",
                                "lost-claim-reply-is-zero-dispatch",
                                "lost-settle-reply-retries-only-cleanup",
                                "host-cancel-joins-real-wait",
                                "unpair-joins-original-and-cancel-script"
                            ])
                        } else if toolbar {
                            json!([
                                "stable-extension-id",
                                "pair-with-app-confirmation",
                                "no-popup-key-message",
                                "toolbar-grant-required",
                                "toolbar-user-share",
                                "toolbar-user-unshare"
                            ])
                        } else {
                            json!(PAIRING_CHECKS)
                        };
                        passed = value["checks"] == expected;
                    } else if value["event"] == "failed" {
                        return Err(
                            "real extension pairing failed; see sanitized probe stage".into()
                        );
                    } else {
                        let reply = host.control(&value)?;
                        writeln!(input, "{reply}").map_err(|_| "pairing probe input closed")?;
                        input.flush().map_err(|_| "pairing probe input closed")?;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let status = child.wait().map_err(|_| "pairing probe exit unavailable")?;
                    if passed && status.success() {
                        return Ok(());
                    }
                    return Err("pairing probe exited without verified postconditions".into());
                }
            }
        }
    })();
    drop(input);
    drop(host);
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    // Job handles retain ownership even after the direct child has exited.
    let cleaned = resources.cleanup();
    let _ = reader.join();
    finish_probe(result, cleaned)?;
    if browser_exit {
        println!("gate: existing_tab_browser_exit PASS browser={kind} checks=3 evidence=complete-browser-exit-same-profile-reopen-and-production-sw-http");
    } else if host_retirement {
        println!("gate: existing_tab_host_retirement PASS browser={kind} checks={} evidence=two-live-product-hosts-and-production-sw-http", restart::RETIREMENT_CHECKS.len());
    } else if restarting {
        println!("gate: existing_tab_restart PASS browser={kind} checks={} evidence=separate-app-host-process-and-production-sw-http", restart::CHECKS.len());
    } else if retention {
        println!("gate: existing_tab_retention PASS browser={kind} checks={} host=process-wide-broker evidence=real-bfcache-occupancy-restoration-and-new-action", dispatch::RETENTION_CHECKS.len());
    } else if dispatch {
        println!("gate: existing_tab_dispatch PASS browser={kind} checks={} host=process-wide-broker evidence=private-fixture-admission-production-sw-and-v2-http-actions", dispatch::CHECKS.len());
    } else if completion {
        println!("gate: existing_tab_completion PASS browser={kind} checks=8 host=process-wide-broker evidence=private-fixture-admission-mv3-actions-and-http-receipts");
    } else if toolbar {
        println!("gate: existing_tab_toolbar PASS browser={kind} checks=6 host=process-wide-broker evidence=native-toolbar-share-candidate");
    } else {
        println!("{}", pairing_pass_line(kind));
    }
    Ok(())
}

// A failed cleanup must not erase the failure that caused cleanup to run.
// Both errors originate in this owned probe, never in credential-bearing HTTP bodies.
fn finish_probe(result: Result<(), String>, cleanup: Result<(), String>) -> Result<(), String> {
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!(
            "{error}; owned probe cleanup also failed: {cleanup}"
        )),
    }
}

fn control_reply(
    broker: &ComputerUseBroker,
    endpoint: &str,
    value: &Value,
) -> Result<Value, String> {
    match value["command"].as_str().unwrap_or_default() {
        command if command.starts_with("completion-") => completion::control(broker, value),
        command if command.starts_with("mcp-") => mcp::control(broker, endpoint, value),
        "ready" => Ok(json!({"ok": true})),
        "begin" => {
            let ch = broker.begin_pairing()?;
            Ok(json!({"endpoint": endpoint, "nonce": ch.nonce, "code": ch.verification_code}))
        }
        "confirm" => Ok(
            json!({"ok": broker.confirm_pairing(value["nonce"].as_str().unwrap_or_default()).is_ok()}),
        ),
        "status" => Ok(
            json!({"paired": broker.tabs().pairing_session_key().is_some(),
            "stored": broker.tabs().has_stored_pairing_connection_for_test()}),
        ),
        "shared" => Ok(json!(broker
            .tabs()
            .list_shared_candidates()
            .into_iter()
            .map(|(id, title, url)| json!({"id": id, "title": title, "url": url}))
            .collect::<Vec<_>>())),
        "observe-shared" => {
            // Private probe pipe supplies explicit fixture consent. This is not App UI/MCP acceptance.
            let tab = broker
                .tabs()
                .grant_picker_tab(
                    "probe-owner",
                    "probe-run",
                    value["selector"].as_str().unwrap_or_default(),
                )
                .map_err(|_| "probe candidate changed")?;
            let observed = broker.tabs().observe_existing_document(
                "probe-owner",
                "probe-run",
                &tab.tab_id,
                value["screenshot"].as_bool().unwrap_or(false),
            );
            let _ = broker.tabs().return_borrowed("probe-run", &tab.tab_id);
            Ok(match observed {
                Ok(observation) => json!({"ok": true, "observation": observation}),
                Err(_) => json!({"ok": false}),
            })
        }
        "revoke" => {
            broker.tabs().revoke_pairing();
            Ok(json!({"ok": true}))
        }
        _ => Err("unsupported probe control request".into()),
    }
}

fn capable_live_browser() -> Option<(PathBuf, &'static str)> {
    let mut found = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let pw = PathBuf::from(&local).join("ms-playwright");
        if let Ok(entries) = std::fs::read_dir(&pw) {
            for entry in entries.flatten() {
                for rel in ["chrome-win64/chrome.exe", "chrome-win/chrome.exe"] {
                    let p = entry.path().join(rel);
                    if p.is_file() {
                        found.push((p, "chrome-for-testing"));
                    }
                }
            }
        }
        let cft = PathBuf::from(local).join("Google/Chrome for Testing/Application/chrome.exe");
        if cft.is_file() {
            found.push((cft, "chrome-for-testing"));
        }
    }
    for p in [
        r"C:\Program Files\Google\Chrome for Testing\Application\chrome.exe",
        r"C:\chrome-for-testing\chrome.exe",
    ] {
        let p = PathBuf::from(p);
        if p.is_file() {
            found.push((p, "chrome-for-testing"));
        }
    }
    let packaged = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("computer-use")
        .join("seed")
        .join("chromium")
        .join("chrome-win")
        .join("chrome.exe");
    if packaged.is_file() {
        found.push((packaged, "packaged-chromium"));
    }
    found.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::PAIRING_CHECKS;
    use super::{finish_probe, pairing_pass_line};

    #[test]
    fn pairing_pass_line_uses_the_evaluated_list_length() {
        let line = pairing_pass_line("fixture");
        assert!(line.contains(&format!("checks={}", PAIRING_CHECKS.len())));
        assert!(PAIRING_CHECKS.contains(&"stable-extension-id"));
        assert!(
            line.starts_with("gate: existing_tab_extension_pairing PASS browser=fixture checks=")
        );
    }

    #[test]
    fn cleanup_failure_preserves_the_primary_probe_failure() {
        assert_eq!(finish_probe(Ok(()), Ok(())), Ok(()));
        assert_eq!(
            finish_probe(Err("primary".into()), Ok(())),
            Err("primary".into())
        );
        assert_eq!(
            finish_probe(Ok(()), Err("cleanup".into())),
            Err("cleanup".into())
        );
        assert_eq!(
            finish_probe(Err("primary".into()), Err("cleanup".into())),
            Err("primary; owned probe cleanup also failed: cleanup".into())
        );
    }
}
