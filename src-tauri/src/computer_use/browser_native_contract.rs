//! Actual App Broker/worker/native-operation control gates; no product debug hook.
use crate::computer_use::playwright_worker::LoopbackPlaywrightWorker;
use grok_computer_use_core::{
    adapter::SurfaceKind,
    broker::{BrokerOptions, ComputerUseBroker, StopState},
    browser::{ManagedBrowserWorker, WorkerRunPhase, WorkerRunRevision},
    protocol::{ActionKind, ActionRequest, ActionTarget, OutcomeKind, PROTOCOL_VERSION},
};
use std::{
    path::Path,
    sync::{atomic::Ordering, mpsc, Arc},
    time::{Duration, Instant},
};
#[path = "browser_native_fixture.rs"]
mod fixture;

fn wait_for(condition: impl FnMut() -> Result<bool, String>, message: &str) -> Result<(), String> {
    wait_for_with_timeout(condition, Duration::from_secs(3), message)
}

fn wait_for_with_timeout(
    mut condition: impl FnMut() -> Result<bool, String>,
    timeout: Duration,
    message: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while !condition()? {
        if Instant::now() >= deadline {
            return Err(message.into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

pub fn verify(worker: Arc<LoopbackPlaywrightWorker>, root: &Path) -> Result<(), String> {
    verify_control(worker.clone(), root, false)?;
    verify_control(worker, root, true)
}

fn verify_control(
    worker: Arc<LoopbackPlaywrightWorker>,
    root: &Path,
    terminal_stop: bool,
) -> Result<(), String> {
    let mut fixture = fixture::Fixture::start()?;
    let mut errors = Vec::new();
    let control = if terminal_stop { "stop" } else { "pause" };
    let other = format!("native-{control}-other");
    let other_page = worker
        .open_profile(&other, &other)
        .map_err(|e| e.to_string())?
        .page;
    for case in ["download", "download-pending", "click", "screenshot"] {
        let gate = match case {
            "download" => &fixture.download,
            "download-pending" => &fixture.pending_download,
            "click" => &fixture.click,
            _ => &fixture.screenshot,
        };
        let run = format!("native-{control}-{case}");
        let broker = ComputerUseBroker::new(
            crate::computer_use::platform_adapter(),
            BrokerOptions {
                feature_enabled: true,
                lease_path: root.join(format!("{run}.lease")),
                ..Default::default()
            },
        );
        broker.tabs().set_profile_root(root.to_owned());
        broker.tabs().set_staging_root(root.join(".staging"));
        broker.tabs().set_worker(worker.clone());
        let checked = (|| -> Result<(), String> {
            broker
                .register_managed_browser_adapter()
                .map_err(|e| e.to_string())?;
            broker
                .open_run("native-contract", &run)
                .map_err(|e| e.to_string())?;
            broker
                .authorize_on_surface(
                    "native-contract",
                    &run,
                    SurfaceKind::ManagedBrowser,
                    &format!("managed-profile:{run}"),
                )
                .map_err(|e| e.to_string())?;
            let (target, _) = broker.authorized_target(&run).map_err(|e| e.to_string())?;
            let page_generation = broker
                .tabs()
                .managed_target_info(&target)
                .ok_or("fixture target missing")?
                .document_generation;
            let landed = grok_computer_use_core::tools::dispatch(
                &broker,
                &grok_computer_use_core::ipc::SessionBinding {
                    session_id: "native-contract".into(),
                    run_id: run.clone(),
                },
                "computer_navigate",
                serde_json::json!({"tabId":target,"pageGeneration":page_generation,"url":format!("{}/{case}",fixture.base),"actionId":"land-native"}),
            );
            if landed["isError"] != false {
                return Err(format!("native fixture navigation failed: {landed}"));
            }
            let observation = broker
                .observe_with_screenshot(&run, false)
                .map_err(|e| e.to_string())?;
            let node = observation
                .nodes
                .first()
                .ok_or("native fixture target missing")?;
            std::thread::scope(|scope| -> Result<(), String> {
                let (sent, result) = mpsc::channel();
                let broker_ref = &broker;
                let run_ref = run.as_str();
                let observed = &observation;
                scope.spawn(move || {
                    let value = match case {
                        "screenshot" => broker_ref.observe_preview(run_ref).map(|_| ()).map_err(|e| e.to_string()),
                        "click" => {
                            let result = broker_ref.act(ActionRequest { version: PROTOCOL_VERSION, action_id: "pending-native".into(), run_id: run_ref.into(), target_id: observed.target_id.clone(), target_generation: observed.target_generation, snapshot_id: observed.snapshot_id.clone(), geometry_revision: observed.geometry_revision, action: ActionKind::Click, target: ActionTarget::Element { element_ref: node.node_ref.clone() }, parameters: serde_json::json!({}) });
                            if matches!(result.kind, OutcomeKind::Applied | OutcomeKind::Verified) { Ok(()) } else { Err(result.reason.unwrap_or_else(|| "native input cancelled".into())) }
                        },
                        _ => {
                            let result = grok_computer_use_core::tools::dispatch(broker_ref, &grok_computer_use_core::ipc::SessionBinding { session_id: "native-contract".into(), run_id: run_ref.into() }, "computer_download", serde_json::json!({"tabId":observed.target_id,"pageGeneration":observed.geometry_revision,"snapshotId":observed.snapshot_id,"elementRef":node.node_ref,"filename":"native.bin","actionId":"pending-native"}));
                            if result["isError"] == false { Ok(()) } else { Err("native download cancelled".into()) }
                        }
                    };
                    let _ = sent.send(value);
                });
                // Always release the HTTP gate before joining, including an
                // assertion failure; a fixture must not deadlock its own cleanup.
                let check = (|| -> Result<(), String> {
                    wait_for(
                        || Ok(gate.entered.load(Ordering::SeqCst)),
                        "native fixture request never arrived",
                    )?;
                    wait_for(
                        || {
                            worker
                                .run_status(&run, WorkerRunRevision::INITIAL)
                                .map(|s| s.active_operations == 1)
                                .map_err(|e| e.to_string())
                        },
                        "worker request did not enter",
                    )?;
                    if case == "screenshot" {
                        std::thread::sleep(Duration::from_millis(250));
                    }
                    if result.try_recv().is_ok() {
                        return Err(
                            "native operation completed before the held fixture was released"
                                .into(),
                        );
                    }
                    let stop_ticket = if terminal_stop {
                        let ticket = broker
                            .fence_stop(&run)
                            .map_err(|e| e.to_string())?
                            .ok_or("native Stop lost its cleanup ticket")?;
                        if broker.stop_state(&run).map_err(|e| e.to_string())?
                            != StopState::StopRequested
                        {
                            return Err(
                                "local fence acknowledged Stop before remote cleanup".into()
                            );
                        }
                        Some(ticket)
                    } else {
                        broker.pause(&run).map_err(|e| e.to_string())?;
                        None
                    };
                    if result
                        .recv_timeout(Duration::from_secs(2))
                        .map_err(|_| "local native request did not cancel")?
                        .is_ok()
                    {
                        return Err("cancelled native request reported success".into());
                    }
                    if let Some(ticket) = stop_ticket {
                        let cleanup_started = Instant::now();
                        let stopped = broker
                            .finish_stop_cleanup(&ticket)
                            .map_err(|e| e.to_string())?;
                        let remote = worker
                            .run_status(&run, WorkerRunRevision::INITIAL)
                            .map_err(|e| e.to_string())?;
                        if stopped != StopState::Stopped
                            || remote.phase != WorkerRunPhase::Stopped
                            || !remote.is_idle()
                        {
                            return Err(
                                "native Stop failed to reach local and remote terminal idle".into(),
                            );
                        }
                        println!(
                            "gate: broker_native_stop_{case} cleanup_ms={}",
                            cleanup_started.elapsed().as_millis()
                        );
                        if case.starts_with("download") {
                            wait_for(
                                || Ok(gate.finished.load(Ordering::SeqCst)),
                                "Stop left native download transfer alive",
                            )?;
                        }
                        if broker.tabs().managed_target_info(&target).is_some()
                            || broker.resume(&run).is_ok()
                            || broker.authorize_target(&run, &target).is_ok()
                            || broker.observe_with_screenshot(&run, false).is_ok()
                        {
                            return Err("terminal Stop retained target or allowed recovery".into());
                        }
                    } else if case == "download" {
                        wait_for(
                            || {
                                worker
                                    .run_status(&run, WorkerRunRevision::INITIAL)
                                    .map(|s| s.is_idle())
                                    .map_err(|e| e.to_string())
                            },
                            "cancelled native download remained active while body was held",
                        )?;
                        wait_for(
                            || Ok(gate.finished.load(Ordering::SeqCst)),
                            "native download left HTTP transfer alive",
                        )?;
                    } else {
                        let state = worker
                            .run_status(&run, WorkerRunRevision::INITIAL)
                            .map_err(|e| e.to_string())?;
                        if state.is_idle() || state.phase != WorkerRunPhase::Paused {
                            return Err("unsettled native operation was reported idle".into());
                        }
                        if broker.resume(&run).is_ok() {
                            return Err(
                                "resume passed while native operation remained pending".into()
                            );
                        }
                        if case == "download-pending" {
                            // The four-second Download-event deadline must
                            // close an unidentified transfer's owned context.
                            // Keep the response held through cleanup: returning
                            // idle while the HTTP body is alive is a failure.
                            wait_for_with_timeout(
                                || {
                                    worker
                                        .run_status(&run, WorkerRunRevision::INITIAL)
                                        .map(|s| s.is_idle())
                                        .map_err(|e| e.to_string())
                                },
                                Duration::from_secs(6),
                                "unidentified download cleanup did not settle",
                            )?;
                            wait_for(
                                || Ok(gate.finished.load(Ordering::SeqCst)),
                                "unidentified download reported idle with an active transfer",
                            )?;
                        }
                    }
                    Ok(())
                })();
                gate.release();
                check?;
                Ok(())
            })?;
            wait_for(
                || {
                    worker
                        .run_status(&run, WorkerRunRevision::INITIAL)
                        .map(|s| s.is_idle())
                        .map_err(|e| e.to_string())
                },
                "native operation failed to settle after release",
            )?;
            if terminal_stop {
                if broker.stop_state(&run).map_err(|e| e.to_string())? != StopState::Stopped {
                    return Err("late native completion changed terminal Stop".into());
                }
            } else if case == "download-pending" {
                broker.resume(&run).map_err(|e| e.to_string())?;
                if broker.authorize_target(&run, &target).is_ok()
                    || broker.observe_with_screenshot(&run, false).is_ok()
                {
                    return Err("closed download context regained authority".into());
                }
            } else {
                broker.resume(&run).map_err(|e| e.to_string())?;
                broker
                    .authorize_target(&run, &target)
                    .map_err(|e| e.to_string())?;
                broker
                    .observe_with_screenshot(&run, false)
                    .map_err(|e| e.to_string())?;
            }
            if case.starts_with("download") {
                let staging = root.join(".staging/computer-use-staging").join(&run);
                if staging.join("native.bin").exists() {
                    return Err("cancelled download published a final file".into());
                }
                for entry in std::fs::read_dir(&staging).map_err(|e| e.to_string())? {
                    let entry = entry.map_err(|e| e.to_string())?;
                    if entry.path().extension().is_some_and(|ext| ext == "part") {
                        return Err("cancelled download left a partial file".into());
                    }
                }
            }
            if gate.hits.load(Ordering::SeqCst) != 1 {
                return Err("native request was replayed".into());
            }
            let other_pages = worker
                .list_pages(&other, &other)
                .map_err(|e| e.to_string())?;
            if !other_pages
                .iter()
                .any(|page| page.page_id == other_page.page_id)
            {
                return Err("native control closed another owner's context".into());
            }
            Ok(())
        })();
        gate.release();
        let stopped = broker
            .request_stop(&run)
            .map_err(|e| e.to_string())
            .and_then(|state| {
                if state == StopState::Stopped {
                    Ok(())
                } else {
                    Err("native fixture cleanup remained pending".into())
                }
            });
        match checked.and(stopped) {
            Ok(()) if terminal_stop => println!("gate: broker_native_stop_{case} pending-operation/terminal-idle/authority-revoked/other-owner-alive/no-replay=passed"),
            Ok(()) if case == "download-pending" => println!("gate: broker_native_{case} local-cancel/bounded-context-close/no-replay/old-target-rejected=passed"),
            Ok(()) => println!("gate: broker_native_{case} local-cancel/remote-quiescence/no-replay/recovery=passed"),
            Err(error) => { println!("gate: broker_native_{case} FAIL {error}"); errors.push(format!("{case}: {error}")); }
        }
    }
    let other_cleanup = worker.cancel_run(&other).map_err(|e| e.to_string());
    fixture.close()?;
    other_cleanup?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
