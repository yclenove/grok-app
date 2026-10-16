//! Payload-free stage timings, including stages abandoned by a caller timeout.
//! Start records identify a hung native step even before its end record exists.
pub(crate) struct Timing {
    stage: &'static str,
    started: tokio::time::Instant,
    sequence: u64,
}
impl Timing {
    // Detailed provider timing is fixture-only. Keep normal production logging
    // and hard tree deadlines independent of per-property diagnostic volume.
    #[cfg_attr(not(windows), allow(dead_code))] // called from the Windows-only UIA FFI layer
    pub(crate) fn provider(stage: &'static str) -> Option<Self> {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        ENABLED
            .get_or_init(|| {
                cfg!(debug_assertions) && std::env::var("GCU_UIA_DIAGNOSTICS").as_deref() == Ok("1")
            })
            .then(|| Self::start(stage))
    }
    pub(crate) fn start(stage: &'static str) -> Self {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        tracing::info!(target: "gcu_windows_timing", stage, phase = "start", sequence, pid = std::process::id(), "windows_driver_timing");
        Self {
            stage,
            started: tokio::time::Instant::now(),
            sequence,
        }
    }
}
impl Drop for Timing {
    fn drop(&mut self) {
        tracing::info!(target: "gcu_windows_timing",
            stage = self.stage,
            phase = "end",
            sequence = self.sequence,
            pid = std::process::id(),
            elapsed_ms = self.started.elapsed().as_millis() as u64,
            "windows_driver_timing"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn timing_injected_clock_records_elapsed_without_wall_clock_wait() {
        let timing = Timing::start("test_initialize");
        tokio::time::advance(std::time::Duration::from_millis(123)).await;
        assert_eq!(
            timing.started.elapsed(),
            std::time::Duration::from_millis(123)
        );
    }
}
