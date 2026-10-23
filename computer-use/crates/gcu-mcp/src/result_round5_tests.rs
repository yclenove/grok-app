//! Round 5 commit-boundary checks. Each refusal is the only one that stops that case.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::files::FilePause;
use gcu_core::action::ActionRouter;
use gcu_core::scope::{FixtureScope, ScopeLease, SCOPE_DENIED_MESSAGE};
use gcu_core::session::WindowLocks;
use gcu_driver_fake::FakeDriver;
use gcu_host_api::testing::{
    FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals,
};
use gcu_protocol::{AppInfo, AppState, Rect, ScreenshotInfo, WindowInfo};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn desk(scope: Option<ScopeLease>) -> Results {
    let driver = Arc::new(FakeDriver::from_scene("form").unwrap());
    let router = ActionRouter::new(
        driver,
        Arc::new(ScriptedApprovals::new()),
        Arc::new(MemoryPolicyStore::new(Default::default())),
        Arc::new(RecordingEventSink::new()),
        WindowLocks::new(),
    );
    Results {
        images: Arc::new(crate::image_cache::ImageCache::default()),
        encoding: Default::default(),
        files: Arc::new(SessionFiles::new().unwrap()),
        session: Arc::new(Session::open(
            "round5",
            Arc::new(FakeUserActivity::new()),
            router.input_release(),
        )),
        scope,
        probe: result_guard::FinishProbe::new(),
    }
}

fn lease(generation: u64) -> ScopeLease {
    ScopeLease::install(
        FixtureScope::try_new(
            "run-s1",
            generation,
            "engine",
            "desktop",
            "lease",
            Vec::new(),
        )
        .unwrap(),
    )
}

fn stamped(overflow: Option<&str>) -> ActionState {
    ActionState {
        text: "visible".into(),
        overflow_text: overflow.map(str::to_owned),
        state: AppState {
            app: AppInfo {
                id: "fixture.a".into(),
                name: "Allowed A".into(),
                pid: None,
                identity: None,
            },
            window: WindowInfo {
                id: "w-a".into(),
                title: "Allowed A".into(),
                frame: Rect {
                    x: 0.,
                    y: 0.,
                    w: 32.,
                    h: 32.,
                },
                scale: 1.,
                on_current_desktop: Some(true),
            },
            snapshot_id: "s_9".into(),
            screenshot: None,
            focused: None,
            truncated: None,
            elements: None,
            action: None,
        },
        image: None,
        scope_run: Some("run-s1".into()),
        scope_generation: Some(1),
    }
}

fn finish_in(text: &str, data: serde_json::Value, pending: Option<PendingShot>) -> FinishIn {
    FinishIn {
        text: text.into(),
        data,
        images: Vec::new(),
        failed: false,
        stamp: result_guard::LiveStamp::epoch("run-s1", 1),
        created: Vec::new(),
        pending,
    }
}

fn shot(id: &str, bytes: &str) -> PendingShot {
    PendingShot {
        id: id.into(),
        mime: "image/png".into(),
        width: 2,
        height: 2,
        bytes: bytes.as_bytes().to_vec(),
        app_id: "fixture.a".into(),
        window_id: "w-a".into(),
        snapshot_id: "s_9".into(),
    }
}

async fn parked(gate: &FilePause) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !gate.is_parked() {
        if tokio::time::Instant::now() > deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    true
}

async fn finished(gate: &FilePause) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !gate.has_finished() {
        if tokio::time::Instant::now() > deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    true
}

fn release_when_parked(gate: Arc<gcu_core::action::PublishGate>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gate.is_parked() {
            if Instant::now() >= deadline {
                gate.release();
                panic!("publish gate did not park");
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        gate.release();
    })
}

#[tokio::test]
async fn file_worker_revoke_or_rebind_before_write_exports_nothing() {
    for remove in [false, true] {
        let scope = lease(1);
        let results = desk(Some(scope.clone()));
        let gate = results.files.arm_file_gate();
        let files = Arc::clone(&results.files);
        let task = tokio::spawn(async move {
            results
                .state(stamped(Some("CANARY-FILE before write")))
                .await
        });
        assert!(parked(&gate).await, "file stage did not park");
        assert!(files.export().unwrap().is_empty());
        assert!(!files.contains_bytes(b"CANARY-FILE"));
        if remove {
            scope.rebind(lease(2).current().unwrap()).unwrap();
        } else {
            scope.revoke();
        }
        gate.release();
        let result = task.await.unwrap();
        let rendered = serde_json::to_string(&result).unwrap();
        assert!(!rendered.contains("CANARY-FILE"), "{rendered}");
        assert!(finished(&gate).await, "staged file was not dropped");
        assert!(!files.contains_bytes(b"CANARY-FILE"));
        assert!(files.export().unwrap().is_empty());
        files.close().unwrap();
    }
}

#[tokio::test]
async fn file_worker_drop_then_resume_leaves_no_bytes() {
    let results = desk(Some(lease(1)));
    let gate = results.files.arm_file_gate();
    let files = Arc::clone(&results.files);
    let task = tokio::spawn(async move {
        results
            .state(stamped(Some("CANARY-DROP late worker")))
            .await
    });
    assert!(parked(&gate).await, "file stage did not park");
    task.abort();
    let _ = task.await;
    gate.release();
    assert!(
        finished(&gate).await,
        "dropped worker did not clean the stage"
    );
    assert!(!files.contains_bytes(b"CANARY-DROP"));
    assert!(files.export().unwrap().is_empty());
    files.close().unwrap();
}

#[tokio::test]
async fn file_worker_timeout_then_resume_leaves_no_bytes() {
    let results = desk(Some(lease(1)));
    let gate = results.files.arm_file_gate();
    let files = Arc::clone(&results.files);
    let started = Instant::now();
    let result = results
        .state(stamped(Some("CANARY-TIMEOUT late worker")))
        .await;
    assert!(
        started.elapsed() >= Duration::from_secs(4),
        "spawn_blocking returned before the 5s cap"
    );
    let rendered = serde_json::to_string(&result).unwrap();
    assert!(!rendered.contains("CANARY-TIMEOUT"), "{rendered}");
    gate.release();
    assert!(
        finished(&gate).await,
        "timed-out worker did not drop the stage"
    );
    assert!(!files.contains_bytes(b"CANARY-TIMEOUT"));
    assert!(files.export().unwrap().is_empty());
    files.close().unwrap();
}

#[tokio::test]
async fn discovery_and_script_spill_drop_canary_on_revoke_or_rebind() {
    for remove in [false, true] {
        for script in [false, true] {
            let scope = lease(1);
            let results = desk(Some(scope.clone()));
            let stamp = results.opened_stamp();
            let gate = results.files.arm_file_gate();
            let files = Arc::clone(&results.files);
            let task = tokio::spawn(async move {
                if script {
                    let output = format!("CANARY-PRIOR {}", "界".repeat(8_000));
                    results.script(&output, None, stamp).await
                } else {
                    results
                        .discovery(
                            json!({"apps":[{"name": format!("CANARY-DISC {}", "界".repeat(8_000))}]}),
                            stamp,
                        )
                        .await
                }
            });
            assert!(parked(&gate).await, "spill did not park");
            assert!(files.export().unwrap().is_empty());
            if remove {
                scope.rebind(lease(2).current().unwrap()).unwrap();
            } else {
                scope.revoke();
            }
            gate.release();
            let result = task.await.unwrap();
            let rendered = serde_json::to_string(&result).unwrap();
            assert!(!rendered.contains("CANARY-PRIOR"), "{rendered}");
            assert!(!rendered.contains("CANARY-DISC"), "{rendered}");
            assert!(finished(&gate).await);
            assert!(!files.contains_bytes(b"CANARY-PRIOR"));
            assert!(!files.contains_bytes(b"CANARY-DISC"));
            assert!(files.export().unwrap().is_empty());
            files.close().unwrap();
        }
    }
}

#[tokio::test]
async fn unstamped_fixed_sentence_drops_action_state_facts() {
    let results = desk(Some(lease(1)));
    let mut state = stamped(None);
    state.text = SCOPE_DENIED_MESSAGE.into();
    state.scope_run = None;
    state.scope_generation = None;
    state.state.app.id = "CANARY-STATE-APP".into();
    state.state.window.id = "CANARY-STATE-WIN".into();
    state.state.screenshot = Some(ScreenshotInfo {
        id: "CANARY-SHOT".into(),
        width: 2,
        height: 2,
        mime: "image/png".into(),
        scale_to_window: Some(1.0),
    });
    let result = results.state(state).await;
    let rendered = serde_json::to_string(&result).unwrap();
    assert!(!rendered.contains("CANARY-STATE-APP"), "{rendered}");
    assert!(!rendered.contains("CANARY-STATE-WIN"), "{rendered}");
    assert!(!rendered.contains("CANARY-SHOT"), "{rendered}");
    let value = serde_json::to_value(&result).unwrap();
    assert!(value["structuredContent"].get("state").is_none(), "{value}");
    results.files.close().unwrap();
}

#[tokio::test]
async fn finish_json_spill_probe_blocks_without_revocation() {
    let results = desk(Some(lease(1)));
    results.probe.arm("json-spill");
    let outcome = results
        .finish(finish_in(
            "short",
            json!({"mark":"CANARY-JSON","pad":"x".repeat(9_000)}),
            None,
        ))
        .await;
    assert!(outcome.is_err(), "json spill was published");
    assert!(!results.files.contains_bytes(b"CANARY-JSON"));
    assert!(results.files.export().unwrap().is_empty());
    results.files.close().unwrap();
}

#[tokio::test]
async fn finish_text_spill_probe_blocks_without_revocation() {
    let results = desk(Some(lease(1)));
    results.probe.arm("text-spill");
    let text = format!("CANARY-TEXT{}", "y".repeat(20_000));
    let outcome = results
        .finish(finish_in(&text, json!({"ok": true}), None))
        .await;
    assert!(outcome.is_err(), "text spill was published");
    assert!(!results.files.contains_bytes(b"CANARY-TEXT"));
    assert!(results.files.export().unwrap().is_empty());
    results.files.close().unwrap();
}

#[tokio::test]
async fn finish_commit_probe_blocks_without_revocation() {
    let results = desk(Some(lease(1)));
    results.probe.arm("commit");
    let outcome = results
        .finish(finish_in(
            "short",
            json!({"ok": true}),
            Some(shot("i_commit", "CANARY-COMMIT")),
        ))
        .await;
    assert!(outcome.is_err(), "response was published");
    assert!(results.images.get("i_commit").is_none());
    results.files.close().unwrap();
}

#[tokio::test]
async fn finish_cache_commit_probe_blocks_while_cache_locked() {
    let results = desk(Some(lease(1)));
    results.probe.arm("cache-commit");
    let gate = results.images.admission_gate();
    gate.arm();
    let release = release_when_parked(gate);
    let outcome = results
        .finish(finish_in(
            "short",
            json!({"ok": true}),
            Some(shot("i_cache", "CANARY-CACHE")),
        ))
        .await;
    release.join().unwrap();
    assert!(outcome.is_err(), "cache admission published the image");
    assert!(results.images.get("i_cache").is_none());
    results.files.close().unwrap();
}

#[tokio::test]
async fn delivery_revoke_while_snapshot_locked_drops_cache() {
    let scope = lease(1);
    let results = desk(Some(scope.clone()));
    let gate = results.session.delivery_gate();
    gate.arm();
    let release = std::thread::spawn({
        let gate = Arc::clone(&gate);
        move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !gate.is_parked() {
                if Instant::now() >= deadline {
                    gate.release();
                    panic!("delivery did not park");
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            scope.revoke();
            gate.release();
        }
    });
    let outcome = results
        .finish(finish_in(
            "short",
            json!({"ok": true}),
            Some(shot("i_deliver", "CANARY-DELIVER")),
        ))
        .await;
    release.join().unwrap();
    assert!(outcome.is_err(), "delivery published after revoke");
    assert!(results.images.get("i_deliver").is_none());
    let rendered = format!("{outcome:?}");
    assert!(!rendered.contains("CANARY-DELIVER"), "{rendered}");
    results.files.close().unwrap();
}
