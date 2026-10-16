//! Copy belongs to the embedding Host's locale catalog, not native renderers.

/// Complete overlay copy table. The App maps these keys to `createT(locale)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyKey {
    /// Working in the background.
    Running,
    /// Foreground keyboard/mouse use and takeover guidance.
    Foreground,
    /// Waiting for approval.
    Waiting,
    /// Stop remains latched until explicitly resumed.
    Stopped,
    /// Emergency input monitoring is unavailable.
    Unavailable,
    /// Stop button.
    Stop,
    /// Resume button.
    Resume,
    /// Open the existing approval UI, without granting permission.
    Approval,
    /// Native Wayland and XWayland lack verifiable physical input and Stop.
    WaylandUnavailable,
}
impl CopyKey {
    /// All required entries for catalog parity checks.
    pub const ALL: [Self; 9] = [
        Self::Running,
        Self::Foreground,
        Self::Waiting,
        Self::Stopped,
        Self::Unavailable,
        Self::Stop,
        Self::Resume,
        Self::Approval,
        Self::WaylandUnavailable,
    ];
    /// Stable App i18n key, never shown directly by the native UI.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "computerUse.overlay.running",
            Self::Foreground => "computerUse.overlay.foreground",
            Self::Waiting => "computerUse.overlay.waiting",
            Self::Stopped => "computerUse.overlay.stopped",
            Self::Unavailable => "computerUse.overlay.unavailable",
            Self::Stop => "computerUse.overlay.stop",
            Self::Resume => "computerUse.overlay.resume",
            Self::Approval => "computerUse.overlay.approval",
            Self::WaylandUnavailable => "computerUse.overlay.waylandUnavailable",
        }
    }
}
/// The Host supplies a complete localized table before opening a window.
/// Calls are synchronous memory lookups; no IO, waiting or cancellation.
pub trait Catalog: Send + Sync {
    /// Resolve a key in the user's selected locale, with Host-managed fallback.
    fn text(&self, key: CopyKey) -> String;
}
