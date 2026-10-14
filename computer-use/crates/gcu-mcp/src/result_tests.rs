//! Boundary checks for safe result encoding and resource lifetime.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use gcu_core::{
    action::ActionRouter,
    driver::Capture,
    model::Geometry,
    redact::{self, CapturedFrame},
    session::WindowLocks,
    tree::ProcessedTree,
};
use gcu_driver_fake::FakeDriver;
use gcu_host_api::testing::{
    FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals,
};
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
