//! Metadata-only Debug for frames that carry raw pixels.

use super::capture::RawCapturedFrame;
use std::fmt;

impl fmt::Debug for RawCapturedFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawCapturedFrame")
            .field("stream_node_id", &self.stream_node_id)
            .field("sequence", &self.sequence)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("crop_region", &self.crop_region)
            .field("transform", &self.transform)
            .field("captured_at_secs", &self.captured_at_secs)
            .field("rgba_len", &self.rgba.len())
            .finish()
    }
}
