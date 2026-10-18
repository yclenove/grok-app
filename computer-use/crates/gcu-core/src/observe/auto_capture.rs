use super::{QualityThresholds, TreeQuality};

/// Evidence for architecture §4.4. Pixel evidence must come from redacted images.
#[derive(Debug, Clone, Copy, Default)]
pub struct CaptureEvidence {
    /// The delivered action used screenshot coordinates.
    pub action_used_coordinates: bool,
    /// The model's rendered state (including header/footer) is unchanged.
    pub diff_is_empty: bool,
    /// Some(true) only when comparable safe physical images differ.
    pub pixels_changed: Option<bool>,
    /// A trusted caller explicitly requested an image for an action result.
    pub explicit: bool,
}

/// Screenshot delivery policy; it cannot return or construct raw image bytes.
/// All decisions are synchronous memory work, without IO, waits or cancellation.
#[derive(Debug, Clone, Copy, Default)]
pub struct AutoCapturePolicy {
    thresholds: QualityThresholds,
}
impl AutoCapturePolicy {
    /// Supply evaluation boundaries before sharing a router. No IO or blocking.
    /// Uncalibrated/invalid boundaries retain conservative observation delivery.
    pub fn new(thresholds: QualityThresholds) -> Self {
        Self { thresholds }
    }

    /// Decide an opt-in auto observation. Without a calibrated interactive-node
    /// boundary it keeps the image; explicit boolean requests bypass this policy.
    pub fn observation(&self, quality: &TreeQuality, evidence: CaptureEvidence) -> bool {
        self.thresholds.min_interactive_nodes.is_none() || self.action(quality, evidence)
    }

    /// Decide an action result using coordinates, canvas, invisible pixel changes,
    /// explicit requests and demonstrably insufficient tree quality. Uncalibrated
    /// rich trees retain the existing action omission rule, not a guessed cutoff.
    pub fn action(&self, quality: &TreeQuality, evidence: CaptureEvidence) -> bool {
        evidence.explicit
            || evidence.action_used_coordinates
            || quality.canvas_like(&self.thresholds)
            || (evidence.diff_is_empty && evidence.pixels_changed == Some(true))
            || quality.useful_controls() == 0
            || quality.truncated
            || self
                .thresholds
                .min_interactive_nodes
                .is_some_and(|minimum| quality.useful_controls() < minimum)
    }
}
