//! Software cursor and status capsule. The Host draws them. Screenshots must
//! be able to exclude them when the platform allows it.

use crate::types::{ScreenPoint, SessionCtx, WindowInfo};

/// Which overlay cursor to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CursorKind {
    /// Moving toward a click or press.
    Pointer,
    /// Text insertion.
    Caret,
    /// Drag in progress.
    Drag,
    /// Scroll in progress.
    Scroll,
}

/// Text for the status capsule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusText {
    /// Short status line. The engine writes it; it is not model text.
    pub message: String,
    /// True when the engine is about to use foreground input.
    pub foreground: bool,
}

/// Cursor and capsule for one session.
///
/// Calls are notifications. They must return without waiting for the user.
/// The engine calls [`OverlayProvider::hide`] when the session ends or the
/// turn goes idle. These methods are not cancelled individually.
pub trait OverlayProvider: Send + Sync {
    /// Move the software cursor to `at` on `window`.
    ///
    /// `at` is in the same physical-pixel screen space as `window.frame`.
    ///
    /// Blocking: none. Return after the Host accepts the update. Do not wait
    /// for the animation.
    /// Timeout: none.
    /// Cancellation: none. [`OverlayProvider::hide`] drops the cursor.
    /// Thread safety: `Send + Sync`. Sessions may update concurrently.
    fn show_target(
        &self,
        session: &SessionCtx,
        window: &WindowInfo,
        at: ScreenPoint,
        kind: CursorKind,
    );

    /// Replace the capsule text.
    ///
    /// Blocking: none. Do not wait for the user to read it.
    /// Timeout: none.
    /// Cancellation: none.
    /// Thread safety: `Send + Sync`.
    fn status(&self, session: &SessionCtx, text: &StatusText);

    /// Remove the cursor and capsule for `session`.
    ///
    /// Idempotent.
    ///
    /// Blocking: none.
    /// Timeout: none.
    /// Cancellation: none. This is how a finished session clears the overlay.
    /// Thread safety: `Send + Sync`.
    fn hide(&self, session: &SessionCtx);

    /// Whether this Host can keep the overlay out of screenshots.
    ///
    /// A `false` result is a capability fact, not a failure. The engine still
    /// calls the other methods.
    ///
    /// Blocking: none.
    /// Timeout: none.
    /// Cancellation: none.
    /// Thread safety: `Send + Sync`. The answer may be cached.
    fn exclude_from_capture_supported(&self) -> bool;
}
