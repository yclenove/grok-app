//! Versioned native report data and pure fail-closed validation.

use super::budgets::{latency_budget, Operation, Platform, MIN_SAMPLES, SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Actual run state; skipped native work must never pass the gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// All operations were sampled on the matching native desktop.
    Measured,
    /// Native sampling has not run.
    NotRun,
}

/// Fixed workload from quality gates §4; recorded values must match exactly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    /// Fixed cross-platform fixture/workload identifier.
    pub fixture: String,
    /// Client width in logical pixels.
    pub window_width_logical: u32,
    /// Client height in logical pixels.
    pub window_height_logical: u32,
    /// Actual OS desktop scaling, not an image resize factor.
    pub scale_percent: u32,
    /// State request node cap.
    pub max_tree_nodes: u32,
    /// State request includes the redacted window screenshot.
    pub include_screenshot: bool,
    /// Number of characters per type_text call.
    pub text_characters: u32,
    /// Discovery/accessibility cache was warmed outside timed samples.
    pub list_apps_warm: bool,
}

impl Setup {
    /// Mandatory workload. Pure construction, without IO, timeout or cancellation.
    pub fn required(platform: Platform) -> Self {
        Self {
            fixture: "gcu-fixture/performance-v1".into(),
            window_width_logical: 1280,
            window_height_logical: 860,
            scale_percent: platform.scale_percent(),
            max_tree_nodes: 1200,
            include_screenshot: true,
            text_characters: 50,
            list_apps_warm: true,
        }
    }
}

/// Reproducibility metadata, with no account information or UI payloads.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeEnvironment {
    /// Machine model/CPU/memory identifier (not a personal hostname).
    pub machine: String,
    /// Native OS and desktop version.
    pub os_version: String,
    /// Engine commit/build identifier.
    pub engine_revision: String,
    /// Driver build identifier.
    pub driver_version: String,
    /// Fixture commit/build identifier.
    pub fixture_revision: String,
    /// UTC timestamp of collection, conventionally RFC3339.
    pub captured_at_utc: String,
    /// Actual event subscription or polling route used by settle.
    pub subscription_route: String,
}

/// Nearest-rank percentiles in milliseconds, without integer truncation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Percentiles {
    /// Nearest-rank median.
    pub p50_ms: f64,
    /// Nearest-rank ninety-fifth percentile.
    pub p95_ms: f64,
}

/// One operation's raw durations and their independently checked summary.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    /// Operation measured through its complete response.
    pub operation: Operation,
    /// Real wall-clock milliseconds in collection order; no filtered failures.
    pub samples_ms: Vec<f64>,
    /// Published summary; the gate recomputes it from samples_ms.
    pub percentiles: Percentiles,
}

/// One matching desktop's versioned report. All fields are required; unknown
/// fields, enums and duplicate JSON struct fields fail serde deserialization.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchReport {
    /// Must equal SCHEMA_VERSION; newer contracts require explicit reader support.
    pub schema_version: u32,
    /// Native sampling platform, also bound to its filename by xtask.
    pub platform: Platform,
    /// Must be native_wall_clock. Fake settle output cannot satisfy this contract.
    pub kind: String,
    /// not_run is always a failed gate, even with durations attached.
    pub status: Status,
    /// Native run provenance.
    pub environment: NativeEnvironment,
    /// Observed setup, checked against the fixed workload.
    pub setup: Setup,
    /// Exactly one entry for each mandatory operation.
    pub measurements: Vec<Measurement>,
}

/// A verified percentile row, returned only when the entire platform set passes.
#[derive(Clone, Debug)]
pub struct CheckedMetric {
    /// Native platform.
    pub platform: Platform,
    /// Timed operation.
    pub operation: Operation,
    /// Number of raw durations, never a caller-supplied count.
    pub samples: usize,
    /// Recomputed percentiles.
    pub percentiles: Percentiles,
}

/// All validation failures; no partial platform set can produce success.
#[derive(Debug, thiserror::Error)]
#[error("{}", .issues.join("\n"))]
pub struct GateError {
    /// Missing evidence, malformed setup or exceeded limits.
    pub issues: Vec<String>,
}

/// Recompute nearest-rank p50/p95. Reject empty, nonfinite and nonpositive
/// durations. Pure bounded-by-input CPU work; no IO, timeout or cancellation.
pub fn summarize_samples(samples: &[f64]) -> Result<Percentiles, GateError> {
    if samples.is_empty()
        || samples
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return Err(GateError {
            issues: vec!["samples must contain finite positive wall-clock milliseconds".into()],
        });
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = |percent: usize| (sorted.len() * percent).div_ceil(100) - 1;
    Ok(Percentiles {
        p50_ms: sorted[rank(50)],
        p95_ms: sorted[rank(95)],
    })
}

/// Check every platform, operation, raw sample count and percentile against the
/// immutable budgets. Pure CPU work; no IO, timeout or cancellation. Errors
/// include every discovered violation and never turn not_run into a pass.
pub fn check_reports(reports: &[BenchReport]) -> Result<Vec<CheckedMetric>, GateError> {
    let mut issues = Vec::new();
    let mut checked = Vec::new();
    let mut platforms = BTreeSet::new();
    for report in reports {
        let platform = report.platform;
        if !platforms.insert(platform) {
            issues.push(format!("duplicate platform {platform}"));
        }
        if report.schema_version != SCHEMA_VERSION {
            issues.push(format!(
                "{platform}: unsupported schema_version {}, expected {SCHEMA_VERSION}",
                report.schema_version
            ));
        }
        if report.kind != "native_wall_clock" {
            issues.push(format!("{platform}: kind must be native_wall_clock"));
        }
        if report.status != Status::Measured {
            issues.push(format!("{platform}: not_run is not a passing measurement"));
        }
        if report.setup != Setup::required(platform) {
            issues.push(format!(
                "{platform}: report differs from required fixed setup"
            ));
        }
        let env = &report.environment;
        if let Some(baseline) = reports.first() {
            if env.engine_revision != baseline.environment.engine_revision
                || env.fixture_revision != baseline.environment.fixture_revision
            {
                issues.push(format!(
                    "{platform}: engine/fixture revisions differ between platform reports"
                ));
            }
        }
        for (field, value) in [
            ("machine", &env.machine),
            ("os_version", &env.os_version),
            ("engine_revision", &env.engine_revision),
            ("driver_version", &env.driver_version),
            ("fixture_revision", &env.fixture_revision),
            ("captured_at_utc", &env.captured_at_utc),
            ("subscription_route", &env.subscription_route),
        ] {
            if value.trim().is_empty() {
                issues.push(format!("{platform}: missing environment.{field}"));
            }
        }
        let mut operations = BTreeSet::new();
        for measurement in &report.measurements {
            let operation = measurement.operation;
            let label = format!("{platform}/{operation}");
            if !operations.insert(operation) {
                issues.push(format!("{label}: duplicate operation"));
            }
            if measurement.samples_ms.len() < MIN_SAMPLES {
                issues.push(format!(
                    "{label}: requires at least {MIN_SAMPLES} samples, found {}",
                    measurement.samples_ms.len()
                ));
            }
            let percentiles = match summarize_samples(&measurement.samples_ms) {
                Ok(value) => value,
                Err(error) => {
                    issues.push(format!("{label}: {error}"));
                    continue;
                }
            };
            if measurement.percentiles != percentiles {
                issues.push(format!("{label}: percentiles do not match samples"));
            }
            let limits = latency_budget(platform, operation);
            for (name, actual, limit) in [
                ("p50", percentiles.p50_ms, limits.p50_ms),
                ("p95", percentiles.p95_ms, limits.p95_ms),
            ] {
                if actual > limit as f64 {
                    issues.push(format!(
                        "{label}: {name} {actual} ms exceeds budget {limit} ms"
                    ));
                }
            }
            checked.push(CheckedMetric {
                platform,
                operation,
                samples: measurement.samples_ms.len(),
                percentiles,
            });
        }
        for operation in Operation::ALL {
            if !operations.contains(&operation) {
                issues.push(format!("{platform}: missing operation {operation}"));
            }
        }
    }
    for platform in Platform::ALL {
        if !platforms.contains(&platform) {
            issues.push(format!(
                "missing platform {platform}: native measurement not_run"
            ));
        }
    }
    if issues.is_empty() {
        Ok(checked)
    } else {
        Err(GateError { issues })
    }
}
