//! Lifecycle client acceptance used against both source and packaged workers.
//! Separate owners keep this protocol check out of the preview/action oracle.
use grok_computer_use_core::browser::{
    ManagedBrowserWorker, WorkerCompletion, WorkerRunPhase, WorkerRunRevision,
};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use super::LoopbackPlaywrightWorker;

/// App Broker recovery independently checks the actual worker revision/phase.
pub fn verify_broker_resume(
    broker: &grok_computer_use_core::broker::ComputerUseBroker,
    worker: &LoopbackPlaywrightWorker,
) -> Result<(), String> {
    let run = "managed-contract";
    let (target, generation) = broker.authorized_target(run).map_err(|e| e.to_string())?;
    broker.pause(run).map_err(|e| e.to_string())?;
    let paused = worker
        .run_status(run, WorkerRunRevision::INITIAL)
        .map_err(|e| e.to_string())?;
    if paused.phase != WorkerRunPhase::Paused || !paused.is_idle() {
        return Err("Broker pause did not pause the actual worker".into());
    }
    broker.resume(run).map_err(|e| e.to_string())?;
    let next_revision = WorkerRunRevision::INITIAL
        .successor()
        .map_err(|e| e.to_string())?;
    let resumed = worker
        .run_status(run, next_revision)
        .map_err(|e| e.to_string())?;
    if resumed.phase != WorkerRunPhase::Running || !resumed.is_idle() {
        return Err("Broker resume did not advance the actual worker".into());
    }
    if broker.has_authorized_target(run)
        || broker.model_snapshot_id(run).is_some()
        || broker.observe_with_screenshot(run, false).is_ok()
    {
        return Err("resume retained browser authority or a model snapshot".into());
    }
    let next = broker
        .authorize_target(run, &target)
        .map_err(|e| e.to_string())?;
    let observed = broker
        .observe_with_screenshot(run, false)
        .map_err(|e| e.to_string())?;
    if next <= generation || observed.target_generation != next || observed.target_id != target {
        return Err("browser reauthorization did not publish a fresh target identity".into());
    }
    println!(
        "gate: broker_resume real-managed-page old-grant=revoked reauthorization=fresh no-input"
    );
    Ok(())
}

/// Exercise cancellation during a real worker request, with worker status as
/// the admission oracle. This is a DOM Wait, not pending native click coverage.
pub fn verify_broker_pending_wait(
    broker: &grok_computer_use_core::broker::ComputerUseBroker,
    worker: &LoopbackPlaywrightWorker,
) -> Result<(), String> {
    use grok_computer_use_core::protocol::{
        ActionKind, ActionRequest, ActionTarget, OutcomeKind, PROTOCOL_VERSION,
    };
    let run = "managed-contract";
    let revision = WorkerRunRevision::new(2).map_err(|e| e.to_string())?;
    let observed = broker
        .observe_with_screenshot(run, false)
        .map_err(|e| e.to_string())?;
    let node = observed
        .nodes
        .iter()
        .find(|n| n.actions.iter().any(|a| a == "wait"))
        .ok_or("fixture has no waitable node")?;
    let action = ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "wait-cancelled-by-app-pause".into(),
        run_id: run.into(),
        target_id: observed.target_id.clone(),
        target_generation: observed.target_generation,
        snapshot_id: observed.snapshot_id.clone(),
        geometry_revision: observed.geometry_revision,
        action: ActionKind::Wait,
        target: ActionTarget::Element {
            element_ref: node.node_ref.clone(),
        },
        parameters: json!({"nameEquals":"this fixture name never matches","timeoutMs":10000}),
    };
    std::thread::scope(|scope| -> Result<(), String> {
        let (finished, result) = std::sync::mpsc::channel();
        let pending = action.clone();
        scope.spawn(move || {
            let _ = finished.send(broker.act(pending));
        });
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            let state = worker
                .run_status(run, revision)
                .map_err(|e| e.to_string())?;
            if state.active_operations == 1 {
                break;
            }
            if Instant::now() >= until {
                return Err("real Wait never became active at worker".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let started = Instant::now();
        broker.pause(run).map_err(|e| e.to_string())?;
        let outcome = result
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "paused Wait did not cancel its local HTTP request")?;
        if started.elapsed() > Duration::from_secs(3)
            || matches!(outcome.kind, OutcomeKind::Applied | OutcomeKind::Verified)
        {
            return Err("pending Wait was not promptly cancelled".into());
        }
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            let state = worker
                .run_status(run, revision)
                .map_err(|e| e.to_string())?;
            if state.phase != WorkerRunPhase::Paused {
                return Err("worker escaped paused state".into());
            }
            if state.is_idle() {
                break;
            }
            if Instant::now() >= until {
                return Err("worker Wait did not settle physically".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        broker.resume(run).map_err(|e| e.to_string())?;
        let next = revision.successor().map_err(|e| e.to_string())?;
        let state = worker.run_status(run, next).map_err(|e| e.to_string())?;
        if state.phase != WorkerRunPhase::Running || !state.is_idle() {
            return Err("pending-Wait recovery did not advance revision".into());
        }
        broker
            .authorize_target(run, &observed.target_id)
            .map_err(|e| e.to_string())?;
        let stale = broker.act(action);
        if matches!(stale.kind, OutcomeKind::Applied | OutcomeKind::Verified) || stale.executed {
            return Err("old Wait gained authority after resume".into());
        }
        broker
            .observe_with_screenshot(run, false)
            .map_err(|e| e.to_string())?;
        println!("gate: broker_pending_wait actual-active-request cancelled/local-http-settled/remote-idle/resume=passed revision=2->3");
        Ok(())
    })
}

pub fn verify_worker(worker: &LoopbackPlaywrightWorker) -> Result<(), String> {
    let owner = "lifecycle-protocol-contract";
    let revision = WorkerRunRevision::INITIAL;
    let paused = worker
        .pause_run(owner, revision)
        .map_err(|e| e.to_string())?;
    if paused.phase != WorkerRunPhase::Paused || !paused.is_idle() {
        return Err("fresh owner did not acknowledge a quiescent pause".into());
    }
    let resumed = worker
        .resume_run(owner, revision)
        .map_err(|e| e.to_string())?;
    let next = revision.successor().map_err(|e| e.to_string())?;
    if resumed.revision != next || resumed.phase != WorkerRunPhase::Running || !resumed.is_idle() {
        return Err("resume did not advance the paused owner exactly once".into());
    }
    for stale in [
        worker.pause_run(owner, revision),
        worker.resume_run(owner, revision),
        worker.run_status(owner, revision),
    ] {
        if !matches!(stale, Err(ref e) if e.code == "stale_run_revision" && e.completion == WorkerCompletion::NotStarted)
        {
            return Err("stale lifecycle control was not rejected".into());
        }
    }
    let current = worker.run_status(owner, next).map_err(|e| e.to_string())?;
    if current != resumed {
        return Err("stale control changed current run state".into());
    }
    worker.cancel_run(owner).map_err(|e| e.to_string())?;
    let stopped = worker.run_status(owner, next).map_err(|e| e.to_string())?;
    if stopped.phase != WorkerRunPhase::Stopped || !stopped.is_idle() {
        return Err("cancel-run did not leave the versioned owner terminal".into());
    }
    println!(
        "gate: worker_lifecycle Rust-client pause/resume/status/stale/stop=passed revision=1->2"
    );
    Ok(())
}

fn accept(listener: &TcpListener) -> Result<TcpStream, String> {
    let until = Instant::now() + Duration::from_secs(4);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                // On Windows an accepted socket can inherit nonblocking mode.
                // The bounded read below requires a blocking socket + timeout.
                stream.set_nonblocking(false).map_err(|e| e.to_string())?;
                return Ok(stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return Err("lifecycle fixture accept failed or timed out".into()),
        }
    }
}

fn read_request(stream: &mut TcpStream, route: &str) -> Result<Value, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).map_err(|e| {
            format!(
                "incomplete fixture headers: {:?}, bytes={}",
                e.kind(),
                headers.len()
            )
        })?;
        headers.push(byte[0]);
        if headers.len() > 8192 {
            return Err("oversized fixture headers".into());
        }
    }
    let headers = String::from_utf8(headers).map_err(|_| "invalid fixture headers")?;
    if !headers.starts_with(&format!("POST {route} "))
        || !headers
            .to_ascii_lowercase()
            .contains("authorization: bearer lifecycle-fixture\r\n")
        || (route != "/health"
            && !headers
                .to_ascii_lowercase()
                .contains("x-grok-cu-host: 1\r\n"))
    {
        return Err("lifecycle route/auth/Host marker mismatch".into());
    }
    let length = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            if key.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .filter(|n| *n <= 8192)
        .ok_or("invalid fixture body length")?;
    let mut body = vec![0; length];
    stream
        .read_exact(&mut body)
        .map_err(|_| "incomplete fixture body")?;
    serde_json::from_slice(&body).map_err(|_| "invalid fixture JSON".into())
}

fn reply(stream: &mut TcpStream, body: &Value) -> Result<(), String> {
    let body = body.to_string();
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).map_err(|e| e.to_string())
}

pub fn verify_transport() -> Result<(), String> {
    for case in ["legacy", "lost", "stale", "busy", "wrong-phase"] {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let base = format!(
            "http://{}",
            listener.local_addr().map_err(|e| e.to_string())?
        );
        let server = std::thread::spawn(move || -> Result<(), String> {
            let mut health = accept(&listener)?;
            read_request(&mut health, "/health").map_err(|e| format!("{case} health: {e}"))?;
            reply(
                &mut health,
                &if case == "legacy" {
                    json!({"ok":true})
                } else {
                    json!({"ok":true,"runLifecycle":1})
                },
            )?;
            drop(health);
            if case != "legacy" {
                let mut control = accept(&listener)?;
                let body = read_request(&mut control, "/resume-run")
                    .map_err(|e| format!("{case} resume: {e}"))?;
                if body != json!({"owner":"fixture-run","runRevision":1,"nextRunRevision":2}) {
                    return Err("lifecycle request changed the admitted revision".into());
                }
                if case != "lost" {
                    let response = json!({"ok":true,"runRevision":if case == "stale" {1} else {2},"phase":if case == "wrong-phase" {"paused"} else {"running"},"activeOperations":if case == "busy" {1} else {0},"idle":case != "busy"});
                    reply(&mut control, &response)?;
                }
                drop(control);
            }
            let until = Instant::now() + Duration::from_millis(250);
            while Instant::now() < until {
                match listener.accept() {
                    Ok(_) => {
                        return Err(
                            "lifecycle client replayed a control or bypassed capability preflight"
                                .into(),
                        )
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => return Err(e.to_string()),
                }
            }
            Ok(())
        });
        let worker = LoopbackPlaywrightWorker::new(base, "lifecycle-fixture");
        let result = worker.resume_run("fixture-run", WorkerRunRevision::INITIAL);
        if let Err(server_error) = server.join().map_err(|_| "lifecycle fixture panicked")? {
            return Err(format!(
                "{server_error}; client code={}",
                result
                    .as_ref()
                    .err()
                    .map(|e| e.code.as_str())
                    .unwrap_or("unexpected_success")
            ));
        }
        let error = result
            .err()
            .ok_or("invalid lifecycle acknowledgement was accepted")?;
        if case == "legacy" {
            if error.code != "run_lifecycle_unavailable"
                || error.completion != WorkerCompletion::NotStarted
            {
                return Err("legacy runtime was not rejected before lifecycle dispatch".into());
            }
        } else if error.completion != WorkerCompletion::Unknown {
            return Err("unacknowledged resume must remain unknown".into());
        }
    }
    println!(
        "gate: lifecycle_transport legacy/lost/stale/busy/wrong-phase=rejected no-replay checks=5"
    );
    Ok(())
}
