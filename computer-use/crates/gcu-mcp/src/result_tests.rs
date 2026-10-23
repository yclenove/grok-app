//! Boundary checks for safe result encoding and resource lifetime.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use gcu_core::{
    action::{ActionRouter, ActionState, ToolOutcome},
    driver::Capture,
    model::Geometry,
    redact::{self, CapturedFrame},
    scope::{FixtureScope, ScopeLease},
    session::WindowLocks,
    tree::ProcessedTree,
};
use gcu_driver_fake::FakeDriver;
use gcu_host_api::testing::{
    FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals,
};
use gcu_protocol::result::{ActionResult, Delivery, Effect, Outcome, Settle, SettleReason};
use gcu_protocol::{AppInfo, AppState, ErrorCode, Rect, ToolError, ToolName, WindowInfo};
use std::time::Duration;

fn results() -> Results {
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
            "encoding",
            Arc::new(FakeUserActivity::new()),
            router.input_release(),
        )),
        scope: None,
    }
}
#[tokio::test]
async fn oversized_facts_preserve_error_and_fresh_snapshot_in_prefix() {
    let r = results();
    let result = r.finish("error: stale_element: state changed".into(), json!({
        "error":{"code":"stale_element","message":"state changed","state":{"snapshot_id":"s_7"}},
        "action":{"outcome":"refused"},"large":"界".repeat(9000)
    }), Vec::new(), true).await.unwrap();
    assert_eq!(result.is_error, Some(true));
    let facts = result.structured_content.unwrap();
    assert_eq!(facts["error"]["state"]["snapshot_id"], "s_7");
    assert_eq!(facts["error"]["message"], "state changed");
    assert_eq!(facts["action"]["outcome"], "refused");
    let path = facts["details_file"].as_str().unwrap();
    assert!(std::fs::read(path).unwrap().len() > 8_000);
    assert!(serde_json::to_vec(&facts).unwrap().len() < 1000);
    r.files.close().unwrap();
    assert!(!std::path::Path::new(path).exists());
}
#[tokio::test]
async fn long_discovery_spills_and_closed_file_area_returns_safe_error() {
    let r = results();
    let result = r.discovery(json!({"apps":["界".repeat(10_000)]})).await;
    let value = serde_json::to_value(result).unwrap();
    let text = value["content"][0]["text"].as_str().unwrap();
    assert!(text.len() <= TEXT_LIMIT);
    let path = text
        .split("full text: ")
        .nth(1)
        .unwrap()
        .split(']')
        .next()
        .unwrap();
    assert!(std::fs::read_to_string(path)
        .unwrap()
        .contains(&"界".repeat(10_000)));
    r.files.close().unwrap();
    let error =
        serde_json::to_value(r.discovery(json!({"long":"x".repeat(20_000)})).await).unwrap();
    assert_eq!(error["isError"], true);
    assert_eq!(error["structuredContent"]["error"]["code"], "internal");
    assert!(r.files.write(b"no", "txt").is_err());
    r.files.close().unwrap();
}
#[tokio::test]
async fn noisy_large_redacted_image_is_reduced_before_private_png_delivery() {
    let r = results();
    let (width, height) = (1024u32, 768u32);
    let mut random = 42u32;
    let pixels: Vec<u8> = (0..width * height)
        .flat_map(|_| {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            let b = random.to_le_bytes();
            [b[0], b[1], b[2], 255]
        })
        .collect();
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
    assert!(bytes.len() > 1_500_000);
    let geometry = Geometry {
        frame: gcu_protocol::Rect {
            x: 0.,
            y: 0.,
            w: width as f64,
            h: height as f64,
        },
        scale: 1.,
    };
    let mut tree = ProcessedTree::default();
    tree.state_version = Some(1);
    tree.observed_at = Some(Duration::ZERO);
    tree.sensitive_regions_reliable = true;
    let image = redact::apply(
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
    .unwrap();
    let (encoded, path) = r.image(image.clone()).await.unwrap();
    assert!(encoded.reduced());
    assert!(encoded.bytes().len() <= 300_000);
    assert!(encoded.width() < width && encoded.height() < height);
    assert_eq!(std::fs::read(&path).unwrap(), encoded.bytes());
    r.files.close().unwrap();
    assert!(r.image(image).await.is_err());
}

fn scope_lease(generation: u64) -> ScopeLease {
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

fn app_state(snapshot_id: &str) -> AppState {
    AppState {
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
                w: 64.,
                h: 64.,
            },
            scale: 1.,
            on_current_desktop: Some(true),
        },
        snapshot_id: snapshot_id.into(),
        screenshot: None,
        focused: None,
        truncated: None,
        elements: None,
        action: None,
    }
}

fn solid_image() -> gcu_core::redact::RedactedImage {
    let (width, height) = (256u32, 256u32);
    let pixels = [255u8, 0, 0, 255].repeat((width * height) as usize);
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
            w: width as f64,
            h: height as f64,
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

fn unknown_outcome(state: Option<ActionState>, error_state: Option<AppState>) -> ToolOutcome {
    ToolOutcome {
        action: ActionResult {
            tool: ToolName::Click,
            target: None,
            delivery: Delivery::Accessibility,
            outcome: Outcome::Unknown,
            effect: Effect::NotChecked,
            verify: None,
            duration_ms: 4,
            settle: Settle {
                ms: 0,
                reason: SettleReason::Budget,
            },
            notes: vec!["Execution is uncertain; do not automatically retry.".into()],
        },
        error: Some(ToolError {
            code: ErrorCode::DriverError,
            message: "evaluation scope changed during delivery".into(),
            hint: "Use fresh state.".into(),
            retryable: false,
            state: error_state,
        }),
        state,
        windows: Vec::new(),
    }
}

fn stamped_state(run: Option<&str>, generation: Option<u64>, image: bool) -> ActionState {
    ActionState {
        text: "CANARY-STALE observation".into(),
        overflow_text: Some("CANARY-STALE overflow".into()),
        state: app_state("s_9"),
        image: image.then(solid_image),
        scope_run: run.map(str::to_owned),
        scope_generation: generation,
    }
}

fn scoped_results(lease: ScopeLease) -> Results {
    let mut built = results();
    built.scope = Some(lease);
    built
}

async fn encoded(results: &Results, out: ToolOutcome) -> (String, serde_json::Value) {
    let result = results.outcome(out).await;
    let value = serde_json::to_value(&result).unwrap();
    let text = value["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|block| block.get("text").and_then(|text| text.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    (text, value)
}

#[tokio::test]
async fn stale_epoch_unknown_drops_state_images_and_overflow_files() {
    let live = scope_lease(1);
    let kept = scoped_results(live.clone());
    let (text, value) = encoded(
        &kept,
        unknown_outcome(
            Some(stamped_state(Some("run-s1"), Some(1), true)),
            Some(app_state("CANARY-STALE-ERR")),
        ),
    )
    .await;
    assert!(text.contains("CANARY-STALE observation"), "{text}");
    assert!(text.contains("Screenshot file:"), "{text}");
    assert!(text.contains("full text:"), "{text}");
    assert_eq!(value["structuredContent"]["action"]["outcome"], "unknown");
    assert_eq!(
        value["structuredContent"]["error"]["code"], "driver_error",
        "{value}"
    );
    assert_eq!(
        value["structuredContent"]["error"]["state"]["snapshot_id"], "s_9",
        "fresh observation replaces the error state"
    );
    assert_eq!(value["structuredContent"]["state"]["snapshot_id"], "s_9");
    let kept_files = kept.files.export().unwrap();
    assert!(kept_files
        .iter()
        .any(|(_, bytes)| String::from_utf8_lossy(bytes).contains("CANARY-STALE overflow")));
    kept.files.close().unwrap();

    for revoke in [false, true] {
        let lease = scope_lease(1);
        if revoke {
            lease.revoke();
        } else {
            lease.rebind(scope_lease(2).current().unwrap()).unwrap();
        }
        let stale = scoped_results(lease);
        let before = stale.files.export().unwrap().len();
        let (text, value) = encoded(
            &stale,
            unknown_outcome(
                Some(stamped_state(Some("run-s1"), Some(1), true)),
                Some(app_state("CANARY-STALE-ERR")),
            ),
        )
        .await;
        let rendered = format!("{text} {value}");
        assert!(!rendered.contains("CANARY-STALE"), "{rendered}");
        assert!(!text.contains("Screenshot file:"), "{text}");
        assert!(!text.contains("full text:"), "{text}");
        assert!(value["content"]
            .as_array()
            .unwrap()
            .iter()
            .all(|block| block.get("type").and_then(|kind| kind.as_str()) != Some("image")));
        assert_eq!(value["structuredContent"]["action"]["outcome"], "unknown");
        assert_eq!(value["structuredContent"]["error"]["code"], "driver_error");
        assert_eq!(
            value["structuredContent"]["error"]["message"],
            "evaluation scope changed during delivery"
        );
        assert!(value["structuredContent"]["error"].get("state").is_none());
        assert!(value["structuredContent"].get("state").is_none());
        let files = stale.files.export().unwrap();
        assert_eq!(files.len(), before);
        assert!(files
            .iter()
            .all(|(_, bytes)| { !String::from_utf8_lossy(bytes).contains("CANARY-STALE") }));
        stale.files.close().unwrap();
    }

    let active = scoped_results(scope_lease(1));
    let (text, value) = encoded(
        &active,
        unknown_outcome(Some(stamped_state(None, None, false)), None),
    )
    .await;
    assert!(text.contains("CANARY-STALE observation"), "{text}");
    assert_eq!(value["structuredContent"]["state"]["snapshot_id"], "s_9");
    active.files.close().unwrap();

    let error_only = scoped_results(scope_lease(1));
    error_only.scope.as_ref().unwrap().revoke();
    let (text, value) = encoded(
        &error_only,
        unknown_outcome(None, Some(app_state("CANARY-STALE-ERR"))),
    )
    .await;
    assert!(!text.contains("CANARY-STALE"), "{text}");
    assert!(value["structuredContent"]["error"].get("state").is_none());
    assert_eq!(value["structuredContent"]["action"]["outcome"], "unknown");
    error_only.files.close().unwrap();

    let partial = scoped_results(scope_lease(1));
    let (text, value) = encoded(
        &partial,
        unknown_outcome(Some(stamped_state(Some("run-s1"), None, true)), None),
    )
    .await;
    assert!(!text.contains("CANARY-STALE"), "{text}");
    assert!(value["structuredContent"].get("state").is_none());
    assert!(partial.files.export().unwrap().is_empty());
    partial.files.close().unwrap();
}
