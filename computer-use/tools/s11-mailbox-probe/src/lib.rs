//! S11-0c mechanical probe for mailbox cases C1, C2, and C8.
//! Real tart and ssh runs are macOS-only. Tests do not invoke them.

mod argv;
mod budget;
mod cases;
mod clone;
mod dispatcher;
mod host;
mod identity;
mod inventory;
mod jsonutil;
mod osutil;
mod pattern;
mod proc;
mod publish;
mod report;
mod runner;
mod sha256;
mod slot;
mod ssh;

pub use budget::Deadline;
pub use cases::Case;
pub use host::{guest_entry, real_path_supported, run_case, RunOutcome, RunRequest};
pub use proc::ProcessRunner;
