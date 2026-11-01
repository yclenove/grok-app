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
    desk_with_activity(
        scope,
        std::sync::Arc::new(gcu_host_api::testing::FakeUserActivity::new()),
    )
}

fn desk_with_activity(
    scope: Option<ScopeLease>,
    activity: std::sync::Arc<gcu_host_api::testing::FakeUserActivity>,
) -> Results {
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
            activity,
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
async fn revoke_before_commit_hides_the_spill_and_revoke_after_commit_reclaims_it() {
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
    let paths = artifact_paths(&rendered);
    assert!(!paths.is_empty(), "{rendered}");
    wait_until_gone(&paths).await;
    assert!(
        files.export().unwrap().is_empty(),
        "revoked spill stayed readable: {rendered}"
    );
    assert!(!files.contains_bytes(b"CANARY-AFTER"));
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
    assert!(
        files.export().unwrap().is_empty(),
        "revoked spill stayed readable: {rendered}"
    );
    assert!(!files.contains_bytes(b"CANARY-DURING"));
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
async fn revoke_after_image_commit_hides_the_published_batch() {
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
    let rendered = serde_json::to_string(&result).unwrap();
    let paths = artifact_paths(&rendered);
    assert!(!paths.is_empty(), "{rendered}");
    wait_until_gone(&paths).await;
    assert!(
        !results.images.resident("i_private"),
        "revoked pixels stayed resident"
    );
    assert!(
        results.images.get("i_private").is_none(),
        "revoked generation stayed readable"
    );
    assert!(results.files.export().unwrap().is_empty());
    assert!(!results.files.contains_bytes(b"CANARY-COMMITTED"));
    results.files.close().unwrap();
}

fn artifact_paths(rendered: &str) -> Vec<std::path::PathBuf> {
    // Tool JSON escapes `\`, so a Windows path is not a raw `C:\...` substring.
    // Parse first, then classify with `Path::is_absolute` (drive prefix or `/`).
    let text = rendered_text(rendered);
    let mut paths = Vec::new();
    for marker in ["full text: ", "Screenshot file: "] {
        for chunk in text.split(marker).skip(1) {
            let stop = chunk
                .find(|ch: char| ch.is_whitespace() || matches!(ch, ']' | '"' | ','))
                .unwrap_or(chunk.len());
            let path = std::path::Path::new(chunk[..stop].trim());
            if path.is_absolute() {
                paths.push(path.to_path_buf());
            }
        }
    }
    paths
}

fn rendered_text(rendered: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(rendered) else {
        return rendered.to_string();
    };
    let mut text = String::new();
    push_json_strings(&value, &mut text);
    text
}

fn push_json_strings(value: &serde_json::Value, text: &mut String) {
    match value {
        serde_json::Value::String(item) => {
            text.push_str(item);
            text.push('\n');
        }
        serde_json::Value::Array(items) => {
            for item in items {
                push_json_strings(item, text);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values() {
                push_json_strings(item, text);
            }
        }
        _ => {}
    }
}

#[test]
fn artifact_paths_parse_json_strings_before_taking_absolute_paths() {
    let logical = "full text: C:\\session\\0.png\nScreenshot file: /tmp/shot.png\nScreenshot file: /tmp/ses\\sion/0.png\n";
    let rendered = serde_json::to_string(&serde_json::json!({
        "content": [{ "text": logical }]
    }))
    .unwrap();
    assert!(
        rendered.contains(r"C:\\session\\0.png"),
        "fixture must keep the escaped backslash: {rendered}"
    );
    assert!(
        rendered.contains(r"/tmp/ses\\sion/0.png"),
        "fixture must keep the escaped backslash: {rendered}"
    );
    let paths = artifact_paths(&rendered);
    let shot = std::path::Path::new("/tmp/shot.png");
    let nested = std::path::Path::new(r"/tmp/ses\sion/0.png");
    let windows = std::path::Path::new(r"C:\session\0.png");
    if shot.is_absolute() {
        assert!(paths.iter().any(|path| path == shot), "{paths:?}");
    }
    if nested.is_absolute() {
        assert!(
            paths.iter().any(|path| path == nested),
            "unescape skipped or backslash still ends the path: {paths:?} from {rendered}"
        );
    }
    if windows.is_absolute() {
        assert!(paths.iter().any(|path| path == windows), "{paths:?}");
    } else {
        assert!(paths.iter().all(|path| path != windows), "{paths:?}");
    }
}

async fn wait_until_gone(paths: &[std::path::PathBuf]) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while paths.iter().any(|path| path.exists()) {
        if tokio::time::Instant::now() >= deadline {
            panic!("invalidation left a delivered file: {paths:?}");
        }
        tokio::task::yield_now().await;
    }
}

#[derive(Debug, Default)]
struct HeldClock(std::sync::atomic::AtomicU64);
impl gcu_core::scope::Clock for HeldClock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(std::sync::atomic::Ordering::SeqCst))
    }
}
impl HeldClock {
    fn expire(&self) {
        self.0.store(4, std::sync::atomic::Ordering::SeqCst);
    }
}

fn timed_lease(clock: std::sync::Arc<HeldClock>) -> ScopeLease {
    ScopeLease::install_with_deadline(lease().current().unwrap(), clock, Duration::from_secs(3))
}

fn assert_only_scope_denied(results: &Results, result: &CallToolResult) {
    let value = serde_json::to_value(result).unwrap();
    assert_eq!(result.is_error, Some(true), "{value}");
    assert_eq!(result.content.len(), 1, "{value}");
    assert_eq!(
        result.content[0].as_text().unwrap().text,
        format!(
            "error: app_denied: {}\n",
            gcu_core::scope::SCOPE_DENIED_MESSAGE
        )
    );
    assert_eq!(
        result.structured_content,
        Some(json!({"error": {
            "code": "app_denied",
            "message": gcu_core::scope::SCOPE_DENIED_MESSAGE,
            "hint": "Stay inside the registered evaluation targets.",
            "retryable": false,
        }})),
        "{value}"
    );
    assert!(!value.to_string().contains("CANARY"), "{value}");
    assert!(results.files.export().unwrap().is_empty());
    assert!(!results.files.contains_bytes(b"CANARY"));
    assert!(!results.files.contains_bytes(b"PNG"));
    assert!(results.images.get("i_private").is_none());
}

#[tokio::test]
async fn rejected_script_publication_has_one_refusal_for_exceptions_and_timeout() {
    for loss in ["expiry", "rebind", "revoke"] {
        for (code, message) in [
            (ErrorCode::Internal, "CANARY-STATE-TEXT"),
            (ErrorCode::DriverError, "CANARY-FORGED-NATIVE"),
            (ErrorCode::StoppedByUser, "CANARY-FORGED-STOP"),
            (ErrorCode::Internal, "exception"),
            (ErrorCode::Timeout, "timeout"),
        ] {
            let clock = std::sync::Arc::new(HeldClock::default());
            let scope = timed_lease(clock.clone());
            let authority = scope.arm_attempt().unwrap();
            let results = desk(Some(scope.clone()));
            let stamp = authority.enter(async { results.opened_stamp() }).await;
            let gate = scope.commit_before_gate();
            gate.arm();
            let watcher = std::thread::spawn({
                let scope = scope.clone();
                let gate = gate.clone();
                move || {
                    let _release = ReleaseGate(&gate);
                    wait_parked(&gate);
                    match loss {
                        // No timer/watch publication: only advance the clock.
                        "expiry" => clock.expire(),
                        "rebind" => scope
                            .rebind(
                                FixtureScope::try_new(
                                    "run-s1",
                                    2,
                                    "engine",
                                    "desktop",
                                    "lease",
                                    vec![],
                                )
                                .unwrap(),
                            )
                            .unwrap(),
                        "revoke" => scope.revoke(),
                        _ => unreachable!(),
                    }
                }
            });
            let mut error = error(code);
            error.message = message.into();
            error.state = Some(shot_state(false).state);
            // Payload errors stage spill files; content-free errors/timeouts
            // reach the same refusal without needing any spill file at all.
            let output = if message == "exception" || message == "timeout" {
                String::new()
            } else {
                format!("CANARY-OUTPUT {}", "x".repeat(30_000))
            };
            let result = authority
                .enter(results.script(&output, Some(error.into()), stamp))
                .await;
            watcher.join().unwrap();
            assert_only_scope_denied(&results, &result);
            results.files.close().unwrap();
        }
    }
}

#[tokio::test]
async fn deadline_crossing_inside_commit_wait_discards_images_and_script_errors() {
    for script in [false, true] {
        let clock = std::sync::Arc::new(HeldClock::default());
        let scope = timed_lease(clock.clone());
        let authority = scope.arm_attempt().unwrap();
        let results = desk(Some(scope.clone()));
        let stamp = authority.enter(async { results.opened_stamp() }).await;
        let gate = scope.commit_during_gate();
        gate.arm();
        let watcher = std::thread::spawn({
            let gate = gate.clone();
            move || {
                let _release = ReleaseGate(&gate);
                wait_parked(&gate);
                clock.expire();
            }
        });
        let result = if script {
            let mut error = error(ErrorCode::Internal);
            error.message = "CANARY-COMMIT-WAIT".into();
            authority
                .enter(results.script("CANARY-OUTPUT", Some(error.into()), stamp))
                .await
        } else {
            authority.enter(results.state(shot_state(true))).await
        };
        watcher.join().unwrap();
        assert_only_scope_denied(&results, &result);
        results.files.close().unwrap();
    }
}

#[tokio::test]
async fn unscoped_script_emit_then_forged_scope_denial_keeps_output() {
    let results = desk(None);
    let mut error = error(ErrorCode::AppDenied);
    error.message = gcu_core::scope::SCOPE_DENIED_MESSAGE.into();
    let result = results
        .script(
            "EMITTED-BEFORE-THROW",
            Some(error.into()),
            result_guard::LiveStamp::off(),
        )
        .await;
    let rendered = serde_json::to_string(&result).unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(rendered.contains("EMITTED-BEFORE-THROW"), "{rendered}");
    assert_eq!(
        result.structured_content.as_ref().unwrap()["output"],
        "EMITTED-BEFORE-THROW"
    );
    assert_eq!(
        result.structured_content.as_ref().unwrap()["error"]["message"],
        gcu_core::scope::SCOPE_DENIED_MESSAGE
    );
    assert!(results.files.export().unwrap().is_empty());
    results.files.close().unwrap();
}

#[tokio::test]
async fn native_delivery_evidence_survives_a_later_script_exception_and_stop_takes_priority() {
    for stop in [false, true] {
        let scope = lease();
        let activity = std::sync::Arc::new(gcu_host_api::testing::FakeUserActivity::new());
        let results = desk_with_activity(Some(scope.clone()), activity.clone());
        let stamp = results.opened_stamp();
        let mut exception = error(ErrorCode::DriverError);
        exception.message = "CANARY-FORGED-LATER-EXCEPTION".into();
        exception.state = Some(shot_state(false).state);
        let mut native = error(ErrorCode::DriverError);
        native.message = "evaluation scope changed during delivery".into();
        let native_message = native.message.clone();
        scope.revoke();
        if stop {
            activity.request_stop();
            results.session.handle_stop().await;
        }
        let result = results
            .script(
                "CANARY-OUTPUT",
                Some(crate::ScriptError::NativeDelivery {
                    script: Some(exception),
                    native,
                }),
                stamp,
            )
            .await;
        let value = serde_json::to_value(&result).unwrap();
        assert!(!value.to_string().contains("CANARY"), "{value}");
        let data = result.structured_content.as_ref().unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(data.as_object().unwrap().len(), 1);
        assert!(data["error"].get("state").is_none());
        if stop {
            assert_eq!(data["error"]["code"], "stopped_by_user");
        } else {
            assert_eq!(data["error"]["code"], "driver_error");
            assert_eq!(data["error"]["message"], native_message);
        }
        assert_eq!(result.content.len(), 1);
        assert!(result.content[0].as_text().is_some());
        assert!(results.files.export().unwrap().is_empty());
        results.files.close().unwrap();
    }
}
