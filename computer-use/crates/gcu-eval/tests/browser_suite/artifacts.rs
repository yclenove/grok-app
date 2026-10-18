//! Temporary serialization probes; never native artifact measurements.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Bundle(PathBuf);

impl Bundle {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "gcu-browser-schema-probe-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("artifacts")).unwrap();
        Self(root)
    }
}

impl Drop for Bundle {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn file_artifacts_roundtrip_raw_schema_and_reject_empty_invalid_or_fake_evidence() {
    let (report, artifacts) = probe();
    let (_, raw) = artifacts.0.iter().next().unwrap();
    let bundle = Bundle::new();
    let relative = Path::new("artifacts/trial.json");
    let full = bundle.0.join(relative);
    let source = FileArtifacts::new(&bundle.0);
    std::fs::write(&full, serde_json::to_vec(raw).unwrap()).unwrap();
    let loaded = source.load(relative).unwrap().unwrap();
    assert_eq!(loaded.task_id, raw.task_id);
    assert_eq!(loaded.metadata, raw.metadata);
    std::fs::write(&full, b"").unwrap();
    assert!(source.load(relative).unwrap().is_none());
    std::fs::write(&full, b"{}").unwrap();
    assert!(source.load(relative).unwrap().is_none());
    let mut invalid = serde_json::to_value(raw).unwrap();
    invalid["schema_version"] = json!(2);
    std::fs::write(&full, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(source.load(relative).unwrap().is_none());
    invalid["schema_version"] = json!(1);
    invalid["recommend_l2"] = json!(true);
    std::fs::write(&full, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(source.load(relative).unwrap().is_none());
    invalid.as_object_mut().unwrap().remove("recommend_l2");
    invalid["kind"] = json!("fake");
    std::fs::write(&full, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert_eq!(
        source.load(relative).unwrap().unwrap().kind,
        EvidenceKind::Fake
    );
    assert_eq!(checked(&report, &source).status, ValidationStatus::NotRun);
    std::fs::write(&full, vec![b' '; 1_048_577]).unwrap();
    assert!(source.load(relative).unwrap().is_none());
}

#[cfg(unix)]
#[test]
fn file_artifact_symlink_is_unavailable_even_with_valid_raw_json() {
    let (_, artifacts) = probe();
    let (_, raw) = artifacts.0.iter().next().unwrap();
    let bundle = Bundle::new();
    let target = bundle.0.join("raw.json");
    std::fs::write(&target, serde_json::to_vec(raw).unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, bundle.0.join("artifacts/link.json")).unwrap();
    assert!(FileArtifacts::new(&bundle.0)
        .load(Path::new("artifacts/link.json"))
        .unwrap()
        .is_none());
}

#[test]
fn not_run_output_roundtrip_keeps_l0_and_escapes_html_reasons() {
    let bundle = Bundle::new();
    let report = browser::not_run_report();
    let mut result = checked(&report, &SyntheticArtifacts::default());
    result.reasons.push("<script>synthetic</script>".into());
    browser::write_report(&bundle.0, &report, &result).unwrap();
    let saved =
        browser::parse_report(&std::fs::read(bundle.0.join("browser-ab.json")).unwrap()).unwrap();
    assert_eq!(
        checked(&saved, &SyntheticArtifacts::default()).status,
        ValidationStatus::NotRun
    );
    let html = std::fs::read_to_string(bundle.0.join("browser-ab.html")).unwrap();
    assert!(html.contains("default=l0"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("<script>synthetic"));
}
