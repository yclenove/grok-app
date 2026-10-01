//! Computer Use Host runtime.
//!
//! Parse → schema → identity/permission → lease/cancel → adapter →
//! normalize → verify → project. Adapters never become the tool surface.

#![allow(unused_imports)]

pub(crate) use grok_computer_use_core::{
    adapter, broker, browser, error, lease, preview, protocol,
};
#[cfg(debug_assertions)]
pub(crate) mod app_shell;
#[cfg(feature = "computer-use-probe")]
mod browser_managed_contract;
#[cfg(feature = "computer-use-probe")]
pub(crate) mod browser_packaged_contract;
mod browser_supervisor;
#[cfg(feature = "computer-use-probe")]
mod browser_supervisor_gates;
#[cfg(feature = "computer-use-probe")]
mod browser_supervisor_gates_nav;
#[cfg(feature = "computer-use-probe")]
mod d8;
#[cfg(feature = "computer-use-probe")]
mod d8_faults;
pub(crate) mod diagnostics;
mod eligibility;
#[cfg(feature = "computer-use-probe")]
mod extension_pair;
mod feature;
mod feature_lifecycle;
pub(crate) mod gnome_helper;
mod inject;
pub(crate) mod ipc;
#[cfg(feature = "computer-use-probe")]
mod mcp_scripted_agent;
#[cfg(feature = "computer-use-probe")]
mod mcp_scripted_agent_run;
mod playwright_worker;
pub(crate) mod runtime;
pub(crate) mod sessions;
pub(crate) mod shutdown;
#[cfg(test)]
mod synthetic;
#[cfg(test)]
pub(crate) mod test_support;
pub(crate) mod webview;
#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod webview_host;

#[cfg(target_os = "windows")]
mod windows_adapter;

#[cfg(target_os = "windows")]
mod windows_clipboard;

#[cfg(target_os = "windows")]
mod windows_identity;

#[cfg(target_os = "windows")]
mod windows_uia;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod windows_fixture;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod windows_s45;

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
mod windows_webview2_fixture;

#[cfg(target_os = "macos")]
mod macos_adapter;

#[cfg(target_os = "linux")]
mod linux_adapter;

#[cfg(target_os = "linux")]
pub(crate) mod linux_parent;
#[cfg(target_os = "linux")]
pub(crate) mod linux_portal;

pub use adapter::{
    ActionScope, ActionSupport, AdapterActResult, Capabilities, ComputerUseAdapter,
    DispatchRequest, SurfaceKind, TargetInfo,
};
pub use broker::{
    BrokerOptions, ComputerUseBroker, RecoverySource, RunTimings, StopCleanupTicket, StopState,
    TraceAudience, TraceEvent,
};
pub use eligibility::refuse_if_not_local;
pub use error::{BrokerError, LeaseError};
pub use feature::{
    is_session_enabled, set_feature_enabled, set_session_enabled, set_session_surface,
    should_inject_session_mcp, FEATURE_DEFAULT,
};
pub(crate) use feature_lifecycle::persist_feature_transition;
pub use inject::{mcp_acp_entry, MCP_SERVER_NAME};
pub use lease::DesktopLease;
pub use preview::PreviewController;
pub use protocol::{
    ActionKind, ActionOutcome, ActionRequest, ActionTarget, Observation, ObservationImage,
    OutcomeKind, PROTOCOL_VERSION,
};
#[cfg(test)]
pub use synthetic::{render_synthetic_png, SyntheticOracle};

use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

static RUNTIME: OnceLock<Mutex<Option<Arc<ComputerUseBroker>>>> = OnceLock::new();
static WEBVIEW: OnceLock<Mutex<Arc<webview::WebViewAdapter>>> = OnceLock::new();

/// Process-wide App WebView backend. Fixture `WebViewAdapter::new()` is not this.
pub fn product_webview() -> Arc<webview::WebViewAdapter> {
    WEBVIEW
        .get_or_init(|| Mutex::new(Arc::new(webview::WebViewAdapter::new())))
        .lock()
        .clone()
}

fn runtime_slot() -> &'static Mutex<Option<Arc<ComputerUseBroker>>> {
    RUNTIME.get_or_init(|| Mutex::new(None))
}

/// Process-wide broker. Tests construct their own instances.
pub fn global_broker() -> Option<Arc<ComputerUseBroker>> {
    runtime_slot().lock().clone()
}

/// Revoke every Computer Use surface before process teardown.
pub fn shutdown_product() {
    if let Err(error) = shutdown_product_checked() {
        tracing::warn!(%error, "computer-use local shutdown did not complete");
    }
}

pub(crate) fn shutdown_product_checked() -> Result<(), String> {
    let mut errors = Vec::new();
    set_feature_enabled(false);
    if let Err(error) = sessions::revoke_all_checked() {
        errors.push(format!("runs: {error}"));
    }
    if let Some(broker) = global_broker() {
        broker.set_feature_enabled(false);
        broker.tabs().revoke_pairing();
    }
    product_webview().unbind();
    if let Err(error) = browser_supervisor::shutdown_product() {
        errors.push(format!("managed browser: {error}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Start the process-wide broker + loopback IPC (used by Tauri commands and MCP inject).
pub fn ensure_host_runtime() -> Arc<ComputerUseBroker> {
    let mut slot = runtime_slot().lock();
    if let Some(existing) = slot.as_ref() {
        if let Err(error) = existing.register_existing_browser_adapter() {
            tracing::warn!(%error, "computer-use existing-tab executor registration failed");
        }
        // Feature transitions are owned by `persist_feature_transition`.
        // Refreshing adapters/IPC must never fence runs and discard their
        // generation-bound cleanup tickets.
        if let Err(error) = existing.register_surface_adapter(
            grok_computer_use_core::adapter::SurfaceKind::WebView,
            product_webview(),
        ) {
            tracing::warn!(%error, "computer-use WebView executor registration failed");
        }
        if ipc::endpoint().is_none() {
            let _ = ipc::ensure(existing.clone());
        }
        if let Err(e) = browser_supervisor::ensure_product_attached(existing) {
            tracing::info!("computer-use managed browser attach retry: {e}");
        }
        return existing.clone();
    }
    let enabled = crate::store::load_settings().computer_use_enabled;
    set_feature_enabled(enabled);
    let cu_root = crate::paths::app_data_root().join("computer-use");
    let _ = grok_computer_use_core::privacy::write_owner(&cu_root, "grok-app");
    let opts = BrokerOptions {
        feature_enabled: enabled,
        lease_path: cu_root.join("desktop.lease"),
        action_timeout: std::time::Duration::from_secs(8),
        ..BrokerOptions::default()
    };
    let broker = Arc::new(ComputerUseBroker::new(platform_adapter(), opts));
    if let Err(error) = broker.register_existing_browser_adapter() {
        tracing::warn!(%error, "computer-use existing-tab executor registration failed");
    }
    if let Err(error) = broker.register_surface_adapter(
        grok_computer_use_core::adapter::SurfaceKind::WebView,
        product_webview(),
    ) {
        tracing::warn!(%error, "computer-use WebView executor registration failed");
    }
    if let Err(e) = browser_supervisor::attach_product(&broker) {
        tracing::info!("computer-use managed browser not attached: {e}");
    }
    *slot = Some(broker.clone());
    if let Ok(ep) = ipc::ensure(broker.clone()) {
        tracing::info!(url = %ep.url, "computer-use ipc bound on loopback");
    }
    broker
}

/// Default adapter for this OS. Unproven backends report capabilities only.
/// Process entry for `cu-probe` bin (avoids the lib test harness exe).
#[cfg(feature = "computer-use-probe")]
pub fn computer_use_probe_main() -> i32 {
    #[cfg(target_os = "windows")]
    windows_adapter::ensure_dpi_aware();
    let mut args = std::env::args().skip(1);
    if let Some(command) = args.next() {
        if args.next().is_some() {
            eprintln!("cu_probe: {command} takes no arguments");
            return 2;
        }
        return match command.as_str() {
            "existing-tab-host-fixture" => match extension_pair::run_restart_host_fixture() {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("restart Host fixture: FAIL {error}");
                    1
                }
            },
            "browser-error-contract" => {
                match browser_supervisor::run_browser_error_contract_gate() {
                    Ok(()) => {
                        println!("computer_use browser error contract: PASS");
                        0
                    }
                    Err(error) => {
                        eprintln!("computer_use browser error contract: FAIL {error}");
                        1
                    }
                }
            }
            "browser-managed-contract" => {
                match browser_managed_contract::run_browser_managed_contract_gate() {
                    Ok(()) => {
                        println!("computer_use browser managed contract: PASS");
                        0
                    }
                    Err(error) => {
                        eprintln!("computer_use browser managed contract: FAIL {error}");
                        1
                    }
                }
            }
            "browser-managed-preview" => {
                match browser_managed_contract::run_browser_managed_preview_gate() {
                    Ok(()) => {
                        println!("computer_use source browser managed preview: PASS");
                        0
                    }
                    Err(error) => {
                        eprintln!("computer_use source browser managed preview: FAIL {error}");
                        1
                    }
                }
            }
            "browser-packaged-preview" => {
                match browser_managed_contract::run_browser_packaged_preview_gate() {
                    Ok(()) => {
                        println!("computer_use packaged browser managed preview: PASS");
                        0
                    }
                    Err(error) => {
                        eprintln!("computer_use packaged browser managed preview: FAIL {error}");
                        1
                    }
                }
            }
            "browser-packaged-contract" => {
                match browser_packaged_contract::run_browser_packaged_contract_gate() {
                    Ok(()) => {
                        println!("computer_use browser packaged contract: PASS");
                        0
                    }
                    Err(error) => {
                        eprintln!("computer_use browser packaged contract: FAIL {error}");
                        1
                    }
                }
            }
            "isolated-runtime-repair" => match runtime::run_isolated_repair_gate() {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("computer_use isolated runtime repair: FAIL {error}");
                    1
                }
            },
            "windows-choice-captured" => {
                #[cfg(target_os = "windows")]
                {
                    match windows_fixture::run_choice_captured_set_value() {
                        Ok(()) => 0,
                        Err(error) => {
                            eprintln!("computer_use windows choice captured: FAIL {error}");
                            1
                        }
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("cu_probe windows-choice-captured: not_run (host is not windows)");
                    2
                }
            }
            "windows-choice" => {
                #[cfg(target_os = "windows")]
                {
                    match windows_fixture::run_choice_click_text() {
                        Ok(()) => 0,
                        Err(error) => {
                            eprintln!("computer_use windows choice: FAIL {error}");
                            1
                        }
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("cu_probe windows-choice: not_run (host is not windows)");
                    2
                }
            }
            "windows-choice-stop" => {
                #[cfg(target_os = "windows")]
                {
                    match windows_fixture::run_choice_stop_and_dead() {
                        Ok(()) => 0,
                        Err(error) => {
                            eprintln!("computer_use windows choice stop: FAIL {error}");
                            1
                        }
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("cu_probe windows-choice-stop: not_run (host is not windows)");
                    2
                }
            }
            "windows-text-peer" => {
                #[cfg(target_os = "windows")]
                {
                    return match windows_adapter::text_probe::run_peer() {
                        Ok(()) => 0,
                        Err(error) => {
                            eprintln!("owned text peer: FAIL {error}");
                            1
                        }
                    };
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("owned text peer requires Windows");
                    return 1;
                }
            }
            "windows-text-isolated" => {
                #[cfg(target_os = "windows")]
                {
                    return match windows_adapter::text_probe::run() {
                        Ok(()) => 0,
                        Err(error) => {
                            eprintln!("isolated native text: FAIL {error}");
                            1
                        }
                    };
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("isolated native text requires Windows");
                    return 1;
                }
            }
            "windows-clipboard-isolated" => {
                #[cfg(target_os = "windows")]
                {
                    return match windows_clipboard::probe::run_isolated() {
                        Ok(()) => 0,
                        Err(error) => {
                            eprintln!("isolated clipboard: FAIL {error}");
                            1
                        }
                    };
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("isolated clipboard requires Windows");
                    return 1;
                }
            }
            "windows-clipboard" => {
                #[cfg(target_os = "windows")]
                {
                    match windows_fixture::run_clipboard_restore() {
                        Ok(()) => 0,
                        Err(error) => {
                            eprintln!("Windows clipboard: FAIL {error}");
                            1
                        }
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("windows-clipboard: not_run (host is not windows)");
                    2
                }
            }
            "windows-scroll" => {
                #[cfg(target_os = "windows")]
                {
                    match windows_fixture::run_scroll_direction() {
                        Ok(()) => 0,
                        Err(error) => {
                            eprintln!("Windows scroll: FAIL {error}");
                            1
                        }
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("Windows scroll: not_run (host is not windows)");
                    2
                }
            }
            "windows-native-stop" => {
                #[cfg(target_os = "windows")]
                {
                    match windows_fixture::run_native_stop() {
                        Ok(()) => 0,
                        Err(error) => {
                            eprintln!("computer_use windows native Stop: FAIL {error}");
                            1
                        }
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("windows-native-stop: not_run (host is not windows)");
                    2
                }
            }
            "windows-wpf" => {
                #[cfg(target_os = "windows")]
                {
                    match windows_s45::run_wpf_fixture() {
                        Ok(()) => 0,
                        Err(error) => {
                            eprintln!("Windows WPF: FAIL {error}");
                            1
                        }
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("windows-wpf: not_run (host is not windows)");
                    2
                }
            }
            "windows-s45" => {
                #[cfg(target_os = "windows")]
                {
                    match windows_s45::run_windows_s45_fixtures() {
                        Ok(()) => 0,
                        Err(error) => {
                            eprintln!("Windows S4.5: FAIL {error}");
                            1
                        }
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("windows-s45: not_run (host is not windows)");
                    2
                }
            }
            "windows-native" => {
                #[cfg(target_os = "windows")]
                {
                    run_windows_native_gates()
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("cu_probe windows-native: not_run (host is not windows)");
                    2
                }
            }
            "mcp-scripted-agent" => match mcp_scripted_agent::run_mcp_scripted_agent_gate() {
                Ok(()) => {
                    println!("computer_use mcp scripted agent: PASS");
                    0
                }
                Err(error) => {
                    eprintln!("computer_use mcp scripted agent: FAIL {error}");
                    1
                }
            },
            "product-managed-route" => match mcp_scripted_agent::run_product_managed_route_gate() {
                Ok(()) => {
                    println!("computer_use product managed route: PASS");
                    0
                }
                Err(error) => {
                    eprintln!("computer_use product managed route: FAIL {error}");
                    1
                }
            },
            "perf" | "faults" => match d8::run_harness() {
                Ok(()) => {
                    println!("computer_use d8 perf/faults: PASS");
                    0
                }
                Err(error) => {
                    eprintln!("computer_use d8 perf/faults: FAIL {error}");
                    1
                }
            },
            "privacy" => match diagnostics::run_host_privacy_gates() {
                Ok(()) => {
                    println!("computer_use privacy: PASS");
                    0
                }
                Err(error) => {
                    eprintln!("computer_use privacy: FAIL {error}");
                    1
                }
            },
            "broker" => match crate::computer_use::broker::run_broker_gates() {
                Ok(()) => {
                    println!("computer_use broker gates: PASS");
                    0
                }
                Err(error) => {
                    eprintln!("computer_use broker gates: FAIL {error}");
                    1
                }
            },
            "session-mcp-inject" => {
                match crate::computer_use::inject::run_session_mcp_inject_gates() {
                    Ok(()) => {
                        println!("computer_use session mcp inject: PASS");
                        0
                    }
                    Err(error) => {
                        eprintln!("computer_use session mcp inject: FAIL {error}");
                        1
                    }
                }
            }
            #[cfg(target_os = "windows")]
            "webview-crash" => match webview::run_renderer_exit_gate() {
                Ok(()) => return 0,
                Err(error) => {
                    eprintln!("computer_use disposable renderer: FAIL {error}");
                    return 1;
                }
            },
            #[cfg(target_os = "windows")]
            "webview-close" => match webview::run_native_close_gate() {
                Ok(()) => return 0,
                Err(error) => {
                    eprintln!("computer_use native close: FAIL {error}");
                    return 1;
                }
            },
            #[cfg(target_os = "windows")]
            mode @ ("webview-close-idle" | "webview-close-completed") => {
                match webview::run_native_close_baseline(mode == "webview-close-completed") {
                    Ok(()) => return 0,
                    Err(error) => {
                        eprintln!("computer_use native close baseline: FAIL {error}");
                        return 1;
                    }
                }
            }
            "webview" => match webview::run_webview_gates() {
                Ok(()) => {
                    println!("computer_use webview: PASS");
                    0
                }
                Err(error) => {
                    eprintln!("computer_use webview: FAIL {error}");
                    1
                }
            },
            "existing-tab-extension"
            | "existing-tab-extension-toolbar"
            | "existing-tab-dispatch"
            | "existing-tab-retention"
            | "existing-tab-restart"
            | "existing-tab-host-retirement"
            | "existing-tab-browser-exit"
            | "existing-tab-completion" => {
                match if command == "existing-tab-browser-exit" {
                    extension_pair::run_live_extension_browser_exit_required()
                } else if command == "existing-tab-host-retirement" {
                    extension_pair::run_live_extension_host_retirement_required()
                } else if command == "existing-tab-restart" {
                    extension_pair::run_live_extension_restart_required()
                } else if command == "existing-tab-retention" {
                    extension_pair::run_live_extension_retention_required()
                } else if command == "existing-tab-dispatch" {
                    extension_pair::run_live_extension_dispatch_required()
                } else if command == "existing-tab-completion" {
                    extension_pair::run_live_extension_completion_required()
                } else if command == "existing-tab-extension-toolbar" {
                    extension_pair::run_live_extension_toolbar_required()
                } else {
                    extension_pair::run_live_extension_pair_required()
                } {
                    Ok(()) => {
                        println!("computer_use existing-tab extension: PASS");
                        0
                    }
                    Err(error) => {
                        eprintln!("computer_use existing-tab extension: FAIL {error}");
                        1
                    }
                }
            }
            _ => {
                eprintln!("cu_probe: unknown gate {command}");
                2
            }
        };
    }
    if std::env::var("GROK_CU_HOLD_IPC").ok().as_deref() == Some("1") {
        #[cfg(target_os = "windows")]
        {
            return match windows_fixture::hold_p8_ipc() {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("hold ipc: {e}");
                    1
                }
            };
        }
        #[cfg(not(target_os = "windows"))]
        {
            eprintln!("GROK_CU_HOLD_IPC is windows-only");
            return 1;
        }
    }
    if let Err(e) = crate::computer_use::broker::run_broker_gates() {
        eprintln!("computer_use broker gates: FAIL {e}");
        return 1;
    }
    println!("computer_use broker gates: PASS");
    if let Err(e) = crate::computer_use::feature::run_surface_inject_gates() {
        eprintln!("computer_use surface inject: FAIL {e}");
        return 1;
    }
    println!("computer_use surface inject: PASS");
    if let Err(e) = crate::computer_use::inject::run_session_mcp_inject_gates() {
        eprintln!("computer_use session mcp inject: FAIL {e}");
        return 1;
    }
    println!("computer_use session mcp inject: PASS");
    if let Err(e) = grok_computer_use_core::tools::run_model_tool_surface_gates() {
        eprintln!("computer_use model tool surface: FAIL {e}");
        return 1;
    }
    println!("computer_use model tool surface: PASS");
    if let Err(e) = grok_computer_use_core::tools::run_agent_loop_gates() {
        eprintln!("computer_use agent loop: FAIL {e}");
        return 1;
    }
    println!("computer_use agent loop: PASS");
    if let Err(e) = crate::computer_use::eligibility::run_classify_session_gates() {
        eprintln!("computer_use classify session: FAIL {e}");
        return 1;
    }
    println!("computer_use classify session: PASS");
    if let Err(e) = crate::computer_use::webview::run_webview_gates() {
        eprintln!("computer_use webview: FAIL {e}");
        return 1;
    }
    println!("computer_use webview: PASS");
    if let Err(e) = crate::computer_use::diagnostics::run_host_privacy_gates() {
        eprintln!("computer_use privacy: FAIL {e}");
        return 1;
    }
    println!("computer_use privacy: PASS");
    println!("computer_use IPC regression: run cargo test -p grok-computer-use-core");
    if let Err(e) = crate::computer_use::browser::run_browser_gates() {
        eprintln!("computer_use browser gates: FAIL {e}");
        return 1;
    }
    println!("computer_use browser gates: PASS");
    if let Err(e) = extension_pair::run_live_extension_pair_gate() {
        eprintln!("computer_use existing-tab pairing live: FAIL {e}");
        return 1;
    }
    if let Err(e) = browser_supervisor::run_supervisor_gate() {
        eprintln!("computer_use managed browser supervisor: FAIL {e}");
        return 1;
    }
    println!("computer_use managed browser supervisor: PASS");
    if let Err(e) = browser_supervisor_gates::run_profile_tab_gate() {
        eprintln!("computer_use managed browser profiles: FAIL {e}");
        return 1;
    }
    println!("computer_use managed browser profiles: PASS");
    if let Err(e) = browser_supervisor_gates::run_observe_act_gate() {
        eprintln!("computer_use managed browser observe/act: FAIL {e}");
        return 1;
    }
    println!("computer_use managed browser observe/act: PASS");
    if let Err(e) = browser_supervisor_gates_nav::run_nav_upload_download_gate() {
        eprintln!("computer_use managed browser nav/upload/download: FAIL {e}");
        return 1;
    }
    println!("computer_use managed browser nav/upload/download: PASS");
    if let Err(e) = browser_supervisor_gates_nav::run_s75_integration_gate() {
        eprintln!("computer_use managed browser s75: FAIL {e}");
        return 1;
    }
    println!("computer_use managed browser s75: PASS");
    #[cfg(target_os = "macos")]
    {
        match macos_adapter::run_native_selftest() {
            Ok(()) => {
                println!("macos native selftest: listed windows (fixture proof still separate)")
            }
            Err(e) => println!("macos native: not_run ({e})"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    println!(
        "macos native: not_run (host is not macOS; CGWindowList adapter compiles on macos only)"
    );
    #[cfg(target_os = "linux")]
    {
        if std::env::var("GROK_CU_X11_FIXTURE").as_deref() != Ok("owned-xvfb") {
            println!("linux native: not_run (owned Xvfb acceptance runner required)");
        } else if let Err(error) = linux_adapter::run_native_selftest() {
            eprintln!("linux X11 native acceptance: FAIL {error}");
            return 1;
        } else {
            println!("linux X11 native acceptance: PASS (not native Wayland or installed App)");
        }
    }
    #[cfg(not(target_os = "linux"))]
    println!("linux native: not_run (host is not linux; X11 adapter compiles on linux only; Wayland is not XWayland)");
    #[cfg(target_os = "windows")]
    {
        run_windows_native_gates()
    }
    #[cfg(not(target_os = "windows"))]
    0
}

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
fn run_windows_native_gates() -> i32 {
    if let Err(e) = windows_fixture::run_uia_element_tree() {
        eprintln!("computer_use windows uia tree: FAIL {e}");
        return 1;
    }
    println!("computer_use windows uia tree: PASS");
    if let Err(e) = windows_fixture::run_geometry_capture() {
        eprintln!("computer_use windows geometry: FAIL {e}");
        return 1;
    }
    println!("computer_use windows geometry: PASS");
    if let Err(e) = windows_fixture::run_uia_full_actions() {
        eprintln!("computer_use windows uia actions: FAIL {e}");
        return 1;
    }
    println!("computer_use windows uia actions: PASS");
    if let Err(e) = windows_fixture::run_scroll_direction() {
        eprintln!("computer_use windows scroll direction: FAIL {e}");
        return 1;
    }
    println!("computer_use windows scroll direction: PASS");
    if let Err(e) = windows_fixture::run_p8_native_loop() {
        eprintln!("computer_use windows p8 native: FAIL {e}");
        return 1;
    }
    println!("computer_use windows p8 native: PASS");
    if let Err(e) = windows_fixture::run_identity_host_protect() {
        eprintln!("computer_use windows identity: FAIL {e}");
        return 1;
    }
    println!("computer_use windows identity: PASS");
    if let Err(e) = windows_fixture::run_clipboard_restore() {
        eprintln!("computer_use windows clipboard: FAIL {e}");
        return 1;
    }
    println!("computer_use windows clipboard: PASS");
    if let Err(e) = windows_fixture::run_focus_drift_pauses() {
        eprintln!("computer_use windows focus drift: FAIL {e}");
        return 1;
    }
    println!("computer_use windows focus drift: PASS");
    if let Err(e) = windows_fixture::run_user_takeover_pauses() {
        eprintln!("computer_use windows takeover: FAIL {e}");
        return 1;
    }
    println!("computer_use windows takeover: PASS");
    if let Err(e) = windows_fixture::run_security_and_cancel() {
        eprintln!("computer_use windows security/cancel: FAIL {e}");
        return 1;
    }
    println!("computer_use windows security/cancel: PASS");
    if let Err(e) = windows_s45::run_windows_s45_fixtures() {
        eprintln!("computer_use windows s45 fixtures: FAIL {e}");
        return 1;
    }
    println!("computer_use windows s45 fixtures: PASS");
    let adapter = windows_adapter::WindowsAdapter::new();
    match adapter.list_targets() {
        Ok(list) => {
            println!("windows list_targets: {}", list.len());
            // Enumeration is a gate, not permission to record unrelated window titles.
            if let Ok(want) = std::env::var("GROK_CU_OBSERVE_TITLE") {
                if let Some(t) = list.iter().find(|x| x.title.contains(&want)) {
                    match adapter.observe(&t.target_id) {
                        Ok(obs) => {
                            println!(
                                "observe {}: {}x{} snap={}",
                                want, obs.image.width, obs.image.height, obs.snapshot_id
                            );
                            let click = crate::computer_use::protocol::ActionRequest {
                                version: crate::computer_use::PROTOCOL_VERSION,
                                action_id: "fix-click".into(),
                                run_id: "probe".into(),
                                target_id: t.target_id.clone(),
                                target_generation: 1,
                                snapshot_id: obs.snapshot_id.clone(),
                                geometry_revision: obs.geometry_revision,
                                action: crate::computer_use::protocol::ActionKind::Click,
                                target: crate::computer_use::protocol::ActionTarget::Coord {
                                    x: 70.0,
                                    y: 72.0,
                                },
                                parameters: serde_json::json!({}),
                            };
                            let dispatch = crate::computer_use::adapter::DispatchRequest {
                                managed_request: None,
                                cancellation: Default::default(),
                                run_id: click.run_id.clone(),
                                action_id: click.action_id.clone(),
                                generation: 1,
                                target_id: click.target_id.clone(),
                                target_generation: 1,
                                snapshot_id: click.snapshot_id.clone(),
                                geometry_revision: click.geometry_revision,
                                action: click.action,
                                target: click.target.clone(),
                                parameters: click.parameters.clone(),
                                scope: crate::computer_use::adapter::ActionScope::Directed,
                            };
                            match adapter.act(&dispatch) {
                                Ok(r) => {
                                    println!("act applied={} detail={}", r.applied, r.detail)
                                }
                                Err(e) => eprintln!("act: {e}"),
                            }
                        }
                        Err(e) => {
                            eprintln!("observe {want}: FAIL {e}");
                            return 1;
                        }
                    }
                } else {
                    eprintln!("observe title not found: {want}");
                    return 1;
                }
            }
        }
        Err(e) => {
            eprintln!("windows list_targets: FAIL {e}");
            return 1;
        }
    }
    0
}

fn os_adapter() -> Arc<dyn ComputerUseAdapter> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows_adapter::WindowsAdapter::new())
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos_adapter::MacosAdapter::new())
    }
    #[cfg(target_os = "linux")]
    {
        if linux_portal::selected() {
            linux_portal::registry()
        } else {
            Arc::new(linux_adapter::LinuxAdapter::new())
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        panic!("Computer Use is not supported on this operating system")
    }
}

/// An affordance, never a target or a grant. Read-only discovery must not open
/// the system picker or publish a fictional monitor identity to the model.
pub fn desktop_selection_mode() -> &'static str {
    #[cfg(target_os = "linux")]
    if linux_portal::selected() {
        return "portal";
    }
    "targets"
}

/// Product desktop path: Host-owned wrapper around the OS adapter.
/// Attaches a private worker when the App runtime pack contains a driver binary.
pub fn platform_adapter() -> Arc<dyn ComputerUseAdapter> {
    let owned = grok_computer_use_core::owned::HostOwnedAdapter::new(os_adapter());
    if let Ok(binary) = runtime::resolve(grok_computer_use_core::runtime::DRIVER) {
        if let Some(pack) = binary.parent().and_then(|p| p.parent()) {
            let manifest = pack.join("manifest.json");
            if let Ok(worker) = grok_computer_use_core::driver::PrivateWorker::start(
                grok_computer_use_core::driver::WorkerOptions {
                    binary,
                    manifest,
                    host_bundle_id: "com.grokapp.desktop".into(),
                    startup_timeout: std::time::Duration::from_secs(5),
                },
            ) {
                owned.attach_worker(Arc::new(worker));
            }
        }
    }
    Arc::new(owned)
}
