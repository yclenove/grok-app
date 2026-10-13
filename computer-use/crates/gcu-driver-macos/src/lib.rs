//! macOS discovery driver. Native bindings compile only on macOS.
//!
//! Tree, capture, and input stay unavailable until P1-03+. Portable identity,
//! resolution, window alignment, Spotlight cache policy, and [`gate::MacGate`]
//! run on every platform through [`testing::FakeMacApi`].
//!
//! A code signature does not prove that every byte running inside the process
//! is trusted. Plugins and writable load paths can still change that code.
//! [`identity::MacIdentity::trust_note`] says so on every identity.
//!
//! `DriverError::AmbiguousApp` carries no candidates. The pure resolver returns
//! them on [`apps::ResolveOutcome::Ambiguous`].
#![deny(missing_docs, unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod api;
pub mod apps;
mod discovery;
mod driver;
#[cfg(target_os = "macos")]
mod ffi;
pub mod gate;
pub mod identity;
#[cfg(target_os = "macos")]
mod real;
pub mod spotlight;
pub mod testing;
pub mod windows;

pub use driver::{MacElement, MacosDriver};
#[cfg(target_os = "macos")]
pub use real::RealMacApi;
