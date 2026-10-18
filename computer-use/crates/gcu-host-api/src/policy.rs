//! Read-only view of Host policy.
//!
//! The Host writes persistent grants when the user picks
//! [`crate::ApprovalDecision::Always`], and when the user revokes one in
//! settings. The engine only reads. After the Host reports that policy
//! changed, the engine calls [`PolicyStore::refresh`] and then reads again.

use crate::types::AppIdentity;

/// Default approval wait, in seconds. Architecture §7.3 and §12.
pub const DEFAULT_APPROVAL_TIMEOUT_SECS: u64 = 120;

/// How often a sensitive action needs its own confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfirmationLevel {
    /// Do not confirm sensitive actions.
    Off,
    /// Confirm high-risk actions. This is the default.
    Standard,
    /// Confirm every mutating action.
    Strict,
}

/// Whether the engine may use foreground input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForegroundPolicy {
    /// Never activate the target or inject foreground input.
    Never,
    /// Ask the user before foreground input.
    Ask,
    /// Foreground input is allowed without another prompt.
    Always,
}

/// One snapshot of Host policy knobs and persistent grants.
///
/// `always` lists the identities the Host stored. Membership tests use
/// [`AppIdentity::grant_eq`], not structural [`Eq`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyConfig {
    /// When false, the engine must ignore `always` and must not offer that choice.
    pub always_allow_enabled: bool,
    /// Sensitive-action confirmation level.
    pub sensitive_action_confirmation: ConfirmationLevel,
    /// Approval wait in seconds. The Host applies this to approval UI.
    pub timeout_secs: u64,
    /// Foreground-input policy.
    pub allow_foreground: ForegroundPolicy,
    /// Whether the macOS private SPI delivery path is allowed.
    pub macos_private_spi: bool,
    /// Physical input newer than this many milliseconds blocks foreground actions.
    pub user_idle_ms: u64,
    /// Stop hotkey name. `"default"` selects the platform default.
    pub stop_hotkey: String,
    /// Tree node budget.
    pub max_tree_nodes: u32,
    /// Longest screenshot edge, in pixels.
    pub max_image_edge: u32,
    /// Screenshot format name, such as `png`.
    pub image_format: String,
    /// Extra denied apps appended by the user or an enterprise policy.
    ///
    /// The built-in denylist is not in this list. Core owns that table.
    pub extra_denied_apps: Vec<String>,
    /// Persistent grants, as stored by the Host.
    pub always: Vec<AppIdentity>,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            always_allow_enabled: true,
            sensitive_action_confirmation: ConfirmationLevel::Standard,
            timeout_secs: DEFAULT_APPROVAL_TIMEOUT_SECS,
            allow_foreground: ForegroundPolicy::Ask,
            macos_private_spi: true,
            user_idle_ms: 3_000,
            stop_hotkey: "default".to_string(),
            max_tree_nodes: 1_200,
            max_image_edge: 1_568,
            image_format: "png".to_string(),
            extra_denied_apps: Vec::new(),
            always: Vec::new(),
        }
    }
}

impl PolicyConfig {
    /// Drop later entries that [`AppIdentity::grant_eq`] an earlier one.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: takes
    /// `&mut self`.
    pub fn normalize_grants(&mut self) {
        let mut kept: Vec<AppIdentity> = Vec::with_capacity(self.always.len());
        for identity in self.always.drain(..) {
            if !kept.iter().any(|existing| existing.grant_eq(&identity)) {
                kept.push(identity);
            }
        }
        self.always = kept;
    }
}

/// Host policy, as seen by the engine.
///
/// There is no `grant_always` and no `revoke`. Persistent edits happen inside
/// the Host. [`PolicyStore::refresh`] is the hook the engine calls after
/// `engine.policy.changed`. This crate does not define that protocol message.
pub trait PolicyStore: Send + Sync {
    /// Return an owned snapshot.
    ///
    /// `always` and [`PolicyStore::always_allowed`] describe the same grants:
    /// `always_allowed(id)` is true when some entry `grant_eq` `id`.
    /// `always_allow_enabled` does not hide entries. The engine checks that
    /// knob itself before honoring a grant.
    ///
    /// Blocking: no user, disk, or network wait. May wait on an internal lock.
    /// Timeout: none.
    /// Cancellation: none.
    /// Thread safety: `Send + Sync`. One call observes one snapshot.
    fn load(&self) -> PolicyConfig;

    /// Whether `id` satisfies a stored persistent grant.
    ///
    /// Comparison is [`AppIdentity::grant_eq`]. Path, content hash, code hash,
    /// certificate, publisher, team, and product/build are not widened. A
    /// missing fact does not match. A `Valid` grant does not match another
    /// signature state.
    ///
    /// This does not consult [`PolicyConfig::always_allow_enabled`].
    ///
    /// Blocking: no user or IO wait. May wait on an internal lock.
    /// Timeout: none.
    /// Cancellation: none.
    /// Thread safety: `Send + Sync`.
    fn always_allowed(&self, id: &AppIdentity) -> bool;

    /// Reload the snapshot from the Host's own store.
    ///
    /// The engine calls this after the Host says policy changed. It does not
    /// pass the new grants. Implementations must not show UI.
    ///
    /// Blocking: no user wait. May wait on an internal lock while swapping in
    /// the Host's latest snapshot.
    /// Timeout: none.
    /// Cancellation: none.
    /// Thread safety: `Send + Sync`. After it returns, later reads observe the
    /// new snapshot. A read that started earlier may still return the old one.
    fn refresh(&self);
}
