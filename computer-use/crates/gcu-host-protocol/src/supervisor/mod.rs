//! App-side engine supervisor: admission, handshake, idle exit, and the crash breaker.
//!
//! The App links this module through `gcu-host-protocol` and does not link core or
//! a driver. The preferred transport is an inherited socketpair (Windows: a
//! randomly named owner pipe created for this child, `first_pipe_instance`,
//! remote clients rejected). The Unix fallback is the pinned private listener
//! already used by the engine. Restart copies the Host stop latch into the next
//! handshake and never clears it. Three unexpected exits in ten minutes disable
//! the feature until the user turns it off. Losing the control connection
//! without a requested shutdown latches a persistent Stop and does not restart
//! a delivering engine, including a crash that also drops that connection. A
//! reason file is display-only. A test double that keeps the connection uses
//! the crash breaker instead.

mod callbacks;
mod policy;
mod session;
mod state;
mod task;

pub use callbacks::{closed_callbacks, Observed, ScriptedHost};
pub use policy::{
    admit, baseline_policy, content_sha256, ensure_engine_file, parse_codesign_display,
    privilege_allowed, CodesignFacts, EngineChoice, EngineFacts, HostBuild, Reject,
    ReleaseIdentity, BREAKER_LIMIT, BREAKER_WINDOW_MS, IDLE_MS,
};
pub use session::{EngineProcess, EngineSession, LaunchRequest, LivePrivilege, PollExit};
pub use state::{HostState, Phase, Signal, Step};
pub use task::{
    Admission, Control, Launch, OsLaunch, PersistStop, ReadStopReason, SpawnFuture, Status,
};

/// Supervisor failures. Display text never includes paths, tokens, or proofs.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The child could not complete mutual authentication.
    #[error("engine handshake failed")]
    Handshake(#[source] crate::Error),
    /// Admission refused the binary before it counted as a crash.
    #[error(transparent)]
    Refused(#[from] Reject),
    /// Process or transport IO failed.
    #[error("engine process IO failed")]
    Io(#[from] std::io::Error),
    /// The child exited before its process id could be read.
    #[error("engine exited before handshake")]
    Exited,
    /// No authenticated engine peer is available for session RPC.
    #[error("engine is not connected")]
    Disconnected,
    /// The crash breaker is open. Disable the feature to clear it.
    #[error("engine restart breaker is open")]
    Tripped,
    /// Persistent Stop is not durable yet. Do not launch a delivering engine.
    #[error("persistent stop is not durable")]
    NotDurable,
    /// A path change was requested while a child is still alive.
    #[error("engine is running")]
    Busy,
}
