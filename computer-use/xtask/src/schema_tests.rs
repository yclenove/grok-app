use super::schema::{budget_report, canonical_json, enforce_budget, golden, profile_catalogs};
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
    let files = [
        "tools.json",
        "errors.json",
        "profiles/minimal.json",
        "profiles/default.json",
        "profiles/full.json",
    ]
    .map(|name| (name, fs::read(temp.0.join(name)).unwrap()));
    golden(&temp.0, true).unwrap();
    for (name, expected) in files {
        assert_eq!(expected, fs::read(temp.0.join(name)).unwrap());
    }
}

#[test]
fn golden_check_tampered_file_fails_without_rewriting() {
    let temp = TempDir::new();
    golden(&temp.0, true).unwrap();
    for file in [
        "tools.json",
        "tools-browser.json",
        "errors.json",
        "profiles/minimal.json",
        "profiles/default.json",
        "profiles/full.json",
    ] {
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
fn golden_check_missing_profile_fails_without_regenerating() {
    let temp = TempDir::new();
    golden(&temp.0, true).unwrap();
    let path = temp.0.join("profiles/full.json");
    fs::remove_file(&path).unwrap();
    assert!(golden(&temp.0, false)
        .unwrap_err()
        .to_string()
        .contains("profiles/full.json"));
    assert!(!path.exists());
}

#[test]
fn budget_default_surface_is_within_exact_utf8_limit() {
    let (_, tools) = profile_catalogs()
        .into_iter()
        .find(|(name, _)| *name == "default")
        .unwrap();
    let report = budget_report(&tools, 14000).unwrap();
    assert!(report.passed());
    assert_eq!(report.total, serde_json::to_vec(&tools).unwrap().len());
    assert_eq!(report.tools.len(), 15);
}

#[test]
fn budget_all_profiles_including_enabled_script_fit_same_limit() {
    for ((name, tools), count) in profile_catalogs().into_iter().zip([9, 15, 16]) {
        let report = budget_report(&tools, 14_000).unwrap();
        assert!(report.passed(), "{name}: {}", report.render());
        assert_eq!(report.tools.len(), count);
        assert!(tools
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool["name"] != "browser_navigate"));
    }
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
