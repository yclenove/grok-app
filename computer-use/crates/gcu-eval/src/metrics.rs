//! Success, latency, and paired A/B metrics.
//!
//! Calculations are pure. They do not block, time out, or cancel.

use crate::runner::{RunStatus, TaskOutcome};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One scored or skipped task reduced to the fields an A/B comparison needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trial {
    /// Task id.
    pub task_id: String,
    /// Repeat index. The plan averages three repeats; the test can pass more.
    pub repeat: u32,
    /// True only when the oracle passed and no side effect fired.
    pub success: bool,
    /// Wall time in milliseconds.
    pub duration_ms: u64,
}

/// Success rate for one category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryRate {
    /// Tasks that ran an oracle. `not_run` is excluded.
    pub scored: usize,
    /// Tasks whose status is pass.
    pub passed: usize,
    /// `passed / scored`, absent when nothing was scored.
    pub success_rate: Option<f64>,
}

/// Suite totals. `not_run` rows do not enter rates or percentiles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// Rows in the report, including `not_run`.
    pub task_count: usize,
    /// Rows that were scored.
    pub scored: usize,
    /// Passing rows.
    pub passed: usize,
    /// Overall success rate.
    pub success_rate: Option<f64>,
    /// Rates keyed by category.
    pub by_category: BTreeMap<String, CategoryRate>,
    /// Nearest-rank p50 of scored durations.
    pub p50_ms: Option<u64>,
    /// Nearest-rank p95 of scored durations.
    pub p95_ms: Option<u64>,
    /// Mean tool-call count over scored rows.
    pub mean_steps: f64,
    /// Sum of consecutive duplicate tool calls.
    pub redundant_calls: u64,
    /// Unknown results divided by tool calls. Zero when there were no calls.
    pub unknown_rate: f64,
    /// Forbidden side-effect and fixture violations.
    pub violations: u64,
    /// Agent claimed success while the oracle failed.
    pub false_success: u64,
}

/// One task's paired comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairRow {
    /// Task id.
    pub task_id: String,
    /// Mean success of group A over aligned repeats.
    pub mean_a: f64,
    /// Mean success of group B over aligned repeats.
    pub mean_b: f64,
    /// `mean_b - mean_a`.
    pub mean_diff: f64,
    /// Two-sided sign-test p-value on paired success differences.
    pub p_value: f64,
    /// True when `p_value < 0.05`. Three agreeing repeats are not enough.
    pub significant: bool,
    /// Aligned repeat count, including ties.
    pub pairs: u32,
}

/// Aggregate scored outcomes.
pub fn summarize(outcomes: &[TaskOutcome]) -> Summary {
    let scored: Vec<&TaskOutcome> = outcomes.iter().filter(|row| row.scored()).collect();
    let passed = scored
        .iter()
        .filter(|row| row.status == RunStatus::Pass)
        .count();
    let mut durations: Vec<u64> = scored.iter().map(|row| row.metrics.duration_ms).collect();
    durations.sort_unstable();
    let calls: u64 = scored.iter().map(|row| row.metrics.tool_calls).sum();
    let unknown: u64 = scored.iter().map(|row| row.metrics.unknown_calls).sum();
    let steps: u64 = scored.iter().map(|row| row.metrics.steps).sum();
    let mut by_category = BTreeMap::new();
    for row in &scored {
        let entry = by_category
            .entry(row.category.clone())
            .or_insert(CategoryRate {
                scored: 0,
                passed: 0,
                success_rate: None,
            });
        entry.scored += 1;
        if row.status == RunStatus::Pass {
            entry.passed += 1;
        }
    }
    for rate in by_category.values_mut() {
        rate.success_rate = ratio(rate.passed, rate.scored);
    }
    Summary {
        task_count: outcomes.len(),
        scored: scored.len(),
        passed,
        success_rate: ratio(passed, scored.len()),
        by_category,
        p50_ms: percentile(&durations, 50.0),
        p95_ms: percentile(&durations, 95.0),
        mean_steps: if scored.is_empty() {
            0.0
        } else {
            steps as f64 / scored.len() as f64
        },
        redundant_calls: scored.iter().map(|row| row.metrics.redundant_calls).sum(),
        unknown_rate: if calls == 0 {
            0.0
        } else {
            unknown as f64 / calls as f64
        },
        violations: scored.iter().map(|row| row.violations.len() as u64).sum(),
        false_success: scored
            .iter()
            .filter(|row| row.metrics.false_success)
            .count() as u64,
    }
}

/// Nearest-rank percentile. `samples` must already be sorted ascending.
#[must_use]
pub fn percentile(sorted: &[u64], percent: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (percent / 100.0 * sorted.len() as f64).ceil() as usize;
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    Some(sorted[index])
}

fn ratio(passed: usize, scored: usize) -> Option<f64> {
    if scored == 0 {
        None
    } else {
        Some(passed as f64 / scored as f64)
    }
}

/// Paired sign test. Repeats align on `(task_id, repeat)`.
///
/// With three repeats the smallest two-sided p-value is 0.25, so `significant`
/// stays false. Callers that need a significant row must supply more repeats.
pub fn compare_trials(group_a: &[Trial], group_b: &[Trial]) -> Vec<PairRow> {
    let mut ids = BTreeMap::<String, ()>::new();
    for trial in group_a.iter().chain(group_b) {
        ids.insert(trial.task_id.clone(), ());
    }
    let mut rows = Vec::new();
    for task_id in ids.into_keys() {
        let mut positive = 0u32;
        let mut negative = 0u32;
        let mut sum_a = 0.0;
        let mut sum_b = 0.0;
        let mut pairs = 0u32;
        let repeats = repeats_for(group_a, &task_id);
        for repeat in repeats {
            let Some(left) = find(group_a, &task_id, repeat) else {
                continue;
            };
            let Some(right) = find(group_b, &task_id, repeat) else {
                continue;
            };
            pairs += 1;
            sum_a += f64::from(u8::from(left.success));
            sum_b += f64::from(u8::from(right.success));
            match (left.success, right.success) {
                (false, true) => positive += 1,
                (true, false) => negative += 1,
                _ => {}
            }
        }
        if pairs == 0 {
            continue;
        }
        let p_value = sign_test_p(positive, negative);
        rows.push(PairRow {
            task_id,
            mean_a: sum_a / f64::from(pairs),
            mean_b: sum_b / f64::from(pairs),
            mean_diff: (sum_b - sum_a) / f64::from(pairs),
            p_value,
            significant: p_value < 0.05,
            pairs,
        });
    }
    rows
}

fn repeats_for(trials: &[Trial], task_id: &str) -> Vec<u32> {
    let mut repeats: Vec<u32> = trials
        .iter()
        .filter(|trial| trial.task_id == task_id)
        .map(|trial| trial.repeat)
        .collect();
    repeats.sort_unstable();
    repeats.dedup();
    repeats
}

fn find<'a>(trials: &'a [Trial], task_id: &str, repeat: u32) -> Option<&'a Trial> {
    trials
        .iter()
        .find(|trial| trial.task_id == task_id && trial.repeat == repeat)
}

fn sign_test_p(positive: u32, negative: u32) -> f64 {
    let n = positive + negative;
    if n == 0 {
        return 1.0;
    }
    let k = positive.min(negative);
    let mut cdf = 0.0;
    let scale = 2f64.powi(i32::try_from(n).unwrap_or(i32::MAX));
    for index in 0..=k {
        cdf += binomial(n, index) as f64 / scale;
    }
    (2.0 * cdf).min(1.0)
}

fn binomial(n: u32, k: u32) -> u128 {
    let mut value = 1u128;
    for index in 0..k {
        value = value * u128::from(n - index) / u128::from(index + 1);
    }
    value
}
