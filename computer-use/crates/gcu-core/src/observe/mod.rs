//! Pure screenshot attachment policy. It never mints images or overrides redaction.
//! All functions operate on in-memory evidence without IO, waits or cancellation.
mod auto_capture;
mod quality;

pub use auto_capture::{AutoCapturePolicy, CaptureEvidence};
pub use quality::{QualityThresholds, TreeQuality};
