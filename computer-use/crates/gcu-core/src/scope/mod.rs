//! Evaluation fixture scope.
//!
//! The trusted assembler builds a [`FixtureScope`] and wraps the platform
//! driver in [`ScopedDriver`]. Model-facing tools cannot install, renew, or
//! widen that scope. Discovery returns only admitted targets. Every other
//! driver call checks evidence before it touches content, and launch is always
//! refused. A missing lease, a missing proof, or an identity mismatch all
//! surface as the same fixed sentence.

mod authority;
mod commit;
mod driver;
mod events;
mod guard;
mod model;

pub use authority::AttemptAuthority;
pub(crate) use authority::{expected_epoch, with_attempt};
pub use driver::ScopedDriver;
pub use model::{
    AttemptPhase, BootIdentity, ElementBinding, ExecutableProof, FixtureScope, FixtureTarget,
    InstanceEvidence, OsStartTime, ProcessInstance, RegisteredWindow, ScopeError, ScopeLease,
    WindowBinding,
};

/// Model-facing sentence for every scope refusal.
///
/// Nonexistent, external, and same-name impostor targets use this exact text.
/// It contains no app name, path, window list, or underlying driver detail.
pub const SCOPE_DENIED_MESSAGE: &str = "Target unavailable in this evaluation scope.";
