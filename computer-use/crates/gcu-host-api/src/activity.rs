//! Physical input and the user stop latch.
//!
//! The stop latch has one bit. [`UserActivity::stop_requested`] and
//! [`UserActivity::latched_stop`] both read it, and neither read clears it.
//! The only clearer is [`UserActivity::clear_stop_by_user`]. The engine and
//! the model have no method that clears the latch. Ending a turn does not
//! clear it either. A trusted Host control calls `clear_stop_by_user` when
//! the user resumes.

/// What the engine needs to know about the person at the machine.
pub trait UserActivity: Send + Sync {
    /// Milliseconds since the last physical input the Host observed.
    ///
    /// `None` means the Host has not observed physical input, or cannot tell
    /// it from input this process injected. `Some(0)` means input was observed
    /// at the current instant. The value saturates at `u64::MAX`.
    ///
    /// Blocking: none. Do not wait for the next input.
    /// Timeout: none.
    /// Cancellation: none.
    /// Thread safety: `Send + Sync`.
    fn ms_since_physical_input(&self) -> Option<u64>;

    /// Whether the stop latch is set.
    ///
    /// This is the check the engine uses before and during an action. Reading
    /// it does not clear the latch. The result stays `true` until
    /// [`UserActivity::clear_stop_by_user`].
    ///
    /// Blocking: none.
    /// Timeout: none.
    /// Cancellation: none.
    /// Thread safety: `Send + Sync`.
    fn stop_requested(&self) -> bool;

    /// Whether the stop latch is set.
    ///
    /// Same bit as [`UserActivity::stop_requested`]. The second name is the
    /// one Host UI should read when showing "stopped until you resume". There
    /// is no separate edge flag, because clearing an edge without the user
    /// would be an engine clear, and this trait does not have one.
    ///
    /// Blocking: none.
    /// Timeout: none.
    /// Cancellation: none.
    /// Thread safety: `Send + Sync`.
    fn latched_stop(&self) -> bool;

    /// Clear the stop latch because the user resumed from a trusted Host UI.
    ///
    /// Idempotent. Does not change the physical-input clock.
    ///
    /// Engine code must not call this. Model tools must not map to it. A test
    /// double exposes it so a test can simulate that user action.
    ///
    /// Blocking: none.
    /// Timeout: none.
    /// Cancellation: none. This is not cancelled by session close.
    /// Thread safety: `Send + Sync`.
    fn clear_stop_by_user(&self);
}
