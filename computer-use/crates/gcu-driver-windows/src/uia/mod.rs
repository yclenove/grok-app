//! Portable UI Automation traversal and event contracts.
//!
//! Only `ffi` contains COM bindings; its objects never leave the MTA worker.
//! Reads are cancellable and bounded by two seconds, including queued work.
pub mod cache;
pub mod events;
pub(crate) mod observe;
pub mod roles;
pub mod sensitive;
pub mod source;
pub mod tree;
pub use observe::UiaElement;
#[cfg(windows)]
mod ffi;
#[cfg(any(windows, test))]
mod worker;
#[cfg(windows)]
pub use ffi::RealUia;
