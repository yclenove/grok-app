//! Deliberately excludes arbitrary strings, paths, session IDs and content.

use serde::{Deserialize, Serialize};

const MAX_COUNTER: u64 = 9_007_199_254_740_991;
const MAX_MILLISECONDS: f64 = 31_536_000_000.0;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrontendSample {
    pub version: u32,
    pub sequence: u64,
    pub elapsed_ms: f64,
    pub interval_ms: f64,
    pub timer_lag_ms: f64,
    pub frame_count: u64,
    pub max_frame_gap_ms: f64,
    pub document_visible: bool,
    pub stream: StreamSample,
    pub tools: ToolSample,
    pub previous_write_failures: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StreamSample {
    pub received_chunks: u64,
    pub flushed_chunks: u64,
    pub pending_keys: u64,
    pub pending_text_code_units: u64,
    pub peak_pending_keys: u64,
    pub peak_pending_text_code_units: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolSample {
    pub received_events: u64,
    pub flushed_events: u64,
    pub pending_events: u64,
    pub peak_pending_events: u64,
}

impl FrontendSample {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("telemetry_unsupported_version".into());
        }
        for value in [
            self.elapsed_ms,
            self.interval_ms,
            self.timer_lag_ms,
            self.max_frame_gap_ms,
        ] {
            if !value.is_finite() || !(0.0..=MAX_MILLISECONDS).contains(&value) {
                return Err("telemetry_invalid_duration".into());
            }
        }
        for value in [
            self.sequence,
            self.frame_count,
            self.previous_write_failures,
            self.stream.received_chunks,
            self.stream.flushed_chunks,
            self.stream.pending_keys,
            self.stream.pending_text_code_units,
            self.stream.peak_pending_keys,
            self.stream.peak_pending_text_code_units,
            self.tools.received_events,
            self.tools.flushed_events,
            self.tools.pending_events,
            self.tools.peak_pending_events,
        ] {
            if value > MAX_COUNTER {
                return Err("telemetry_invalid_counter".into());
            }
        }
        // Raw inputs and merged flushes are different units. Enabling midway
        // may also flush earlier buffered data; never assert conservation.
        Ok(())
    }
}
