//! Platform-independent Computer Use contracts, authorization and execution.
//! Native adapters live in the Host; tests do not load Tauri or OS UI libraries.

pub mod adapter;
#[cfg(test)]
mod adapter_contract;
pub mod archive_zip;
pub mod broker;
pub mod browser;
pub mod choice;
pub mod clipboard;
pub mod driver;
pub mod error;
pub mod execution;
#[cfg(any(test, feature = "test-support"))]
pub mod fake;
pub mod ipc;
pub mod lease;
pub mod native_action;
pub mod native_parent;
pub mod nsis_contract;
pub mod owned;
pub mod pairing;
pub mod perf;
pub mod playwright_materialize;
pub mod preview;
pub mod privacy;
pub mod process_lifetime;
pub mod protocol;
#[cfg(test)]
mod protocol_golden;
pub mod quartz_frame;
pub mod runtime;
pub mod runtime_chromium;
pub mod runtime_lock;
pub mod runtime_mutation;
pub mod runtime_prepare;
#[cfg(test)]
mod runtime_test_seed;
pub mod session_grants;
pub mod staging;
pub mod surface;
pub mod tools;
mod tools_browser;
mod tools_wait;
