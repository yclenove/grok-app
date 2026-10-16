//! Payload-free stage timings, including stages abandoned by a caller timeout.
//! Start records identify a hung native step even before its end record exists.
pub(crate) struct Timing {
    stage: &'static str,
    started: tokio::time::Instant,
}
impl Timing {
    pub(crate) fn start(stage: &'static str) -> Self {
        tracing::info!(target: "gcu_windows_timing", stage, phase = "start", "windows_driver_timing");
        Self {
            stage,
            started: tokio::time::Instant::now(),
        }
    }
}
impl Drop for Timing {
    fn drop(&mut self) {
        tracing::info!(target: "gcu_windows_timing",
            stage = self.stage,
            phase = "end",
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
