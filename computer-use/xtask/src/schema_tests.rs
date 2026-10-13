use super::schema::{budget_report, canonical_json, enforce_budget, golden};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "gcu-golden-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn canonical_json_nested_keys_are_sorted_indented_and_newline_terminated() {
    assert_eq!(
        canonical_json(&json!({"z":[{"b":1,"a":2}],"a":true})).unwrap(),
        "{\n  \"a\": true,\n  \"z\": [\n    {\n      \"a\": 2,\n      \"b\": 1\n    }\n  ]\n}\n"
    );
}

#[test]
fn golden_update_then_check_passes_and_update_is_idempotent() {
    let temp = TempDir::new();
    golden(&temp.0, true).unwrap();
    golden(&temp.0, false).unwrap();
    let tools = fs::read(temp.0.join("tools.json")).unwrap();
    golden(&temp.0, true).unwrap();
    assert_eq!(tools, fs::read(temp.0.join("tools.json")).unwrap());
}

#[test]
fn golden_check_tampered_file_fails_without_rewriting() {
    let temp = TempDir::new();
    golden(&temp.0, true).unwrap();
    for file in ["tools.json", "errors.json"] {
        let path = temp.0.join(file);
        fs::write(&path, "{}\n").unwrap();
        assert!(golden(&temp.0, false)
            .unwrap_err()
            .to_string()
            .contains(file));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
        golden(&temp.0, true).unwrap();
    }
}

#[test]
fn golden_check_missing_file_fails() {
    let temp = TempDir::new();
    assert!(golden(&temp.0, false).is_err());
}

#[test]
fn budget_default_surface_is_within_exact_utf8_limit() {
    let tools = gcu_protocol::tools_list_json();
    let report = budget_report(&tools, 14000).unwrap();
    assert!(report.passed());
    assert_eq!(report.total, serde_json::to_vec(&tools).unwrap().len());
    assert_eq!(report.tools.len(), 16);
}

#[test]
fn budget_over_limit_reports_each_tool_and_counts_utf8_bytes() {
    let tools = json!([{"name":"first","description":"中"}, {"name":"second"}]);
    let size = serde_json::to_vec(&tools).unwrap().len();
    let report = budget_report(&tools, size - 1).unwrap();
    assert!(!report.passed());
    assert!(enforce_budget(&report).is_err());
    assert_eq!(report.tools.len(), 2);
    assert_eq!(
        report.tools[0].1,
        serde_json::to_vec(&tools[0]).unwrap().len()
    );
    assert!(report.render().contains("first:"));
    assert!(report.render().contains("second:"));
    assert!(enforce_budget(&budget_report(&tools, size).unwrap()).is_ok());
    assert!(budget_report(&json!({}), size).is_err());
}
