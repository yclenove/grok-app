//! Windows discovery driver, with portable identity and safety logic.
//!
//! Native bindings exist only on Windows. Tree, capture and input are deliberately
//! unavailable until P2-02+. See the crate README for trust and runner boundaries.
#![deny(missing_docs, unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod api;
pub mod apps;
pub mod desktop;
mod discovery;
mod driver;
#[cfg(windows)]
mod ffi;
pub mod gate;
pub mod identity;
pub mod testing;

pub use driver::{WindowsDriver, WindowsElement};
#[cfg(windows)]
pub use ffi::RealWinApi;
