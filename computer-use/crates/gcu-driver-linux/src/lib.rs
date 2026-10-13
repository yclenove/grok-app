//! Linux AT-SPI, X11, and Wayland computer-use driver.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

/// AT-SPI role mapping, tree construction, event subscriptions, and connection management.
pub mod atspi;
/// Linux computer-use driver implementation adhering to `gcu_core::driver::Driver`.
pub mod driver;
/// Linux application identity, filesystem provenance, and re-authorization checks.
pub mod identity;

pub use driver::LinuxDriver;
pub use identity::{needs_reauth, same_process_instance, AppIdentity};
