//! Segmented timing summaries. Cold and warm samples stay in separate series.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesSummary {
    pub phase: String,
    pub segment: String,
    pub n: usize,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub max_ms: u64,
    pub failure_rate: f64,
}

pub fn percentile(samples: &[u64], p: f64) -> Option<u64> {
    if samples.is_empty() {
        return None;
    }
    let mut v = samples.to_vec();
    v.sort_unstable();
    let max_i = v.len() - 1;
    let idx = ((p / 100.0) * max_i as f64).round() as usize;
    Some(v[idx.min(max_i)])
}

pub fn summarize(
    phase: &str,
    segment: &str,
    ok_ms: &[u64],
    failures: usize,
) -> Option<SeriesSummary> {
    if ok_ms.is_empty() && failures == 0 {
        return None;
    }
    let n = ok_ms.len() + failures;
    Some(SeriesSummary {
        phase: phase.into(),
        segment: segment.into(),
        n,
        p50_ms: percentile(ok_ms, 50.0).unwrap_or(0),
        p95_ms: percentile(ok_ms, 95.0).unwrap_or(0),
        max_ms: ok_ms.iter().copied().max().unwrap_or(0),
        failure_rate: failures as f64 / n as f64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_are_monotonic_on_real_samples() {
        let s = [4u64, 1, 9, 2, 6, 3, 8, 5, 7];
        let p50 = percentile(&s, 50.0).unwrap();
        let p95 = percentile(&s, 95.0).unwrap();
        let max = *s.iter().max().unwrap();
        assert!(p50 <= p95, "{p50} {p95}");
        assert!(p95 <= max, "{p95} {max}");
        let cold = summarize("cold", "observe", &[1, 2, 3], 0).unwrap();
        let warm = summarize("warm", "observe", &[10, 20, 30], 0).unwrap();
        assert_ne!(cold.p50_ms, warm.p50_ms, "cold/warm must not be mixed");
        assert_eq!(cold.phase, "cold");
        assert_eq!(warm.phase, "warm");
    }

    #[test]
    fn empty_series_is_none_not_a_fake_zero() {
        assert!(summarize("cold", "x", &[], 0).is_none());
        let failed = summarize("warm", "x", &[], 3).unwrap();
        assert_eq!(failed.failure_rate, 1.0);
        assert_eq!(failed.n, 3);
    }
}
