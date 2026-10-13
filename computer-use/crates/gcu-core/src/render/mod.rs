//! Model-facing text and Host-facing structured facts. Neither renderer performs IO.

mod format;
mod limit;
/// Redacted architecture §4.3 structuredContent.
pub mod structured;
/// Text observation format and whole-output byte budgeting.
pub mod text;
use gcu_protocol::{result::ActionResult, types::ScreenshotInfo, AppInfo, WindowInfo};
pub use structured::render_structured;
pub use text::render_text;

/// Observation presentation options, independent of native tree-read deadlines.
#[derive(Debug, Clone)]
pub struct RenderOpts {
    /// Resolved app discovery record, not model-supplied identity.
    pub app: AppInfo,
    /// Resolved window geometry and title.
    pub window: WindowInfo,
    /// Unicode-scalar limit per field. None is the protocol's `"max"`; default 500.
    /// The whole text result still fits the fixed 20,000-byte client prefix budget.
    pub text_limit: Option<usize>,
    /// Include elements in structuredContent only for detail=full; false by default.
    pub full_detail: bool,
    /// SnapshotStore-issued identifier; None until the snapshot has been recorded.
    pub snapshot_id: Option<String>,
    /// Reference to an already-redacted screenshot; no bytes are serialized here.
    pub screenshot: Option<ScreenshotInfo>,
    /// Optional executed action result, already redacted by the router.
    pub action: Option<ActionResult>,
}
impl RenderOpts {
    /// Construct the default compact observation with a 500-character field limit.
    pub fn new(app: AppInfo, window: WindowInfo) -> Self {
        Self {
            app,
            window,
            text_limit: Some(500),
            full_detail: false,
            snapshot_id: None,
            screenshot: None,
            action: None,
        }
    }
}
