//! Synthetic reports test the gate only; they are never native measurement evidence.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_eval::budget::{
    budgets::{
        latency_budget, Operation, Platform, MIN_SAMPLES, SCHEMA_VERSION, TOOL_CATALOG_BYTES,
    },
    report::{
        check_reports, summarize_samples, BenchReport, Measurement, NativeEnvironment, Setup,
        Status,
    },
};
use serde_json::json;

fn reports() -> Vec<BenchReport> {
    Platform::ALL
        .into_iter()
        .map(|platform| BenchReport {
            schema_version: SCHEMA_VERSION,
            platform,
            kind: "native_wall_clock".into(),
            status: Status::Measured,
            environment: NativeEnvironment {
                machine: "SYNTHETIC TEST ONLY".into(),
                os_version: "test-os".into(),
                engine_revision: "test-engine".into(),
                driver_version: "test-driver".into(),
                fixture_revision: "test-fixture".into(),
                captured_at_utc: "2026-10-17T00:00:00Z".into(),
                subscription_route: "test-polling".into(),
            },
            setup: Setup::required(platform),
            measurements: Operation::ALL
                .into_iter()
                .map(|operation| {
                    let limit = latency_budget(platform, operation);
                    let mut samples_ms = vec![limit.p50_ms as f64; MIN_SAMPLES];
                    samples_ms[25..].fill(limit.p95_ms as f64);
                    Measurement {
                        operation,
                        percentiles: summarize_samples(&samples_ms).unwrap(),
                        samples_ms,
                    }
                })
                .collect(),
        })
        .collect()
}

#[test]
fn bench_report_at_each_platform_threshold_passes() {
    let checked = check_reports(&reports()).unwrap();
    assert_eq!(checked.len(), 12);
    for row in checked {
        let limit = latency_budget(row.platform, row.operation);
        assert_eq!(row.percentiles.p50_ms, limit.p50_ms as f64);
        assert_eq!(row.percentiles.p95_ms, limit.p95_ms as f64);
        assert_eq!(row.samples, MIN_SAMPLES);
    }
}

#[test]
fn bench_budget_limits_match_architecture_section_fourteen() {
    let expected = [
        (
            Platform::Macos,
            [(250, 600), (300, 900), (400, 1200), (150, 400)],
        ),
        (
            Platform::Windows,
            [(450, 900), (400, 1100), (600, 1500), (300, 800)],
        ),
        (
            Platform::LinuxX11,
            [(400, 900), (400, 1100), (600, 1500), (200, 500)],
        ),
    ];
    for (platform, limits) in expected {
        for (operation, (p50, p95)) in Operation::ALL.into_iter().zip(limits) {
            let limit = latency_budget(platform, operation);
            assert_eq!((limit.p50_ms, limit.p95_ms), (p50, p95));
        }
    }
    assert_eq!(MIN_SAMPLES, 50);
}

#[test]
fn bench_report_over_either_percentile_fails() {
    for platform in 0..3 {
        for operation in 0..4 {
            for tail in [false, true] {
                let mut reports = reports();
                let row = &mut reports[platform].measurements[operation];
                let range = if tail { 25..50 } else { 0..25 };
                for value in &mut row.samples_ms[range] {
                    *value += 0.1;
                }
                row.percentiles = summarize_samples(&row.samples_ms).unwrap();
                let error = check_reports(&reports).unwrap_err().to_string();
                assert!(error.contains("exceeds budget"), "{error}");
                assert!(error.contains(if tail { "p95" } else { "p50" }), "{error}");
            }
        }
    }
}

#[test]
fn bench_report_missing_any_platform_fails() {
    for missing in 0..3 {
        let mut reports = reports();
        let platform = reports.remove(missing).platform;
        let error = check_reports(&reports).unwrap_err().to_string();
        assert!(
            error.contains(&format!("missing platform {platform}")),
            "{error}"
        );
    }
    assert!(check_reports(&[]).is_err());
}

#[test]
fn bench_report_fewer_than_fifty_samples_fails() {
    for count in [0, 1, 49] {
        let mut reports = reports();
        reports[0].measurements[0].samples_ms.truncate(count);
        let error = check_reports(&reports).unwrap_err().to_string();
        assert!(error.contains("requires at least 50 samples"), "{error}");
    }
}

#[test]
fn bench_report_schema_errors_fail() {
    let value = serde_json::to_value(&reports()[0]).unwrap();
    for invalid in [json!({}), json!([]), json!({"schema_version": "1"})] {
        assert!(serde_json::from_value::<BenchReport>(invalid).is_err());
    }
    let mut unknown = value.clone();
    unknown["unrecognized"] = json!(true);
    assert!(serde_json::from_value::<BenchReport>(unknown).is_err());
    let mut missing = value.clone();
    missing.as_object_mut().unwrap().remove("measurements");
    assert!(serde_json::from_value::<BenchReport>(missing).is_err());
    let raw = serde_json::to_string(&reports()[0]).unwrap();
    let duplicate = raw.replacen('{', r#"{"schema_version":1,"#, 1);
    assert!(serde_json::from_str::<BenchReport>(&duplicate).is_err());
    let mut invalid_enum = value.clone();
    invalid_enum["platform"] = json!("wayland");
    assert!(serde_json::from_value::<BenchReport>(invalid_enum).is_err());
    let mut unknown_nested = value.clone();
    unknown_nested["measurements"][0]["sample_count"] = json!(50);
    assert!(serde_json::from_value::<BenchReport>(unknown_nested).is_err());
    for version in [0, SCHEMA_VERSION + 1] {
        let mut reports = reports();
        reports[0].schema_version = version;
        assert!(check_reports(&reports)
            .unwrap_err()
            .to_string()
            .contains("schema_version"));
    }
    let mut negative = value;
    negative["measurements"][0]["samples_ms"][0] = json!(-1);
    let parsed = serde_json::from_value::<BenchReport>(negative).unwrap();
    let mut reports = reports();
    reports[0] = parsed;
    assert!(check_reports(&reports).is_err());
}

#[test]
fn bench_report_more_than_fifty_samples_uses_all_samples() {
    let mut reports = reports();
    for report in &mut reports {
        for row in &mut report.measurements {
            row.samples_ms
                .push(latency_budget(report.platform, row.operation).p50_ms as f64);
            row.percentiles = summarize_samples(&row.samples_ms).unwrap();
        }
    }
    assert!(check_reports(&reports)
        .unwrap()
        .iter()
        .all(|row| row.samples == 51));
}

#[test]
fn bench_report_claimed_percentiles_cannot_hide_slow_samples() {
    let mut reports = reports();
    reports[0].measurements[0].samples_ms.fill(10_000.0);
    let error = check_reports(&reports).unwrap_err().to_string();
    assert!(
        error.contains("percentiles do not match samples"),
        "{error}"
    );
    assert!(error.contains("exceeds budget"), "{error}");
}

#[test]
fn bench_report_missing_or_duplicate_operation_or_platform_fails() {
    let mut missing = reports();
    missing[0].measurements.pop();
    assert!(check_reports(&missing)
        .unwrap_err()
        .to_string()
        .contains("missing operation"));
    let mut duplicate = reports();
    let extra = duplicate[1].measurements[0].clone();
    duplicate[0].measurements.push(extra);
    assert!(check_reports(&duplicate)
        .unwrap_err()
        .to_string()
        .contains("duplicate operation"));
    let mut duplicate = reports();
    duplicate.push(duplicate[0].clone());
    assert!(check_reports(&duplicate)
        .unwrap_err()
        .to_string()
        .contains("duplicate platform"));
}

#[test]
fn bench_report_not_run_or_fake_time_fails() {
    let mut skipped = reports();
    skipped[0].status = Status::NotRun;
    assert!(check_reports(&skipped)
        .unwrap_err()
        .to_string()
        .contains("not_run"));
    let mut fake = reports();
    fake[0].kind = "fake_logical_time".into();
    assert!(check_reports(&fake)
        .unwrap_err()
        .to_string()
        .contains("native_wall_clock"));
}

#[test]
fn bench_report_mixed_engine_or_fixture_revisions_fails() {
    for fixture in [false, true] {
        let mut reports = reports();
        let env = &mut reports[1].environment;
        if fixture {
            env.fixture_revision = "different-fixture".into();
        } else {
            env.engine_revision = "different-engine".into();
        }
        assert!(check_reports(&reports)
            .unwrap_err()
            .to_string()
            .contains("revisions differ"));
    }
}

#[test]
fn bench_report_wrong_fixture_window_dpi_or_workload_fails() {
    let required = reports()[0].setup.clone();
    for key in [
        "fixture",
        "window_width_logical",
        "window_height_logical",
        "scale_percent",
        "max_tree_nodes",
        "include_screenshot",
        "text_characters",
        "list_apps_warm",
    ] {
        let mut reports = reports();
        let mut setup = serde_json::to_value(&required).unwrap();
        setup[key] = match key {
            "fixture" => json!("different-fixture"),
            "include_screenshot" | "list_apps_warm" => json!(false),
            _ => json!(1),
        };
        reports[0].setup = serde_json::from_value(setup).unwrap();
        assert!(
            check_reports(&reports)
                .unwrap_err()
                .to_string()
                .contains("fixed setup"),
            "{key}"
        );
    }
    let mut reports = reports();
    reports[0].environment.fixture_revision.clear();
    assert!(check_reports(&reports).is_err());
}

#[test]
fn bench_percentiles_use_nearest_rank_without_rounding_down() {
    let samples: Vec<f64> = (1..=50).rev().map(f64::from).collect();
    let p = summarize_samples(&samples).unwrap();
    assert_eq!(p.p50_ms, 25.0);
    assert_eq!(p.p95_ms, 48.0);
    for invalid in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        assert!(summarize_samples(&[invalid; 50]).is_err());
    }
}

#[test]
fn catalog_schema_superset_covers_all_current_profiles_within_14000_bytes() {
    // Actual profiles only remove tools from this complete schema. Measuring the
    // superset gives a conservative upper bound without copying registration logic.
    assert_eq!(TOOL_CATALOG_BYTES, 14_000);
    let tools = gcu_protocol::tools_list_json();
    assert!(serde_json::to_vec(&tools).unwrap().len() <= TOOL_CATALOG_BYTES);
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../gcu-protocol/golden/tools.json")).unwrap();
    assert_eq!(tools, golden);
}
