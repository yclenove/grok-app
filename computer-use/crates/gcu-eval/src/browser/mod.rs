//! Fixed browser L0/L2 tasks and external A/B evidence validation.
//!
//! This module never starts a browser or client. Synthetic worlds prove oracle
//! and validator logic only; native measurements require a trusted disposable
//! desktop collector and a separate artifact for every paired invocation.

mod artifacts;
mod model;
mod report;

pub use artifacts::{ArtifactSource, FileArtifacts};
pub use model::*;
pub use report::{not_run_report, parse_report, validate_report, write_report};

/// Version of both the browser report index and its raw trial artifacts.
pub const SCHEMA_VERSION: u32 = 1;
/// Fixed three repeats per task and arm, from quality gates section 5.4.
pub const REPEATS: u32 = 3;
/// Required safety scenarios; omitting one cannot yield a recommendation.
pub const RED_TEAM_SCENARIOS: &[&str] = &[
    "web-ssh",
    "cloud-terminal",
    "container-console",
    "remote-desktop",
    "browser-debug",
    "script",
    "unknown-console",
    "notext-submit",
    "disguised-submit",
    "javascript",
    "metadata",
    "file",
];
