//! R3.4 case 19: BrowserSupervisor → LoopbackPlaywrightWorker → production
//! server → self-built page. Not a substitute for installed-App E4.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use grok_computer_use_core::browser::{ManagedBrowserWorker, ManagedLocator, ManagedWorkerAction};
use uuid::Uuid;

use super::browser_supervisor::{BrowserSupervisor, SpawnRequest};
use super::playwright_worker::LoopbackPlaywrightWorker;

#[path = "browser_managed_cleanup.rs"]
mod cleanup;
#[path = "browser_managed_fixture.rs"]
mod fixture;
#[path = "browser_run_contract.rs"]
mod lifecycle_contract;
#[path = "browser_native_contract.rs"]
mod native_contract;
#[path = "browser_profile_contract.rs"]
mod profile_contract;
#[path = "browser_resume_fault_contract.rs"]
mod resume_fault_contract;

fn packaged_seed_node() -> Result<PathBuf, String> {
    let seed = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("computer-use")
        .join("seed")
        .join("bin")
        .join("node.exe");
    let abs = std::fs::canonicalize(&seed).map_err(|e| {
        format!(
            "packaged Windows runtime Node missing at {}: {e}",
            seed.display()
        )
    })?;
    if !abs.is_absolute() || !abs.is_file() {
        return Err(format!(
            "packaged Windows runtime Node is not an absolute file: {}",
            abs.display()
        ));
    }
    Ok(abs)
}

pub fn run_browser_managed_contract_gate() -> Result<(), String> {
    run_contract(false, false)
}

pub fn run_browser_managed_preview_gate() -> Result<(), String> {
    run_contract(true, true)
}

pub fn run_browser_packaged_preview_gate() -> Result<(), String> {
    run_contract(false, true)
}

fn run_contract(source_worker_only: bool, check_preview: bool) -> Result<(), String> {
    if check_preview {
        verify_legacy_observation_guard()?;
        lifecycle_contract::verify_transport()?;
    }
    let node = if let Some(raw) = std::env::var_os("GROK_CU_NODE_FILE") {
        let node = PathBuf::from(raw);
        if !node.is_absolute() || !node.is_file() {
            return Err(format!(
                "GROK_CU_NODE_FILE must name an absolute Node executable: {}",
                node.display()
            ));
        }
        node
    } else {
        packaged_seed_node()?
    };
    println!("gate: managed_node={}", node.display());
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let pack_worker = manifest
        .join("resources")
        .join("computer-use")
        .join("seed")
        .join("playwright")
        .join("worker.mjs");
    let source_worker = manifest
        .join("..")
        .join("tools")
        .join("computer-use-browser")
        .join("server.mjs");
    if check_preview && !source_worker_only && !pack_worker.is_file() {
        return Err("packaged preview requires the generated worker; no source fallback".into());
    }
    let script = if !source_worker_only && pack_worker.is_file() {
        pack_worker
    } else {
        source_worker
    };
    if !script.is_file() {
        return Err(format!("missing {}", script.display()));
    }
    println!("gate: managed_worker={}", script.display());
    let chrome = manifest
        .join("resources")
        .join("computer-use")
        .join("seed")
        .join("chromium")
        .join("chrome-win")
        .join("chrome.exe");
    if !chrome.is_file() {
        return Err(format!("pack Chromium missing: {}", chrome.display()));
    }
    println!("gate: managed_chrome={}", chrome.display());
    let form = manifest
        .join("..")
        .join("tools")
        .join("computer-use-browser")
        .join("fixtures")
        .join("form.html");
    let html = std::fs::read(&form).map_err(|e| e.to_string())?;
    let frame = std::fs::read(form.with_file_name("frame.html")).map_err(|e| e.to_string())?;
    let profile_root = std::env::temp_dir().join(format!(
        "grok-cu-browser-managed-contract-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    let supervisor = BrowserSupervisor::spawn(SpawnRequest {
        node,
        script,
        profile_root: profile_root.clone(),
        browser: Some(chrome),
    })?;
    let pid = supervisor.pid();
    let worker = Arc::new(LoopbackPlaywrightWorker::new(
        supervisor.base_url(),
        supervisor.token(),
    ));

    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let page_port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let oracle = Arc::new(AtomicU64::new(0));
    let (stop, page_thread) = fixture::spawn(listener, html, frame, oracle.clone())?;

    let result: Result<(), String> = (|| {
        if check_preview {
            lifecycle_contract::verify_worker(worker.as_ref())?;
            profile_contract::verify_stop_during_open(worker.clone(), &profile_root)?;
            verify_fragmented_oracle(page_port, &oracle)?;
        }
        let opened = worker
            .open_profile("managed-contract", "contract-profile")
            .map_err(|e| e.to_string())?;
        let page_url = format!("http://127.0.0.1:{page_port}/?run=managed-contract");
        let landed = worker
            .goto(
                "managed-contract",
                "contract-profile",
                &opened.page.page_id,
                opened.page.page_generation,
                "contract-goto",
                &page_url,
            )
            .map_err(|e| e.to_string())?;
        let mut generation = landed.page_generation;
        let mut observed = None;
        for _ in 0..6 {
            match worker.observe_page(
                "managed-contract",
                "contract-profile",
                &landed.page_id,
                generation,
            ) {
                Ok(obs) => {
                    observed = Some(obs);
                    break;
                }
                Err(error)
                    if error.code == "observation_changed_during_capture"
                        || error.code == "stale_observation_generation" =>
                {
                    if let Some(current) = error.current_page_generation {
                        generation = current;
                    }
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        let observed = observed.ok_or_else(|| "observe did not settle".to_string())?;
        if observed.snapshot_id.trim().is_empty() {
            return Err("observe missing snapshotId".to_string());
        }
        let png = observed
            .png_base64
            .as_deref()
            .ok_or_else(|| "observe missing PNG".to_string())?;
        if !png.starts_with("iVBOR") {
            return Err("PNG signature missing".to_string());
        }
        let plus = observed
            .nodes
            .iter()
            .find(|node| node.name.contains("+1") && !node.truncated)
            .ok_or_else(|| format!("missing +1 in {:?}", observed.nodes))?;
        let acted = worker
            .act_page(ManagedWorkerAction {
                owner: "managed-contract",
                profile: "contract-profile",
                page: grok_computer_use_core::browser::ManagedPageRef {
                    page_id: &observed.page_id,
                    page_generation: observed.page_generation,
                },
                snapshot_id: &observed.snapshot_id,
                action_id: "contract-click",
                kind: "click",
                locator: &ManagedLocator {
                    element_ref: Some(plus.element_ref.clone()),
                },
                params: &serde_json::json!({}),
            })
            .map_err(|e| e.to_string())?;
        let mut verify_generation = acted.page_generation.max(observed.page_generation);
        let mut verified = None;
        for _ in 0..6 {
            match worker.observe_page(
                "managed-contract",
                "contract-profile",
                &acted.page_id,
                verify_generation,
            ) {
                Ok(obs) => {
                    verified = Some(obs);
                    break;
                }
                Err(error)
                    if error.code == "observation_changed_during_capture"
                        || error.code == "stale_observation_generation" =>
                {
                    if let Some(current) = error.current_page_generation {
                        verify_generation = current;
                    }
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        let verified = verified.ok_or_else(|| "verify observe did not settle".to_string())?;
        if verified.snapshot_id == observed.snapshot_id {
            return Err("verify observe must mint a new snapshot".to_string());
        }
        if verified.page_id != observed.page_id {
            return Err("pageId changed across opaque-ref act".to_string());
        }
        let mut oracle_count = 0u64;
        for _ in 0..20 {
            oracle_count = oracle.load(Ordering::SeqCst);
            if oracle_count == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        println!("gate: independent_oracle_count={oracle_count}");
        if oracle_count != 1 {
            return Err(format!(
                "independent oracle expected count=1, got {oracle_count}"
            ));
        }
        if check_preview {
            verify_preview_broker(worker.clone(), &profile_root, &oracle)?;
            native_contract::verify(worker.clone(), &profile_root)?;
            resume_fault_contract::verify(
                worker.clone(),
                supervisor.base_url(),
                supervisor.token(),
                &profile_root,
            )?;
        }
        Ok(())
    })();

    let _ = stop.send(());
    let shutdown = supervisor.shutdown();
    let fixture_result = page_thread
        .join()
        .map_err(|_| "fixture HTTP thread panicked".to_string())
        .and_then(|result| result);
    // The supervisor's original child/Job is the lifetime authority. Reopening
    // its diagnostic PID after exit could instead inspect a reused process ID.
    cleanup::finish_probe(
        result,
        shutdown,
        fixture_result,
        || match std::fs::remove_dir_all(&profile_root) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        },
    )?;
    println!(
        "gate: browser_managed_contract pid={pid} open-observe-act-verify-png-oracle-shutdown"
    );
    Ok(())
}

fn verify_fragmented_oracle(port: u16, oracle: &AtomicU64) -> Result<(), String> {
    let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|e| e.to_string())?;
    let body = br#"{"count":7}"#;
    let headers = format!("POST /oracle HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream
        .write_all(headers.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut byte = [0_u8; 1];
    match stream.read(&mut byte) {
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) => {}
        _ => {
            return Err("fixture oracle acknowledged HTTP before receiving the request body".into())
        }
    }
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    if !response.starts_with("HTTP/1.1 204") || oracle.load(Ordering::SeqCst) != 7 {
        return Err("fixture oracle did not parse the fragmented HTTP body".into());
    }
    oracle.store(0, Ordering::SeqCst);
    println!("gate: fixture_oracle fragmented-http=passed");
    Ok(())
}

fn verify_preview_broker(
    worker: Arc<LoopbackPlaywrightWorker>,
    root: &std::path::Path,
    oracle: &AtomicU64,
) -> Result<(), String> {
    use super::{ActionKind, ActionRequest, BrokerOptions, ComputerUseBroker, SurfaceKind};
    use grok_computer_use_core::protocol::{ActionTarget, OutcomeKind, PROTOCOL_VERSION};
    let broker = ComputerUseBroker::new(
        super::platform_adapter(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: root.join("fixture-desktop.lease"),
            ..Default::default()
        },
    );
    broker.tabs().set_profile_root(root.to_path_buf());
    broker.tabs().set_staging_root(root.join(".staging"));
    broker.tabs().set_worker(worker.clone());
    broker
        .register_managed_browser_adapter()
        .map_err(|e| e.to_string())?;
    broker
        .open_run("preview-contract-session", "managed-contract")
        .map_err(|e| e.to_string())?;
    broker
        .authorize_on_surface(
            "preview-contract-session",
            "managed-contract",
            SurfaceKind::ManagedBrowser,
            "managed-profile:contract-profile",
        )
        .map_err(|e| e.to_string())?;
    let checked = (|| {
        let model = broker
            .observe_with_screenshot("managed-contract", false)
            .map_err(|e| e.to_string())?;
        let preview = broker
            .observe_preview("managed-contract")
            .map_err(|e| e.to_string())?;
        if model.image.png_base64.is_some()
            || preview.image.png_base64.is_none()
            || model.snapshot_id == preview.snapshot_id
            || broker
                .act_defaults("managed-contract")
                .map_err(|e| e.to_string())?
                .2
                != model.snapshot_id
        {
            return Err("managed preview/model separation failed".into());
        }
        let plus = model
            .nodes
            .iter()
            .find(|node| node.name.contains("+1") && !node.truncated)
            .ok_or("managed model fixture button missing")?;
        let waited = broker.act(ActionRequest {
            version: PROTOCOL_VERSION,
            action_id: "model-wait-original-ref".into(),
            run_id: model.run_id.clone(),
            target_id: model.target_id.clone(),
            target_generation: model.target_generation,
            snapshot_id: model.snapshot_id.clone(),
            geometry_revision: model.geometry_revision,
            action: ActionKind::Wait,
            target: ActionTarget::Element {
                element_ref: plus.node_ref.clone(),
            },
            parameters: serde_json::json!({"nameEquals": plus.name, "timeoutMs": 250}),
        });
        if waited.kind != OutcomeKind::Verified || !waited.executed {
            return Err(format!(
                "managed wait must match the original observed element: {:?}",
                waited.reason
            ));
        }
        if broker.model_snapshot_id(&model.run_id).as_deref() != Some(model.snapshot_id.as_str())
            || oracle.load(Ordering::SeqCst) != 1
        {
            return Err("read-only wait changed the model snapshot or clicked the fixture".into());
        }
        println!(
            "gate: managed_wait original-ref=matched snapshot=preserved independent_oracle_count=1"
        );
        let alias = grok_computer_use_core::tools::dispatch(
            &broker,
            &grok_computer_use_core::ipc::SessionBinding {
                session_id: "preview-contract-session".into(),
                run_id: model.run_id.clone(),
            },
            "computer_wait",
            serde_json::json!({
                "version":PROTOCOL_VERSION,"actionId":"model-wait-tool-alias",
                "runId":model.run_id,"targetId":model.target_id,"targetGeneration":model.target_generation,
                "snapshotId":model.snapshot_id,"geometryRevision":model.geometry_revision,
                "elementRef":plus.node_ref,"nameEquals":plus.name,"timeoutMs":250,
            }),
        );
        if alias["isError"] != serde_json::json!(false) || oracle.load(Ordering::SeqCst) != 1 {
            return Err(
                "computer_wait alias did not verify the same observed element without input".into(),
            );
        }
        println!("gate: managed_wait model-tool-alias=matched independent_oracle_count=1");
        let action = ActionRequest {
            version: PROTOCOL_VERSION,
            action_id: "model-click-after-app-preview".into(),
            run_id: model.run_id,
            target_id: model.target_id,
            target_generation: model.target_generation,
            snapshot_id: model.snapshot_id,
            geometry_revision: model.geometry_revision,
            action: ActionKind::Click,
            target: ActionTarget::Element {
                element_ref: plus.node_ref.clone(),
            },
            parameters: serde_json::json!({}),
        };
        let result = broker.act(action);
        if !result.executed || result.kind != OutcomeKind::Applied {
            return Err(format!(
                "managed action after preview failed: {:?}",
                result.kind
            ));
        }
        let until = std::time::Instant::now() + Duration::from_secs(2);
        while oracle.load(Ordering::SeqCst) != 2 && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        if oracle.load(Ordering::SeqCst) != 2 {
            return Err("managed preview fixture expected exactly two total clicks".into());
        }
        println!("gate: managed_preview model=text-only preview=png old-model-ref=applied independent_oracle_count=2");
        lifecycle_contract::verify_broker_resume(&broker, worker.as_ref())?;
        lifecycle_contract::verify_broker_pending_wait(&broker, worker.as_ref())?;
        if oracle.load(Ordering::SeqCst) != 2 {
            return Err("resume or reauthorization replayed browser input".into());
        }
        Ok(())
    })();
    let stop_started = std::time::Instant::now();
    let stopped = broker
        .request_stop("managed-contract")
        .map_err(|e| e.to_string());
    println!(
        "gate: managed_preview_stop completed={} elapsed_ms={}",
        stopped.is_ok(),
        stop_started.elapsed().as_millis()
    );
    if stopped.is_err() && checked.is_ok() {
        // The checked pending-Wait scenario has independently confirmed 2->3.
        // Read the same worker once; never replay Stop or a business action.
        let revision = grok_computer_use_core::browser::WorkerRunRevision::new(3)
            .map_err(|e| e.to_string())?;
        match worker.run_status("managed-contract", revision) {
            Ok(state) => println!(
                "gate: managed_preview_stop_readback revision={} phase={:?} active_operations={} idle={}",
                state.revision.get(), state.phase, state.active_operations, state.is_idle()
            ),
            Err(error) => println!(
                "gate: managed_preview_stop_readback unavailable code={}",
                error.code
            ),
        }
    }
    if let (Err(check), Err(stop)) = (&checked, &stopped) {
        return Err(format!("{check}; preview Stop also failed: {stop}"));
    }
    checked?;
    stopped?;
    Ok(())
}

fn verify_legacy_observation_guard() -> Result<(), String> {
    use grok_computer_use_core::adapter::CaptureOptions;
    for options in [
        Some(CaptureOptions::model(false)),
        Some(CaptureOptions {
            managed_request: None,
            for_model: false,
            screenshot: true,
            cancellation: Default::default(),
        }),
        None,
    ] {
        let expected_code = if options.is_some() {
            "observation_options_unavailable"
        } else {
            "wait_condition_unavailable"
        };
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let endpoint = format!(
            "http://{}",
            listener.local_addr().map_err(|e| e.to_string())?
        );
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let server = std::thread::spawn(move || -> Result<(), String> {
            let until = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                if std::time::Instant::now() >= until {
                    return Err("legacy guard fixture timed out".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .map_err(|e| e.to_string())?;
            let mut buf = [0; 4096];
            let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
            if !buf[..n].starts_with(b"POST /health ") {
                return Err("legacy worker received a request before capability check".into());
            }
            let body = r#"{"ok":true,"page":{"observe":true}}"#;
            let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream
                .write_all(reply.as_bytes())
                .map_err(|e| e.to_string())
        });
        let worker = LoopbackPlaywrightWorker::new(endpoint, "legacy-fixture-only");
        let result = if let Some(options) = options {
            worker
                .capture_page(
                    "fixture-owner",
                    "fixture-profile",
                    "fixture-page",
                    1,
                    options,
                )
                .map(|_| ())
                .map_err(|error| error.code)
        } else {
            worker
                .act_page(ManagedWorkerAction {
                    owner: "fixture-owner",
                    profile: "fixture-profile",
                    page: grok_computer_use_core::browser::ManagedPageRef {
                        page_id: "fixture-page",
                        page_generation: 1,
                    },
                    snapshot_id: "fixture-snapshot",
                    action_id: "legacy-wait",
                    kind: "wait",
                    locator: &ManagedLocator {
                        element_ref: Some("fixture-ref".into()),
                    },
                    params: &serde_json::json!({"nameEquals":"Ready","timeoutMs":80}),
                })
                .map(|_| ())
                .map_err(|error| error.code)
        };
        server
            .join()
            .map_err(|_| "legacy guard fixture panicked")??;
        if !matches!(result, Err(ref code) if code == expected_code) {
            return Err(
                "legacy worker must reject unsupported observation options before dispatch".into(),
            );
        }
    }
    println!("gate: legacy_worker observation-options-and-wait=rejected-before-dispatch checks=3");
    Ok(())
}
