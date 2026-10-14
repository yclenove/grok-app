//! Windows discovery, UIA and window capture with portable safety logic.
//!
//! Native bindings exist only on Windows. Input remains unavailable.
//! See the crate README for trust, completeness, and native runner boundaries.
#![deny(missing_docs, unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod api;
pub mod apps;
pub mod capture;
pub mod desktop;
mod discovery;
mod driver;
#[cfg(windows)]
mod ffi;
pub mod gate;
pub mod identity;
pub mod testing;
pub mod uia;

pub use driver::{WindowsDriver, WindowsElement};
#[cfg(windows)]
pub use ffi::RealWinApi;
