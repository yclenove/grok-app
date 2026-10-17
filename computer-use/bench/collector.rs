//! Native collection boundary. This card deliberately supplies no desktop
//! implementation, simulator, report writer or fallback measurements.

use super::{budgets::Platform, report::BenchReport};
use std::{sync::atomic::AtomicBool, time::Instant};

/// One authorized run on an isolated matching native desktop. The implementation
/// uses Setup::required(platform) and budgets::MIN_SAMPLES for every operation;
/// neither thresholds nor a reduced sample count are configurable here.
pub struct CollectionRequest<'a> {
    /// Must match the actual native driver/desktop; Linux must use X11.
    pub platform: Platform,
    /// Monotonic hard deadline for the whole run, including setup and warm-up.
    pub deadline: Instant,
    /// Checked before/after each sample and forwarded to native calls. A set
    /// flag aborts the run; it must not result in a measured partial report.
    pub cancelled: &'a AtomicBool,
}

/// Interface for future native runner adapters, never for the fake settle bench.
/// Implementations block on a dedicated OS worker and honor the request deadline,
/// cancellation and normal router safety/Stop controls. No adapter is installed
/// by this card; missing native evidence stays not_run and fails bench-check.
pub trait NativeCollector {
    /// Native setup, cancellation, timeout or sampling failure.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Collect at least 50 unfiltered wall-clock samples per operation. Measure
    /// with Instant from tool dispatch to the complete response; click/type_text
    /// include settle, observation and rendering, not just delivery or settle.ms.
    /// Reset the fixture's counter/text/focus and warm discovery/accessibility
    /// outside the timer, without hiding failed/unknown calls or replaying them.
    /// Verify successful outcomes and the independent fixture oracle; any failed
    /// attempt aborts the report rather than being dropped and replaced.
    /// Record observed geometry/DPI and build/OS/subscription metadata; validate
    /// samples/percentiles using report::summarize_samples before publication.
    /// A timeout, cancellation, failed permission or absent platform returns an
    /// error or a Status::NotRun report, never placeholder durations or a pass.
    fn collect(&mut self, request: &CollectionRequest<'_>) -> Result<BenchReport, Self::Error>;
}
