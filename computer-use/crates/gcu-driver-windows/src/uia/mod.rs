//! Portable UI Automation observation, semantic action and event contracts.
//!
//! Only `ffi` contains COM bindings; its objects never leave the MTA worker.
//! Reads are cancellable within two seconds; actions share DRIVER_DEADLINE.
pub mod actions;
pub mod cache;
mod diagnostics;
pub mod events;
mod fake_actions;
pub(crate) mod observe;
pub mod patterns;
pub mod roles;
mod scroll;
pub mod sensitive;
pub mod source;
pub mod tree;
mod value;
pub use observe::UiaElement;
#[cfg(windows)]
mod ffi;
#[cfg(any(windows, test))]
pub(crate) mod worker;
#[cfg(windows)]
pub use ffi::RealUia;
