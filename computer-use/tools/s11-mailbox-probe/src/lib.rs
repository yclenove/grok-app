//! S11-0c mechanical probe. Host driver and guest probe are subcommands of one
//! std-only binary. Real tart / hdiutil / ssh runs are macOS-only and are not
//! invoked by the tests.

// Guest and process-runner paths compile only on macOS. On other targets those
// items are intentionally unreferenced.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

mod argv;
mod budget;
mod cases;
mod clone;
mod dispatcher;
mod host;
mod identity;
mod inventory;
mod jsonutil;
mod ledger;
mod osutil;
mod pattern;
mod plist;
mod publish;
mod report;
mod runner;
mod sha256;
mod slot;
mod sshdriver;
mod volume;

#[cfg(target_os = "macos")]
mod proc;

pub use cases::Case;
pub use host::{guest_entry, real_path_supported, run_case, RunOutcome, RunRequest};

#[cfg(target_os = "macos")]
pub use proc::ProcessRunner;
