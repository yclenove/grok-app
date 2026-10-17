//! App-suite and section 8 release-gate tests for the runner binary.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::{
    absent_report, judge_release, parse_release_evidence, prove_oracle, EvidenceSource,
    LayerStatus, PlatformRates, ReleaseEvidence, ReleaseTier, World,
};
use std::collections::BTreeMap;
use std::process::Command;

const APP_TASKS: [&str; 6] = [
    "app-approval-01",
    "app-image-session-01",
    "app-permission-01",
    "app-ssh-inject-01",
    "app-stop-01",
    "app-switch-01",
];

fn gate_rates(native: u16, text: u16, overall: u16) -> PlatformRates {
    PlatformRates {
        native_bp: native,
        text_bp: text,
        overall_bp: overall,
    }
}

fn gate_layers(status: LayerStatus) -> BTreeMap<String, BTreeMap<String, LayerStatus>> {
    let mut all = BTreeMap::new();
    for platform in ["macos", "windows", "linux"] {
        let mut rows = BTreeMap::new();
        for index in 1..=7 {
            rows.insert(format!("L{index}"), status);
        }
        all.insert(platform.to_string(), rows);
    }
    all
}

fn formal_evidence(source: EvidenceSource) -> ReleaseEvidence {
    let mut l6 = BTreeMap::new();
    for platform in ["macos", "windows", "linux"] {
        l6.insert(platform.to_string(), gate_rates(9_000, 9_000, 9_000));
    }
    ReleaseEvidence {
        source,
        layers: gate_layers(LayerStatus::Pass),
        l6,
        security_signed: vec![true; 10],
        screenshot_chain_redacted: true,
        unreliable_screenshot_refused: true,
        ide_unknown_stop_regression: true,
        performance_budget_met: true,
        compatibility_apps: 30,
        signed_and_notarized: true,
        red_team_violations: 0,
    }
}

#[test]
fn app_suite_covers_host_gates_and_not_the_core_suite() {
    let loaded = super::tasks();
    for id in APP_TASKS {
        let task = loaded.iter().find(|item| item.id == id).expect(id);
        assert_eq!(task.category, "app", "{id}");
        assert_eq!(task.suites, ["app".to_string()], "{id}");
        let pass = World::from_script(&std::fs::read(&task.pass_script).unwrap()).unwrap();
        let fail = World::from_script(&std::fs::read(&task.fail_script).unwrap()).unwrap();
        let partial = World::from_script(&std::fs::read(&task.partial_script).unwrap()).unwrap();
        prove_oracle(&task.oracle, &pass, &fail, &partial).unwrap();
    }
    assert!(loaded
        .iter()
        .filter(|task| task.suites.iter().any(|suite| suite == "core"))
        .all(|task| task.category != "app"));
}

#[test]
fn release_gate_keeps_experimental_until_runner_meets_section_8() {
    let missing = absent_report();
    assert!(!missing.promote && !missing.default_formal);
    assert_eq!(missing.tier, ReleaseTier::Experimental);
    assert_eq!(missing.reasons, ["no eval runner report"]);

    let bench = judge_release(&formal_evidence(EvidenceSource::Bench));
    assert_eq!(bench.tier, ReleaseTier::Experimental);
    assert!(!bench.promote && !bench.default_formal);
    assert_eq!(
        bench.reasons,
        ["performance data cannot replace the eval runner"]
    );

    let mut partial = formal_evidence(EvidenceSource::Runner);
    partial
        .layers
        .get_mut("macos")
        .unwrap()
        .insert("L4".into(), LayerStatus::Partial);
    let partial = judge_release(&partial);
    assert_eq!(partial.tier, ReleaseTier::Experimental);
    assert!(partial.reasons.iter().any(|item| item.contains("L1-L4")));

    let mut skipped = formal_evidence(EvidenceSource::Runner);
    skipped
        .layers
        .get_mut("linux")
        .unwrap()
        .insert("L7".into(), LayerStatus::NotRun);
    let skipped = judge_release(&skipped);
    assert_eq!(skipped.tier, ReleaseTier::Beta);
    assert!(!skipped.promote && !skipped.default_formal);
    assert!(skipped.reasons.iter().any(|item| item.contains("L6-L7")));

    let mut preview = formal_evidence(EvidenceSource::Runner);
    preview
        .l6
        .insert("macos".into(), gate_rates(7_000, 1_000, 9_000));
    preview.security_signed = vec![true; 6];
    preview.performance_budget_met = false;
    preview.compatibility_apps = 0;
    preview.signed_and_notarized = false;
    let preview = judge_release(&preview);
    assert_eq!(preview.tier, ReleaseTier::Alpha);
    assert!(!preview.default_formal);

    let mut low = formal_evidence(EvidenceSource::Runner);
    low.l6.get_mut("macos").unwrap().native_bp = 6_999;
    assert_eq!(judge_release(&low).tier, ReleaseTier::Experimental);

    let mut overall = formal_evidence(EvidenceSource::Runner);
    overall.l6.get_mut("macos").unwrap().overall_bp = 8_499;
    assert_eq!(judge_release(&overall).tier, ReleaseTier::Beta);
    overall.l6.get_mut("macos").unwrap().overall_bp = 8_500;
    overall.l6.get_mut("windows").unwrap().overall_bp = 7_499;
    assert_eq!(judge_release(&overall).tier, ReleaseTier::Beta);
    overall.l6.get_mut("windows").unwrap().overall_bp = 7_500;
    overall.l6.get_mut("linux").unwrap().overall_bp = 7_499;
    assert_eq!(judge_release(&overall).tier, ReleaseTier::Beta);
    overall.l6.get_mut("linux").unwrap().overall_bp = 7_500;
    let mut matrix = overall.clone();
    matrix.compatibility_apps = 29;
    assert!(!judge_release(&matrix).promote);
    matrix.compatibility_apps = 30;
    matrix.security_signed = vec![true; 6];
    assert_eq!(judge_release(&matrix).tier, ReleaseTier::Beta);

    let mut violations = formal_evidence(EvidenceSource::Runner);
    violations.red_team_violations = 1;
    violations.security_signed = vec![true; 6];
    violations.performance_budget_met = false;
    violations.compatibility_apps = 0;
    violations.signed_and_notarized = false;
    assert_eq!(judge_release(&violations).tier, ReleaseTier::Alpha);

    let mut budget_only = formal_evidence(EvidenceSource::Runner);
    budget_only.layers.clear();
    budget_only.l6.clear();
    budget_only.security_signed.clear();
    budget_only.screenshot_chain_redacted = false;
    budget_only.unreliable_screenshot_refused = false;
    budget_only.ide_unknown_stop_regression = false;
    budget_only.compatibility_apps = 0;
    budget_only.signed_and_notarized = false;
    budget_only.red_team_violations = 1;
    let budget_only = judge_release(&budget_only);
    assert!(!budget_only.promote && !budget_only.default_formal);
    assert_eq!(budget_only.tier, ReleaseTier::Experimental);
    assert!(budget_only
        .reasons
        .iter()
        .any(|item| item.contains("L1-L4")));

    let mut invalid = formal_evidence(EvidenceSource::Runner);
    invalid.l6.get_mut("macos").unwrap().native_bp = 10_001;
    assert_eq!(
        judge_release(&invalid).reasons,
        ["release evidence is invalid"]
    );

    let formal = judge_release(&formal_evidence(EvidenceSource::Runner));
    assert_eq!(formal.tier, ReleaseTier::Formal);
    assert!(formal.promote && formal.default_formal);
    assert!(formal.reasons.is_empty());
}

#[test]
fn cli_app_suite_is_not_run_and_release_gate_writes_a_report() {
    let desktop = Command::new(super::eval_bin())
        .args(["--suite", "app", "--env", "desktop"])
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&desktop.stdout);
    assert_eq!(
        desktop.status.code(),
        Some(0),
        "{stdout} {}",
        String::from_utf8_lossy(&desktop.stderr)
    );
    for id in APP_TASKS {
        assert!(stdout.contains(&format!("not_run {id}")), "{stdout}");
    }
    assert!(stdout.contains("not_run desktop-oneshot"));
    assert!(!stdout.contains("pass "));

    let dir = super::scratch("gate");
    let bench_path = dir.join("bench.json");
    std::fs::write(
        &bench_path,
        serde_json::to_vec(&formal_evidence(EvidenceSource::Bench)).unwrap(),
    )
    .unwrap();
    let bench = Command::new(super::eval_bin())
        .arg("--release-gate")
        .arg(&bench_path)
        .arg("--out")
        .arg(dir.join("bench-out"))
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let bench_out = String::from_utf8_lossy(&bench.stdout);
    assert_eq!(bench.status.code(), Some(0), "{bench_out}");
    assert!(bench_out.contains("tier=experimental"));
    assert!(bench_out.contains("promote=false"));
    assert!(bench_out.contains("default_formal=false"));
    assert!(bench_out.contains("performance data cannot replace the eval runner"));
    let report = std::fs::read_to_string(dir.join("bench-out").join("release-gate.json")).unwrap();
    assert!(report.contains("\"channel\": \"experimental\""));
    assert!(report.contains("\"default_formal\": false"));
    assert!(!report.contains("prompt"));

    let formal_path = dir.join("formal.json");
    std::fs::write(
        &formal_path,
        serde_json::to_vec(&formal_evidence(EvidenceSource::Runner)).unwrap(),
    )
    .unwrap();
    let formal = Command::new(super::eval_bin())
        .arg("--release-gate")
        .arg(&formal_path)
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    let formal_out = String::from_utf8_lossy(&formal.stdout);
    assert_eq!(formal.status.code(), Some(0), "{formal_out}");
    assert!(formal_out.contains("tier=formal"));
    assert!(formal_out.contains("promote=true"));
    assert!(formal_out.contains("default_formal=true"));

    let junk = Command::new(super::eval_bin())
        .arg("--release-gate")
        .arg(dir.join("missing.json"))
        .env_remove("GCU_APPROVAL")
        .output()
        .unwrap();
    assert_eq!(junk.status.code(), Some(2));
    let bytes = serde_json::to_vec(&formal_evidence(EvidenceSource::Runner)).unwrap();
    let parsed = parse_release_evidence(&bytes).unwrap();
    assert!(parse_release_evidence(br#"{"source":"runner","promote":true}"#).is_err());
    assert_eq!(parsed.source, EvidenceSource::Runner);
    let _ = std::fs::remove_dir_all(&dir);
}
