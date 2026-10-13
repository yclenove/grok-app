//! Production cleanup consumer + real isolated source worker/Chromium.
//! Host/session/ACP and the delay are synthetic; not human/installed acceptance.
use super::*;
use crate::computer_use::test_support::{
    remove_owned_root, run_fixture_cleanup_watch, BrowserSupervisor, SpawnRequest,
};
use grok_computer_use_core::broker::{HostTaskDecision, StopState};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
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
            return Err(format!("{label} did not reach original terminal"));
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[test]
fn managed_native_cleanup_consumer_retains_original_owner_past_reply_deadline() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let f = runtime.block_on(TaskViewFixture::new_managed());
    let manager = manager(&f);
    let endpoint = manager.task_picker_endpoint("view-app").unwrap();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    let root = temporary.join(format!("grok-cu-managed-task-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    for marker in ["owned-task-fixture", "hold-close"] {
        std::fs::write(root.join(marker), b"synthetic isolated gate").unwrap();
    }
    let supervisor = BrowserSupervisor::spawn(SpawnRequest {
        node: manifest.join("resources/computer-use/seed/bin/node.exe"),
        script: manifest
            .join("../tools/computer-use-browser/fixtures/task-approval-worker.mjs")
            .canonicalize()
            .unwrap(),
        profile_root: root.clone(),
        browser: Some(manifest.join("resources/computer-use/seed/chromium/chrome-win/chrome.exe")),
    })
    .unwrap();
    let broker = f.broker();
    let setup = (|| -> Result<String, String> {
        supervisor.attach(&broker)?;
        let (mut approval, publish) =
            f.pending_managed_approval(manager.task_approval_pin("view-app", &endpoint));
        approval.publish_feature().map_err(|e| e.to_string())?;
        approval.prepare_execution().map_err(|e| e.to_string())?;
        approval.prepare_publication().map_err(|e| e.to_string())?;
        let outcome = publish(&mut approval)?;
        let started = Instant::now();
        manager.decide_computer_use_task("view-app", &f.request_key(), HostTaskDecision::Stop)?;
        require(
            started.elapsed() < Duration::from_millis(500),
            "Stop waited for physical close",
        )?;
        require(
            broker.executable_grant_count("view-app") == 0
                && broker.claim_continuation("view-app").is_none(),
            "Stop retained authority",
        )?;
        Ok(outcome.run_id)
    })();
    let errors = Arc::new(AtomicUsize::new(0));
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let watcher = runtime.spawn(run_fixture_cleanup_watch(
        broker.clone(),
        stopped,
        errors.clone(),
    ));
    let result = runtime.block_on(async {
        let run = setup?;
        wait_for(|| Ok(events(&root)?.iter().any(|e| e["phase"] == "close-dispatched")),
            "native original close admission").await?;
        wait_for(|| Ok(errors.load(Ordering::SeqCst) > 0), "bounded cleanup pending reply").await?;
        require(broker.stop_state(&run).map_err(|e| e.to_string())? == StopState::StopRequested,
            "pending reply was misreported as Stopped")?;
        require(!broker.tabs().managed_run_idle(&run), "pending native context was reported idle")?;
        require(events(&root)?.iter().all(|e| e["phase"] != "close-event" && e["phase"] != "close-native-settled"),
            "held native context closed before release")?;
        std::fs::write(root.join("release-close"), b"explicit fixture release").map_err(|e| e.to_string())?;
        wait_for(|| Ok(broker.stop_state(&run).map_err(|e| e.to_string())? == StopState::Stopped),
            "original stopped run").await?;
        let observed = events(&root)?;
        for phase in ["launch-native-ready", "close-dispatched", "close-event", "close-native-settled"] {
            require(observed.iter().filter(|e| e["phase"] == phase).count() == 1,
                "native launch/close was replayed or its settlement is missing")?;
        }
        require(broker.tabs().managed_run_idle(&run), "original native cleanup never confirmed idle")?;
        for event in observed { println!("managed cleanup native phase: {event}"); }
        println!("managed cleanup native: retained pending replies={}; one original close; Stopped; no input",
            errors.load(Ordering::SeqCst));
        Ok::<(), String>(())
    });
    // Release and join ORIGINAL owners even when the assertion body failed.
    std::fs::write(root.join("release-close"), b"fixture failure cleanup").unwrap();
    let _ = stop.send(());
    runtime.block_on(watcher).unwrap();
    drop(manager);
    runtime.block_on(f.finish());
    let shutdown = supervisor.shutdown();
    if result.is_ok() && shutdown.is_ok() {
        remove_owned_root(&root, &temporary);
    }
    assert!(
        shutdown.is_ok(),
        "original supervisor cleanup unconfirmed: {shutdown:?}"
    );
    assert!(
        result.is_ok(),
        "native cleanup failed; retain {root:?}: {result:?}"
    );
    assert!(!root.exists());
}
