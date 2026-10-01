//! Real worker acknowledgement-loss and superseding-control races at Broker.
use crate::computer_use::playwright_worker::LoopbackPlaywrightWorker;
use grok_computer_use_core::{
    adapter::SurfaceKind,
    broker::{BrokerOptions, ComputerUseBroker, StopState},
    browser::{ManagedBrowserWorker, WorkerCompletion, WorkerRunPhase, WorkerRunRevision},
    error::BrokerError,
};
use std::{
    path::Path,
    sync::{atomic::Ordering, mpsc, Arc},
    time::{Duration, Instant},
};

#[path = "browser_resume_relay.rs"]
mod relay;

pub fn verify(
    worker: Arc<LoopbackPlaywrightWorker>,
    upstream: &str,
    token: &str,
    root: &Path,
) -> Result<(), String> {
    for case in ["lost", "pause", "stop"] {
        let mut relay = relay::Relay::start(upstream, token, case == "lost")?;
        let run = format!("resume-wire-{case}");
        let broker = ComputerUseBroker::new(
            crate::computer_use::platform_adapter(),
            BrokerOptions {
                feature_enabled: true,
                lease_path: root.join(format!("{run}.lease")),
                ..Default::default()
            },
        );
        broker.tabs().set_profile_root(root.to_path_buf());
        broker
            .tabs()
            .set_worker(Arc::new(LoopbackPlaywrightWorker::new(&relay.base, token)));
        let checked = (|| -> Result<(), String> {
            broker
                .register_managed_browser_adapter()
                .map_err(|e| e.to_string())?;
            broker
                .open_run("resume-wire", &run)
                .map_err(|e| e.to_string())?;
            broker
                .authorize_on_surface(
                    "resume-wire",
                    &run,
                    SurfaceKind::ManagedBrowser,
                    &format!("managed-profile:{run}"),
                )
                .map_err(|e| e.to_string())?;
            let (target, _) = broker.authorized_target(&run).map_err(|e| e.to_string())?;
            broker
                .observe_with_screenshot(&run, false)
                .map_err(|e| e.to_string())?;
            broker.pause(&run).map_err(|e| e.to_string())?;
            std::thread::scope(|scope| -> Result<(), String> {
                let (sent, finished) = mpsc::channel();
                let resumed_broker = &broker;
                let resumed_run = &run;
                scope.spawn(move || {
                    let _ = sent.send(resumed_broker.resume(resumed_run));
                });
                let held = (|| -> Result<(), String> {
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while !relay.committed.load(Ordering::SeqCst) {
                        if Instant::now() >= deadline {
                            return Err("real resume never reached response gate".into());
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    let revision = WorkerRunRevision::new(2).map_err(|e| e.to_string())?;
                    let remote = worker
                        .run_status(&run, revision)
                        .map_err(|e| e.to_string())?;
                    if remote.phase != WorkerRunPhase::Running || !remote.is_idle() {
                        return Err("relay did not hold an actually committed resume".into());
                    }
                    if finished.try_recv().is_ok() {
                        return Err("resume completed before its reply was released".into());
                    }
                    if case == "pause" {
                        // This Pause must locally fence, even though its remote
                        // revision is uncertain until the held reply arrives.
                        if broker.pause(&run).is_ok() {
                            return Err("superseding Pause hid an uncertain resume".into());
                        }
                    } else if case == "stop" {
                        let state = broker.request_stop(&run).map_err(|e| e.to_string())?;
                        if state != StopState::StopRequested {
                            return Err(
                                "Stop acknowledged while local resume admission remained active"
                                    .into(),
                            );
                        }
                        let remote = worker
                            .run_status(&run, revision)
                            .map_err(|e| e.to_string())?;
                        if remote.phase != WorkerRunPhase::Stopped || !remote.is_idle() {
                            return Err("Stop failed to clean the resumed browser with its reply still held".into());
                        }
                    }
                    if broker.observe_with_screenshot(&run, false).is_ok()
                        || broker.authorize_target(&run, &target).is_ok()
                        || broker.resume(&run).is_ok()
                    {
                        return Err(
                            "unacknowledged resume granted authority or permitted replay".into(),
                        );
                    }
                    Ok(())
                })();
                // Always release before scoped thread join, including failures.
                relay.release();
                let resumed = finished.recv_timeout(Duration::from_secs(3));
                held?;
                let resumed = resumed.map_err(|_| "resume did not settle after relay release")?;
                let Err(error) = resumed else {
                    return Err("lost or superseded resume reported success".into());
                };
                if case == "lost"
                    && !matches!(error, BrokerError::BrowserWorker(ref error)
                    if error.completion == WorkerCompletion::Unknown)
                {
                    return Err("lost HTTP resume reply did not retain unknown completion".into());
                }
                Ok(())
            })?;
            if !broker.is_paused(&run).map_err(|e| e.to_string())?
                || broker.model_snapshot_id(&run).is_some()
                || broker.resume(&run).is_ok()
                || broker.observe_with_screenshot(&run, false).is_ok()
            {
                return Err("late resume reply restored local authority".into());
            }
            if relay.calls.load(Ordering::SeqCst) != 1 {
                return Err("resume was replayed after an uncertain result".into());
            }
            if broker.request_stop(&run).map_err(|e| e.to_string())? != StopState::Stopped {
                return Err("uncertain resume could not reach terminal Stop".into());
            }
            let remote = worker
                .run_status(&run, WorkerRunRevision::new(2).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            if remote.phase != WorkerRunPhase::Stopped
                || !remote.is_idle()
                || broker.tabs().managed_target_info(&target).is_some()
            {
                return Err("uncertain resume cleanup retained browser authority".into());
            }
            // Another owner can reopen the same persistent profile only after
            // the real browser has released it. This also detects a leaked lock.
            let next_owner = format!("{run}-new");
            worker
                .open_profile(&next_owner, &run)
                .map_err(|e| e.to_string())?;
            worker.cancel_run(&next_owner).map_err(|e| e.to_string())?;
            Ok(())
        })();
        relay.release();
        let cleanup = broker.request_stop(&run).map_err(|e| e.to_string());
        let closed = relay.close();
        checked.map_err(|e| format!("resume {case}: {e}"))?;
        cleanup?;
        closed?;
        println!("gate: broker_resume_wire_{case} real-transition/late-reply-fenced/no-replay/terminal-cleanup/profile-reused=passed");
    }
    Ok(())
}
