//! Host contracts for approvals, overlays, events, policy, and user activity.
//!
//! The engine calls these traits. A Host (the `gcu` helper, or Grok App through
//! the host protocol) implements them. Persistent "always allow" grants are
//! written by the Host when the user decides. This crate has no method that
//! lets the engine store or revoke one. The engine re-reads through
//! [`PolicyStore::load`] and [`PolicyStore::always_allowed`], and calls
//! [`PolicyStore::refresh`] after the Host says policy changed.
//!
//! Cancellation uses [`Cancel`]. Tokio's `CancellationToken` is not used:
//! architecture §2 allows this crate only `gcu-protocol` and `async-trait`.
//!
//! Enable the `testing` feature for the in-memory doubles core tests drive.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod activity;
pub mod approval;
pub mod events;
pub mod overlay;
pub mod policy;
pub mod types;

#[cfg(feature = "testing")]
pub mod testing;

pub use activity::UserActivity;
pub use approval::{ApprovalDecision, ApprovalProvider, Cancel, Cancelled, HandoffReason};
pub use events::{CuEvent, EventSink};
pub use overlay::{CursorKind, OverlayProvider, StatusText};
pub use policy::{
    ConfirmationLevel, ForegroundPolicy, PolicyConfig, PolicyStore, DEFAULT_APPROVAL_TIMEOUT_SECS,
};
pub use types::{
    ActionSummary, AppIdentity, AppInfo, ImageRef, ModelReason, Platform, Risk, ScreenPoint,
    SessionCtx, SignatureState, WindowInfo,
};

pub(crate) fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A panicked callback must not stick the Host mutex. The critical section
    // only moves data; recovering it keeps later reads working.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn register_waker(wakers: &mut Vec<std::task::Waker>, waker: &std::task::Waker) {
    if !wakers.iter().any(|existing| existing.will_wake(waker)) {
        wakers.push(waker.clone());
    }
}
