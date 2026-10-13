//! Safe image delivery, private-file fallback, and Grok's text prefix budget.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;
use base64::{engine::general_purpose::STANDARD, Engine};
use gcu_driver_fake::{scene, FakeDriver};
use gcu_host_api::ApprovalDecision;
use serde_json::{json, Value};
use support::{text, Harness};

fn image_path(r: &Value) -> String {
    text(r)
        .lines()
        .find_map(|line| line.strip_prefix("Screenshot file: "))
        .unwrap()
        .into()
}
#[tokio::test]
async fn extremely_thin_window_uses_file_fallback_when_enlargement_is_unsafe() {
    let fixture = scene::builtin("form")
        .unwrap()
        .replace("w: 800.0, h: 480.0", "w: 4096.0, h: 1.0");
    let mut h = Harness::driver(
        FakeDriver::from_ron(&fixture).unwrap(),
        &[ApprovalDecision::Session],
    );
    h.initialize(false).await;
    let r = h.call("get_app_state", json!({"app":"fake.form"})).await;
    assert_ne!(r["isError"], true, "{r}");
    assert!(r["content"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["type"] != "image"));
    assert!(text(&r).contains("attachment omitted"));
    let path = image_path(&r);
    assert!(!std::fs::read(&path).unwrap().is_empty());
    h.close().await;
    assert!(!std::path::Path::new(&path).exists());
}
#[tokio::test]
async fn small_screenshot_is_enlarged_and_private_file_is_removed_on_eof() {
    let fixture = scene::builtin("form")
        .unwrap()
        .replace("w: 800.0, h: 480.0", "w: 160.0, h: 96.0");
    let mut h = Harness::driver(
        FakeDriver::from_ron(&fixture).unwrap(),
        &[ApprovalDecision::Session],
    );
    h.initialize(false).await;
    let r = h.call("get_app_state", json!({"app":"fake.form"})).await;
    assert_ne!(r["isError"], true, "{r}");
    let img = r["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["type"] == "image")
        .unwrap();
    let bytes = STANDARD.decode(img["data"].as_str().unwrap()).unwrap();
    assert!(bytes.len() <= 300_000);
    let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
    let png = decoder.read_info().unwrap();
    assert!(png.info().width.min(png.info().height) >= 256);
    let path = image_path(&r);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(std::path::Path::new(&path).parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    let state = &r["structuredContent"]["state"];
    assert_eq!(state["screenshot"]["width"], png.info().width);
    // A point over the text field, expressed in the enlarged screenshot pixels.
    let x = 40. * f64::from(png.info().width) / 160.;
    let click = h
        .call(
            "click",
            json!({"app":"fake.form","x":x,"y":x,"screenshot_id":state["screenshot"]["id"]}),
        )
        .await;
    assert_ne!(click["isError"], true, "{click}");
    h.close().await;
    assert!(!std::path::Path::new(&path).exists());
}

#[tokio::test]
async fn oversized_text_is_spilled_and_compact_facts_fit_twenty_kilobytes() {
    let mut children = (0..90).map(|i|format!("(id: \"b{i}\", role: button, states: \"ENABLED\", name: Some(\"{i}{}\"), actions: [press])","界".repeat(200))).collect::<Vec<_>>().join(",");
    children.push_str(r#",(id:"secret",role:text_field,states:"PASSWORD | ENABLED",name:Some("secret-name"),value:Some(text("secret-value")),frame:Some((x:1.0,y:1.0,w:10.0,h:10.0)))"#);
    let fixture = format!("(apps:[(id:\"fake.long\",name:\"Long\",executable:\"/fixtures/long\",pid:1001,windows:[(id:\"long\",title:\"Long\",frame:(x:0.0,y:0.0,w:800.0,h:480.0),scale:1.0,root:Some((id:\"root\",role:window,states:\"ENABLED\",name:Some(\"Long\"),children:[{children}])) )])])");
    let mut h = Harness::driver(
        FakeDriver::from_ron(&fixture).unwrap(),
        &[ApprovalDecision::Session],
    );
    h.initialize(false).await;
    let r = h
        .call(
            "get_app_state",
            json!({"app":"fake.long","include_screenshot":false,"text_limit":"max"}),
        )
        .await;
    assert_ne!(r["isError"], true, "{r}");
    let text = text(&r);
    assert!(text.len() <= 19_000);
    assert!(text.len() + serde_json::to_vec(&r["structuredContent"]).unwrap().len() <= 20_000);
    assert!(text.contains("[truncated"));
    let path = text
        .split("full text: ")
        .nth(1)
        .unwrap()
        .split(']')
        .next()
        .unwrap()
        .to_string();
    let full = std::fs::read_to_string(&path).unwrap();
    assert!(full.len() > 19_000);
    assert!(!full.contains("secret-name"));
    assert!(!full.contains("secret-value"));
    assert!(
        full.contains(&"界".repeat(200)),
        "max field limit must survive in the file"
    );
    assert!(r["structuredContent"].get("elements").is_none());
    assert!(serde_json::to_vec(&r["structuredContent"]).unwrap().len() < 1000);
    h.close().await;
    assert!(!std::path::Path::new(&path).exists());
}
