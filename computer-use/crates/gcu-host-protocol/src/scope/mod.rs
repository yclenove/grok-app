//! ADR 0017 private scope control. These types are never MCP methods or params.
//!
//! The inherited connection is the capability. JSON from any other source is
//! not authority. Native evidence validation belongs to the engine assembler.
use crate::Error;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod launch;

/// First private scope protocol version. Unknown versions are terminal errors.
pub const VERSION: u32 = 1;
/// Tight bound on scope-control JSON, including parse/conversion work. MCP and
/// ordinary Host frames retain their existing limits.
pub const MAX_CONTROL_FRAME: usize = 64 * 1024;
/// Engine-local monotonic lease lifetime; a sender cannot choose a longer TTL.
pub const LEASE: std::time::Duration = std::time::Duration::from_secs(3);

/// Commands accepted only on the inherited supervisor connection.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Control {
    /// First and only installation in this process.
    Install {
        /// Exact supported protocol version.
        version: u32,
        /// One evaluation run.
        run_id: String,
        /// Nonzero initial generation.
        generation: u64,
        /// SHA-256 of the canonical record, including run and generation.
        scope_digest: String,
        /// Explicit allowed set, never inferred from names or ancestry.
        scope: Scope,
    },
    /// Replace the set; the generation must strictly increase.
    Rebind {
        /// Must match the installed run.
        run_id: String,
        /// Strictly newer generation.
        generation: u64,
        /// Complete replacement set.
        scope: Scope,
    },
    /// Extend the current lease without changing its set.
    Renew {
        /// Must match the installed run.
        run_id: String,
        /// Must match the current generation.
        generation: u64,
        /// Strictly increasing within this generation, starting above zero.
        seq: u64,
    },
    /// Terminal revocation; later Install is never accepted.
    Revoke {
        /// Must match the installed run.
        run_id: String,
    },
}

/// Confirmation of exactly the record installed by the engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ack {
    /// Bound run.
    pub run_id: String,
    /// Bound generation.
    pub generation: u64,
    /// Canonical digest computed by the engine.
    pub scope_digest: String,
}

/// Wire record, independent of core/driver implementations. All fields required.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    /// Dedicated engine instance label.
    pub engine_instance: String,
    /// Dedicated desktop session label.
    pub desktop_session: String,
    /// Private control lease label, not a bearer or secret.
    pub control_lease: String,
    /// Explicit process/window registrations. Empty admits nothing.
    pub targets: Vec<Target>,
}
impl Scope {
    /// Hash deterministic JSON field order plus run/generation. Memory only;
    /// no IO, waiting or cancellation. This digest does not authenticate a peer.
    pub fn digest(&self, run_id: &str, generation: u64) -> Result<String, Error> {
        let bytes =
            serde_json::to_vec(&(VERSION, run_id, generation, self)).map_err(|_| Error::Invalid)?;
        Ok(Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
}
/// One process registration; its proof is checked independently by the driver.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// Non-authoritative task role.
    pub role: String,
    /// Exact process instance.
    pub instance: Instance,
    /// Recorded executable proof. A pathname alone is not authority.
    pub executable_proof: String,
    /// Exact registered windows, each with its own owner instance.
    pub windows: Vec<Window>,
}
/// Process instance evidence, never a bare PID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    /// OS process id (synthetic on the S3 fake route).
    pub pid: u32,
    /// Native-resolution start time (synthetic in S3).
    pub os_start_time: u64,
    /// Boot identity (synthetic in S3).
    pub boot_identity: String,
}
/// Exact window incarnation and owning process.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    /// Native window id.
    pub native_id: String,
    /// Nonzero incarnation.
    pub incarnation: u64,
    /// Independently verified owner, not inferred from a title.
    pub owner_instance: Instance,
}
