//! Round 6: one atomic commit. These cases fail if a product is visible
//! before that commit, or if an unscoped script grows past the old prefix.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use gcu_core::driver::Capture;
use gcu_core::model::Geometry;
use gcu_core::redact::{self, CapturedFrame};
use gcu_core::scope::{FixtureScope, ScopeLease};
use gcu_core::tree::ProcessedTree;
use gcu_protocol::{AppInfo, AppState, Rect, ScreenshotInfo, WindowInfo};
use std::time::Duration;

fn lease() -> ScopeLease {
    ScopeLease::install(
        FixtureScope::try_new("run-s1", 1, "engine", "desktop", "lease", Vec::new()).unwrap(),
    )
}

fn desk(scope: Option<ScopeLease>) -> Results {
    let driver = std::sync::Arc::new(gcu_driver_fake::FakeDriver::from_scene("form").unwrap());
    let router = gcu_core::action::ActionRouter::new(
        driver,
        std::sync::Arc::new(gcu_host_api::testing::ScriptedApprovals::new()),
        std::sync::Arc::new(gcu_host_api::testing::MemoryPolicyStore::new(
            Default::default(),
        )),
        std::sync::Arc::new(gcu_host_api::testing::RecordingEventSink::new()),
        gcu_core::session::WindowLocks::new(),
    );
    Results {
        images: std::sync::Arc::new(crate::image_cache::ImageCache::default()),
        encoding: Default::default(),
        files: std::sync::Arc::new(SessionFiles::new().unwrap()),
        session: std::sync::Arc::new(gcu_core::session::Session::open(
            "round6",
            std::sync::Arc::new(gcu_host_api::testing::FakeUserActivity::new()),
            router.input_release(),
        )),
        scope,
        probe: result_guard::FinishProbe::new(),
    }
}

fn shot_state(image: bool) -> gcu_core::action::ActionState {
    gcu_core::action::ActionState {
        text: "visible".into(),
        overflow_text: None,
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
                    w: 256.,
                    h: 256.,
                },
                scale: 1.,
                on_current_desktop: Some(true),
            },
            snapshot_id: "s_9".into(),
            screenshot: Some(ScreenshotInfo {
                id: "i_private".into(),
                width: 256,
                height: 256,
                mime: "image/png".into(),
                scale_to_window: Some(1.),
            }),
            focused: None,
            truncated: None,
            elements: None,
            action: None,
        },
        image: image.then(solid_png),
        scope_run: Some("run-s1".into()),
        scope_generation: Some(1),
    }
}

fn solid_png() -> gcu_core::redact::RedactedImage {
    let (width, height) = (256u32, 256u32);
    let pixels = [9u8, 8, 7, 255].repeat((width * height) as usize);
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    let geometry = Geometry {
        frame: Rect {
            x: 0.,
            y: 0.,
            w: f64::from(width),
            h: f64::from(height),
        },
        scale: 1.,
    };
    let mut tree = ProcessedTree::default();
    tree.state_version = Some(1);
    tree.observed_at = Some(Duration::ZERO);
    tree.sensitive_regions_reliable = true;
    redact::apply(
        &tree,
        CapturedFrame::new(
            Capture {
                bytes,
                width,
                height,
                mime: "image/png".into(),
                geometry,
            },
            1,
            Duration::ZERO,
        ),
    )
    .unwrap()
}

fn wait_parked(gate: &gcu_core::action::PublishGate) {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while !gate.is_parked() {
        if std::time::Instant::now() >= deadline {
            gate.release();
            panic!("publish gate did not park");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Release even when a parked assertion panics. Otherwise the publisher stays
/// inside the gate and the test task never reaches `join`.
struct ReleaseGate<'a>(&'a gcu_core::action::PublishGate);

impl Drop for ReleaseGate<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[tokio::test]
async fn unscoped_script_keeps_the_historical_2000_byte_prefix() {
    let results = desk(None);
    let three_kb = format!(
        "{}{}{}",
        "H".repeat(2_000),
        "x".repeat(984),
        "TAIL-CANARY-3000"
    );
    assert_eq!(three_kb.len(), 3_000);
    let baseline = results
        .script(&"H".repeat(2_000), None, result_guard::LiveStamp::off())
        .await;
    let short = results
        .script(&three_kb, None, result_guard::LiveStamp::off())
        .await;
    let rendered = serde_json::to_string(&short).unwrap();
    assert_eq!(
        serde_json::to_value(&short).unwrap(),
        serde_json::to_value(&baseline).unwrap()
    );
    assert!(
        !rendered.contains("TAIL-CANARY-3000"),
        "3KB script returned bytes past the historical prefix: {rendered}"
    );
    assert!(rendered.contains(&"H".repeat(64)), "{rendered}");
    assert!(
        results.files.export().unwrap().is_empty(),
        "3KB script created a spill file"
    );

    let longer = format!("{}{}", "H".repeat(2_000), "Q".repeat(40_000));
    let long = results
        .script(&longer, None, result_guard::LiveStamp::off())
        .await;
    let long_rendered = serde_json::to_string(&long).unwrap();
    assert_eq!(
        serde_json::to_value(&long).unwrap(),
        serde_json::to_value(&baseline).unwrap()
    );
    assert!(
        !long_rendered.contains('Q'),
        "long unscoped script kept a tail or a spill marker: {long_rendered}"
    );
    assert!(
        !long_rendered.contains("truncated to 20 KB"),
        "{long_rendered}"
    );
    assert!(
        results.files.export().unwrap().is_empty(),
        "long unscoped script spilled"
    );
    assert_eq!(long.is_error, Some(false), "{long_rendered}");
    results.files.close().unwrap();
}

#[tokio::test]
async fn private_image_is_invisible_until_commit_and_revoke_rejects_it() {
    let scope = lease();
    let results = desk(Some(scope.clone()));
    let gate = results.session.delivery_gate();
    gate.arm();
    let files = std::sync::Arc::clone(&results.files);
    let images = std::sync::Arc::clone(&results.images);
    let watcher = std::thread::spawn({
        let gate = std::sync::Arc::clone(&gate);
        let files = std::sync::Arc::clone(&files);
        let images = std::sync::Arc::clone(&images);
        let scope = scope.clone();
        move || {
            wait_parked(&gate);
            let _release = ReleaseGate(&gate);
            assert!(
                files.export().unwrap().is_empty(),
                "export saw a file before commit"
            );
            assert!(
                images.get("i_private").is_none(),
                "cache was readable before commit"
            );
            scope.revoke();
        }
    });
    let result = results.state(shot_state(true)).await;
    watcher.join().unwrap();
    let rendered = serde_json::to_string(&result).unwrap();
    assert!(
        !rendered.contains("Screenshot file:"),
        "refused commit returned the image path: {rendered}"
    );
    assert!(images.get("i_private").is_none());
    assert!(files.export().unwrap().is_empty());
    assert!(!files.contains_bytes(&[9, 8, 7, 255]));
    files.close().unwrap();
}

#[tokio::test]
async fn connection_export_and_cached_image_stay_empty_before_commit() {
    use gcu_host_api::{ApprovalDecision, ConfirmationLevel, ForegroundPolicy, PolicyConfig};
    let approvals = std::sync::Arc::new(gcu_host_api::testing::ScriptedApprovals::new());
    approvals.push_decision(ApprovalDecision::Session);
    approvals.push_decision(ApprovalDecision::Session);
    let host = crate::Host {
        approvals,
        policy: std::sync::Arc::new(gcu_host_api::testing::MemoryPolicyStore::new(
            PolicyConfig {
                sensitive_action_confirmation: ConfirmationLevel::Off,
                allow_foreground: ForegroundPolicy::Always,
                ..PolicyConfig::default()
            },
        )),
        events: std::sync::Arc::new(gcu_host_api::testing::RecordingEventSink::new()),
        activity: std::sync::Arc::new(gcu_host_api::testing::FakeUserActivity::new()),
    };
    let driver = std::sync::Arc::new(
        gcu_driver_fake::FakeDriver::from_scene("form")
            .unwrap()
            .with_image_digest([7; 32])
            .unwrap(),
    );
    let connection = std::sync::Arc::new(
        crate::Connection::open_host_only(driver, host, Default::default())
            .await
            .unwrap(),
    );
    let gate = connection.session().delivery_gate();
    gate.arm();
    let report = std::sync::Arc::new(std::sync::Mutex::new((true, true)));
    let watcher = std::thread::spawn({
        let connection = std::sync::Arc::clone(&connection);
        let gate = std::sync::Arc::clone(&gate);
        let report = std::sync::Arc::clone(&report);
        move || {
            wait_parked(&gate);
            let _release = ReleaseGate(&gate);
            let export_empty = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(connection.export_private_files())
                .unwrap()
                .is_empty();
            let cache_empty = connection.cached_image("i_1").is_none();
            *report.lock().unwrap_or_else(|err| err.into_inner()) = (export_empty, cache_empty);
        }
    });
    let result = connection
        .call(
            "get_app_state",
            serde_json::json!({"app": "fake.form", "include_screenshot": true}),
        )
        .await;
    watcher.join().unwrap();
    let (export_empty, cache_empty) = *report.lock().unwrap_or_else(|err| err.into_inner());
    assert!(
        export_empty,
        "Connection::export_private_files saw a file before commit"
    );
    assert!(
        cache_empty,
        "Connection::cached_image saw pixels before commit"
    );
    let rendered = serde_json::to_string(&result).unwrap();
    assert_ne!(result.is_error, Some(true), "{rendered}");
    let id = result
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/state/screenshot/id"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    assert_eq!(id, "i_1", "{rendered}");
    assert!(connection.cached_image("i_1").is_some(), "{rendered}");
    assert!(
        !connection.export_private_files().await.unwrap().is_empty(),
        "committed screenshot was not exported"
    );
}

#[tokio::test]
async fn revoke_before_commit_hides_the_spill_and_after_commit_keeps_it() {
    let scope = lease();
    let results = desk(Some(scope.clone()));
    let gate = scope.commit_before_gate();
    gate.arm();
    let files = std::sync::Arc::clone(&results.files);
    let watcher = std::thread::spawn({
        let gate = std::sync::Arc::clone(&gate);
        let files = std::sync::Arc::clone(&files);
        let scope = scope.clone();
        move || {
            wait_parked(&gate);
            let _release = ReleaseGate(&gate);
            assert!(
                files.export().unwrap().is_empty(),
                "export saw a spill before the commit lock"
            );
            scope.revoke();
        }
    });
    let mut hidden = shot_state(false);
    hidden.overflow_text = Some("CANARY-BEFORE".into());
    let refused = results.state(hidden).await;
    watcher.join().unwrap();
    let rendered = serde_json::to_string(&refused).unwrap();
    assert!(!rendered.contains("CANARY-BEFORE"), "{rendered}");
    assert!(files.export().unwrap().is_empty());
    assert!(!files.contains_bytes(b"CANARY-BEFORE"));
    files.close().unwrap();

    let scope = lease();
    let results = desk(Some(scope.clone()));
    let gate = scope.commit_after_gate();
    gate.arm();
    let files = std::sync::Arc::clone(&results.files);
    let watcher = std::thread::spawn({
        let gate = std::sync::Arc::clone(&gate);
        let files = std::sync::Arc::clone(&files);
        let scope = scope.clone();
        move || {
            wait_parked(&gate);
            let _release = ReleaseGate(&gate);
            let rows = files.export().unwrap();
            assert!(
                rows.iter().any(|(_, bytes)| bytes
                    .windows(b"CANARY-AFTER".len())
                    .any(|window| window == b"CANARY-AFTER")),
                "committed spill was invisible at the after barrier"
            );
            scope.revoke();
        }
    });
    let mut kept = shot_state(false);
    kept.overflow_text = Some("CANARY-AFTER".into());
    let published = results.state(kept).await;
    watcher.join().unwrap();
    let rendered = serde_json::to_string(&published).unwrap();
    assert_eq!(published.is_error, Some(false), "{rendered}");
    let rows = files.export().unwrap();
    assert!(
        rows.iter().any(|(_, bytes)| bytes
            .windows(b"CANARY-AFTER".len())
            .any(|window| window == b"CANARY-AFTER")),
        "revoke rolled back a committed spill: {rendered}"
    );
    files.close().unwrap();
}

#[tokio::test]
async fn revoke_during_commit_waits_until_registration_finishes() {
    let scope = lease();
    let results = desk(Some(scope.clone()));
    let gate = scope.commit_during_gate();
    gate.arm();
    let files = std::sync::Arc::clone(&results.files);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watcher = std::thread::spawn({
        let gate = std::sync::Arc::clone(&gate);
        let scope = scope.clone();
        let done = std::sync::Arc::clone(&done);
        move || {
            wait_parked(&gate);
            let release = ReleaseGate(&gate);
            let revoked = std::sync::Arc::clone(&done);
            let revoker = std::thread::spawn(move || {
                scope.revoke();
                revoked.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            std::thread::sleep(Duration::from_millis(200));
            assert!(
                !done.load(std::sync::atomic::Ordering::SeqCst),
                "revoke interleaved the commit critical section"
            );
            drop(release);
            revoker.join().unwrap();
        }
    });
    let mut state = shot_state(false);
    state.overflow_text = Some("CANARY-DURING".into());
    let published = results.state(state).await;
    watcher.join().unwrap();
    let rendered = serde_json::to_string(&published).unwrap();
    assert_eq!(published.is_error, Some(false), "{rendered}");
    let rows = files.export().unwrap();
    assert!(
        rows.iter().any(|(_, bytes)| bytes
            .windows(b"CANARY-DURING".len())
            .any(|window| window == b"CANARY-DURING")),
        "{rendered}"
    );
    files.close().unwrap();
}

#[tokio::test]
async fn cancel_after_the_first_file_deletes_the_staged_bytes() {
    let results = desk(Some(lease()));
    let gate = results.files.arm_file_gate_at(2);
    let files = std::sync::Arc::clone(&results.files);
    let mut state = shot_state(true);
    state.overflow_text = Some("CANARY-SECOND".into());
    let task = tokio::spawn(async move { results.state(state).await });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while !gate.is_parked() {
        if tokio::time::Instant::now() >= deadline {
            gate.release();
            panic!("second file write did not park");
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(files.contains_bytes(b"PNG"), "first image was not staged");
    assert!(
        files.export().unwrap().is_empty(),
        "first file was exported before commit"
    );
    task.abort();
    let _ = task.await;
    gate.release();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !gate.has_finished()
        || files.contains_bytes(b"PNG")
        || files.contains_bytes(b"CANARY-SECOND")
    {
        if std::time::Instant::now() >= deadline {
            panic!("staged bytes remained after cancel");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(files.export().unwrap().is_empty());
    files.close().unwrap();
}

#[tokio::test]
async fn revoke_while_cache_admission_waits_does_not_deadlock_or_publish() {
    let scope = lease();
    let results = desk(Some(scope.clone()));
    let gate = results.images.admission_gate();
    gate.arm();
    let watcher = std::thread::spawn({
        let gate = std::sync::Arc::clone(&gate);
        move || {
            let _release = ReleaseGate(&gate);
            wait_parked(&gate);
            // This must not wait for the gate: cache lock acquisition precedes
            // the lease read lock. The gate itself has a bounded timeout.
            scope.revoke();
        }
    });
    let result = results.state(shot_state(true)).await;
    watcher.join().unwrap();
    assert!(results.images.get("i_private").is_none());
    assert!(results.files.export().unwrap().is_empty());
    assert!(!results.files.contains_bytes(b"PNG"));
    let rendered = serde_json::to_string(&result).unwrap();
    assert!(!rendered.contains("Screenshot file:"), "{rendered}");
    results.files.close().unwrap();
}

#[tokio::test]
async fn revoke_after_image_commit_keeps_the_entire_published_batch() {
    let scope = lease();
    let results = desk(Some(scope.clone()));
    let gate = scope.commit_after_gate();
    gate.arm();
    let watcher = std::thread::spawn({
        let gate = std::sync::Arc::clone(&gate);
        let images = std::sync::Arc::clone(&results.images);
        let files = std::sync::Arc::clone(&results.files);
        move || {
            let _release = ReleaseGate(&gate);
            wait_parked(&gate);
            assert!(
                images.get("i_private").is_some(),
                "committed image was not readable"
            );
            assert_eq!(
                files.export().unwrap().len(),
                2,
                "batch was only partly registered"
            );
            scope.revoke();
        }
    });
    let mut state = shot_state(true);
    state.overflow_text = Some("CANARY-COMMITTED".into());
    let result = results.state(state).await;
    watcher.join().unwrap();
    assert_eq!(result.is_error, Some(false));
    assert!(results.images.get("i_private").is_some());
    assert_eq!(results.files.export().unwrap().len(), 2);
    assert!(results.files.contains_bytes(b"CANARY-COMMITTED"));
    results.files.close().unwrap();
}
