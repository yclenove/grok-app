//! `GCU_LOG_PAYLOADS` gate. The flag is not read from the environment here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_host_protocol::approval::CropView;
use gcu_host_protocol::inject::{crash_report_enabled, remember_secret};
use gcu_host_protocol::log_policy::{
    error_record, format_bytes, format_text, payloads_enabled_value,
};
use gcu_host_protocol::messages::Image;
use std::path::Path;

fn token(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

#[test]
fn payloads_flag_accepts_only_exact_one() {
    assert!(!payloads_enabled_value(None));
    assert!(!payloads_enabled_value(Some("")));
    assert!(!payloads_enabled_value(Some("0")));
    assert!(!payloads_enabled_value(Some("true")));
    assert!(!payloads_enabled_value(Some("1 ")));
    assert!(!payloads_enabled_value(Some("01")));
    assert!(payloads_enabled_value(Some("1")));
}

#[test]
fn payloads_disabled_omits_text_tree_error_and_token() {
    let secret = token(0x51);
    let tree =
        format!(r#"{{"name":"Save button","password":"{secret}","text":"type hunter2-secret"}}"#);
    let hidden = format_text(false, "tree", &tree);
    assert_eq!(hidden, format!("tree bytes={}", tree.len()));
    assert!(!hidden.contains("Save button"));
    assert!(!hidden.contains("hunter2-secret"));
    assert!(!hidden.contains(&secret));
    assert!(!hidden.contains("type "));
    let bytes = format_bytes("capture", secret.len());
    assert_eq!(bytes, format!("capture bytes={}", secret.len()));
    assert!(!bytes.contains(&secret));
    let error = error_record("src/app.rs:10:2", "panic_str");
    assert_eq!(error, "category=panic_str location=src/app.rs:10:2");
    assert!(!error.contains("hunter2-secret"));
    let crash = crash_report_enabled(false, "driver", &format!("input={secret} original-error"));
    assert!(!crash.contains(&secret));
    assert!(!crash.contains("original-error"));
    assert!(crash.contains("category="));
    assert!(crash.contains("location="));
}

#[test]
fn payloads_enabled_still_redacts_secrets_passwords_and_tokens() {
    let secret = token(0x52);
    remember_secret(&secret);
    let body = format!(
        r#"click Save {{"password":"hunter2-secret","token":"{secret}","note":"keep-visible"}} password={secret}"#
    );
    let shown = format_text(true, "tool", &body);
    assert!(shown.contains("click Save"));
    assert!(shown.contains("keep-visible"));
    assert!(shown.starts_with("tool "));
    assert!(!shown.contains("hunter2-secret"));
    assert!(!shown.contains(&secret));
    assert!(shown.contains("[redacted]"));
    assert!(!format_bytes("capture", 4).contains("hunter2-secret"));
    let crash = crash_report_enabled(
        true,
        "died",
        &format!(r#"GCU_TOKEN={secret} password=hunter2-secret keep-visible"#),
    );
    assert!(!crash.contains(&secret));
    assert!(!crash.contains("hunter2-secret"));
    assert!(!crash.contains("GCU_TOKEN="));
    assert!(crash.contains("keep-visible"));
    assert!(crash.contains("[redacted]"));
}

#[test]
fn wire_image_and_crop_debug_omit_bytes() {
    let secret = b"SUPER-SECRET-PIXELS-99".to_vec();
    let image = Image {
        mime: "image/png".into(),
        width: 3,
        height: 4,
        bytes: secret.clone(),
    };
    let rendered = format!("{image:?}");
    assert!(!rendered.contains("SUPER-SECRET-PIXELS-99"));
    assert!(rendered.contains("byte_count"));
    assert!(rendered.contains("image/png"));
    let crop = CropView::Redacted {
        mime: "image/png".into(),
        width: 3,
        height: 4,
        bytes: secret,
    };
    let rendered = format!("{crop:?}");
    assert!(!rendered.contains("SUPER-SECRET-PIXELS-99"));
    assert!(rendered.contains("byte_count"));
}

#[test]
fn gcu_log_payloads_env_read_is_only_in_log_policy() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = crate_dir.join("../..");
    let mut hits = Vec::new();
    walk(&workspace, &mut hits);
    let hits: Vec<_> = hits
        .into_iter()
        .map(|path| path.canonicalize().unwrap())
        .collect();
    let expected = crate_dir.join("src/log_policy.rs").canonicalize().unwrap();
    assert_eq!(hits, vec![expected]);
}

fn walk(dir: &Path, hits: &mut Vec<std::path::PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap();
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str());
        if name == Some("target") || name == Some("tests") {
            continue;
        }
        if path.is_dir() {
            walk(&path, hits);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        if source.contains("env::var(\"GCU_LOG_PAYLOADS\")")
            || source.contains("env::var_os(\"GCU_LOG_PAYLOADS\")")
        {
            hits.push(path);
        }
    }
}
