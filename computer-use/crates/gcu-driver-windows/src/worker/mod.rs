//! Killable Windows native execution. Framing/supervision also run on Unix.
//! Stop bypasses the RPC queue; mutations are never replayed after transport loss.
mod action;
pub mod codec;
mod driver;
mod server;
mod supervisor;
mod tree;
#[cfg(windows)]
mod unsafe_job;
mod wire;

pub use action::Action;
pub use driver::WorkerDriver;
pub use server::serve;
pub use supervisor::{StopHandle, Supervisor, WorkerConfig};
pub use wire::{CaptureOptions, Element, Failure, Reply, Request, Response, TreeOptions, Window};

/// Native execution selection. Worker is the production default. InProcess is
/// for controlled diagnostics/benchmarks and cannot terminate a blocked FFI call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecMode {
    /// Dedicated native threads in the calling process.
    #[serde(alias = "in-process")]
    InProcess,
    /// Persistent killable `gcu worker` child.
    #[default]
    #[serde(alias = "isolated")]
    Worker,
}
