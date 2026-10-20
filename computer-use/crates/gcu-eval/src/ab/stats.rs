//! Paired task means, median percent change, sign-flip test, and bootstrap interval.
//!
//! Pure functions. No IO, timeout, or cancellation. The seed is recorded on every
//! result. Exact enumeration ignores the seed; the seeded path uses it.

use serde::Serialize;

/// Fixed seed for every core A/B sign-flip and bootstrap.
pub const AB_SEED: u64 = 0x6C61_B007;
/// Drop of 30.000 percent, in thousandths of a percent.
pub const DROP_FLOOR_MILLI_PCT: i64 = 30_000;
const EXACT_SIGN_LIMIT: usize = 20;
const EXACT_BOOTSTRAP_LIMIT: usize = 5;
const SEEDED_DRAWS: u32 = 4096;

/// Median change and a deterministic paired test.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PairStats {
    /// Median of `(mean_b - mean_a) / mean_a`, in thousandths of a percent.
    pub median_change_milli_pct: i64,
    /// Median of `(mean_a - mean_b) / mean_a`, in thousandths of a percent.
    pub median_drop_milli_pct: i64,
    /// Two-sided sign-flip p-value on the integer paired differences.
    pub p_value: f64,
    /// Lower end of the 95% bootstrap interval of the median change.
    pub ci95_low_milli_pct: i64,
    /// Upper end of that interval.
    pub ci95_high_milli_pct: i64,
    /// `exact_sign_flip` or `seeded_sign_flip`.
    pub method: &'static str,
    /// [`AB_SEED`], including when the exact path does not consume it.
    pub seed: u64,
    /// Paired tasks.
    pub pairs: usize,
}

/// `(sum_b - sum_a) / sum_a` in thousandths of a percent.
///
/// `None` when the baseline sum is zero. Half away from zero. Pure.
#[must_use]
pub fn change_milli_pct(sum_a: u128, sum_b: u128) -> Option<i64> {
    let Ok(den) = i128::try_from(sum_a) else {
        return None;
    };
    if den <= 0 {
        return None;
    }
    let Ok(num) = i128::try_from(sum_b) else {
        return None;
    };
    Some(div_round(
        num.saturating_sub(den).saturating_mul(100_000),
        den,
    ))
}

/// Median of thousandths. Even counts average toward zero. Pure.
#[must_use]
pub fn median_milli(values: &[i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        Some(sorted[mid])
    } else {
        Some(sorted[mid - 1].saturating_add(sorted[mid]) / 2)
    }
}

/// Sign-flip test on `raw_diffs` and bootstrap CI on `changes_milli`.
///
/// `raw_diffs` are paired integer differences (`sum_b - sum_a`), one per task.
/// `changes_milli` are the matching percent changes. Lengths must match.
/// Pure; no IO, timeout, or cancellation.
#[must_use]
pub fn paired_metric(changes_milli: &[i64], raw_diffs: &[i128]) -> Option<PairStats> {
    if changes_milli.is_empty() || changes_milli.len() != raw_diffs.len() {
        return None;
    }
    let median_change = median_milli(changes_milli)?;
    let (low, high) = bootstrap_ci(changes_milli);
    let (p_value, method) = sign_flip_p(raw_diffs);
    Some(PairStats {
        median_change_milli_pct: median_change,
        median_drop_milli_pct: median_change.saturating_neg(),
        p_value,
        ci95_low_milli_pct: low,
        ci95_high_milli_pct: high,
        method,
        seed: AB_SEED,
        pairs: changes_milli.len(),
    })
}

/// True when the median drop is at least 30.000 percent.
#[must_use]
pub const fn drop_meets_floor(drop_milli_pct: i64) -> bool {
    drop_milli_pct >= DROP_FLOOR_MILLI_PCT
}

/// Two-sided paired sign-flip p-value. Exact through 20 nonzero diffs.
#[must_use]
pub fn sign_flip_p(diffs: &[i128]) -> (f64, &'static str) {
    let nonzero: Vec<i128> = diffs.iter().copied().filter(|diff| *diff != 0).collect();
    if nonzero.is_empty() {
        return (1.0, "exact_sign_flip");
    }
    if nonzero.len() <= EXACT_SIGN_LIMIT {
        return (exact_sign_flip(&nonzero), "exact_sign_flip");
    }
    (
        seeded_sign_flip(&nonzero, AB_SEED, SEEDED_DRAWS),
        "seeded_sign_flip",
    )
}

fn exact_sign_flip(diffs: &[i128]) -> f64 {
    let Some(bits) = u32::try_from(diffs.len()).ok() else {
        return 1.0;
    };
    let Some(total) = 1u32.checked_shl(bits) else {
        return 1.0;
    };
    let observed = sum_abs(diffs);
    let mut extreme = 0u32;
    for mask in 0..total {
        let mut sum = 0i128;
        for (index, diff) in diffs.iter().enumerate() {
            let Some(shift) = u32::try_from(index).ok() else {
                return 1.0;
            };
            let subtract = mask & (1u32 << shift) != 0;
            sum = if subtract { sum - diff } else { sum + diff };
        }
        if sum.unsigned_abs() >= observed {
            extreme += 1;
        }
    }
    f64::from(extreme) / f64::from(total)
}

fn seeded_sign_flip(diffs: &[i128], seed: u64, samples: u32) -> f64 {
    if samples == 0 {
        return 1.0;
    }
    let observed = sum_abs(diffs);
    let mut rng = Lcg(seed);
    let mut extreme = 1u32;
    for _ in 1..samples {
        let mut sum = 0i128;
        for diff in diffs {
            sum = if rng.next() & 1 == 0 {
                sum + diff
            } else {
                sum - diff
            };
        }
        if sum.unsigned_abs() >= observed {
            extreme = extreme.saturating_add(1);
        }
    }
    f64::from(extreme) / f64::from(samples)
}

fn sum_abs(diffs: &[i128]) -> u128 {
    diffs
        .iter()
        .fold(0i128, |sum, diff| sum.saturating_add(*diff))
        .unsigned_abs()
}

fn bootstrap_ci(values: &[i64]) -> (i64, i64) {
    if values.len() == 1 {
        return (values[0], values[0]);
    }
    if values.len() <= EXACT_BOOTSTRAP_LIMIT {
        let mut samples = Vec::new();
        let mut index = vec![0usize; values.len()];
        loop {
            let draw: Vec<i64> = index.iter().map(|slot| values[*slot]).collect();
            if let Some(median) = median_milli(&draw) {
                samples.push(median);
            }
            if !odometer(&mut index, values.len()) {
                break;
            }
        }
        percentile_ends(&mut samples)
    } else {
        let mut rng = Lcg(AB_SEED);
        let mut samples = Vec::with_capacity(SEEDED_DRAWS as usize);
        for _ in 0..SEEDED_DRAWS {
            let draw: Vec<i64> = (0..values.len())
                .map(|_| values[rng.below(values.len())])
                .collect();
            if let Some(median) = median_milli(&draw) {
                samples.push(median);
            }
        }
        percentile_ends(&mut samples)
    }
}

fn percentile_ends(samples: &mut [i64]) -> (i64, i64) {
    if samples.is_empty() {
        return (0, 0);
    }
    samples.sort_unstable();
    let last = samples.len() - 1;
    let low = (0.025 * last as f64).floor() as usize;
    let high = (0.975 * last as f64).ceil() as usize;
    (samples[low.min(last)], samples[high.min(last)])
}

fn odometer(index: &mut [usize], base: usize) -> bool {
    for slot in index.iter_mut() {
        *slot += 1;
        if *slot < base {
            return true;
        }
        *slot = 0;
    }
    false
}

fn div_round(num: i128, den: i128) -> i64 {
    let neg = num < 0;
    let den = den.unsigned_abs().max(1);
    let num = num.unsigned_abs();
    let mut quot = num / den;
    if (num % den).saturating_mul(2) >= den {
        quot = quot.saturating_add(1);
    }
    let value = i64::try_from(quot).unwrap_or(i64::MAX);
    if neg {
        value.saturating_neg()
    } else {
        value
    }
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        self.0
    }

    fn below(&mut self, limit: usize) -> usize {
        let Some(limit) = u64::try_from(limit).ok().filter(|value| *value > 0) else {
            return 0;
        };
        usize::try_from(self.next() % limit).unwrap_or(0)
    }
}
