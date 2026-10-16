//! macOS discovery and accessibility-tree driver. Native bindings compile only
//! on macOS.
//!
//! [`ax::build_raw_tree`] reads a window tree. [`capture`] takes a window
//! screenshot and [`MacosDriver::window_geometry`] reports physical pixels.
//! Semantic accessibility actions do not activate the target app. Pointer and
//! keyboard input go through [`input`] when a backend is connected. Focus
//! queries stay unavailable. Portable identity, resolution, window alignment,
//! Spotlight cache policy, and [`gate::MacGate`] run on every platform through
//! [`testing::FakeMacApi`].
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
pub mod ax;
pub mod capture;
mod discovery;
mod driver;
#[cfg(target_os = "macos")]
mod ffi;
pub mod gate;
mod guards;
pub mod identity;
pub mod input;
#[cfg(target_os = "macos")]
mod real;
mod semantic;
pub mod spotlight;
pub mod testing;
mod text_input;
pub mod windows;

pub use driver::{MacElement, MacosDriver};
#[cfg(target_os = "macos")]
pub use real::RealMacApi;
