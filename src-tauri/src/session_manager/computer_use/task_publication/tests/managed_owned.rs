//! Real isolated source Node/Chromium + original owned approval transaction.
//! Host gestures, gates, session and ACP peer are synthetic, not installed E2E.
use super::*;
use crate::computer_use::test_support::{
    remove_owned_root, run_fixture_cleanup_watch, BrowserSupervisor, FixtureApprovals, SpawnRequest,
};
use grok_computer_use_core::broker::{HostTaskDecision, StopState};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

fn require(value: bool, message: &str) -> Result<(), String> {
    value.then_some(()).ok_or_else(|| message.into())
}

fn events(root: &Path) -> Result<Vec<serde_json::Value>, String> {
    let path = root.join("fixture-events.ndjson");
    if !path.exists() {
        return Ok(vec![]);
    }
    std::fs::read_to_string(path)
        .map_err(|e| e.to_string())?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(|e| e.to_string()))
        .collect()
}

async fn wait_for(
    mut check: impl FnMut() -> Result<bool, String>,
    label: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if check()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("original owned {label} did not settle"));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn release_launch(root: &Path) -> Result<(), String> {
    std::fs::write(
        root.join("release-launch"),
        b"explicit isolated fixture release",
    )
    .map_err(|e| e.to_string())
}

#[test]
fn managed_owned_approval_persists_publishes_and_delivers_one_original_continuation() {
    scenario("original");
}

#[test]
fn managed_owned_stop_during_native_launch_fences_and_joins_late_resources() {
    scenario("stop-launch");
}

#[test]
fn managed_owned_waiter_drop_during_native_launch_retains_original_cleanup_owner() {
    scenario("drop-launch");
}

#[test]
fn managed_owned_session_replacement_at_publication_rolls_back_real_storage() {
    scenario("session-replaced");
}

#[test]
fn managed_owned_transport_replacement_at_publication_cannot_borrow_live_bearer() {
    scenario("transport-replaced");
}

#[test]
fn managed_owned_feature_off_during_native_launch_keeps_explicit_store_choice() {
    scenario("feature-off-launch");
}

#[test]
fn managed_owned_queued_continuation_stop_is_rejected_before_original_stdin() {
    scenario("queued-stop");
}

#[test]
#[ignore = "manual real 10-minute request deadline; no shortened product clock"]
fn managed_owned_real_request_deadline_during_native_launch_retires_original_resources() {
    scenario("deadline-launch");
}

fn scenario(terminal: &str) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    let root = temporary.join(format!("grok-cu-managed-task-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    for marker in ["owned-task-fixture", "hold-launch"] {
        std::fs::write(root.join(marker), b"synthetic isolated gate").unwrap();
    }
    let approvals = Arc::new(FixtureApprovals::new(&root));
    let mut f = runtime.block_on(TaskViewFixture::new_managed());
    let manager = manager(&f);
    let broker = f.broker();
    let supervisor = Arc::new(parking_lot::Mutex::new(None::<Arc<BrowserSupervisor>>));
    let commit_entered = Arc::new(AtomicBool::new(false));
    let commit_release = Arc::new(AtomicBool::new(false));
    let endpoint = manager.task_picker_endpoint("view-app").unwrap();
    let original_pin = manager.task_approval_pin("view-app", &endpoint);
    let entered = commit_entered.clone();
    let release = commit_release.clone();
    let pin = Arc::new(
        move |owner: &crate::acp_client::computer_use::TaskApprovalOwner,
              approval: &mut grok_computer_use_core::broker::AccessApproval| {
            entered.store(true, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(20);
            while !release.load(Ordering::SeqCst) {
                if Instant::now() >= deadline {
                    return Err("synthetic publication gate not released".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            original_pin(owner, approval)
        },
    );
    let errors = Arc::new(AtomicUsize::new(0));
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let watcher = runtime.spawn(run_fixture_cleanup_watch(
        broker.clone(),
        stopped,
        errors.clone(),
    ));
    let mut waiter = None;
    let result = runtime.block_on(async {
        let pending = manager.computer_use_task_view("view-app")?;
        require(pending.executions.is_empty() && pending.grants.is_empty() && !pending.feature_enabled,
            "readback prepared an execution")?;
        require(supervisor.lock().is_none() && events(&root)?.is_empty(),
            "request/readback booted a native worker")?;
        if terminal == "deadline-launch" {
            let remaining = pending.request.as_ref().ok_or("original request missing")?.remaining_ms;
            require((590_000..=600_000).contains(&remaining), "native deadline was shortened or original request lost")?;
            println!("managed owned wall-clock deadline: original remainingMs={remaining}; waiting without runtime/profile/grant");
            tokio::time::sleep(Duration::from_millis(remaining - 4_000)).await;
            require(supervisor.lock().is_none() && events(&root)?.is_empty(),
                "deadline wait booted a worker before explicit gesture")?;
        }
        let client = f.client.clone();
        let key = f.request_key();
        let workers = approvals.clone();
        let spawned = supervisor.clone();
        let prepare_broker = broker.clone();
        let profile_root = root.clone();
        waiter = Some(tokio::spawn(async move {
            client.approve_managed_fixture(&key, pin, workers, move || {
                let original = Arc::new(BrowserSupervisor::spawn(SpawnRequest {
                    node: manifest.join("resources/computer-use/seed/bin/node.exe"),
                    script: manifest.join("../tools/computer-use-browser/fixtures/task-approval-worker.mjs")
                        .canonicalize().map_err(|e| e.to_string())?,
                    profile_root,
                    browser: Some(manifest.join("resources/computer-use/seed/chromium/chrome-win/chrome.exe")),
                })?);
                *spawned.lock() = Some(original.clone());
                original.attach(&prepare_broker)
            }).await
        }));
        wait_for(|| Ok(events(&root)?.iter().any(|e| e["phase"] == "launch-native-ready")), "native launch gate").await?;
        let preparing = manager.computer_use_task_view("view-app")?;
        require(preparing.executions.len() == 1 && preparing.grants.is_empty(),
            "original resource owner missing or executable before Bound")?;
        let run = preparing.executions[0].run_id.clone();
        require(approvals.owners() == 1 && approvals.settings()["computerUseEnabled"] == true,
            "owned worker/storage was not retained during native preparation")?;
        require(broker.executable_grant_count("view-app") == 0 && broker.claim_continuation("view-app").is_none(),
            "preparation published execution or continuation")?;
        require(events(&root)?.iter().all(|e| e["path"] != "/observe" && e["path"] != "/act" && e["path"] != "/scope-identity"),
            "held launch read content/input/identity before returning")?;
        // Full production constructor rejects the duplicate before its runtime closure.
        require(f.client.approve_managed_fixture(&f.request_key(), manager.task_approval_pin("view-app", &endpoint),
            approvals.clone(), || panic!("duplicate gesture started a second runtime")).await.is_err(),
            "duplicate pending gesture was accepted")?;
        if terminal == "drop-launch" {
            let original = waiter.take().unwrap();
            original.abort();
            require(original.await.err().is_some_and(|e| e.is_cancelled()), "waiter did not cancel")?;
            require(approvals.owners() == 1, "cancelled waiter discarded original approval owner")?;
        } else if terminal == "stop-launch" {
            let started = Instant::now();
            manager.decide_computer_use_task("view-app", &f.request_key(), HostTaskDecision::Stop)?;
            require(started.elapsed() < Duration::from_millis(500), "Stop blocked on native launch")?;
        } else if terminal == "feature-off-launch" {
            approvals.disable(manager.clone(), broker.clone()).await?;
            require(approvals.settings()["computerUseEnabled"] == false && !broker.feature_enabled(),
                "explicit feature-off did not fence and persist during original launch")?;
        } else if terminal == "deadline-launch" {
            wait_for(|| Ok(manager.computer_use_task_view("view-app")?.request
                .is_some_and(|request| request.phase == grok_computer_use_core::task_consent::HostRequestPhase::Expired)),
                "actual monotonic request deadline").await?;
            println!("managed owned wall-clock deadline: original request expired while actual native context was held");
        }
        if terminal.ends_with("launch") {
            require(broker.stop_state(&run).map_err(|e| e.to_string())? == StopState::StopRequested,
                "held native resource was reported physically stopped")?;
            require(!broker.tabs().managed_run_idle(&run), "held context was reported idle")?;
            commit_release.store(true, Ordering::SeqCst);
        }
        release_launch(&root)?;
        if !terminal.ends_with("launch") {
            wait_for(|| Ok(commit_entered.load(Ordering::SeqCst)), "final publication gate").await?;
            require(manager.inner.try_lock().is_some() && manager.background.try_lock().is_some()
                && manager.parked.try_lock().is_some(), "native preparation held App slot locks")?;
            require(broker.executable_grant_count("view-app") == 0 && broker.claim_continuation("view-app").is_none(),
                "Bound prepared authority before original publication")?;
            require(events(&root)?.iter().any(|e| e["path"] == "/scope-identity")
                && events(&root)?.iter().all(|e| e["path"] != "/observe" && e["path"] != "/act"),
                "native proof missing or webpage/input requested before publication")?;
            match terminal {
                "session-replaced" => manager.inner.lock().as_mut().unwrap().process_id = "replacement-process".into(),
                "transport-replaced" => f.replace_approval_transport_same_identity(),
                _ => {}
            }
            commit_release.store(true, Ordering::SeqCst);
        }
        let completed = if let Some(original) = waiter.as_mut() {
            Some(tokio::time::timeout(Duration::from_secs(20), original).await
                .map_err(|_| "original approval waiter did not settle")?)
        } else { None };
        // Keep the handle on timeout; retire it only after its actual join.
        waiter.take();
        let outcome = completed.map(|joined| joined.map_err(|e| e.to_string())).transpose()?;
        approvals.join().await?;
        require(approvals.owners() == 0, "approval registry did not join original worker")?;
        if matches!(terminal, "original" | "queued-stop") {
            let outcome = outcome.unwrap()?;
            require(outcome.run_id == run && broker.executable_grant_count("view-app") == 1
                && broker.continuation_count("view-app") == 1, "publication changed owner or duplicated authority")?;
            if terminal == "original" {
                let observing = broker.clone();
                let owned_run = run.clone();
                let observation = tokio::task::spawn_blocking(move || observing.observe_with_screenshot(&owned_run, false))
                    .await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
                require(observation.target_id == outcome.binding.target_id, "fresh observe borrowed another page")?;
                f.deliver_managed_continuation_once().await?;
                manager.decide_computer_use_task("view-app", &f.request_key(), HostTaskDecision::Stop)?;
            } else {
                let (scheduled, delivery, turn) = manager.reserve_computer_use_delivery("view-app")
                    .ok_or("actual manager did not reserve original continuation")?;
                require(scheduled.agent_session_id == "synthetic-view-agent" && !delivery.was_admitted(),
                    "manager queued a different owner or consumed before stdin")?;
                let original = manager.clone();
                let key = f.request_key();
                f.queued_continuation_stops_before_wire(delivery, turn, move || {
                    original.decide_computer_use_task("view-app", &key, HostTaskDecision::Stop).map(|_| ())
                }).await?;
                crate::turn_lease::clear_lease("view-app");
            }
        } else {
            require(outcome.is_none_or(|value| value.is_err()), "retired approval succeeded")?;
            require(approvals.settings()["computerUseEnabled"] == false && !broker.feature_enabled(),
                "failed original worker did not roll back real preference")?;
        }
        require(broker.executable_grant_count("view-app") == 0 && broker.claim_continuation("view-app").is_none(),
            "retired original retained executable authority")?;
        wait_for(|| Ok(broker.stop_state(&run).map_err(|e| e.to_string())? == StopState::Stopped), "Stopped resource").await?;
        let observed = events(&root)?;
        for phase in ["launch-native-ready", "launch-returned", "close-dispatched", "close-event", "close-native-settled"] {
            require(observed.iter().filter(|e| e["phase"] == phase).count() == 1,
                "original native launch/close was replayed or never settled")?;
        }
        require(observed.iter().filter(|e| e["path"] == "/open").count() == 1
            && observed.iter().filter(|e| e["path"] == "/observe").count() == usize::from(terminal == "original")
            && observed.iter().all(|e| !["/act", "/goto", "/new-tab", "/upload", "/download"].iter().any(|p| e["path"] == *p)),
            "unexpected native content/input or duplicate context request")?;
        require(broker.tabs().managed_run_idle(&run), "Stopped left original native context occupied")?;
        for event in observed { println!("managed owned native {terminal}: {event}"); }
        println!("managed owned terminal {terminal}: original storage/worker/ACP pin; one launch/close; Stopped; no input");
        Ok::<(), String>(())
    });
    // Release ORIGINAL gates even on a failed oracle; do not replay any native input/close.
    release_launch(&root).unwrap();
    commit_release.store(true, Ordering::SeqCst);
    if let Some(original) = waiter.take() {
        runtime.block_on(original).unwrap().ok();
    }
    let joined = runtime.block_on(approvals.join());
    let _ = stop.send(());
    runtime.block_on(watcher).unwrap();
    drop(manager);
    runtime.block_on(f.finish());
    let shutdown = supervisor
        .lock()
        .take()
        .map(|original| original.shutdown())
        .transpose();
    approvals.reset_feature(&broker);
    crate::turn_lease::clear_lease("view-app");
    if result.is_ok() && joined.is_ok() && shutdown.is_ok() {
        remove_owned_root(&root, &temporary);
    }
    assert!(
        joined.is_ok(),
        "original approval join failed; retain {root:?}: {joined:?}"
    );
    assert!(
        shutdown.is_ok(),
        "original supervisor shutdown failed; retain {root:?}: {shutdown:?}"
    );
    assert!(
        result.is_ok(),
        "owned native {terminal} failed; retain {root:?}: {result:?}"
    );
    assert!(!root.exists());
}
